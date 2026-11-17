//! Wallpaper playback: prefer mpv (HWND embed + HW decode), else ffmpeg → UI-thread GDI.

use crate::config::{BackendPref, FitMode, GradeSection};
use crate::framebuf::FrameBuffer;
use crate::grade;
use crate::window::WallpaperWindow;
use anyhow::{Context, Result, bail};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject, SetDIBits, StretchBlt, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    SRCCOPY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
};

#[derive(Debug, Clone)]
pub struct PlaybackOptions {
    pub muted: bool,
    pub loop_playback: bool,
    pub backend: BackendPref,
    pub fit: FitMode,
    pub max_decode_w: u32,
    pub max_decode_h: u32,
    pub desktop: bool,
    pub allow_path_fallback: bool,
    pub grade: GradeSection,
}

impl Default for PlaybackOptions {
    fn default() -> Self {
        Self {
            muted: true,
            loop_playback: true,
            backend: BackendPref::Default,
            fit: FitMode::Fill,
            max_decode_w: 0,
            max_decode_h: 0,
            desktop: true,
            allow_path_fallback: false,
            grade: GradeSection::default(),
        }
    }
}

pub struct Player {
    pub window: WallpaperWindow,
    backend: Backend,
    frames: FrameBuffer,
    desktop: bool,
    fit: FitMode,
    ticks: u64,
    paused: bool,
}

enum Backend {
    Mpv {
        child: Child,
    },
    Ffmpeg {
        child: Child,
        pump: Option<JoinHandle<()>>,
        stop: Arc<AtomicBool>,
        paused: Arc<AtomicBool>,
    },
}

impl Player {
    pub fn start(video: &Path, muted: bool, desktop: bool) -> Result<Self> {
        let opts = PlaybackOptions {
            muted,
            desktop,
            ..PlaybackOptions::default()
        };
        let window = WallpaperWindow::create_primary()?;
        if desktop {
            let rect = windows::Win32::Foundation::RECT {
                left: 0,
                top: 0,
                right: window.width,
                bottom: window.height,
            };
            crate::desktop::attach_to_desktop(window.hwnd, rect)?;
        }
        Self::start_on_window(video, &opts, window)
    }

    pub fn start_on_window(
        video: &Path,
        opts: &PlaybackOptions,
        window: WallpaperWindow,
    ) -> Result<Self> {
        if !video.is_file() {
            bail!("Video not found: {}", video.display());
        }

        let frames = FrameBuffer::new();
        let backend = select_backend(opts, window.hwnd, video, &frames, window.width, window.height)?;

        Ok(Self {
            window,
            backend,
            frames,
            desktop: opts.desktop,
            fit: opts.fit,
            ticks: 0,
            paused: false,
        })
    }

    pub fn poll(&mut self) -> Result<bool> {
        pump_messages_once();
        if !self.paused {
            if let Some(frame) = self.frames.take_latest() {
                if let Err(err) = blit_bgra(
                    self.window.hwnd,
                    &frame.bgra,
                    frame.width,
                    frame.height,
                    self.fit,
                ) {
                    tracing::warn!(error = %err, "blit failed");
                }
            }
        }
        self.ticks = self.ticks.wrapping_add(1);
        if self.desktop && self.ticks % 120 == 0 {
            let _ = crate::desktop::refresh_zorder(self.window.hwnd);
        }
        Ok(self.alive())
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        if let Backend::Ffmpeg { paused: flag, .. } = &mut self.backend {
            flag.store(paused, Ordering::SeqCst);
        }
        // mpv: visibility handled via layered alpha in the engine.
    }

    pub fn set_alpha(&self, alpha: u8) -> Result<()> {
        self.window.set_alpha(alpha)
    }

