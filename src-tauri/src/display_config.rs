//! Whole-topology fallback for outputs rejected by the legacy per-output API.
use crate::display::Monitor;
use std::{collections::HashSet, mem::size_of};
use windows::Win32::{
    Devices::Display::*, Foundation::POINTL, Graphics::Gdi::DISPLAYCONFIG_PATH_ACTIVE,
};

fn text(s: &[u16]) -> String {
    String::from_utf16_lossy(&s[..s.iter().position(|c| *c == 0).unwrap_or(s.len())])
}
fn source_size(width: u32, height: u32, orientation: u32) -> (u32, u32) {
    if orientation % 2 == 0 {
        (width, height)
    } else {
        (height, width)
    }
}
fn query(
    flags: QUERY_DISPLAY_CONFIG_FLAGS,
) -> Result<(Vec<DISPLAYCONFIG_PATH_INFO>, Vec<DISPLAYCONFIG_MODE_INFO>), String> {
    for _ in 0..3 {
        let (mut np, mut nm) = (0, 0);
        let code = unsafe { GetDisplayConfigBufferSizes(flags, &mut np, &mut nm) }.0;
        if code != 0 {
            return Err(format!("Display path inventory failed ({code})."));
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        let code = unsafe {
            QueryDisplayConfig(
                flags,
                &mut np,
                paths.as_mut_ptr(),
                &mut nm,
                modes.as_mut_ptr(),
                None,
            )
        }
        .0;
        if code == 122 {
            continue;
        }
        if code != 0 {
            return Err(format!("Display path query failed ({code})."));
        }
        paths.truncate(np as usize);
        modes.truncate(nm as usize);
        return Ok((paths, modes));
    }
    Err("Displays changed during detection. Refresh and retry.".into())
}

pub fn validate_current() -> Result<(), String> {
    let (paths, modes) = query(QDC_ONLY_ACTIVE_PATHS)?;
    describe("native", &paths, &modes);
    let code = unsafe {
        SetDisplayConfig(
            Some(&paths),
            Some(&modes),
            SDC_VALIDATE | SDC_USE_SUPPLIED_DISPLAY_CONFIG,
        )
    };
    if code == 0 {
        Ok(())
    } else {
        Err(format!("Current native configuration rejected ({code})."))
    }
}
fn describe(kind: &str, paths: &[DISPLAYCONFIG_PATH_INFO], modes: &[DISPLAYCONFIG_MODE_INFO]) {
    crate::diagnostics::event(
        "display_paths",
        serde_json::json!({"kind":kind,"paths":paths.iter().map(|p|unsafe { serde_json::json!({"source":p.sourceInfo.id,"target":p.targetInfo.id,"rotation":p.targetInfo.rotation.0,"scaling":p.targetInfo.scaling.0,"refresh":[p.targetInfo.refreshRate.Numerator,p.targetInfo.refreshRate.Denominator],"scan":p.targetInfo.scanLineOrdering.0,"flags":p.flags,"source_mode":p.sourceInfo.Anonymous.modeInfoIdx,"target_mode":p.targetInfo.Anonymous.modeInfoIdx}) }).collect::<Vec<_>>(),"modes":modes.iter().map(|m|unsafe { if m.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE { let s=m.Anonymous.sourceMode;serde_json::json!({"source":m.id,"size":[s.width,s.height],"position":[s.position.x,s.position.y],"pixel":s.pixelFormat.0}) } else { let t=m.Anonymous.targetMode.targetVideoSignalInfo;serde_json::json!({"target":m.id,"size":[t.activeSize.cx,t.activeSize.cy],"refresh":[t.vSyncFreq.Numerator,t.vSyncFreq.Denominator]}) } }).collect::<Vec<_>>()}),
    );
}
pub fn configure(monitors: &[Monitor], apply: bool) -> Result<(), String> {
    crate::display::validate(monitors)?;
    let (available, current_modes) = query(QDC_ALL_PATHS)?;
    let mut used_sources = HashSet::new();
    let mut used_targets = HashSet::new();
    let mut selected = vec![];
    let mut modes = vec![];
    let mut active: Vec<_> = monitors.iter().filter(|m| m.enabled).collect();
    active.sort_by_key(|m| !m.primary);
    for monitor in active {
        if monitor.bits != 32 {
            return Err("Whole-layout switching requires 32-bit display color.".into());
        }
        let mut candidates = vec![];
        for path in &available {
            if !path.targetInfo.targetAvailable.as_bool() {
                continue;
            }
            let source_key = (
                path.sourceInfo.adapterId.HighPart,
                path.sourceInfo.adapterId.LowPart,
                path.sourceInfo.id,
            );
            let target_key = (
                path.targetInfo.adapterId.HighPart,
                path.targetInfo.adapterId.LowPart,
                path.targetInfo.id,
            );
            if used_sources.contains(&source_key) || used_targets.contains(&target_key) {
                continue;
            }
            let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            target.header = DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME,
                size: size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32,
                adapterId: path.targetInfo.adapterId,
                id: path.targetInfo.id,
            };
            if unsafe { DisplayConfigGetDeviceInfo(&mut target.header) } != 0
                || !text(&target.monitorDevicePath).eq_ignore_ascii_case(&monitor.identity)
            {
                continue;
            }
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            source.header = DISPLAYCONFIG_DEVICE_INFO_HEADER {
                r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                size: size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                adapterId: path.sourceInfo.adapterId,
                id: path.sourceInfo.id,
            };
            let same_source = unsafe { DisplayConfigGetDeviceInfo(&mut source.header) } == 0
                && text(&source.viewGdiDeviceName) == monitor.device;
            candidates.push((
                !same_source,
                path.flags & DISPLAYCONFIG_PATH_ACTIVE == 0,
                *path,
            ));
        }
        candidates.sort_by_key(|(different, inactive, _)| (*different, *inactive));
        let Some((_, _, mut path)) = candidates.into_iter().next() else {
            return Err(format!("No available display path for {}.", monitor.label));
        };
        used_sources.insert((
            path.sourceInfo.adapterId.HighPart,
            path.sourceInfo.adapterId.LowPart,
            path.sourceInfo.id,
        ));
        used_targets.insert((
            path.targetInfo.adapterId.HighPart,
            path.targetInfo.adapterId.LowPart,
            path.targetInfo.id,
        ));
        let existing_target = unsafe { path.targetInfo.Anonymous.modeInfoIdx };
        path.flags = DISPLAYCONFIG_PATH_ACTIVE;
        path.sourceInfo.Anonymous.modeInfoIdx = modes.len() as u32;
        path.targetInfo.Anonymous.modeInfoIdx = u32::MAX;
        path.targetInfo.rotation = DISPLAYCONFIG_ROTATION(monitor.orientation as i32 + 1);
        path.targetInfo.scaling = DISPLAYCONFIG_SCALING_IDENTITY;
        path.targetInfo.refreshRate = DISPLAYCONFIG_RATIONAL {
            Numerator: monitor.hz,
            Denominator: 1,
        };
        path.targetInfo.scanLineOrdering = DISPLAYCONFIG_SCANLINE_ORDERING_PROGRESSIVE;
        let (source_width, source_height) =
            source_size(monitor.width, monitor.height, monitor.orientation);
        modes.push(DISPLAYCONFIG_MODE_INFO {
            infoType: DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE,
            id: path.sourceInfo.id,
            adapterId: path.sourceInfo.adapterId,
            Anonymous: DISPLAYCONFIG_MODE_INFO_0 {
                sourceMode: DISPLAYCONFIG_SOURCE_MODE {
                    width: source_width,
                    height: source_height,
                    pixelFormat: DISPLAYCONFIG_PIXELFORMAT_32BPP,
                    position: POINTL {
                        x: monitor.x,
                        y: monitor.y,
                    },
                },
            },
        });
        // Retain the driver's exact signal timing for an already active mode.
        // Integer refresh labels (e.g. 240 Hz) can hide fractional/DSC timings.
        if let Some(existing) = current_modes.get(existing_target as usize) {
            if existing.infoType == DISPLAYCONFIG_MODE_INFO_TYPE_TARGET {
                let signal = unsafe { existing.Anonymous.targetMode.targetVideoSignalInfo };
                let (width, height) = if monitor.orientation % 2 == 0 {
                    (monitor.width, monitor.height)
                } else {
                    (monitor.height, monitor.width)
                };
                let hz = if signal.vSyncFreq.Denominator == 0 {
                    0
                } else {
                    ((signal.vSyncFreq.Numerator as u64 + signal.vSyncFreq.Denominator as u64 / 2)
                        / signal.vSyncFreq.Denominator as u64) as u32
                };
                if signal.activeSize.cx == width
                    && signal.activeSize.cy == height
                    && hz == monitor.hz
                {
                    path.targetInfo.Anonymous.modeInfoIdx = modes.len() as u32;
                    modes.push(*existing);
                }
            }
        }
        selected.push(path);
    }
    let flags = SDC_USE_SUPPLIED_DISPLAY_CONFIG
        | if apply {
            SDC_APPLY | SDC_SAVE_TO_DATABASE
        } else {
            SDC_VALIDATE
        };
    describe("constructed", &selected, &modes);
    let code = unsafe { SetDisplayConfig(Some(&selected), Some(&modes), flags) };
    crate::diagnostics::event(
        "display_configuration",
        serde_json::json!({"apply":apply,"code":code,"monitors":monitors}),
    );
    if code == 0 {
        Ok(())
    } else {
        Err(format!(
            "Windows rejected the complete display configuration (code {code})."
        ))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn portrait_desktop_uses_unrotated_source_dimensions() {
        assert_eq!(super::source_size(1080, 1920, 1), (1920, 1080));
        assert_eq!(super::source_size(1080, 1920, 3), (1920, 1080));
        assert_eq!(super::source_size(3440, 1440, 0), (3440, 1440));
    }
}
