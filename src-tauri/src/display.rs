use serde::{Deserialize, Serialize};
use std::{collections::HashSet, mem::size_of};
use windows::{
    core::PCWSTR,
    Win32::{Foundation::POINTL, Graphics::Gdi::*},
};

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn string(s: &[u16]) -> String {
    String::from_utf16_lossy(&s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())])
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Mode {
    pub width: u32,
    pub height: u32,
    pub hz: u32,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Monitor {
    pub device: String,
    pub identity: String,
    pub label: String,
    pub primary: bool,
    pub enabled: bool,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub hz: u32,
    pub orientation: u32,
    pub bits: u32,
    #[serde(default)]
    pub modes: Vec<Mode>,
}
fn dm() -> DEVMODEW {
    DEVMODEW {
        dmSize: size_of::<DEVMODEW>() as u16,
        ..Default::default()
    }
}
pub fn enumerate(with_modes: bool) -> Result<Vec<Monitor>, String> {
    let mut result = vec![];
    let names = friendly_names();
    unsafe {
        for i in 0..64 {
            let mut d = DISPLAY_DEVICEW {
                cb: size_of::<DISPLAY_DEVICEW>() as u32,
                ..Default::default()
            };
            if !EnumDisplayDevicesW(PCWSTR::null(), i, &mut d, 0).as_bool() {
                break;
            }
            if d.StateFlags.contains(DISPLAY_DEVICE_MIRRORING_DRIVER) {
                continue;
            }
            let device = string(&d.DeviceName);
            let name = wide(&device);
            let mut child = DISPLAY_DEVICEW {
                cb: size_of::<DISPLAY_DEVICEW>() as u32,
                ..Default::default()
            };
            if !EnumDisplayDevicesW(PCWSTR(name.as_ptr()), 0, &mut child, 1).as_bool() {
                continue;
            }
            let mut current = dm();
            let enabled = d.StateFlags.contains(DISPLAY_DEVICE_ATTACHED_TO_DESKTOP);
            if !EnumDisplaySettingsW(
                PCWSTR(name.as_ptr()),
                if enabled {
                    ENUM_CURRENT_SETTINGS
                } else {
                    ENUM_REGISTRY_SETTINGS
                },
                &mut current,
            )
            .as_bool()
            {
                continue;
            }
            let mut modes = vec![];
            if current.dmBitsPerPel == 0 {
                current.dmBitsPerPel = 32;
            }
            if with_modes {
                for n in 0..4096 {
                    let mut m = dm();
                    if !EnumDisplaySettingsW(
                        PCWSTR(name.as_ptr()),
                        ENUM_DISPLAY_SETTINGS_MODE(n),
                        &mut m,
                    )
                    .as_bool()
                    {
                        break;
                    }
                    if m.dmBitsPerPel == current.dmBitsPerPel
                        && m.dmPelsWidth > 0
                        && m.dmDisplayFrequency > 1
                        && m.Anonymous2.dmDisplayFlags & 2 == 0
                    {
                        modes.push(Mode {
                            width: m.dmPelsWidth,
                            height: m.dmPelsHeight,
                            hz: m.dmDisplayFrequency,
                        });
                    }
                }
                modes.sort();
                modes.dedup();
                modes.reverse();
            }
            let pos = current.Anonymous1.Anonymous2;
            let label = names
                .get(&device)
                .cloned()
                .unwrap_or_else(|| string(&child.DeviceString));
            result.push(Monitor {
                device,
                identity: string(&child.DeviceID),
                label,
                primary: d.StateFlags.contains(DISPLAY_DEVICE_PRIMARY_DEVICE),
                enabled,
                x: pos.dmPosition.x,
                y: pos.dmPosition.y,
                width: current.dmPelsWidth,
                height: current.dmPelsHeight,
                hz: current.dmDisplayFrequency,
                orientation: pos.dmDisplayOrientation.0,
                bits: current.dmBitsPerPel,
                modes,
            });
        }
    }
    if result.is_empty() {
        Err("Windows reported no configurable displays.".into())
    } else {
        Ok(result)
    }
}
fn friendly_names() -> std::collections::HashMap<String, String> {
    use windows::Win32::Devices::Display::*;
    let mut names = std::collections::HashMap::new();
    unsafe {
        let (mut np, mut nm) = (0, 0);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm).0 != 0 {
            return names;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut np,
            paths.as_mut_ptr(),
            &mut nm,
            modes.as_mut_ptr(),
            None,
        )
        .0 != 0
        {
            return names;
        }
        for p in paths.iter().take(np as usize) {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            source.header = DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                size: size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                adapterId: p.sourceInfo.adapterId,
                id: p.sourceInfo.id,
            };
            let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            target.header = DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                size: size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32,
                adapterId: p.targetInfo.adapterId,
                id: p.targetInfo.id,
            };
            if DisplayConfigGetDeviceInfo(&mut source.header) == 0
                && DisplayConfigGetDeviceInfo(&mut target.header) == 0
            {
                let label = string(&target.monitorFriendlyDeviceName);
                if !label.is_empty() {
                    names.insert(string(&source.viewGdiDeviceName), label);
                }
            }
        }
    }
    names
}
pub fn validate(monitors: &[Monitor]) -> Result<(), String> {
    if monitors.is_empty() || monitors.len() > 32 {
        return Err("Select at least one display.".into());
    }
    let active: Vec<_> = monitors.iter().filter(|m| m.enabled).collect();
    if active.is_empty() || active.iter().filter(|m| m.primary).count() != 1 {
        return Err("Exactly one enabled display must be primary.".into());
    }
    let mut names = HashSet::new();
    for m in monitors {
        if !names.insert(&m.device) {
            return Err("Duplicate display in profile.".into());
        }
        if m.enabled
            && (m.width < 320
                || m.height < 200
                || m.width > 32768
                || m.height > 32768
                || m.hz < 2
                || m.hz > 1000
                || m.x.unsigned_abs() > 100000
                || m.y.unsigned_abs() > 100000)
        {
            return Err("Invalid display geometry or refresh rate.".into());
        }
    }
    for (i, a) in active.iter().enumerate() {
        for b in active.iter().skip(i + 1) {
            if a.x < b.x + b.width as i32
                && b.x < a.x + a.width as i32
                && a.y < b.y + b.height as i32
                && b.y < a.y + a.height as i32
            {
                return Err("Displays overlap. Adjust their X/Y positions before applying.".into());
            }
        }
    }
    Ok(())
}
pub fn normalize(monitors: &mut [Monitor]) {
    if let Some(p) = monitors.iter().find(|m| m.primary && m.enabled) {
        let (x, y) = (p.x, p.y);
        for m in monitors {
            m.x = m.x.saturating_sub(x);
            m.y = m.y.saturating_sub(y);
        }
    }
}
fn native(m: &Monitor) -> DEVMODEW {
    let mut d = dm();
    d.dmFields = DM_POSITION | DM_PELSWIDTH | DM_PELSHEIGHT;
    d.Anonymous1.Anonymous2.dmPosition = POINTL { x: m.x, y: m.y };
    if m.enabled {
        d.dmPelsWidth = m.width;
        d.dmPelsHeight = m.height;
        d.dmDisplayFrequency = m.hz;
        d.dmBitsPerPel = m.bits;
        d.Anonymous1.Anonymous2.dmDisplayOrientation = DEVMODE_DISPLAY_ORIENTATION(m.orientation);
        d.dmFields |= DM_DISPLAYFREQUENCY | DM_BITSPERPEL | DM_DISPLAYORIENTATION;
    }
    d
}
pub fn test(monitors: &[Monitor]) -> Result<(), String> {
    validate(monitors)?;
    let current = enumerate(false)?;
    if current.len() != monitors.len() {
        return Err("Connected displays changed. Refresh and recapture this profile.".into());
    }
    for m in monitors {
        if !current
            .iter()
            .any(|c| c.device == m.device && c.identity == m.identity)
        {
            return Err(format!("Display {} is missing or its connection changed. Refresh and recapture the profile.", m.label));
        }
        let name = wide(&m.device);
        let d = native(m);
        let status = unsafe {
            ChangeDisplaySettingsExW(PCWSTR(name.as_ptr()), Some(&d), None, CDS_TEST, None)
        };
        if status != DISP_CHANGE_SUCCESSFUL {
            return Err(format!(
                "Windows rejected {}: {}×{} at {} Hz (code {}).",
                m.label, m.width, m.height, m.hz, status.0
            ));
        }
    }
    Ok(())
}
pub fn apply(monitors: &[Monitor]) -> Result<(), String> {
    // Stage every output before committing the complete topology once.
    for m in monitors
        .iter()
        .filter(|m| m.primary)
        .chain(monitors.iter().filter(|m| !m.primary))
    {
        let name = wide(&m.device);
        let d = native(m);
        let flags = CDS_UPDATEREGISTRY
            | CDS_NORESET
            | if m.primary {
                CDS_SET_PRIMARY
            } else {
                CDS_TYPE(0)
            };
        let status =
            unsafe { ChangeDisplaySettingsExW(PCWSTR(name.as_ptr()), Some(&d), None, flags, None) };
        if status != DISP_CHANGE_SUCCESSFUL {
            return Err(format!("Could not stage {} (code {}).", m.label, status.0));
        }
    }
    let result = unsafe { ChangeDisplaySettingsExW(PCWSTR::null(), None, None, CDS_TYPE(0), None) };
    if result != DISP_CHANGE_SUCCESSFUL {
        Err(format!(
            "Windows could not apply the layout (code {}).",
            result.0
        ))
    } else {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn monitor(x: i32, primary: bool) -> Monitor {
        Monitor {
            device: x.to_string(),
            identity: String::new(),
            label: String::new(),
            primary,
            enabled: true,
            x,
            y: 0,
            width: 1920,
            height: 1080,
            hz: 60,
            orientation: 0,
            bits: 32,
            modes: vec![],
        }
    }
    #[test]
    fn prevents_overlaps_and_all_disabled() {
        assert!(validate(&[monitor(0, true), monitor(100, false)]).is_err());
        assert!(validate(&[monitor(0, true), monitor(1920, false)]).is_ok());
        let mut m = monitor(0, true);
        m.enabled = false;
        assert!(validate(&[m]).is_err());
    }
    #[test]
    fn translates_primary_to_origin() {
        let mut m = vec![monitor(-1920, false), monitor(0, true)];
        m[0].primary = true;
        m[1].primary = false;
        normalize(&mut m);
        assert_eq!(m[0].x, 0);
        assert_eq!(m[1].x, 1920);
    }
}
