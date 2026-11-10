use crate::display::wide;
use serde::{Deserialize, Serialize};
use std::{os::windows::process::CommandExt, path::PathBuf, process::Command};
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{LPARAM, WPARAM},
        System::Registry::*,
        UI::WindowsAndMessaging::*,
    },
};
pub fn reg_read(key: &str, name: &str) -> String {
    let k = wide(key);
    let n = wide(name);
    let mut buf = vec![0u16; 32768];
    let mut size = (buf.len() * 2) as u32;
    unsafe {
        if RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(k.as_ptr()),
            PCWSTR(n.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut size),
        )
        .0 != 0
        {
            return String::new();
        }
    }
    String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())])
}
pub fn reg_write(key: &str, name: &str, value: &str) -> Result<(), String> {
    let k = wide(key);
    let n = wide(name);
    let v = wide(value);
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(k.as_ptr()),
            PCWSTR(n.as_ptr()),
            REG_SZ.0,
            Some(v.as_ptr().cast()),
            (v.len() * 2) as u32,
        )
        .ok()
        .map_err(|e| e.to_string())
    }
}
const DESKTOP: &str = "Control Panel\\Desktop";
#[derive(Clone, Serialize, Deserialize)]
pub struct Saver {
    pub enabled: bool,
    pub timeout: u32,
    pub secure: bool,
    pub path: String,
    pub available: Vec<String>,
}
pub fn get() -> Result<Saver, String> {
    let mut enabled = 0u32;
    let mut timeout = 0u32;
    let mut secure = 0u32;
    unsafe {
        SystemParametersInfoW(
            SPI_GETSCREENSAVEACTIVE,
            0,
            Some((&mut enabled as *mut u32).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .map_err(|e| e.to_string())?;
        SystemParametersInfoW(
            SPI_GETSCREENSAVETIMEOUT,
            0,
            Some((&mut timeout as *mut u32).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .map_err(|e| e.to_string())?;
        SystemParametersInfoW(
            SPI_GETSCREENSAVESECURE,
            0,
            Some((&mut secure as *mut u32).cast()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .map_err(|e| e.to_string())?;
    }
    let path = reg_read(DESKTOP, "SCRNSAVE.EXE");
    let dir =
        PathBuf::from(std::env::var_os("WINDIR").unwrap_or("C:\\Windows".into())).join("System32");
    let mut available: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("scr")))
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    if !path.is_empty() && !available.contains(&path) {
        available.push(path.clone())
    }
    available.sort();
    Ok(Saver {
        enabled: enabled != 0,
        timeout,
        secure: secure != 0,
        path,
        available,
    })
}
pub fn save(s: &Saver) -> Result<(), String> {
    if !(60..=86400).contains(&s.timeout) {
        return Err("Timeout must be between 1 and 1440 minutes.".into());
    }
    if s.enabled
        && (s.path.is_empty()
            || !PathBuf::from(&s.path).is_file()
            || !s.path.to_ascii_lowercase().ends_with(".scr"))
    {
        return Err("Select an existing .scr screensaver file.".into());
    }
    let old = get()?;
    let result = (|| {
        reg_write(DESKTOP, "SCRNSAVE.EXE", &s.path)?;
        unsafe {
            let flags = SPIF_UPDATEINIFILE | SPIF_SENDCHANGE;
            SystemParametersInfoW(SPI_SETSCREENSAVETIMEOUT, s.timeout, None, flags)
                .map_err(|e| e.to_string())?;
            SystemParametersInfoW(SPI_SETSCREENSAVESECURE, s.secure as u32, None, flags)
                .map_err(|e| e.to_string())?;
            SystemParametersInfoW(SPI_SETSCREENSAVEACTIVE, s.enabled as u32, None, flags)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = reg_write(DESKTOP, "SCRNSAVE.EXE", &old.path);
        unsafe {
            let flags = SPIF_UPDATEINIFILE | SPIF_SENDCHANGE;
            let _ = SystemParametersInfoW(SPI_SETSCREENSAVETIMEOUT, old.timeout, None, flags);
            let _ = SystemParametersInfoW(SPI_SETSCREENSAVESECURE, old.secure as u32, None, flags);
            let _ = SystemParametersInfoW(SPI_SETSCREENSAVEACTIVE, old.enabled as u32, None, flags);
        }
    }
    result
}
pub fn start() -> Result<(), String> {
    let s = get()?;
    if s.path.is_empty() {
        return Err("Choose a screensaver first.".into());
    }
    Command::new(&s.path)
        .arg("/s")
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn settings() -> Result<(), String> {
    Command::new("rundll32.exe")
        .args(["shell32.dll,Control_RunDLL", "desk.cpl,,1"])
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}
pub fn sleep_displays() {
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SYSCOMMAND,
            WPARAM(SC_MONITORPOWER as usize),
            LPARAM(2),
            SMTO_ABORTIFHUNG,
            1000,
            None,
        );
    }
}