    fn alive(&mut self) -> bool {
        match &mut self.backend {
            Backend::Mpv { child } | Backend::Ffmpeg { child, .. } => {
                matches!(child.try_wait(), Ok(None))
            }
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        crate::desktop::detach_from_desktop(self.window.hwnd);
        match &mut self.backend {
            Backend::Mpv { child } => {
                let _ = child.kill();
                let _ = child.wait();
            }
            Backend::Ffmpeg { child, pump, stop, .. } => {
                stop.store(true, Ordering::SeqCst);
                let _ = child.kill();
                let _ = child.wait();
                if let Some(handle) = pump.take() {
                    let _ = handle.join();
                }
            }
        }
    }
}

fn select_backend(
    opts: &PlaybackOptions,
    hwnd: HWND,
    video: &Path,
    frames: &FrameBuffer,
    screen_w: i32,
    screen_h: i32,
) -> Result<Backend> {
    let prefer_path = opts.allow_path_fallback;
    match opts.backend {
        BackendPref::Mpv => {
            let mpv = find_mpv(prefer_path).context("mpv.exe not found")?;
            Ok(Backend::Mpv {
                child: spawn_mpv(&mpv, hwnd, video, opts)?,
            })
        }
        BackendPref::Ffmpeg => {
            let ffmpeg = find_ffmpeg(prefer_path).context("ffmpeg.exe not found")?;
            spawn_ffmpeg_backend(&ffmpeg, opts, video, frames, screen_w, screen_h)
        }
        BackendPref::Default | BackendPref::Auto => {
            if let Some(mpv) = find_mpv(prefer_path) {
                tracing::info!(path = %mpv.display(), "using mpv backend");
                Ok(Backend::Mpv {
                    child: spawn_mpv(&mpv, hwnd, video, opts)?,
                })
            } else if let Some(ffmpeg) = find_ffmpeg(prefer_path) {
                tracing::warn!(
                    path = %ffmpeg.display(),
                    "mpv not found — using ffmpeg software fallback (higher CPU)"
                );
                spawn_ffmpeg_backend(&ffmpeg, opts, video, frames, screen_w, screen_h)
            } else {
                bail!(
                    "No player backend found. Place mpv.exe or ffmpeg.exe under .\\bin\\ (or .\\tools\\)"
                );
            }
        }
    }
}

fn spawn_ffmpeg_backend(
    ffmpeg: &Path,
    opts: &PlaybackOptions,
    video: &Path,
    frames: &FrameBuffer,
    screen_w: i32,
    screen_h: i32,
) -> Result<Backend> {
    let play = grade::resolve_play_path(Some(ffmpeg), video, &opts.grade)?;
    let live_vf = if opts.grade.bake {
        None
    } else {
        grade::filter_graph(&opts.grade)
    };
    let (dw, dh) = decode_size(
        screen_w,
        screen_h,
        opts.max_decode_w,
        opts.max_decode_h,
    );
    tracing::info!(decode_w = dw, decode_h = dh, "ffmpeg decode size");
    let (child, pump, stop, paused) = spawn_ffmpeg_frames(
        ffmpeg,
        dw,
        dh,
        &play,
        frames.clone(),
        opts.loop_playback,
        live_vf.as_deref(),
    )?;
    Ok(Backend::Ffmpeg {
        child,
        pump: Some(pump),
        stop,
        paused,
    })
}

fn decode_size(screen_w: i32, screen_h: i32, max_w: u32, max_h: u32) -> (i32, i32) {
    let max_edge = if max_w > 0 || max_h > 0 {
        max_w.max(max_h).max(2) as i32
    } else {
        1280
    };
    let longest = screen_w.max(screen_h).max(1);
    if longest <= max_edge {
        return (screen_w, screen_h);
    }
    let scale = max_edge as f64 / longest as f64;
    let w = ((screen_w as f64 * scale) as i32).max(2) & !1;
    let h = ((screen_h as f64 * scale) as i32).max(2) & !1;
    (w, h)
}

fn spawn_mpv(mpv: &Path, hwnd: HWND, video: &Path, opts: &PlaybackOptions) -> Result<Child> {
    let ffmpeg = find_ffmpeg(opts.allow_path_fallback);
    let play = grade::resolve_play_path(ffmpeg.as_deref(), video, &opts.grade)?;
    let wid = format!("{}", hwnd.0 as isize);
    let mut cmd = Command::new(mpv);
    cmd.arg(format!("--wid={wid}"))
        .arg("--force-window=yes")
        .arg("--no-border")
        .arg("--no-osc")
        .arg("--no-input-default-bindings")
        .arg("--input-vo-keyboard=no")
        .arg("--hwdec=auto-safe")
        .arg("--vo=gpu")
        .arg("--gpu-context=auto")
        .arg("--keep-open=yes")
        .arg("--really-quiet")
        .arg("--start=0");

    if opts.loop_playback {
        cmd.arg("--loop-file=inf");
    } else {
        cmd.arg("--loop-file=no");
    }

    match opts.fit {
        FitMode::Fill => {
            cmd.arg("--keepaspect=yes").arg("--panscan=1.0");
        }
        FitMode::Contain | FitMode::Native => {
            cmd.arg("--keepaspect=yes").arg("--panscan=0.0");
        }
        FitMode::Stretch => {
            cmd.arg("--keepaspect=no");
        }
    }

    if opts.muted {
        cmd.arg("--mute=yes").arg("--volume=0");
    }

    // Live grade when not baking (baked file already includes filters).
    if !opts.grade.bake {
        if let Some(vf) = grade::filter_graph(&opts.grade) {
            cmd.arg(format!("--vf=lavfi=[{vf}]"));
        }
    }

    cmd.arg(&play);

    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to spawn {}", mpv.display()))
}

fn spawn_ffmpeg_frames(
    ffmpeg: &Path,
    width: i32,
    height: i32,
    video: &Path,
    frames: FrameBuffer,
    loop_playback: bool,
    live_vf: Option<&str>,
) -> Result<(Child, JoinHandle<()>, Arc<AtomicBool>, Arc<AtomicBool>)> {
    let stop = Arc::new(AtomicBool::new(false));
    let paused = Arc::new(AtomicBool::new(false));
    let stop_flag = Arc::clone(&stop);
    let paused_flag = Arc::clone(&paused);

    let mut cmd = Command::new(ffmpeg);
    cmd.arg("-hide_banner")
        .arg("-loglevel")
        .arg("error");
    if loop_playback {
        cmd.arg("-stream_loop").arg("-1");
    }
    cmd.arg("-re").arg("-i").arg(video);
    // Scale + optional live grade in one filter chain.
    let mut vf = format!("scale={width}:{height}");
    if let Some(grade_vf) = live_vf {
        vf = format!("{grade_vf},{vf}");
    }
    cmd.arg("-vf")
        .arg(vf)
        .arg("-f")
        .arg("rawvideo")
        .arg("-pix_fmt")
        .arg("bgra")
        .arg("-r")
        .arg("30")
        .arg("-an")
        .arg("pipe:1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("failed to spawn {}", ffmpeg.display()))?;

    let stdout = child.stdout.take().context("ffmpeg stdout missing")?;
    let frame_bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .context("frame size overflow")?;

    let pump = std::thread::spawn(move || {
        let mut reader = BufReader::with_capacity(frame_bytes, stdout);
        let mut buf = vec![0u8; frame_bytes];
        while !stop_flag.load(Ordering::SeqCst) {
            if paused_flag.load(Ordering::SeqCst) {
                std::thread::sleep(std::time::Duration::from_millis(50));
                continue;
            }
            match read_exact_interruptible(&mut reader, &mut buf, &stop_flag) {
                Ok(true) => frames.publish(width, height, buf.clone()),
                Ok(false) => break,
                Err(err) => {
                    tracing::error!(error = %err, "ffmpeg frame reader stopped");
                    break;
                }
            }
        }
    });

    Ok((child, pump, stop, paused))
}

fn blit_bgra(hwnd: HWND, bgra: &[u8], width: i32, height: i32, fit: FitMode) -> Result<()> {
    let hdc_window = unsafe { GetDC(Some(hwnd)) };
    if hdc_window.0.is_null() {
        bail!("GetDC failed");
    }
    let hdc_mem = unsafe { CreateCompatibleDC(Some(hdc_window)) };
    let hbmp = unsafe { CreateCompatibleBitmap(hdc_window, width, height) };
    let old = unsafe { SelectObject(hdc_mem, hbmp.into()) };

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0 as u32,
            ..Default::default()
        },
        ..Default::default()
    };

