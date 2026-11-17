//! Native Win32 Settings dialog (programmatic controls).

use crate::config::{BackendPref, FitMode, UiToolkit};
use crate::settings_model::SettingsModel;
use crate::tray;
use anyhow::{Context, Result};
use windows::core::{PCWSTR, w};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, HBRUSH, WHITE_BRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetDlgItem, GetMessageW,
    GetWindowTextW, IsDialogMessageW, LoadCursorW, MessageBoxW, MoveWindow, PostQuitMessage,
    RegisterClassExW, SendMessageW, SetWindowTextW, ShowWindow, TranslateMessage, BS_AUTOCHECKBOX,
    BS_PUSHBUTTON, CBS_DROPDOWNLIST, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, ES_AUTOHSCROLL,
    ES_LEFT, HMENU, IDC_ARROW, MB_OK, MSG, SW_SHOW, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY,
    WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSEXW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_OVERLAPPED,
    WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL, BM_GETCHECK, BM_SETCHECK, CB_ADDSTRING,
    CB_GETCURSEL, CB_SETCURSEL,
};

const BST_CHECKED: usize = 1;
const BST_UNCHECKED: usize = 0;

const IDC_TOOLKIT: i32 = 1001;
const IDC_VIDEO: i32 = 1002;
const IDC_BROWSE: i32 = 1003;
const IDC_LOOP: i32 = 1004;
const IDC_MUTE: i32 = 1005;
const IDC_PREVIEW: i32 = 1006;
const IDC_STARTUP: i32 = 1007;
const IDC_EXPAND: i32 = 1008;
const IDC_SYNC: i32 = 1009;
const IDC_BACKEND: i32 = 1010;
const IDC_FIT: i32 = 1011;
const IDC_DEC_W: i32 = 1012;
const IDC_DEC_H: i32 = 1013;
const IDC_PAUSE_LOCK: i32 = 1014;
const IDC_FADE: i32 = 1015;
const IDC_FADE_MS: i32 = 1016;
const IDC_APPLY: i32 = 1020;
const IDC_CLOSE: i32 = 1021;
const IDC_MON_BASE: i32 = 1100;

struct DialogState {
    model: SettingsModel,
    mon_ids: Vec<String>,
}

static mut STATE: Option<DialogState> = None;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn hmenu_id(id: i32) -> HMENU {
    HMENU(id as isize as *mut _)
}

pub fn open() -> Result<()> {
    let model = SettingsModel::load()?;
    let mon_ids: Vec<_> = model.monitors.iter().map(|m| m.id.clone()).collect();
    unsafe {
        STATE = Some(DialogState { model, mon_ids });
    }

    let hinstance = unsafe { GetModuleHandleW(None) }.context("GetModuleHandleW")?;
    let class_name = w!("WallFXSettingsWin32");
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(dlg_proc),
        hInstance: windows::Win32::Foundation::HINSTANCE(hinstance.0),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
        hbrBackground: HBRUSH(unsafe { GetStockObject(WHITE_BRUSH) }.0),
        lpszClassName: class_name,
        ..Default::default()
    };
    unsafe {
        let _ = RegisterClassExW(&wc);
    }

    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("WallFX Settings"),
            WINDOW_STYLE(
                WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0 | WS_VISIBLE.0 | WS_VSCROLL.0,
            ),
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            560,
            700,
            None,
            None,
            Some(windows::Win32::Foundation::HINSTANCE(hinstance.0)),
            None,
        )
    }
    .context("CreateWindowExW settings")?;

    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOW);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if !IsDialogMessageW(hwnd, &mut msg).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        STATE = None;
    }
    Ok(())
}

