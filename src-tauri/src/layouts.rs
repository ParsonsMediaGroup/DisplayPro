use serde::{Deserialize, Serialize};
use std::{collections::HashSet, mem::size_of};
use windows::{
    core::{BOOL, PWSTR},
    Win32::{
        Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT},
        Graphics::Gdi::{GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST},
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
        UI::WindowsAndMessaging::*,
    },
};
struct DpiGuard(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT);
fn workspace_offset(rect: &RECT) -> (i32, i32) {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if unsafe { GetMonitorInfoW(MonitorFromRect(rect, MONITOR_DEFAULTTONEAREST), &mut info) }
        .as_bool()
    {
        (
            info.rcWork.left - info.rcMonitor.left,
            info.rcWork.top - info.rcMonitor.top,
        )
    } else {
        (0, 0)
    }
}
impl DpiGuard {
    fn new() -> Self {
        Self(unsafe {
            windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(
                windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            )
        })
    }
}
impl Drop for DpiGuard {
    fn drop(&mut self) {
        if !self.0 .0.is_null() {
            unsafe {
                windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(self.0);
            }
        }
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SavedWindow {
    pub handle: usize,
    pub pid: u32,
    pub exe: String,
    pub class: String,
    pub title: String,
    pub rect: [i32; 4],
    pub maximized: bool,
    pub minimized: bool,
    #[serde(default)]
    pub launch: Option<crate::workspace::LaunchCommand>,
    #[serde(default)]
    pub restore_only: bool,
}
unsafe extern "system" fn collect(hwnd: HWND, param: LPARAM) -> BOOL {
    if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) == 0 {
        return BOOL(1);
    }
    let mut cloaked = 0u32;
    let _ = windows::Win32::Graphics::Dwm::DwmGetWindowAttribute(
        hwnd,
        windows::Win32::Graphics::Dwm::DWMWA_CLOAKED,
        (&mut cloaked as *mut u32).cast(),
        size_of::<u32>() as u32,
    );
    if cloaked != 0 {
        return BOOL(1);
    }
    if GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
        return BOOL(1);
    }
    let mut pid = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == std::process::id() {
        return BOOL(1);
    }
    let mut title = vec![0u16; GetWindowTextLengthW(hwnd) as usize + 1];
    let n = GetWindowTextW(hwnd, &mut title);
    let mut class = [0u16; 256];
    let cn = GetClassNameW(hwnd, &mut class);
    let class = String::from_utf16_lossy(&class[..cn.max(0) as usize]);
    if [
        "Progman",
        "WorkerW",
        "Shell_TrayWnd",
        "Shell_SecondaryTrayWnd",
    ]
    .contains(&class.as_str())
    {
        return BOOL(1);
    }
    let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
        return BOOL(1);
    };
    let mut path = vec![0u16; 32768];
    let mut len = path.len() as u32;
    let read = QueryFullProcessImageNameW(
        process,
        PROCESS_NAME_FORMAT(0),
        PWSTR(path.as_mut_ptr()),
        &mut len,
    );
    let _ = CloseHandle(process);
    if read.is_err() {
        return BOOL(1);
    }
    let mut p = WINDOWPLACEMENT {
        length: size_of::<WINDOWPLACEMENT>() as u32,
        ..Default::default()
    };
    if GetWindowPlacement(hwnd, &mut p).is_err() {
        return BOOL(1);
    }
    let mut r = p.rcNormalPosition;
    // Snapped windows can keep an old restore rectangle even while their
    // actual normal-state window occupies a different area of the screen.
    if !IsIconic(hwnd).as_bool() && !IsZoomed(hwnd).as_bool() {
        let mut actual = RECT::default();
        if GetWindowRect(hwnd, &mut actual).is_ok() {
            let (dx, dy) = workspace_offset(&actual);
            r = RECT {
                left: actual.left - dx,
                top: actual.top - dy,
                right: actual.right - dx,
                bottom: actual.bottom - dy,
            };
        }
    }
    if r.right <= r.left || r.bottom <= r.top {
        return BOOL(1);
    }
    (*(param.0 as *mut Vec<SavedWindow>)).push(SavedWindow {
        handle: hwnd.0 as usize,
        pid,
        exe: String::from_utf16_lossy(&path[..len as usize]),
        class,
        title: String::from_utf16_lossy(&title[..n.max(0) as usize]),
        rect: [r.left, r.top, r.right, r.bottom],
        maximized: p.showCmd == SW_SHOWMAXIMIZED.0 as u32,
        minimized: IsIconic(hwnd).as_bool(),
        launch: None,
        restore_only: false,
    });
    BOOL(1)
}
pub fn capture() -> Result<Vec<SavedWindow>, String> {
    let _dpi = DpiGuard::new();
    let mut windows = vec![];
    unsafe {
        EnumWindows(Some(collect), LPARAM(&mut windows as *mut _ as isize))
            .map_err(|e| e.to_string())?;
    }
    Ok(windows)
}
#[derive(Serialize)]
pub struct RestoreReport {
    pub restored: usize,
    pub skipped: Vec<String>,
}
// Never guess among ambiguous same-application windows; that can swap documents.
pub fn match_window(
    saved: &SavedWindow,
    current: &[SavedWindow],
    used: &HashSet<usize>,
) -> Option<usize> {
    let candidates: Vec<_> = current
        .iter()
        .enumerate()
        .filter(|(_, w)| {
            !used.contains(&w.handle)
                && w.exe.eq_ignore_ascii_case(&saved.exe)
                && w.class == saved.class
        })
        .collect();
    if let Some((i, _)) = candidates
        .iter()
        .find(|(_, w)| w.handle == saved.handle && w.pid == saved.pid)
    {
        return Some(*i);
    }
    let exact: Vec<_> = candidates
        .iter()
        .filter(|(_, w)| w.title == saved.title)
        .collect();
    if exact.len() == 1 {
        return Some(exact[0].0);
    }
    if candidates.len() == 1 {
        return Some(candidates[0].0);
    }
    None
}
pub fn restore(saved: &[SavedWindow]) -> Result<RestoreReport, String> {
    let _dpi = DpiGuard::new();
    let current = capture()?;
    let mut used = HashSet::new();
    let mut report = RestoreReport {
        restored: 0,
        skipped: vec![],
    };
    let monitors = crate::display::enumerate(false)?;
    for w in saved {
        let Some(i) = match_window(w, &current, &used) else {
            crate::diagnostics::event("placement_unmatched", crate::diagnostics::window(w));
            report
                .skipped
                .push(format!("{} — closed or ambiguous", w.title));
            continue;
        };
        let c = &current[i];
        used.insert(c.handle);
        let [mut l, mut t, mut r, mut b] = w.rect;
        // Keep at least a reachable strip on a connected display when topology changed.
        if !monitors.iter().filter(|m| m.enabled).any(|m| {
            r > m.x + 60
                && l < m.x + m.width as i32 - 60
                && b > m.y + 60
                && t < m.y + m.height as i32 - 60
        }) {
            let m = monitors
                .iter()
                .find(|m| m.primary)
                .ok_or("No primary display")?;
            let width = (r - l).min(m.width as i32 - 80).max(100);
            let height = (b - t).min(m.height as i32 - 100).max(100);
            l = m.x + 40;
            t = m.y + 40;
            r = l + width;
            b = t + height;
        }
        let p = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            showCmd: if w.minimized {
                SW_SHOWMINNOACTIVE.0 as u32
            } else if w.maximized {
                SW_SHOWMAXIMIZED.0 as u32
            } else {
                SW_RESTORE.0 as u32
            },
            flags: WPF_ASYNCWINDOWPLACEMENT,
            ptMinPosition: POINT { x: -1, y: -1 },
            ptMaxPosition: POINT { x: -1, y: -1 },
            rcNormalPosition: RECT {
                left: l,
                top: t,
                right: r,
                bottom: b,
            },
            ..Default::default()
        };
        let hwnd = HWND(c.handle as *mut _);
        let outcome = unsafe { SetWindowPlacement(hwnd, &p) };
        let position_result = if outcome.is_ok() && !w.maximized && !w.minimized {
            let (dx, dy) = workspace_offset(&p.rcNormalPosition);
            Some(unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    l + dx,
                    t + dy,
                    r - l,
                    b - t,
                    SWP_NOACTIVATE | SWP_NOZORDER | SWP_ASYNCWINDOWPOS,
                )
            })
        } else {
            None
        };
        let mut outer = RECT::default();
        let outer_result = unsafe { GetWindowRect(hwnd, &mut outer) };
        let mut measured = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        let measured_result = unsafe { GetWindowPlacement(hwnd, &mut measured) };
        crate::diagnostics::event(
            "placement_attempt",
            serde_json::json!({
                "requested":crate::diagnostics::window(w), "matched_before":crate::diagnostics::window(c),
                "applied_rect":[l,t,r,b], "adjusted_for_reachability":w.rect != [l,t,r,b],
                "api_error":outcome.as_ref().err().map(|e|e.to_string()),
                "set_window_pos_error":position_result.as_ref().and_then(|r|r.as_ref().err()).map(|e|e.to_string()),
                "dpi":unsafe { windows::Win32::UI::HiDpi::GetDpiForWindow(hwnd) },
                "immediate_outer_rect":outer_result.is_ok().then_some([outer.left,outer.top,outer.right,outer.bottom]),
                "immediate_normal_rect":measured_result.is_ok().then_some([measured.rcNormalPosition.left,measured.rcNormalPosition.top,measured.rcNormalPosition.right,measured.rcNormalPosition.bottom]),
                "measurement_note":"SetWindowPlacement is asynchronous; workspace_final records settled measurements"
            }),
        );
        match outcome.and(position_result.unwrap_or(Ok(()))) {
            Ok(()) => report.restored += 1,
            Err(e) => report.skipped.push(format!("{} — {}", w.title, e)),
        }
    }
    Ok(report)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn w(h: usize, title: &str) -> SavedWindow {
        SavedWindow {
            handle: h,
            pid: 1,
            exe: "app.exe".into(),
            class: "App".into(),
            title: title.into(),
            rect: [0, 0, 100, 100],
            maximized: false,
            minimized: false,
            launch: None,
            restore_only: false,
        }
    }
    #[test]
    fn matching_avoids_ambiguous_documents() {
        let saved = w(1, "old");
        let current = vec![w(2, "new A"), w(3, "new B")];
        assert_eq!(match_window(&saved, &current, &HashSet::new()), None);
        assert_eq!(
            match_window(&w(9, "new B"), &current, &HashSet::new()),
            Some(1)
        );
    }
}
