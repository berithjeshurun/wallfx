//! Workstation lock / unlock notifications.

use anyhow::{Context, Result};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::RemoteDesktop::{
    WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    WM_WTSSESSION_CHANGE, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};

pub fn register(hwnd: HWND) -> Result<()> {
    unsafe {
        WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)
            .context("WTSRegisterSessionNotification")?;
    }
    Ok(())
}

pub fn unregister(hwnd: HWND) {
    unsafe {
        let _ = WTSUnRegisterSessionNotification(hwnd);
    }
}

pub fn is_lock_msg(msg: u32, wparam: usize) -> Option<bool> {
    if msg != WM_WTSSESSION_CHANGE {
        return None;
    }
    match wparam as u32 {
        WTS_SESSION_LOCK => Some(true),
        WTS_SESSION_UNLOCK => Some(false),
        _ => None,
    }
}