    let result = unsafe {
        SetDIBits(
            Some(hdc_mem),
            hbmp,
            0,
            height as u32,
            bgra.as_ptr() as *const _,
            &bmi,
            DIB_RGB_COLORS,
        );

        let mut rc = windows::Win32::Foundation::RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rc);
        let dst_w = (rc.right - rc.left).max(1);
        let dst_h = (rc.bottom - rc.top).max(1);

        let (dx, dy, dw, dh) = fit_rect(width, height, dst_w, dst_h, fit);

        if !StretchBlt(
            hdc_window,
            dx,
            dy,
            dw,
            dh,
            Some(hdc_mem),
            0,
            0,
            width,
            height,
            SRCCOPY,
        )
        .as_bool()
        {
            bail!("StretchBlt failed");
        }
        Ok(())
    };

    unsafe {
        SelectObject(hdc_mem, old);
        let _ = DeleteObject(hbmp.into());
        let _ = DeleteDC(hdc_mem);
        ReleaseDC(Some(hwnd), hdc_window);
    }
    result.map(|_| ())
}

fn fit_rect(src_w: i32, src_h: i32, dst_w: i32, dst_h: i32, fit: FitMode) -> (i32, i32, i32, i32) {
    match fit {
        FitMode::Stretch => (0, 0, dst_w, dst_h),
        FitMode::Native => {
            let w = src_w.min(dst_w);
            let h = src_h.min(dst_h);
            ((dst_w - w) / 2, (dst_h - h) / 2, w, h)
        }
        FitMode::Contain => {
            let scale = (dst_w as f64 / src_w as f64).min(dst_h as f64 / src_h as f64);
            let w = ((src_w as f64 * scale) as i32).max(1);
            let h = ((src_h as f64 * scale) as i32).max(1);
            ((dst_w - w) / 2, (dst_h - h) / 2, w, h)
        }
        FitMode::Fill => {
            let scale = (dst_w as f64 / src_w as f64).max(dst_h as f64 / src_h as f64);
            let w = ((src_w as f64 * scale) as i32).max(1);
            let h = ((src_h as f64 * scale) as i32).max(1);
            ((dst_w - w) / 2, (dst_h - h) / 2, w, h)
        }
    }
}

