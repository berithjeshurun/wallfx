//! Color grading + LUT via ffmpeg filter graphs (live or baked).

use crate::config::GradeSection;
use anyhow::{bail, Context, Result};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Build an ffmpeg/lavfi filter chain for eq + optional lut3d.
pub fn filter_graph(grade: &GradeSection) -> Option<String> {
    if !grade.enabled {
        return None;
    }
    let mut parts = Vec::new();
    let needs_eq = (grade.brightness - 0.0).abs() > 0.000_1
        || (grade.contrast - 1.0).abs() > 0.000_1
        || (grade.saturation - 1.0).abs() > 0.000_1
        || (grade.gamma - 1.0).abs() > 0.000_1;
    if needs_eq {
        parts.push(format!(
            "eq=brightness={}:contrast={}:saturation={}:gamma={}",
            grade.brightness, grade.contrast, grade.saturation, grade.gamma
        ));
    }
    let lut = grade.lut.trim();
    if !lut.is_empty() {
        let path = PathBuf::from(lut);
        if path.is_file() {
            parts.push(format!(
                "lut3d=file={}:interp=tetrahedral",
                escape_filter_path(&path)
            ));
        } else {
            tracing::warn!(path = %path.display(), "LUT file not found — skipping lut3d");
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(","))
    }
}

/// Escape a Windows path for use inside an ffmpeg filter graph.
pub fn escape_filter_path(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    // Escape characters special to filtergraphs: \ : ' [ ]
    let mut out = String::with_capacity(s.len() * 2);
    for ch in s.chars() {
        match ch {
            '\\' | ':' | '\'' | '[' | ']' | ',' | ';' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}

/// Resolve the file to play: either original, or a baked graded MP4 in cache\\graded.
pub fn resolve_play_path(
    ffmpeg: Option<&Path>,
    source: &Path,
    grade: &GradeSection,
) -> Result<PathBuf> {
    if !grade.enabled {
        return Ok(source.to_path_buf());
    }
    if !grade.bake {
        return Ok(source.to_path_buf());
    }
    let Some(ffmpeg) = ffmpeg else {
        bail!("Bake grade requires ffmpeg.exe");
    };
    bake(ffmpeg, source, grade)
}

fn bake(ffmpeg: &Path, source: &Path, grade: &GradeSection) -> Result<PathBuf> {
    let Some(vf) = filter_graph(grade) else {
        return Ok(source.to_path_buf());
    };
    let out_dir = std::env::current_dir()?.join("cache").join("graded");
    std::fs::create_dir_all(&out_dir).context("mkdir cache/graded")?;

    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    grade.brightness.to_bits().hash(&mut hasher);
    grade.contrast.to_bits().hash(&mut hasher);
    grade.saturation.to_bits().hash(&mut hasher);
    grade.gamma.to_bits().hash(&mut hasher);
    grade.lut.hash(&mut hasher);
    let name = format!("{:016x}.mp4", hasher.finish());
    let out = out_dir.join(name);
    if out.is_file() {
        tracing::info!(path = %out.display(), "using cached graded video");
        return Ok(out);
    }

    tracing::info!(src = %source.display(), out = %out.display(), vf = %vf, "baking graded wallpaper");
    let status = Command::new(ffmpeg)
        .arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(source)
        .arg("-vf")
        .arg(&vf)
        .arg("-an")
        .arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg("veryfast")
        .arg("-crf")
        .arg("18")
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg(&out)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .with_context(|| format!("spawn bake {}", ffmpeg.display()))?;
    if !status.success() {
        let _ = std::fs::remove_file(&out);
        bail!("ffmpeg grade bake failed with status {status}");
    }
    Ok(out)
}