unsafe extern "system" fn dlg_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            create_controls(hwnd);
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xffff) as i32;
            match id {
                IDC_BROWSE => {
                    if let Ok(Some(path)) = crate::dialog::browse_video() {
                        if let Ok(edit) = GetDlgItem(Some(hwnd), IDC_VIDEO) {
                            let t = wide(&path.display().to_string());
                            let _ = SetWindowTextW(edit, PCWSTR(t.as_ptr()));
                        }
                    }
                }
                IDC_APPLY => {
                    if let Err(err) = apply_from_ui(hwnd) {
                        let t = wide(&format!("{err:#}"));
                        let _ = MessageBoxW(Some(hwnd), PCWSTR(t.as_ptr()), w!("WallFX"), MB_OK);
                    } else {
                        let _ = MessageBoxW(Some(hwnd), w!("Saved."), w!("WallFX"), MB_OK);
                    }
                }
                IDC_CLOSE => {
                    let _ = DestroyWindow(hwnd);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn create_controls(hwnd: HWND) {
    let Some(state) = STATE.as_ref() else {
        return;
    };
    let hinst = GetModuleHandleW(None).ok();
    let hi = hinst.map(|h| windows::Win32::Foundation::HINSTANCE(h.0));
    let mut y = 12i32;

    let mk_static = |text: &str, x: i32, y: i32, w: i32| {
        let t = wide(text);
        let _ = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("STATIC"),
            PCWSTR(t.as_ptr()),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0),
            x,
            y,
            w,
            22,
            Some(hwnd),
            None,
            hi,
            None,
        );
    };
    let mk_edit = |id: i32, text: &str, x: i32, y: i32, w: i32| {
        let t = wide(text);
        let _ = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("EDIT"),
            PCWSTR(t.as_ptr()),
            WINDOW_STYLE(
                WS_CHILD.0 | WS_VISIBLE.0 | WS_BORDER.0 | WS_TABSTOP.0 | ES_LEFT as u32 | ES_AUTOHSCROLL as u32,
            ),
            x,
            y,
            w,
            24,
            Some(hwnd),
            Some(hmenu_id(id)),
            hi,
            None,
        );
    };
    let mk_btn = |id: i32, text: &str, x: i32, y: i32, w: i32| {
        let t = wide(text);
        let _ = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            PCWSTR(t.as_ptr()),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | BS_PUSHBUTTON as u32),
            x,
            y,
            w,
            26,
            Some(hwnd),
            Some(hmenu_id(id)),
            hi,
            None,
        );
    };
    let mk_check = |id: i32, text: &str, y: i32, checked: bool| {
        let t = wide(text);
        if let Ok(h) = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("BUTTON"),
            PCWSTR(t.as_ptr()),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | BS_AUTOCHECKBOX as u32),
            12,
            y,
            500,
            22,
            Some(hwnd),
            Some(hmenu_id(id)),
            hi,
            None,
        ) {
            SendMessageW(
                h,
                BM_SETCHECK,
                Some(WPARAM(if checked {
                    BST_CHECKED
                } else {
                    BST_UNCHECKED
                })),
                Some(LPARAM(0)),
            );
        }
    };
    let mk_combo = |id: i32, y: i32, items: &[&str], sel: usize| {
        if let Ok(h) = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("COMBOBOX"),
            PCWSTR::null(),
            WINDOW_STYLE(WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0 | CBS_DROPDOWNLIST as u32),
            160,
            y,
            220,
            200,
            Some(hwnd),
            Some(hmenu_id(id)),
            hi,
            None,
        ) {
            for item in items {
                let t = wide(item);
                SendMessageW(
                    h,
                    CB_ADDSTRING,
                    Some(WPARAM(0)),
                    Some(LPARAM(t.as_ptr() as isize)),
                );
            }
            SendMessageW(h, CB_SETCURSEL, Some(WPARAM(sel)), Some(LPARAM(0)));
        }
    };

    mk_static("Settings UI", 12, y, 140);
    mk_combo(
        IDC_TOOLKIT,
        y,
        &["egui", "Native Win32"],
        match state.model.toolkit {
            UiToolkit::Egui => 0,
            UiToolkit::Win32 => 1,
        },
    );
    y += 32;

    mk_static("Video", 12, y, 140);
    mk_edit(IDC_VIDEO, &state.model.video, 160, y, 280);
    mk_btn(IDC_BROWSE, "Browse…", 450, y, 80);
    y += 32;

    mk_check(IDC_LOOP, "Loop", y, state.model.loop_playback);
    y += 26;
    mk_check(IDC_MUTE, "Mute", y, state.model.mute);
    y += 26;
    mk_check(IDC_PREVIEW, "Preview window (not desktop)", y, state.model.preview);
    y += 26;
    mk_check(IDC_STARTUP, "Start with Windows", y, state.model.startup);
    y += 26;
    mk_check(IDC_EXPAND, "self.expand (self-extract)", y, state.model.expand);
    y += 26;
    mk_check(IDC_SYNC, "Multi-monitor time sync", y, state.model.time_sync);
    y += 30;

    mk_static("Backend", 12, y, 140);
    mk_combo(
        IDC_BACKEND,
        y,
        &["Default", "Auto", "mpv", "ffmpeg"],
        match state.model.backend {
            BackendPref::Default => 0,
            BackendPref::Auto => 1,
            BackendPref::Mpv => 2,
            BackendPref::Ffmpeg => 3,
        },
    );
    y += 32;

    mk_static("Fit", 12, y, 140);
    mk_combo(
        IDC_FIT,
        y,
        &["Fill", "Contain", "Stretch", "Native"],
        match state.model.fit {
            FitMode::Fill => 0,
            FitMode::Contain => 1,
            FitMode::Stretch => 2,
            FitMode::Native => 3,
        },
    );
    y += 32;

    mk_static("Max decode W/H", 12, y, 140);
    mk_edit(
        IDC_DEC_W,
        &state.model.max_decode_w.to_string(),
        160,
        y,
        80,
    );
    mk_edit(
        IDC_DEC_H,
        &state.model.max_decode_h.to_string(),
        250,
        y,
        80,
    );
    y += 32;

    mk_check(
        IDC_PAUSE_LOCK,
        "Pause / hide when locked",
        y,
        state.model.unlock.pause_when_locked,
    );
    y += 26;
    mk_check(
        IDC_FADE,
        "Fade in on unlock",
        y,
        state.model.unlock.fade_on_unlock,
    );
    y += 26;
    mk_static("Fade ms", 12, y, 140);
    mk_edit(
        IDC_FADE_MS,
        &state.model.unlock.fade_ms.to_string(),
        160,
        y,
        80,
    );
    y += 36;

    mk_static("Monitors", 12, y, 140);
    y += 24;
    for (i, mon) in state.model.monitors.iter().enumerate() {
        let label = format!(
            "{} {}x{}{}",
            mon.name,
            mon.rect.right - mon.rect.left,
            mon.rect.bottom - mon.rect.top,
            if mon.is_primary { " (primary)" } else { "" }
        );
        mk_check(
            IDC_MON_BASE + i as i32,
            &label,
            y,
            state.model.monitor_checked(&mon.id),
        );
        y += 24;
    }

    y += 8;
    mk_btn(IDC_APPLY, "Apply", 160, y, 100);
    mk_btn(IDC_CLOSE, "Close", 280, y, 100);
    let _ = MoveWindow;
}

