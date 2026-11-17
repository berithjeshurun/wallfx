mod config;
mod desktop;
mod dialog;
mod engine;
mod expand;
mod framebuf;
mod grade;
mod instance;
mod layered_blit;
mod monitors;
mod parallax;
mod player;
mod session;
mod settings_egui;
mod settings_model;
mod settings_win32;
mod startup;
mod tray;
mod window;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use instance::{InstanceGuard, request_stop, stop_requested};
use player::Player;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};

#[derive(Parser, Debug)]
#[command(name = "wallfx", version, about = "Safe live wallpaper engine for Windows")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run in the system tray (default when no command is given)
    Tray,
    /// Set a local video as the live wallpaper (one-shot CLI)
    Set {
        /// Path to a video file (mp4/webm/mkv/…)
        video: PathBuf,
        /// Keep video audio (muted by default)
        #[arg(long, default_value_t = false)]
        audio: bool,
        /// Show as a normal fullscreen window instead of desktop wallpaper (debug)
        #[arg(long, default_value_t = false)]
        preview: bool,
    },
    /// Stop the running wallpaper and restore the desktop
    Stop,
    /// Show runtime status
    Status,
}

fn main() {
    init_dpi();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    if let Err(err) = run() {
        tracing::error!("{err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let cfg = config::load().context("load config.yaml")?;
    if matches!(
        expand::maybe_expand(&cfg).context("self-extract expand")?,
        expand::Outcome::ExtractedRestart
    ) {
        return Ok(());
    }

    match cli.command {
        None | Some(Commands::Tray) => tray::run(),
        Some(Commands::Set {
            video,
            audio,
            preview,
        }) => cmd_set(video, !audio, !preview),
        Some(Commands::Stop) => {
            request_stop()?;
            println!("Stop signal sent.");
            Ok(())
        }
        Some(Commands::Status) => cmd_status(),
    }
}

fn cmd_set(video: PathBuf, muted: bool, desktop: bool) -> Result<()> {
    let video = resolve_video(&video)?;
    let _guard = InstanceGuard::acquire()?;

    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = Arc::clone(&stop);
        ctrlc::set_handler(move || {
            stop.store(true, Ordering::SeqCst);
            let _ = request_stop();
        })
        .context("install Ctrl+C handler")?;
    }

    tracing::info!(path = %video.display(), muted, desktop, "starting wallpaper");
    let mut player = Player::start(&video, muted, desktop)?;
    if desktop {
        println!("Wallpaper running. Minimize windows to see the desktop. Ctrl+C or `wallfx stop` to quit.");
    } else {
        println!("Preview window running. Ctrl+C or `wallfx stop` to quit.");
    }

    while !stop.load(Ordering::SeqCst) && !stop_requested() {
        if !player.poll()? {
            anyhow::bail!("wallpaper player process exited unexpectedly");
        }
        std::thread::sleep(std::time::Duration::from_millis(16));
    }

    drop(player);
    tracing::info!("wallpaper stopped; desktop detached");
    Ok(())
}

fn cmd_status() -> Result<()> {
    let pid_file = instance::pid_path()?;
    if pid_file.exists() {
        let pid = std::fs::read_to_string(&pid_file).unwrap_or_default();
        println!("running (pid {})", pid.trim());
    } else {
        println!("not running");
    }
    Ok(())
}

fn resolve_video(path: &PathBuf) -> Result<PathBuf> {
    let p = if path.is_absolute() {
        path.clone()
    } else {
        std::env::current_dir()?.join(path)
    };
    let canonical = std::fs::canonicalize(&p).with_context(|| format!("resolve {}", p.display()))?;
    Ok(strip_extended_path(canonical))
}

fn strip_extended_path(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path
    }
}

fn init_dpi() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}
