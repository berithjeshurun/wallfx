//! System tray host: Enable/Disable, Settings, Exit.

use crate::config::{self, UiToolkit};
use crate::engine::Engine;
use crate::instance::InstanceGuard;
use crate::session;
use anyhow::{Context, Result};
use std::sync::Mutex;
use std::thread;
use windows::core::{PCWSTR, w};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    DispatchMessageW, GetCursorPos, GetMessageW, LoadIconW, PostMessageW, PostQuitMessage,
    RegisterClassExW, SetForegroundWindow, SetTimer, TrackPopupMenu,
    TranslateMessage, CS_HREDRAW, CS_VREDRAW, HWND_MESSAGE, IDI_APPLICATION, MF_CHECKED,
    MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MSG, TPM_BOTTOMALIGN, TPM_LEFTALIGN,
    TPM_RIGHTBUTTON, WM_APP, WM_COMMAND, WM_DESTROY, WM_LBUTTONDBLCLK, WM_NULL, WM_RBUTTONUP,
    WM_TIMER, WM_USER, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSEXW,
};

const WM_TRAY: u32 = WM_USER + 1;
const WM_SETTINGS_APPLIED: u32 = WM_APP + 2;
const WM_SETTINGS_DONE: u32 = WM_APP + 3;
const ID_ENABLE: usize = 1;
const ID_SETTINGS: usize = 2;
const ID_EXIT: usize = 3;
const TIMER_POLL: usize = 1;

struct TrayState {
    hwnd: isize,
    enabled: bool,
    engine: Option<Engine>,
    _guard: Option<InstanceGuard>,
    settings_open: bool,
}

// SAFETY: tray state is only touched from the tray UI thread / via PostMessage.
unsafe impl Send for TrayState {}

static STATE: Mutex<Option<TrayState>> = Mutex::new(None);
static TRAY_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

fn hwnd_from(raw: isize) -> HWND {
    HWND(raw as *mut _)
}

pub fn notify_settings_applied() {
    let hwnd = TRAY_HWND.load(std::sync::atomic::Ordering::SeqCst);
    if hwnd != 0 {
        unsafe {
            let _ = PostMessageW(
                Some(hwnd_from(hwnd)),
                WM_SETTINGS_APPLIED,
                WPARAM(0),
                LPARAM(0),
            );
        }
    }
}

pub fn notify_settings_closed() {
    let hwnd = TRAY_HWND.load(std::sync::atomic::Ordering::SeqCst);
    if hwnd != 0 {
        unsafe {
            let _ = PostMessageW(Some(hwnd_from(hwnd)), WM_SETTINGS_DONE, WPARAM(0), LPARAM(0));
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn run() -> Result<()> {
    let hinstance = unsafe { GetModuleHandleW(None) }.context("GetModuleHandleW")?;
    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(tray_proc),
        hInstance: windows::Win32::Foundation::HINSTANCE(hinstance.0),
        lpszClassName: w!("WallFXTray"),
        ..Default::default()
    };
    unsafe {
        if RegisterClassExW(&class) == 0 {
            // may already be registered
        }
    }

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("WallFXTray"),
            w!("WallFX"),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(windows::Win32::Foundation::HINSTANCE(hinstance.0)),
            None,
        )
    }
    .context("CreateWindowExW tray")?;

    let cfg = config::load().unwrap_or_default();
    let mut engine = None;
    let mut guard = None;
    let mut enabled = cfg.wallpaper.enabled;
    if enabled {
        match start_engine() {
            Ok((g, e)) => {
                guard = Some(g);
                engine = Some(e);
            }
            Err(err) => {
                tracing::error!("auto-enable failed: {err:#}");
                enabled = false;
                let mut c = cfg;
                c.wallpaper.enabled = false;
                let _ = config::save(&c);
            }
        }
    }

    let hwnd_raw = hwnd.0 as isize;
    *STATE.lock().unwrap() = Some(TrayState {
        hwnd: hwnd_raw,
        enabled,
        engine,
        _guard: guard,
        settings_open: false,
    });
    TRAY_HWND.store(hwnd_raw, std::sync::atomic::Ordering::SeqCst);

    add_tray_icon(hwnd)?;
    let _ = session::register(hwnd);
    unsafe {
        SetTimer(Some(hwnd), TIMER_POLL, 16, None);
    }

    tracing::info!("WallFX tray running");
    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    session::unregister(hwnd);
    remove_tray_icon(hwnd);
    *STATE.lock().unwrap() = None;
    Ok(())
}

fn start_engine() -> Result<(InstanceGuard, Engine)> {
    let cfg = config::load()?;
    let guard = InstanceGuard::acquire()?;
    let engine = Engine::start_from_config(&cfg)?;
    Ok((guard, engine))
}

fn add_tray_icon(hwnd: HWND) -> Result<()> {
    let mut tip = [0u16; 128];
    let tip_src = wide("WallFX");
    tip[..tip_src.len().min(127)].copy_from_slice(&tip_src[..tip_src.len().min(127)]);

    let icon = unsafe { LoadIconW(None, IDI_APPLICATION) }.context("LoadIconW")?;
    let mut nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: icon,
        ..Default::default()
    };
    nid.szTip = tip;
    unsafe {
        Shell_NotifyIconW(NIM_ADD, &nid)
            .ok()
            .context("Shell_NotifyIconW NIM_ADD")?;
    }
    Ok(())
}

