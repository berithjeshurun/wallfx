//! Pack mpv + ffmpeg into release wallfx.exe as a zstd self-extract overlay.
//!
//! Usage (from repo root):
//!   cargo build --release
//!   cargo run --release --bin pack

use anyhow::{bail, Context, Result};
use std::fs;
use std::path::PathBuf;
use wallfx::overlay;

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let root = find_repo_root()?;
    let wallfx_exe = root.join("target").join("release").join("wallfx.exe");
    let mpv = first_existing(&[
        root.join("tools").join("mpv").join("mpv.exe"),
        root.join("tools").join("mpv.exe"),
    ])
    .context("mpv.exe not found under tools\\")?;
    let ffmpeg = root.join("tools").join("ffmpeg.exe");
    if !ffmpeg.is_file() {
        bail!("ffmpeg.exe not found at {}", ffmpeg.display());
    }
    if !wallfx_exe.is_file() {
        bail!(
            "wallfx.exe not found at {} — run `cargo build --release` first",
            wallfx_exe.display()
        );
    }

    println!("reading thin PE from {}", wallfx_exe.display());
    let thin = overlay::load_thin_pe(&wallfx_exe)?;
    println!("  thin PE: {} bytes", thin.len());

    println!("reading {}", mpv.display());
    let mpv_bytes = fs::read(&mpv)?;
    println!("  mpv: {} bytes", mpv_bytes.len());

    println!("reading {}", ffmpeg.display());
    let ffmpeg_bytes = fs::read(&ffmpeg)?;
    println!("  ffmpeg: {} bytes", ffmpeg_bytes.len());

    println!("compressing + writing overlay → {}", wallfx_exe.display());
    overlay::pack_to(&wallfx_exe, &thin, &mpv_bytes, &ffmpeg_bytes)?;

    let fat_len = fs::metadata(&wallfx_exe)?.len();
    println!(
        "done. fat wallfx.exe is {} bytes ({:.1} MB)",
        fat_len,
        fat_len as f64 / (1024.0 * 1024.0)
    );
    println!("Distribute with config.yaml: self.expand: true");
    Ok(())
}

fn first_existing(paths: &[PathBuf]) -> Option<PathBuf> {
    paths.iter().find(|p| p.is_file()).cloned()
}

fn find_repo_root() -> Result<PathBuf> {
    if let Ok(cwd) = std::env::current_dir() {
        if cwd.join("Cargo.toml").is_file() {
            return Ok(cwd);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(release) = exe.parent() {
            if let Some(target) = release.parent() {
                if let Some(root) = target.parent() {
                    if root.join("Cargo.toml").is_file() {
                        return Ok(root.to_path_buf());
                    }
                }
            }
        }
    }
    bail!("could not locate repo root (Cargo.toml)");
}
