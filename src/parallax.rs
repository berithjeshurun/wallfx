//! Depth-map parallax wallpapers (color + depth image / zip packs).

use crate::window::WallpaperWindow;
use anyhow::{bail, Context, Result};
use image::imageops::FilterType;
use image::{DynamicImage, GrayImage, RgbaImage};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetCursorPos, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
};

#[derive(Debug, Clone)]
pub struct ParallaxOptions {
    pub strength: f32,
    pub invert_depth: bool,
    pub desktop: bool,
}

impl Default for ParallaxOptions {
    fn default() -> Self {
        Self {
            strength: 0.045,
            invert_depth: false,
            desktop: true,
        }
    }
}

pub struct ParallaxSurface {
    window: WallpaperWindow,
    color: RgbaImage,
    depth: GrayImage,
    src_w: u32,
    src_h: u32,
    strength: f32,
    invert_depth: bool,
    desktop: bool,
    frame: Vec<u8>,
    alpha: u8,
    last_mx: f32,
    last_my: f32,
    ticks: u64,
}

impl ParallaxSurface {
    pub fn start(source: &Path, opts: &ParallaxOptions, window: WallpaperWindow) -> Result<Self> {
        let (color_path, depth_path) = resolve_pair(source)?;
        tracing::info!(
            color = %color_path.display(),
            depth = %depth_path.display(),
            "loading parallax pack"
        );

        let color_img = image::open(&color_path)
            .with_context(|| format!("open color {}", color_path.display()))?;
        let depth_img = image::open(&depth_path)
            .with_context(|| format!("open depth {}", depth_path.display()))?;

        let tw = window.width.max(1) as u32;
        let th = window.height.max(1) as u32;
        let color = color_img
            .resize_exact(tw, th, FilterType::Triangle)
            .to_rgba8();
        let depth = depth_img
            .resize_exact(tw, th, FilterType::Triangle)
            .to_luma8();

        let mut surface = Self {
            window,
            src_w: color.width(),
            src_h: color.height(),
            frame: vec![0u8; (tw as usize) * (th as usize) * 4],
            color,
            depth,
            strength: opts.strength.clamp(0.0, 0.25),
            invert_depth: opts.invert_depth,
            desktop: opts.desktop,
            alpha: 255,
            last_mx: 0.0,
            last_my: 0.0,
            ticks: 0,
        };
        surface.render(0.0, 0.0);
        surface.blit()?;
        Ok(surface)
    }

    pub fn poll(&mut self) -> Result<bool> {
        pump_messages_once();
        let (mx, my) = mouse_offset_for_hwnd(self.window.hwnd, self.window.width, self.window.height);
        let moved = (mx - self.last_mx).abs() > 0.002 || (my - self.last_my).abs() > 0.002;
        if moved || self.ticks == 0 {
            self.render(mx, my);
            self.blit()?;
            self.last_mx = mx;
            self.last_my = my;
        }
        self.ticks = self.ticks.wrapping_add(1);
        if self.desktop && self.ticks % 120 == 0 {
            let _ = crate::desktop::refresh_zorder(self.window.hwnd);
        }
        Ok(true)
    }

    pub fn set_paused(&mut self, _paused: bool) {}

    pub fn set_alpha(&mut self, alpha: u8) -> Result<()> {
        self.alpha = alpha;
        self.blit()
    }

    fn render(&mut self, mx: f32, my: f32) {
        let w = self.src_w as i32;
        let h = self.src_h as i32;
        let strength = self.strength;
        let invert = self.invert_depth;
        let max_x = (w - 1) as f32;
        let max_y = (h - 1) as f32;

        for y in 0..h {
            for x in 0..w {
                let di = (y as u32 * self.src_w + x as u32) as usize;
                let mut d = self.depth.as_raw()[di] as f32 / 255.0;
                if invert {
                    d = 1.0 - d;
                }
                // Near (bright) moves more with the cursor.
                let amp = (d - 0.5) * 2.0;
                let sx = (x as f32 + mx * strength * amp * w as f32).clamp(0.0, max_x);
                let sy = (y as f32 + my * strength * amp * h as f32).clamp(0.0, max_y);
                let px = sample_rgba(&self.color, self.src_w, self.src_h, sx, sy);
                let oi = di * 4;
                // BGRA for GDI
                self.frame[oi] = px[2];
                self.frame[oi + 1] = px[1];
                self.frame[oi + 2] = px[0];
                self.frame[oi + 3] = 255;
            }
        }
    }

    fn blit(&self) -> Result<()> {
        crate::layered_blit::present_bgra(
            self.window.hwnd,
            &self.frame,
            self.src_w as i32,
            self.src_h as i32,
            self.alpha,
        )
    }
}

impl Drop for ParallaxSurface {
    fn drop(&mut self) {
        crate::desktop::detach_from_desktop(self.window.hwnd);
    }
}

fn sample_rgba(img: &RgbaImage, w: u32, h: u32, x: f32, y: f32) -> [u8; 4] {
    let x0 = x.floor() as i32;
    let y0 = y.floor() as i32;
    let x1 = (x0 + 1).min(w as i32 - 1);
    let y1 = (y0 + 1).min(h as i32 - 1);
    let x0 = x0.clamp(0, w as i32 - 1);
    let y0 = y0.clamp(0, h as i32 - 1);
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;
    let p00 = img.get_pixel(x0 as u32, y0 as u32).0;
    let p10 = img.get_pixel(x1 as u32, y0 as u32).0;
    let p01 = img.get_pixel(x0 as u32, y1 as u32).0;
    let p11 = img.get_pixel(x1 as u32, y1 as u32).0;
    let mut out = [0u8; 4];
    for i in 0..4 {
        let v = p00[i] as f32 * (1.0 - fx) * (1.0 - fy)
            + p10[i] as f32 * fx * (1.0 - fy)
            + p01[i] as f32 * (1.0 - fx) * fy
            + p11[i] as f32 * fx * fy;
        out[i] = v.round().clamp(0.0, 255.0) as u8;
    }
    out
}

