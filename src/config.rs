//! `%CD%\config.yaml` — self-extract + wallpaper / UI settings.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum UiToolkit {
    #[default]
    Egui,
    Win32,
}

impl UiToolkit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Egui => "egui",
            Self::Win32 => "win32",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Egui => "egui",
            Self::Win32 => "Native Win32",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BackendPref {
    #[default]
    Default,
    Auto,
    Mpv,
    Ffmpeg,
}

impl BackendPref {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Auto => "auto",
            Self::Mpv => "mpv",
            Self::Ffmpeg => "ffmpeg",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Auto => "Auto",
            Self::Mpv => "mpv",
            Self::Ffmpeg => "ffmpeg",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WallpaperMode {
    #[default]
    Video,
    Parallax,
    /// Pick video vs parallax from the file (zip/depth pair → parallax)
    Auto,
}

impl WallpaperMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Video => "Video",
            Self::Parallax => "Parallax",
            Self::Auto => "Auto",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallaxSection {
    #[serde(default = "default_parallax_strength")]
    pub strength: f32,
    #[serde(default)]
    pub invert_depth: bool,
}

impl Default for ParallaxSection {
    fn default() -> Self {
        Self {
            strength: 0.045,
            invert_depth: false,
        }
    }
}

fn default_parallax_strength() -> f32 {
    0.045
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FitMode {
    #[default]
    Fill,
    Contain,
    Stretch,
    Native,
}

impl FitMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fill => "fill",
            Self::Contain => "contain",
            Self::Stretch => "stretch",
            Self::Native => "native",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Fill => "Fill",
            Self::Contain => "Contain",
            Self::Stretch => "Stretch",
            Self::Native => "Native",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UiSection {
    #[serde(default)]
    pub toolkit: UiToolkit,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SelfSection {
    #[serde(default)]
    pub expand: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MonitorWallpaper {
    /// Monitor id (from enumerate) or device name
    pub id: String,
    /// Override video for this monitor (empty = use wallpaper.video)
    #[serde(default)]
    pub video: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GradeSection {
    #[serde(default)]
    pub enabled: bool,
    /// ffmpeg `eq` brightness (−1…1), default 0
    #[serde(default)]
    pub brightness: f32,
    #[serde(default = "default_one")]
    pub contrast: f32,
    #[serde(default = "default_one")]
    pub saturation: f32,
    #[serde(default = "default_one")]
    pub gamma: f32,
    /// Path to a .cube / .3dl LUT (optional)
    #[serde(default)]
    pub lut: String,
    /// If true, bake graded MP4 into cache\\graded before playback
    #[serde(default)]
    pub bake: bool,
}

impl Default for GradeSection {
    fn default() -> Self {
        Self {
            enabled: false,
            brightness: 0.0,
            contrast: 1.0,
            saturation: 1.0,
            gamma: 1.0,
            lut: String::new(),
            bake: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WallpaperSection {
    #[serde(default)]
    pub mode: WallpaperMode,
    #[serde(default)]
    pub video: String,
    #[serde(default = "default_true")]
    pub loop_playback: bool,
    #[serde(default = "default_true")]
    pub mute: bool,
    #[serde(default)]
    pub preview: bool,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub backend: BackendPref,
    #[serde(default)]
    pub fit: FitMode,
    /// 0 = auto from screen / defaults
    #[serde(default)]
    pub max_decode_w: u32,
    #[serde(default)]
    pub max_decode_h: u32,
    /// Empty = all monitors
    #[serde(default)]
    pub monitors: Vec<String>,
    #[serde(default = "default_true")]
    pub time_sync: bool,
    /// Optional per-monitor video overrides
    #[serde(default)]
    pub per_monitor: Vec<MonitorWallpaper>,
}

impl Default for WallpaperSection {
    fn default() -> Self {
        Self {
            mode: WallpaperMode::Auto,
            video: String::new(),
            loop_playback: true,
            mute: true,
            preview: false,
            enabled: false,
            backend: BackendPref::Default,
            fit: FitMode::Fill,
            max_decode_w: 0,
            max_decode_h: 0,
            monitors: Vec::new(),
            time_sync: true,
            per_monitor: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartupSection {
    #[serde(default)]
    pub with_windows: bool,
}

impl Default for StartupSection {
    fn default() -> Self {
        Self {
            with_windows: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockSection {
    #[serde(default = "default_true")]
    pub pause_when_locked: bool,
    #[serde(default = "default_true")]
    pub fade_on_unlock: bool,
    #[serde(default = "default_fade_ms")]
    pub fade_ms: u32,
}

impl Default for UnlockSection {
    fn default() -> Self {
        Self {
            pause_when_locked: true,
            fade_on_unlock: true,
            fade_ms: 400,
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_one() -> f32 {
    1.0
}
fn default_fade_ms() -> u32 {
    400
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub self_: SelfSection,
    pub ui: UiSection,
    pub wallpaper: WallpaperSection,
    pub grade: GradeSection,
    pub parallax: ParallaxSection,
    pub startup: StartupSection,
    pub unlock: UnlockSection,
}

impl Config {
    pub fn expand(&self) -> bool {
        self.self_.expand
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct ConfigFile {
    #[serde(rename = "self", default)]
    self_: SelfSection,
    #[serde(default)]
    ui: UiSection,
    #[serde(default)]
    wallpaper: WallpaperSection,
    #[serde(default)]
    grade: GradeSection,
    #[serde(default)]
    parallax: ParallaxSection,
    #[serde(default)]
    startup: StartupSection,
    #[serde(default)]
    unlock: UnlockSection,
}

impl From<ConfigFile> for Config {
    fn from(f: ConfigFile) -> Self {
        Self {
            self_: f.self_,
            ui: f.ui,
            wallpaper: f.wallpaper,
            grade: f.grade,
            parallax: f.parallax,
            startup: f.startup,
            unlock: f.unlock,
        }
    }
}

impl From<&Config> for ConfigFile {
    fn from(c: &Config) -> Self {
        Self {
            self_: c.self_.clone(),
            ui: c.ui.clone(),
            wallpaper: c.wallpaper.clone(),
            grade: c.grade.clone(),
            parallax: c.parallax.clone(),
            startup: c.startup.clone(),
            unlock: c.unlock.clone(),
        }
    }
}

pub fn config_path() -> Result<PathBuf> {
    Ok(std::env::current_dir()?.join("config.yaml"))
}

pub fn load() -> Result<Config> {
    let path = config_path()?;
    if !path.is_file() {
        return Ok(Config::default());
    }
    let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
    let file: ConfigFile =
        serde_yaml::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    Ok(file.into())
}

pub fn save(cfg: &Config) -> Result<()> {
    save_at(&config_path()?, cfg)
}

pub fn save_at(path: &Path, cfg: &Config) -> Result<()> {
    let file = ConfigFile::from(cfg);
    let text = serde_yaml::to_string(&file).context("serialize config.yaml")?;
    fs::write(path, text).with_context(|| format!("write {}", path.display()))?;
    Ok(())
}
