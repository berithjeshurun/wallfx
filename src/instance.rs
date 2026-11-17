//! Single-instance lock + stop signal via a local PID file.

use anyhow::{Context, Result, bail};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

pub fn runtime_dir() -> Result<PathBuf> {
    let base = std::env::temp_dir().join("wallfx");
    fs::create_dir_all(&base).context("create runtime dir")?;
    Ok(base)
}

pub fn pid_path() -> Result<PathBuf> {
    Ok(runtime_dir()?.join("wallfx.pid"))
}

pub fn stop_path() -> Result<PathBuf> {
    Ok(runtime_dir()?.join("wallfx.stop"))
}

pub struct InstanceGuard {
    _file: File,
    pid_file: PathBuf,
}

impl InstanceGuard {
    pub fn acquire() -> Result<Self> {
        let pid_file = pid_path()?;
        if pid_file.exists() {
            if let Ok(contents) = fs::read_to_string(&pid_file) {
                if let Ok(pid) = contents.trim().parse::<u32>() {
                    if process_alive(pid) {
                        bail!(
                            "WallFX is already running (pid {pid}). Use `wallfx stop` first."
                        );
                    }
                }
            }
            let _ = fs::remove_file(&pid_file);
            let _ = stop_path().and_then(|p| {
                let _ = fs::remove_file(p);
                Ok(())
            });
        }

        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&pid_file)
            .context("open pid file")?;
        writeln!(file, "{}", std::process::id()).context("write pid")?;
        if let Ok(stop) = stop_path() {
            let _ = fs::remove_file(stop);
        }
        Ok(Self {
            _file: file,
            pid_file,
        })
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.pid_file);
        if let Ok(stop) = stop_path() {
            let _ = fs::remove_file(stop);
        }
    }
}

pub fn request_stop() -> Result<()> {
    let pid_file = pid_path()?;
    if !pid_file.exists() {
        bail!("WallFX is not running");
    }
    File::create(stop_path()?).context("create stop signal")?;
    Ok(())
}

pub fn stop_requested() -> bool {
    stop_path().map(|p| p.exists()).unwrap_or(false)
}

fn process_alive(pid: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => {
                let _ = CloseHandle(h);
                true
            }
            Err(_) => false,
        }
    }
}
