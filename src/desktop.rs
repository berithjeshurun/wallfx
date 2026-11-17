//! Desktop attachment for live wallpaper.
//!
//! Win11 24H2+: Progman uses WS_EX_NOREDIRECTIONBITMAP — classic WorkerW+GDI is invisible.
//! Approach: WS_EX_LAYERED WS_POPUP under Progman (not WS_CHILD), z-ordered under DefView
//! and above WorkerW. GDI content uses UpdateLayeredWindow (not SetLayeredWindowAttributes).
//! Older Windows: fall back to WorkerW host parenting.

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{InvalidateRect, UpdateWindow};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetSystemMetrics, GetWindowLongPtrW,
    IsWindow, MoveWindow, SendMessageTimeoutW, SetParent, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, GWL_EXSTYLE, GWL_STYLE, HWND_BOTTOM, SMTO_NORMAL, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW,
    WINDOW_EX_STYLE, WS_CHILD, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP,
    WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE,
};
use windows::core::PCWSTR;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn find_window(class: &str) -> Result<HWND> {
    unsafe { FindWindowW(PCWSTR(wide(class).as_ptr()), PCWSTR::null()) }
        .with_context(|| format!("{class} window not found — is Explorer running?"))
}

fn find_child(parent: HWND, after: Option<HWND>, class: &str) -> Option<HWND> {
    unsafe {
        FindWindowExW(
            Some(parent),
            after,
            PCWSTR(wide(class).as_ptr()),
            PCWSTR::null(),
        )
    }
    .ok()
    .filter(|h| !h.0.is_null())
}

fn find_top_level_after(after: HWND, class: &str) -> Option<HWND> {
    unsafe {
        FindWindowExW(
            None,
            Some(after),
            PCWSTR(wide(class).as_ptr()),
            PCWSTR::null(),
        )
    }
    .ok()
    .filter(|h| !h.0.is_null())
}

fn has_defview(hwnd: HWND) -> bool {
    find_child(hwnd, None, "SHELLDLL_DefView").is_some()
}

fn ex_style(hwnd: HWND) -> u32 {
    unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 }
}

fn is_win11_24h2_desktop(progman: HWND) -> bool {
    (ex_style(progman) & WS_EX_NOREDIRECTIONBITMAP.0) != 0
}

/// Raise the desktop so WorkerW / layered slots exist.
pub fn ensure_workerw_split() -> Result<()> {
    let progman = find_window("Progman")?;
    unsafe {
        let mut result = 0_usize;
        let _ = SendMessageTimeoutW(
            progman,
            0x052C,
            WPARAM(0),
            LPARAM(0),
            SMTO_NORMAL,
            1000,
            Some(&mut result),
        );
        let _ = SendMessageTimeoutW(
            progman,
            0x052C,
            WPARAM(0xD),
            LPARAM(0x1),
            SMTO_NORMAL,
            1000,
            Some(&mut result),
        );
    }
    std::thread::sleep(std::time::Duration::from_millis(250));
    Ok(())
}

fn find_shell_defview(progman: HWND) -> Option<HWND> {
    // Win11 24H2: DefView is a direct child of Progman.
    if let Some(v) = find_child(progman, None, "SHELLDLL_DefView") {
        return Some(v);
    }
    // Older: DefView under a top-level WorkerW.
    struct State {
        defview: HWND,
    }
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let state = unsafe { &mut *(lparam.0 as *mut State) };
        let mut class = [0u16; 64];
        let len = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
        if len == 0 {
            return windows::core::BOOL(1);
        }
        if String::from_utf16_lossy(&class[..len]) != "WorkerW" {
            return windows::core::BOOL(1);
        }
        if let Some(v) = find_child(hwnd, None, "SHELLDLL_DefView") {
            state.defview = v;
            return windows::core::BOOL(0); // stop
        }
        windows::core::BOOL(1)
    }
    let mut state = State {
        defview: HWND::default(),
    };
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut state as *mut State as isize));
    }
    if state.defview.0.is_null() {
        None
    } else {
        Some(state.defview)
    }
}

fn find_progman_workerw(progman: HWND) -> Option<HWND> {
    let mut after = None;
    let mut best = None;
    while let Some(child) = find_child(progman, after, "WorkerW") {
        if !has_defview(child) {
            best = Some(child);
        }
        after = Some(child);
    }
    best
}

fn find_legacy_workerw_host() -> Option<HWND> {
    struct State {
        host: HWND,
    }
    unsafe extern "system" fn cb(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let state = unsafe { &mut *(lparam.0 as *mut State) };
        let mut class = [0u16; 64];
        let len = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
        if len == 0 {
            return windows::core::BOOL(1);
        }
        if String::from_utf16_lossy(&class[..len]) != "WorkerW" {
            return windows::core::BOOL(1);
        }
        if has_defview(hwnd) {
            if let Some(host) = find_top_level_after(hwnd, "WorkerW") {
                if !has_defview(host) {
                    state.host = host;
                }
            }
        }
        windows::core::BOOL(1)
    }
    let mut state = State {
        host: HWND::default(),
    };
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut state as *mut State as isize));
    }
    if state.host.0.is_null() {
        None
    } else {
        Some(state.host)
    }
}