fn read_exact_interruptible(
    reader: &mut impl Read,
    buf: &mut [u8],
    stop: &AtomicBool,
) -> Result<bool> {
    let mut filled = 0;
    while filled < buf.len() {
        if stop.load(Ordering::SeqCst) {
            return Ok(false);
        }
        match reader.read(&mut buf[filled..]) {
            Ok(0) => return Ok(false),
            Ok(n) => filled += n,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err.into()),
        }
    }
    Ok(true)
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

fn find_mpv(allow_path: bool) -> Option<PathBuf> {
    candidates("mpv.exe", allow_path)
        .into_iter()
        .find(|p| p.is_file())
}

fn find_ffmpeg(allow_path: bool) -> Option<PathBuf> {
    candidates("ffmpeg.exe", allow_path)
        .into_iter()
        .find(|p| p.is_file())
}

fn candidates(exe: &str, allow_path: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let push_variants = |dir: &std::path::Path, out: &mut Vec<PathBuf>| {
        out.push(dir.join("bin").join(exe));
        out.push(dir.join(exe));
        out.push(dir.join("tools").join(exe));
        if exe.eq_ignore_ascii_case("mpv.exe") {
            out.push(dir.join("tools").join("mpv").join(exe));
            out.push(dir.join("mpv").join(exe));
        }
    };
    if let Ok(cwd) = std::env::current_dir() {
        push_variants(&cwd, &mut out);
        out.push(cwd.join("target").join("debug").join("tools").join(exe));
        out.push(cwd.join("target").join("release").join("tools").join(exe));
    }
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(dir) = exe_path.parent() {
            push_variants(dir, &mut out);
        }
    }
    if allow_path {
        if let Ok(path) = which(exe) {
            out.push(path);
        }
    }
    out
}

fn which(exe: &str) -> Result<PathBuf> {
    let output = Command::new("where.exe")
        .arg(exe)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()?;
    if !output.status.success() {
        bail!("not found");
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let first = text.lines().next().context("empty where")?;
    Ok(PathBuf::from(first.trim()))
}
