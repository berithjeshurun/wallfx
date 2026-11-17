//! Borderless wallpaper surface covering a monitor.

use anyhow::{Context, Result, bail};
use std::sync::OnceLock;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetStockObject, HBRUSH, BLACK_BRUSH};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetSystemMetrics, LoadCursorW, MoveWindow,
    RegisterClassExW, ShowWindow, CS_HREDRAW, CS_OWNDC, CS_VREDRAW, IDC_ARROW, SM_CXSCREEN,
    SM_CYSCREEN, SW_SHOW, WM_DESTROY, WM_ERASEBKGND, WINDOW_EX_STYLE, WINDOW_STYLE, WNDCLASSEXW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP, LWA_ALPHA,
    SetLayeredWindowAttributes,
};
use windows::core::{PCWSTR, w};

static CLASS_ATOM: OnceLock<u16> = OnceLock::new();

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_DESTROY => LRESULT(0),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

fn ensure_class() -> Result<u16> {
    if let Some(atom) = CLASS_ATOM.get() {
        return Ok(*atom);
    }

    let hinstance = unsafe { GetModuleHandleW(None) }.context("GetModuleHandleW failed")?;
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.context("LoadCursorW failed")?;

    let class = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW | CS_OWNDC,
        lpfnWndProc: Some(wnd_proc),
        hInstance: HINSTANCE(hinstance.0),
        hCursor: cursor,
        hbrBackground: HBRUSH(unsafe { GetStockObject(BLACK_BRUSH) }.0),
        lpszClassName: w!("WallFXWallpaper"),
        ..Default::default()
    };

    let atom = unsafe { RegisterClassExW(&class) };
    if atom == 0 {
        bail!("RegisterClassExW failed");
    }
    let _ = CLASS_ATOM.set(atom);
    Ok(atom)
}

pub struct WallpaperWindow {
    pub hwnd: HWND,
    pub width: i32,
    pub height: i32,
}

impl WallpaperWindow {
    pub fn create_primary() -> Result<Self> {
        let width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
        let rect = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };
        Self::create_at(rect)
    }

    pub fn create_at(rect: RECT) -> Result<Self> {
        ensure_class()?;
        let hinstance = unsafe { GetModuleHandleW(None) }.context("GetModuleHandleW failed")?;

        let width = (rect.right - rect.left).max(1);
        let height = (rect.bottom - rect.top).max(1);

        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0 | WS_EX_LAYERED.0),
                w!("WallFXWallpaper"),
                PCWSTR(wide("WallFX").as_ptr()),
                WINDOW_STYLE(WS_POPUP.0),
                rect.left,
                rect.top,
                width,
                height,
                None,
                None,
                Some(HINSTANCE(hinstance.0)),
                None,
            )
        }
        .context("CreateWindowExW failed")?;

        if hwnd.0.is_null() {
            bail!("CreateWindowExW returned null");
        }

        unsafe {
            // Do not call SetLayeredWindowAttributes here — it permanently conflicts with
            // UpdateLayeredWindow (parallax/GDI path). Video path calls set_alpha() after attach.
            let _ = MoveWindow(hwnd, rect.left, rect.top, width, height, true);
            let _ = ShowWindow(hwnd, SW_SHOW);
        }

        Ok(Self {
            hwnd,
            width,
            height,
        })
    }

    pub fn set_alpha(&self, alpha: u8) -> Result<()> {
        unsafe {
            SetLayeredWindowAttributes(self.hwnd, COLORREF(0), alpha, LWA_ALPHA)
                .context("SetLayeredWindowAttributes")?;
        }
        Ok(())
    }
}

impl Drop for WallpaperWindow {
    fn drop(&mut self) {
        if !self.hwnd.0.is_null() {
            unsafe {
                let _ = DestroyWindow(self.hwnd);
            }
            self.hwnd = HWND::default();
        }
    }
}