fn remove_tray_icon(hwnd: HWND) {
    let nid = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        ..Default::default()
    };
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

unsafe extern "system" fn tray_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        m if m == WM_TRAY => {
            match lparam.0 as u32 {
                WM_RBUTTONUP => show_menu(hwnd),
                WM_LBUTTONDBLCLK => open_settings(hwnd),
                _ => {}
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            match wparam.0 & 0xffff {
                ID_ENABLE => toggle_enable(),
                ID_SETTINGS => open_settings(hwnd),
                ID_EXIT => {
                    stop_engine();
                    let _ = DestroyWindow(hwnd);
                }
                _ => {}
            }
            LRESULT(0)
        }
        m if m == WM_SETTINGS_APPLIED => {
            reload_if_enabled();
            LRESULT(0)
        }
        m if m == WM_SETTINGS_DONE => {
            if let Ok(mut g) = STATE.lock() {
                if let Some(st) = g.as_mut() {
                    st.settings_open = false;
                }
            }
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_POLL => {
            poll_engine();
            LRESULT(0)
        }
        x if session::is_lock_msg(x, wparam.0).is_some() => {
            if let Some(locked) = session::is_lock_msg(x, wparam.0) {
                if let Ok(mut g) = STATE.lock() {
                    if let Some(st) = g.as_mut() {
                        if let Some(engine) = st.engine.as_mut() {
                            engine.set_session_locked(locked);
                        }
                    }
                }
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

fn show_menu(hwnd: HWND) {
    let enabled = STATE
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.enabled))
        .unwrap_or(false);

    unsafe {
        let menu = CreatePopupMenu().unwrap_or_default();
        let enable_flags = if enabled { MF_CHECKED } else { MF_UNCHECKED };
        let _ = AppendMenuW(
            menu,
            enable_flags | MF_STRING,
            ID_ENABLE,
            w!("Enable / Disable"),
        );
        let _ = AppendMenuW(menu, MF_STRING, ID_SETTINGS, w!("Settings"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_EXIT, w!("Exit"));

        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            Some(0),
            hwnd,
            None,
        );
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
    }
}

fn toggle_enable() {
    let mut g = match STATE.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    let Some(st) = g.as_mut() else { return };
    if st.enabled {
        st.engine = None;
        st._guard = None;
        st.enabled = false;
        if let Ok(mut cfg) = config::load() {
            cfg.wallpaper.enabled = false;
            let _ = config::save(&cfg);
        }
        tracing::info!("wallpaper disabled");
    } else {
        match start_engine() {
            Ok((guard, engine)) => {
                st._guard = Some(guard);
                st.engine = Some(engine);
                st.enabled = true;
                if let Ok(mut cfg) = config::load() {
                    cfg.wallpaper.enabled = true;
                    let _ = config::save(&cfg);
                }
                tracing::info!("wallpaper enabled");
            }
            Err(err) => {
                tracing::error!("enable failed: {err:#}");
                // Message box would need UI thread — log only
            }
        }
    }
}

fn stop_engine() {
    if let Ok(mut g) = STATE.lock() {
        if let Some(st) = g.as_mut() {
            st.engine = None;
            st._guard = None;
            st.enabled = false;
        }
    }
}

fn reload_if_enabled() {
    let enabled = STATE
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.enabled))
        .unwrap_or(false);
    if !enabled {
        return;
    }
    stop_engine();
    if let Ok(mut g) = STATE.lock() {
        if let Some(st) = g.as_mut() {
            match start_engine() {
                Ok((guard, engine)) => {
                    st._guard = Some(guard);
                    st.engine = Some(engine);
                    st.enabled = true;
                    if let Ok(mut cfg) = config::load() {
                        cfg.wallpaper.enabled = true;
                        let _ = config::save(&cfg);
                    }
                }
                Err(err) => {
                    tracing::error!("reload failed: {err:#}");
                    st.enabled = false;
                }
            }
        }
    }
}

fn poll_engine() {
    if let Ok(mut g) = STATE.lock() {
        if let Some(st) = g.as_mut() {
            if let Some(engine) = st.engine.as_mut() {
                match engine.poll() {
                    Ok(true) => {}
                    Ok(false) => {
                        tracing::warn!("player exited");
                        st.engine = None;
                        st._guard = None;
                        st.enabled = false;
                    }
                    Err(err) => tracing::warn!(error = %err, "poll error"),
                }
            }
        }
    }
}

fn open_settings(_hwnd: HWND) {
    {
        let mut g = match STATE.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let Some(st) = g.as_mut() else { return };
        if st.settings_open {
            return;
        }
        st.settings_open = true;
    }

    let toolkit = config::load()
        .map(|c| c.ui.toolkit)
        .unwrap_or_default();

    thread::spawn(move || {
        let result = match toolkit {
            UiToolkit::Egui => crate::settings_egui::open(),
            UiToolkit::Win32 => crate::settings_win32::open(),
        };
        if let Err(err) = result {
            tracing::error!("settings failed: {err:#}");
        }
        notify_settings_closed();
    });
}