fn mouse_offset_for_hwnd(hwnd: HWND, width: i32, height: i32) -> (f32, f32) {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    // Approximate: use window size; cursor relative to primary for simplicity —
    // map against virtual desktop by using window client origin via ScreenToClient.
    let mut local = pt;
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut local as *mut _);
    }
    let w = width.max(1) as f32;
    let h = height.max(1) as f32;
    let mx = ((local.x as f32 / w) * 2.0 - 1.0).clamp(-1.0, 1.0);
    let my = ((local.y as f32 / h) * 2.0 - 1.0).clamp(-1.0, 1.0);
    (mx, my)
}

/// Resolve a parallax source (zip / folder / color image) into (color, depth) paths.
pub fn resolve_pair(source: &Path) -> Result<(PathBuf, PathBuf)> {
    let source = if source.is_absolute() {
        source.to_path_buf()
    } else {
        std::env::current_dir()?.join(source)
    };

    if source
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
    {
        let dir = extract_zip(&source)?;
        return find_pair_in_dir(&dir);
    }

    if source.is_dir() {
        return find_pair_in_dir(&source);
    }

    if source.is_file() {
        // color image next to *-depth.* sibling
        if let Some(depth) = sibling_depth(&source) {
            return Ok((source, depth));
        }
        bail!(
            "No depth map next to {}. Expected a *-depth.jpg/png sibling or a zip pack.",
            source.display()
        );
    }

    bail!("Parallax source not found: {}", source.display());
}

fn sibling_depth(color: &Path) -> Option<PathBuf> {
    let stem = color.file_stem()?.to_string_lossy();
    if is_depth_filename(color.file_name()?.to_str()?) {
        return None;
    }
    let parent = color.parent()?;
    let ext = color.extension()?.to_string_lossy();
    for cand in [
        format!("{stem}-depth.{ext}"),
        format!("{stem}_depth.{ext}"),
        format!("{stem}-depth.jpg"),
        format!("{stem}-depth.png"),
        format!("{stem}_depth.jpg"),
        format!("{stem}_depth.png"),
    ] {
        let p = parent.join(cand);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn is_depth_filename(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let stem = Path::new(&lower)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&lower);
    // Match only a trailing -depth / _depth suffix (not "Depth-Parallax" in the title).
    stem.ends_with("-depth") || stem.ends_with("_depth")
}

fn find_pair_in_dir(dir: &Path) -> Result<(PathBuf, PathBuf)> {
    let mut depths = Vec::new();
    let mut colors = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let lower = name.to_ascii_lowercase();
        if !(lower.ends_with(".jpg")
            || lower.ends_with(".jpeg")
            || lower.ends_with(".png")
            || lower.ends_with(".webp"))
        {
            continue;
        }
        if is_depth_filename(name) {
            depths.push(path);
        } else {
            colors.push(path);
        }
    }
    if depths.is_empty() {
        bail!("No depth map (*-depth.jpg) in {}", dir.display());
    }
    if colors.is_empty() {
        bail!("No color image in {}", dir.display());
    }
    // Prefer matching stem pairs.
    for depth in &depths {
        let dname = depth.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let base = if let Some(b) = dname
            .strip_suffix("-depth")
            .or_else(|| dname.strip_suffix("_depth"))
            .or_else(|| dname.strip_suffix("-Depth"))
            .or_else(|| dname.strip_suffix("_Depth"))
        {
            b
        } else {
            dname
        };
        if let Some(color) = colors.iter().find(|c| {
            c.file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case(base))
        }) {
            return Ok((color.clone(), depth.clone()));
        }
    }
    Ok((colors[0].clone(), depths[0].clone()))
}

fn extract_zip(zip_path: &Path) -> Result<PathBuf> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    zip_path.hash(&mut hasher);
    if let Ok(meta) = std::fs::metadata(zip_path) {
        meta.len().hash(&mut hasher);
        if let Ok(modified) = meta.modified() {
            if let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH) {
                dur.as_nanos().hash(&mut hasher);
            }
        }
    }
    let out = std::env::current_dir()?
        .join("cache")
        .join("parallax")
        .join(format!("{:016x}", hasher.finish()));
    if out.is_dir() && find_pair_in_dir(&out).is_ok() {
        return Ok(out);
    }
    std::fs::create_dir_all(&out).context("mkdir cache/parallax")?;

    let file = File::open(zip_path).with_context(|| format!("open {}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file).context("read zip")?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).context("zip entry")?;
        let name = file
            .name()
            .map_err(|e| anyhow::anyhow!("zip name: {e}"))?
            .into_owned();
        if name.ends_with('/') {
            continue;
        }
        let out_path = out.join(
            Path::new(&name)
                .file_name()
                .unwrap_or_else(|| Path::new(&name).as_os_str()),
        );
        let mut outfile = File::create(&out_path)
            .with_context(|| format!("create {}", out_path.display()))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        outfile.write_all(&buf)?;
    }
    Ok(out)
}

pub fn looks_like_parallax(path: &Path) -> bool {
    let p = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|c| c.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    if p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
    {
        return true;
    }
    if p.is_dir() {
        return find_pair_in_dir(&p).is_ok();
    }
    sibling_depth(&p).is_some()
}

fn pump_messages_once() {
    unsafe {
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[allow(dead_code)]
fn _keep_dynamic(_: &DynamicImage) {}