unsafe fn apply_from_ui(hwnd: HWND) -> Result<()> {
    let state = STATE.as_mut().context("dialog state")?;
    let read_text = |id: i32| -> String {
        let Ok(h) = GetDlgItem(Some(hwnd), id) else {
            return String::new();
        };
        let mut buf = [0u16; 1024];
        let n = GetWindowTextW(h, &mut buf);
        String::from_utf16_lossy(&buf[..n as usize])
    };
    let checked = |id: i32| -> bool {
        let Ok(h) = GetDlgItem(Some(hwnd), id) else {
            return false;
        };
        SendMessageW(h, BM_GETCHECK, Some(WPARAM(0)), Some(LPARAM(0))).0 as usize == BST_CHECKED
    };
    let combo_sel = |id: i32| -> usize {
        let Ok(h) = GetDlgItem(Some(hwnd), id) else {
            return 0;
        };
        SendMessageW(h, CB_GETCURSEL, Some(WPARAM(0)), Some(LPARAM(0))).0 as usize
    };

    state.model.toolkit = if combo_sel(IDC_TOOLKIT) == 1 {
        UiToolkit::Win32
    } else {
        UiToolkit::Egui
    };
    state.model.video = read_text(IDC_VIDEO);
    state.model.loop_playback = checked(IDC_LOOP);
    state.model.mute = checked(IDC_MUTE);
    state.model.preview = checked(IDC_PREVIEW);
    state.model.startup = checked(IDC_STARTUP);
    state.model.expand = checked(IDC_EXPAND);
    state.model.time_sync = checked(IDC_SYNC);
    state.model.backend = match combo_sel(IDC_BACKEND) {
        1 => BackendPref::Auto,
        2 => BackendPref::Mpv,
        3 => BackendPref::Ffmpeg,
        _ => BackendPref::Default,
    };
    state.model.fit = match combo_sel(IDC_FIT) {
        1 => FitMode::Contain,
        2 => FitMode::Stretch,
        3 => FitMode::Native,
        _ => FitMode::Fill,
    };
    state.model.max_decode_w = read_text(IDC_DEC_W).parse().unwrap_or(0);
    state.model.max_decode_h = read_text(IDC_DEC_H).parse().unwrap_or(0);
    state.model.unlock.pause_when_locked = checked(IDC_PAUSE_LOCK);
    state.model.unlock.fade_on_unlock = checked(IDC_FADE);
    state.model.unlock.fade_ms = read_text(IDC_FADE_MS).parse().unwrap_or(400);

    let mut selected = Vec::new();
    for (i, id) in state.mon_ids.iter().enumerate() {
        if checked(IDC_MON_BASE + i as i32) {
            selected.push(id.clone());
        }
    }
    if selected.len() == state.mon_ids.len() {
        selected.clear();
    }
    state.model.selected_monitors = selected;

    state.model.apply()?;
    tray::notify_settings_applied();
    Ok(())
}
