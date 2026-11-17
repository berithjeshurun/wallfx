//! Shared settings state for Win32 and egui UIs.

use crate::config::{
    self, BackendPref, Config, FitMode, GradeSection, MonitorWallpaper, ParallaxSection,
    UiToolkit, UnlockSection, WallpaperMode, WallpaperSection,
};
use crate::monitors::{self, MonitorInfo};
use anyhow::{bail, Result};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct SettingsModel {
    pub toolkit: UiToolkit,
    pub mode: WallpaperMode,
    pub video: String,
    pub loop_playback: bool,
    pub mute: bool,
    pub preview: bool,
    pub backend: BackendPref,
    pub fit: FitMode,
    pub max_decode_w: u32,
    pub max_decode_h: u32,
    pub time_sync: bool,
    pub expand: bool,
    pub startup: bool,
    pub unlock: UnlockSection,
    pub grade: GradeSection,
    pub parallax: ParallaxSection,
    pub monitors: Vec<MonitorInfo>,
    pub selected_monitors: Vec<String>,
    pub per_monitor_video: HashMap<String, String>,
}

impl SettingsModel {
    pub fn from_config(cfg: &Config) -> Result<Self> {
        let monitors = monitors::enumerate().unwrap_or_default();
        let mut per_monitor_video = HashMap::new();
        for entry in &cfg.wallpaper.per_monitor {
            if !entry.video.trim().is_empty() {
                per_monitor_video.insert(entry.id.clone(), entry.video.clone());
            }
        }
        Ok(Self {
            toolkit: cfg.ui.toolkit,
            mode: cfg.wallpaper.mode,
            video: cfg.wallpaper.video.clone(),
            loop_playback: cfg.wallpaper.loop_playback,
            mute: cfg.wallpaper.mute,
            preview: cfg.wallpaper.preview,
            backend: cfg.wallpaper.backend,
            fit: cfg.wallpaper.fit,
            max_decode_w: cfg.wallpaper.max_decode_w,
            max_decode_h: cfg.wallpaper.max_decode_h,
            time_sync: cfg.wallpaper.time_sync,
            expand: cfg.self_.expand,
            startup: cfg.startup.with_windows,
            unlock: cfg.unlock.clone(),
            grade: cfg.grade.clone(),
            parallax: cfg.parallax.clone(),
            selected_monitors: cfg.wallpaper.monitors.clone(),
            monitors,
            per_monitor_video,
        })
    }

    pub fn load() -> Result<Self> {
        Self::from_config(&config::load()?)
    }

    pub fn apply(&self) -> Result<Config> {
        if !self.video.trim().is_empty() {
            let p = std::path::Path::new(self.video.trim());
            let ok = p.is_file() || p.is_dir();
            if !ok {
                bail!("Wallpaper source not found: {}", self.video);
            }
        }

        if self.grade.enabled && !self.grade.lut.trim().is_empty() {
            let lut = std::path::Path::new(self.grade.lut.trim());
            if !lut.is_file() {
                bail!("LUT not found: {}", self.grade.lut);
            }
        }

        let mut per_monitor = Vec::new();
        for (id, video) in &self.per_monitor_video {
            let v = video.trim();
            if !v.is_empty() {
                let p = std::path::Path::new(v);
                if !(p.is_file() || p.is_dir()) {
                    bail!("Per-monitor source not found ({id}): {v}");
                }
                per_monitor.push(MonitorWallpaper {
                    id: id.clone(),
                    video: v.to_string(),
                });
            }
        }

        let mut cfg = config::load().unwrap_or_default();
        cfg.ui.toolkit = self.toolkit;
        cfg.self_.expand = self.expand;
        cfg.startup.with_windows = self.startup;
        cfg.unlock = self.unlock.clone();
        cfg.grade = self.grade.clone();
        cfg.parallax = self.parallax.clone();
        cfg.wallpaper = WallpaperSection {
            mode: self.mode,
            video: self.video.trim().to_string(),
            loop_playback: self.loop_playback,
            mute: self.mute,
            preview: self.preview,
            enabled: cfg.wallpaper.enabled,
            backend: self.backend,
            fit: self.fit,
            max_decode_w: self.max_decode_w,
            max_decode_h: self.max_decode_h,
            monitors: self.selected_monitors.clone(),
            time_sync: self.time_sync,
            per_monitor,
        };
        config::save(&cfg)?;
        crate::startup::set_enabled(self.startup)?;
        Ok(cfg)
    }

    pub fn monitor_checked(&self, id: &str) -> bool {
        self.selected_monitors.is_empty() || self.selected_monitors.iter().any(|s| s == id)
    }

    pub fn set_monitor_checked(&mut self, id: &str, checked: bool) {
        if self.selected_monitors.is_empty() {
            self.selected_monitors = self.monitors.iter().map(|m| m.id.clone()).collect();
        }
        if checked {
            if !self.selected_monitors.iter().any(|s| s == id) {
                self.selected_monitors.push(id.to_string());
            }
        } else {
            self.selected_monitors.retain(|s| s != id);
        }
        if self.selected_monitors.len() == self.monitors.len() {
            self.selected_monitors.clear();
        }
    }

    pub fn per_monitor_path(&self, id: &str) -> String {
        self.per_monitor_video.get(id).cloned().unwrap_or_default()
    }

    pub fn set_per_monitor_path(&mut self, id: &str, path: String) {
        if path.trim().is_empty() {
            self.per_monitor_video.remove(id);
        } else {
            self.per_monitor_video.insert(id.to_string(), path);
        }
    }
}