fn apply_layered_progman_styles(child: HWND) {
    unsafe {
        // Keep WS_POPUP (not WS_CHILD): UpdateLayeredWindow rejects child windows
        // (ERROR_INVALID_PARAMETER), which we need for GDI/parallax on Win11 24H2.
        let style = GetWindowLongPtrW(child, GWL_STYLE) as u32;
        let new_style = (style | WS_POPUP.0 | WS_VISIBLE.0) & !WS_CHILD.0;
        SetWindowLongPtrW(child, GWL_STYLE, new_style as isize);

        let ex = GetWindowLongPtrW(child, GWL_EXSTYLE) as u32;
        // WS_EX_TRANSPARENT: clicks pass through to desktop icons underneath.
        let new_ex = (ex
            | WS_EX_LAYERED.0
            | WS_EX_NOACTIVATE.0
            | WS_EX_TOOLWINDOW.0
            | WS_EX_TRANSPARENT.0)
            & !WINDOW_EX_STYLE(0x40000).0; // strip WS_EX_APPWINDOW if present
        SetWindowLongPtrW(child, GWL_EXSTYLE, new_ex as isize);
        // Do not call SetLayeredWindowAttributes here — it conflicts with UpdateLayeredWindow.
    }
}

/// Attach wallpaper HWND using the correct strategy for this Windows build.
/// `screen` is the monitor rect in virtual-screen coordinates.
pub fn attach_to_desktop(child: HWND, screen: RECT) -> Result<HWND> {
    if child.0.is_null() || !unsafe { IsWindow(Some(child)).as_bool() } {
        bail!("Invalid wallpaper window handle");
    }

    ensure_workerw_split()?;
    let progman = find_window("Progman")?;
    let width = (screen.right - screen.left).max(1);
    let height = (screen.bottom - screen.top).max(1);
    let vx = unsafe { GetSystemMetrics(SM_XVIRTUALSCREEN) };
    let vy = unsafe { GetSystemMetrics(SM_YVIRTUALSCREEN) };
    let x = screen.left - vx;
    let y = screen.top - vy;

    if is_win11_24h2_desktop(progman) {
        tracing::info!(
            "Win11 24H2+ desktop detected (WS_EX_NOREDIRECTIONBITMAP) — using top-level layered window"
        );
        // Absolute screen coords: must stay top-level (no SetParent) for UpdateLayeredWindow.
        return attach_layered_toplevel(child, screen.left, screen.top, width, height);
    }

    // Legacy WorkerW path (Win10 / early Win11)
    let host = find_progman_workerw(progman)
        .or_else(find_legacy_workerw_host)
        .unwrap_or(progman);

    tracing::info!(host = host.0 as isize, "using legacy WorkerW/Progman host");
    unsafe {
        let style = GetWindowLongPtrW(child, GWL_STYLE) as u32;
        SetWindowLongPtrW(
            child,
            GWL_STYLE,
            ((style & !WS_POPUP.0) | WS_CHILD.0 | WS_VISIBLE.0) as isize,
        );
        SetParent(child, Some(host)).context("SetParent failed")?;
        size_and_show(child, x, y, width, height);
    }
    Ok(host)
}

fn attach_layered_toplevel(
    child: HWND,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> Result<HWND> {
    // SetParent(Progman) makes the HWND a child; UpdateLayeredWindow then fails with
    // ERROR_INVALID_PARAMETER (0x80070057). Stay top-level, pin to HWND_BOTTOM, and use
    // WS_EX_TRANSPARENT so desktop icon clicks still reach DefView.
    apply_layered_progman_styles(child);
    unsafe {
        let _ = SetParent(child, None);
        size_and_show(child, x, y, width, height);
        let _ = ShowWindow(child, SW_SHOW);
        let _ = InvalidateRect(Some(child), None, false);
        let _ = UpdateWindow(child);
    }

    tracing::info!(
        x,
        y,
        width,
        height,
        "top-level layered wallpaper attached (UpdateLayeredWindow path)"
    );
    Ok(child)
}

unsafe fn size_and_show(child: HWND, x: i32, y: i32, width: i32, height: i32) {
    let _ = MoveWindow(child, x, y, width, height, true);
    let _ = SetWindowPos(
        child,
        Some(HWND_BOTTOM),
        x,
        y,
        width,
        height,
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    );
    let _ = ShowWindow(child, SW_SHOW);
}

pub fn detach_from_desktop(child: HWND) {
    if child.0.is_null() || !unsafe { IsWindow(Some(child)).as_bool() } {
        return;
    }
    unsafe {
        let _ = SetParent(child, None);
        let _ = ShowWindow(child, SW_HIDE);
    }
}

/// Re-assert z-order (Explorer sometimes recreates WorkerW).
pub fn refresh_zorder(child: HWND) -> Result<()> {
    if child.0.is_null() || !unsafe { IsWindow(Some(child)).as_bool() } {
        return Ok(());
    }
    let progman = find_window("Progman")?;
    if !is_win11_24h2_desktop(progman) {
        return Ok(());
    }
    unsafe {
        let _ = SetWindowPos(
            child,
            Some(HWND_BOTTOM),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
    Ok(())
}
