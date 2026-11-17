//! UpdateLayeredWindow path — required for visible GDI content on Win11 24H2 layered wallpaper HWNDs.
//! Window must be WS_EX_LAYERED + WS_POPUP (not WS_CHILD), and must NOT have used
//! SetLayeredWindowAttributes (that API permanently conflicts with UpdateLayeredWindow until
//! WS_EX_LAYERED is toggled off/on).

use anyhow::{bail, Context, Result};
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, GetWindowRect, SetWindowLongPtrW, UpdateLayeredWindow, GWL_EXSTYLE,
    GWL_STYLE, ULW_ALPHA, WS_CHILD, WS_EX_LAYERED, WS_POPUP, WS_VISIBLE,
};

/// Clear SetLayeredWindowAttributes mode and ensure styles allow UpdateLayeredWindow.
fn prepare_hwnd(hwnd: HWND) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        SetWindowLongPtrW(
            hwnd,
            GWL_STYLE,
            ((style | WS_POPUP.0 | WS_VISIBLE.0) & !WS_CHILD.0) as isize,
        );

        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        // Toggling WS_EX_LAYERED drops SetLayeredWindowAttributes state.
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (ex & !WS_EX_LAYERED.0) as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, (ex | WS_EX_LAYERED.0) as isize);
    }
}

/// Present a top-down BGRA buffer onto a WS_EX_LAYERED wallpaper window.
pub fn present_bgra(hwnd: HWND, bgra: &[u8], width: i32, height: i32, alpha: u8) -> Result<()> {
    if width <= 0 || height <= 0 {
        bail!("invalid blit size");
    }
    let need = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .context("size overflow")?;
    if bgra.len() < need {
        bail!("frame buffer too small");
    }

    prepare_hwnd(hwnd);

    unsafe {
        let hdc_screen = GetDC(None);
        if hdc_screen.0.is_null() {
            bail!("GetDC(screen) failed");
        }
        let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
        if hdc_mem.0.is_null() {
            ReleaseDC(None, hdc_screen);
            bail!("CreateCompatibleDC failed");
        }

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

        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let hbmp = match CreateDIBSection(Some(hdc_mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(h) => h,
            Err(err) => {
                let _ = DeleteDC(hdc_mem);
                ReleaseDC(None, hdc_screen);
                return Err(err).context("CreateDIBSection");
            }
        };
        if bits.is_null() {
            let _ = DeleteObject(hbmp.into());
            let _ = DeleteDC(hdc_mem);
            ReleaseDC(None, hdc_screen);
            bail!("CreateDIBSection returned null bits");
        }

        std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, need);
        let old = SelectObject(hdc_mem, hbmp.into());

        let mut rc = windows::Win32::Foundation::RECT::default();
        let _ = GetWindowRect(hwnd, &mut rc);
        let pt_dst = POINT {
            x: rc.left,
            y: rc.top,
        };
        let size = SIZE {
            cx: width,
            cy: height,
        };
        let pt_src = POINT { x: 0, y: 0 };
        // Constant alpha only — avoids premultiplied AC_SRC_ALPHA quirks with BI_RGB DIBs.
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: alpha,
            AlphaFormat: 0,
        };

        let ok = UpdateLayeredWindow(
            hwnd,
            None,
            Some(&pt_dst),
            Some(&size),
            Some(hdc_mem),
            Some(&pt_src),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        );

        SelectObject(hdc_mem, old);
        let _ = DeleteObject(hbmp.into());
        let _ = DeleteDC(hdc_mem);
        ReleaseDC(None, hdc_screen);

        ok.with_context(|| {
            format!(
                "UpdateLayeredWindow ({}x{} @ {},{})",
                width, height, pt_dst.x, pt_dst.y
            )
        })?;
    }
    Ok(())
}
