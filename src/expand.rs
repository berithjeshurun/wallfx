//! Self-extract: inflate overlay into `%CD%\bin`, shrink exe, clear expand flag.

use crate::config::{self, Config};
use wallfx::overlay;
use anyhow::{bail, Context, Result};
use std::fs;

pub enum Outcome {
    /// No expand requested or nothing to do.
    Continue,
    /// Tools extracted; thin exe scheduled to replace fat; caller should exit.
    ExtractedRestart,
}

pub fn maybe_expand(cfg: &Config) -> Result<Outcome> {
    let cwd = std::env::current_dir().context("current_dir")?;
    // Fat image renamed aside during expand; remove it once nothing maps it.
    let _ = fs::remove_file(cwd.join("wallfx.exe.old"));
    let _ = fs::remove_file(cwd.join("wallfx.exe.tmp"));

    if !cfg.expand() {
        return Ok(Outcome::Continue);
    }

    let bin_dir = cwd.join("bin");
    let exe_path = std::env::current_exe().context("current_exe")?;

    let payload = match overlay::read_payload(&exe_path)? {
        Some((_footer, payload)) => payload,
        None => {
            let has_bin = bin_dir.join("mpv.exe").is_file() || bin_dir.join("ffmpeg.exe").is_file();
            let has_tools = cwd.join("tools").join("ffmpeg.exe").is_file()
                || cwd.join("tools").join("mpv").join("mpv.exe").is_file()
                || cwd.join("tools").join("mpv.exe").is_file();
            if has_bin || has_tools {
                tracing::warn!(
                    "self.expand is true but this exe has no overlay; local tools already present — clearing expand flag"
                );
                write_expand_false()?;
                return Ok(Outcome::Continue);
            }
            bail!(
                "self.expand is true but {} has no packed overlay, and no tools were found under bin\\ or tools\\. Run `cargo run --release --bin pack` to create a fat build.",
                exe_path.display()
            );
        }
    };

    fs::create_dir_all(&bin_dir).with_context(|| format!("mkdir {}", bin_dir.display()))?;

    let mpv_path = bin_dir.join("mpv.exe");
    let ffmpeg_path = bin_dir.join("ffmpeg.exe");
    tracing::info!(path = %mpv_path.display(), bytes = payload.mpv.len(), "extracting mpv");
    fs::write(&mpv_path, &payload.mpv).with_context(|| format!("write {}", mpv_path.display()))?;
    tracing::info!(
        path = %ffmpeg_path.display(),
        bytes = payload.ffmpeg.len(),
        "extracting ffmpeg"
    );
    fs::write(&ffmpeg_path, &payload.ffmpeg)
        .with_context(|| format!("write {}", ffmpeg_path.display()))?;

    // Shrink in-place using the Windows rename trick: a running image can be
    // renamed, then a thin PE takes its place; delete the fat *.old after exit.
    let thin_final = cwd.join("wallfx.exe");
    let thin_tmp = cwd.join("wallfx.exe.tmp");
    let fat_old = cwd.join("wallfx.exe.old");
    tracing::info!(
        path = %thin_tmp.display(),
        bytes = payload.thin_pe.len(),
        "writing thin wallfx.exe"
    );
    fs::write(&thin_tmp, &payload.thin_pe)
        .with_context(|| format!("write {}", thin_tmp.display()))?;

    if thin_final.exists() {
        let _ = fs::remove_file(&fat_old);
        fs::rename(&thin_final, &fat_old)
            .with_context(|| format!("rename {} → {}", thin_final.display(), fat_old.display()))?;
    }
    fs::rename(&thin_tmp, &thin_final)
        .with_context(|| format!("rename {} → {}", thin_tmp.display(), thin_final.display()))?;

    write_expand_false()?;
    // wallfx.exe.old stays until the next launch (still mapped by this process).

    let thin_len = payload.thin_pe.len();
    println!(
        "Extracted tools to {}\\bin and shrank wallfx.exe to {} bytes.",
        cwd.display(),
        thin_len
    );
    println!("Run your command again.");
    Ok(Outcome::ExtractedRestart)
}

fn write_expand_false() -> Result<()> {
    let mut cfg = config::load().unwrap_or_default();
    cfg.self_.expand = false;
    config::save(&cfg)?;
    Ok(())
}
