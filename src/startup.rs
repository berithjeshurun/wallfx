//! Start with Windows via HKCU\...\Run.

use anyhow::{bail, Context, Result};
use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegSetValueExW, HKEY_CURRENT_USER, KEY_WRITE,
    REG_SZ,
};

const RUN_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "WallFX";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn is_enabled() -> bool {
    use windows::Win32::System::Registry::{RegQueryValueExW, KEY_READ, REG_VALUE_TYPE};
    unsafe {
        let mut key = Default::default();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(wide(RUN_SUBKEY).as_ptr()),
            Some(0),
            KEY_READ,
            &mut key,
        )
        .is_err()
        {
            return false;
        }
        let mut ty = REG_VALUE_TYPE::default();
        let mut size = 0u32;
        let ok = RegQueryValueExW(
            key,
            PCWSTR(wide(VALUE_NAME).as_ptr()),
            None,
            Some(&mut ty),
            None,
            Some(&mut size),
        )
        .is_ok();
        let _ = RegCloseKey(key);
        ok
    }
}

pub fn set_enabled(enabled: bool) -> Result<()> {
    if enabled {
        let exe = std::env::current_exe().context("current_exe")?;
        let cmd = format!("\"{}\"", exe.display());
        let cmd_w = wide(&cmd);
        let name_w = wide(VALUE_NAME);
        let sub_w = wide(RUN_SUBKEY);
        unsafe {
            let mut key = Default::default();
            let err = RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(sub_w.as_ptr()),
                Some(0),
                KEY_WRITE,
                &mut key,
            );
            if err.is_err() {
                bail!("RegOpenKeyExW Run failed: {err:?}");
            }
            let bytes_len = (cmd_w.len() * 2) as u32;
            let err = RegSetValueExW(
                key,
                PCWSTR(name_w.as_ptr()),
                Some(0),
                REG_SZ,
                Some(std::slice::from_raw_parts(
                    cmd_w.as_ptr() as *const u8,
                    bytes_len as usize,
                )),
            );
            let _ = RegCloseKey(key);
            if err.is_err() {
                bail!("RegSetValueExW WallFX failed: {err:?}");
            }
        }
    } else {
        let name_w = wide(VALUE_NAME);
        let sub_w = wide(RUN_SUBKEY);
        unsafe {
            let mut key = Default::default();
            if RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(sub_w.as_ptr()),
                Some(0),
                KEY_WRITE,
                &mut key,
            )
            .is_ok()
            {
                let _ = RegDeleteValueW(key, PCWSTR(name_w.as_ptr()));
                let _ = RegCloseKey(key);
            }
        }
    }
    Ok(())
}
