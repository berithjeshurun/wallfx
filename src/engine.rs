//! Multi-monitor wallpaper engine driven by config.

use crate::config::{BackendPref, Config, FitMode, MonitorWallpaper, WallpaperMode};
use crate::monitors::{self, MonitorInfo};
use crate::parallax::{self, ParallaxOptions, ParallaxSurface};
use crate::player::{PlaybackOptions, Player};
use crate::window::WallpaperWindow;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use windows::Win32::UI::WindowsAndMessaging::SetLayeredWindowAttributes;
use windows::Win32::UI::WindowsAndMessaging::LWA_ALPHA;

enum Surface {
    Video(Player),
    Parallax(ParallaxSurface),
}

impl Surface {
    fn poll(&mut self) -> Result<bool> {
        match self {
            Self::Video(p) => p.poll(),
            Self::Parallax(p) => p.poll(),
        }
    }

    fn set_paused(&mut self, paused: bool) {
        match self {
            Self::Video(p) => p.set_paused(paused),
            Self::Parallax(p) => p.set_paused(paused),
        }
    }

    fn set_alpha(&mut self, alpha: u8) -> Result<()> {
        match self {
            Self::Video(p) => p.set_alpha(alpha),
            Self::Parallax(p) => p.set_alpha(alpha),
        }
    }
}

pub struct Engine {
    surfaces: Vec<Surface>,
    locked: bool,
    fade: Option<FadeState>,
    fade_ms: u32,
    pause_when_locked: bool,
    fade_on_unlock: bool,
}

struct FadeState {
    start: Instant,
    duration: Duration,
}

impl Engine {
    pub fn start_from_config(cfg: &Config) -> Result<Self> {
        let default_source = cfg.wallpaper.video.trim();
        let monitors = monitors::selected(&cfg.wallpaper.monitors)?;
        if monitors.is_empty() {
            bail!("No monitors available");
        }

        for mon in &monitors {
            let path = source_for_monitor(default_source, &cfg.wallpaper.per_monitor, mon)?;
            if path.trim().is_empty() {
                bail!(
                    "No wallpaper source for monitor {} — set a default path or per-monitor override.",
                    mon.name
                );
            }
        }

        let video_opts = PlaybackOptions {
            muted: cfg.wallpaper.mute,
            loop_playback: cfg.wallpaper.loop_playback,
            backend: cfg.wallpaper.backend,
            fit: cfg.wallpaper.fit,
            max_decode_w: cfg.wallpaper.max_decode_w,
            max_decode_h: cfg.wallpaper.max_decode_h,
            desktop: !cfg.wallpaper.preview,
            allow_path_fallback: matches!(cfg.wallpaper.backend, BackendPref::Auto),
            grade: cfg.grade.clone(),
        };
        let parallax_opts = ParallaxOptions {
            strength: cfg.parallax.strength,
            invert_depth: cfg.parallax.invert_depth,
            desktop: !cfg.wallpaper.preview,
        };

        let mut prepared = Vec::new();
        for mon in &monitors {
            let source_str = source_for_monitor(default_source, &cfg.wallpaper.per_monitor, mon)?;
            let source = resolve_path(&PathBuf::from(&source_str))?;
            let win = WallpaperWindow::create_at(mon.rect)?;
            if !cfg.wallpaper.preview {
                crate::desktop::attach_to_desktop(win.hwnd, mon.rect)?;
            }
            prepared.push((source, win));
        }

        let mut surfaces = Vec::with_capacity(prepared.len());
        for (source, win) in prepared {
            let use_parallax = match cfg.wallpaper.mode {
                WallpaperMode::Parallax => true,
                WallpaperMode::Video => false,
                WallpaperMode::Auto => parallax::looks_like_parallax(&source),
            };
            tracing::info!(
                path = %source.display(),
                parallax = use_parallax,
                "starting monitor surface"
            );
            if use_parallax {
                surfaces.push(Surface::Parallax(ParallaxSurface::start(
                    &source,
                    &parallax_opts,
                    win,
                )?));
            } else {
                surfaces.push(Surface::Video(Player::start_on_window(
                    &source,
                    &video_opts,
                    win,
                )?));
            }
        }

        Ok(Self {
            surfaces,
            locked: false,
            fade: None,
            fade_ms: cfg.unlock.fade_ms.max(1),
            pause_when_locked: cfg.unlock.pause_when_locked,
            fade_on_unlock: cfg.unlock.fade_on_unlock,
        })
    }

    pub fn poll(&mut self) -> Result<bool> {
        self.tick_fade();
        let mut alive = true;
        for s in &mut self.surfaces {
            if !s.poll()? {
                alive = false;
            }
        }
        Ok(alive)
    }

    pub fn set_session_locked(&mut self, locked: bool) {
        if self.locked == locked {
            return;
        }
        self.locked = locked;
        if locked {
            if self.pause_when_locked {
                for s in &mut self.surfaces {
                    s.set_paused(true);
                    let _ = s.set_alpha(0);
                }
            }
            self.fade = None;
        } else if self.fade_on_unlock {
            for s in &mut self.surfaces {
                s.set_paused(false);
                let _ = s.set_alpha(0);
            }
            self.fade = Some(FadeState {
                start: Instant::now(),
                duration: Duration::from_millis(self.fade_ms as u64),
            });
        } else {
            for s in &mut self.surfaces {
                s.set_paused(false);
                let _ = s.set_alpha(255);
            }
        }
    }

    fn tick_fade(&mut self) {
        let Some(fade) = &self.fade else {
            return;
        };
        let t = fade.start.elapsed().as_secs_f32() / fade.duration.as_secs_f32().max(0.001);
        let alpha = (t.clamp(0.0, 1.0) * 255.0) as u8;
        for s in &mut self.surfaces {
            let _ = s.set_alpha(alpha);
        }
        if t >= 1.0 {
            self.fade = None;
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.surfaces.clear();
    }
}

fn source_for_monitor(
    default_source: &str,
    per: &[MonitorWallpaper],
    mon: &MonitorInfo,
) -> Result<String> {
    if let Some(entry) = per.iter().find(|p| p.id == mon.id || p.id == mon.name) {
        let v = entry.video.trim();
        if !v.is_empty() {
            return Ok(v.to_string());
        }
    }
    Ok(default_source.to_string())
}

fn resolve_path(path: &Path) -> Result<PathBuf> {
    let p = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    if p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
        || p.is_dir()
    {
        return Ok(p);
    }
    let canonical = std::fs::canonicalize(&p).with_context(|| format!("resolve {}", p.display()))?;
    let s = canonical.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        Ok(PathBuf::from(rest))
    } else {
        Ok(canonical)
    }
}

#[allow(dead_code)]
pub fn set_window_alpha(hwnd: windows::Win32::Foundation::HWND, alpha: u8) -> Result<()> {
    unsafe {
        SetLayeredWindowAttributes(
            hwnd,
            windows::Win32::Foundation::COLORREF(0),
            alpha,
            LWA_ALPHA,
        )
        .context("SetLayeredWindowAttributes")?;
    }
    Ok(())
}

#[allow(dead_code)]
fn _fit(_: FitMode) {}
