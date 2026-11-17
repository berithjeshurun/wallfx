//! Display enumeration for multi-monitor wallpaper.

use anyhow::Result;
use std::sync::Mutex;
use windows::core::BOOL;
use windows::Win32::Foundation::{LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW,
};

#[derive(Debug, Clone)]
pub struct MonitorInfo {
    pub id: String,
    pub name: String,
    pub rect: RECT,
    pub is_primary: bool,
}

pub fn enumerate() -> Result<Vec<MonitorInfo>> {
    let collected = Mutex::new(Vec::<MonitorInfo>::new());
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(enum_proc),
            LPARAM(&collected as *const _ as isize),
        );
    }
    Ok(collected.into_inner().unwrap_or_default())
}

unsafe extern "system" fn enum_proc(
    monitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let list = &*(data.0 as *const Mutex<Vec<MonitorInfo>>);
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(monitor, &mut info as *mut _ as *mut _).as_bool() {
        let name = String::from_utf16_lossy(
            &info
                .szDevice
                .iter()
                .copied()
                .take_while(|&c| c != 0)
                .collect::<Vec<_>>(),
        );
        let primary = (info.monitorInfo.dwFlags & 1) != 0;
        let id = format!(
            "{}:{}x{}+{}+{}",
            name,
            info.monitorInfo.rcMonitor.right - info.monitorInfo.rcMonitor.left,
            info.monitorInfo.rcMonitor.bottom - info.monitorInfo.rcMonitor.top,
            info.monitorInfo.rcMonitor.left,
            info.monitorInfo.rcMonitor.top
        );
        if let Ok(mut guard) = list.lock() {
            guard.push(MonitorInfo {
                id,
                name,
                rect: info.monitorInfo.rcMonitor,
                is_primary: primary,
            });
        }
    }
    BOOL(1)
}

pub fn selected(ids: &[String]) -> Result<Vec<MonitorInfo>> {
    let all = enumerate()?;
    if ids.is_empty() {
        return Ok(all);
    }
    let chosen: Vec<_> = all
        .into_iter()
        .filter(|m| ids.iter().any(|id| id == &m.id || id == &m.name))
        .collect();
    if chosen.is_empty() {
        return enumerate();
    }
    Ok(chosen)
}
