//! Native file open dialog.

use anyhow::{bail, Result};
use std::path::PathBuf;
use windows::core::PCWSTR;
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, OFN_FILEMUSTEXIST, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn browse_video() -> Result<Option<PathBuf>> {
    browse(
        "Select wallpaper video",
        "Video files\0*.mp4;*.webm;*.mkv;*.avi;*.mov;*.wmv\0All files\0*.*\0",
    )
}

pub fn browse_wallpaper_source() -> Result<Option<PathBuf>> {
    browse(
        "Select video or parallax pack",
        "Wallpaper\0*.mp4;*.webm;*.mkv;*.avi;*.mov;*.wmv;*.zip;*.jpg;*.jpeg;*.png\0Video\0*.mp4;*.webm;*.mkv;*.avi;*.mov;*.wmv\0Parallax zip\0*.zip\0Images\0*.jpg;*.jpeg;*.png\0All files\0*.*\0",
    )
}

pub fn browse_lut() -> Result<Option<PathBuf>> {
    browse(
        "Select LUT (.cube / .3dl)",
        "LUT files\0*.cube;*.3dl\0All files\0*.*\0",
    )
}

fn browse(title: &str, filter: &str) -> Result<Option<PathBuf>> {
    let mut file = vec![0u16; 1024];
    let filter = wide(filter);
    let title = wide(title);
    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: unsafe { GetForegroundWindow() },
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: windows::core::PWSTR(file.as_mut_ptr()),
        nMaxFile: file.len() as u32,
        lpstrTitle: PCWSTR(title.as_ptr()),
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    let ok = unsafe { GetOpenFileNameW(&mut ofn as *mut _) }.as_bool();
    if !ok {
        return Ok(None);
    }
    let path = String::from_utf16_lossy(&file)
        .trim_end_matches('\0')
        .to_string();
    if path.is_empty() {
        bail!("empty path from dialog");
    }
    Ok(Some(PathBuf::from(path)))
}
