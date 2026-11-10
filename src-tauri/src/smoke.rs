//! Native integration checks. Only the dedicated temporary test window is moved.
use std::{
    os::windows::process::CommandExt,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{HWND, LPARAM, WPARAM},
        UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
    },
};
pub fn test_window(title: &str) -> Result<(), String> {
    unsafe {
        let name = crate::display::wide(title);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            PCWSTR(name.as_ptr()),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            80,
            80,
            420,
            240,
            None,
            None,
            None,
            None,
        )
        .map_err(|e| e.to_string())?;
        let started = Instant::now();
        let mut msg = MSG::default();
        let mut changed = false;
        while IsWindow(Some(hwnd)).as_bool() && started.elapsed() < Duration::from_secs(45) {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if title.contains("workspace verification")
                && !changed
                && started.elapsed() >= Duration::from_secs(4)
            {
                changed = true;
                let ready = crate::display::wide(&format!("{title} ready"));
                let _ = SetWindowTextW(hwnd, PCWSTR(ready.as_ptr()));
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    300,
                    300,
                    460,
                    280,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if IsWindow(Some(hwnd)).as_bool() {
            let _ = DestroyWindow(hwnd);
        }
        Ok(())
    }
}
pub fn run(path: &Path) -> Result<(), String> {
    let started = Instant::now();
    let monitors = crate::display::enumerate(true)?;
    crate::display::test(&monitors)?;
    let saver = crate::screensaver::get()?;
    let title = format!("DisplayProLite verification {}", std::process::id());
    let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .args(["--test-window", &title])
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|e| e.to_string())?;
    let result = (|| {
        let mut found = None;
        for _ in 0..60 {
            found = crate::layouts::capture()?
                .into_iter()
                .find(|w| w.title == title && w.pid == child.id());
            if found.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let window = found.ok_or("Could not capture the temporary test window")?;
        let hwnd = HWND(window.handle as *mut _);
        unsafe {
            SetWindowPos(
                hwnd,
                None,
                220,
                200,
                560,
                360,
                SWP_NOZORDER | SWP_NOACTIVATE,
            )
            .map_err(|e| e.to_string())?;
        }
        let moved = crate::layouts::capture()?
            .into_iter()
            .find(|w| w.handle == window.handle)
            .ok_or("Test window vanished")?;
        if moved.rect == window.rect {
            return Err("Test window did not move".into());
        }
        let report = crate::layouts::restore(&[window.clone()])?;
        if report.restored != 1 {
            return Err(format!("Window restore failed: {:?}", report.skipped));
        }
        std::thread::sleep(Duration::from_millis(150));
        let restored = crate::layouts::capture()?
            .into_iter()
            .find(|w| w.handle == window.handle)
            .ok_or("Restored window vanished")?;
        if restored.rect != window.rect {
            return Err(format!(
                "Window bounds mismatch: {:?} versus {:?}",
                restored.rect, window.rect
            ));
        }
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        let mut registered = None;
        for key in 0x70..=0x7A {
            if unsafe {
                RegisterHotKey(
                    None,
                    0x5D01,
                    MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT,
                    key,
                )
            }
            .is_ok()
            {
                registered = Some(key);
                break;
            }
        }
        let key = registered.ok_or("No spare test hotkey available")?;
        let conflict = unsafe {
            RegisterHotKey(
                None,
                0x5D02,
                MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT,
                key,
            )
        };
        unsafe {
            let _ = UnregisterHotKey(None, 0x5D01);
            if conflict.is_ok() {
                let _ = UnregisterHotKey(None, 0x5D02);
            }
        }
        if conflict.is_ok() {
            return Err("Expected native hotkey conflict was not detected".into());
        }
        let temp = path.with_extension("roundtrip.json");
        let expected = serde_json::json!({"profile":"test","windows":[window],"monitors":monitors});
        crate::write_json(&temp, &expected)?;
        crate::write_json(&temp, &expected)?;
        let actual: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&temp).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&temp);
        if actual != expected {
            return Err("Profile persistence mismatch".into());
        }
        Ok(
            serde_json::json!({"success":true,"checks":["enumerate displays and supported modes","CDS_TEST validates current monitor modes without applying changes","read Windows screensaver settings","capture dedicated test window","move dedicated test window","restore exact original window placement","native global hotkey registration and conflict detection","atomic profile JSON save, replace and load"],"displayCount":monitors.len(),"screensaverEnabled":saver.enabled,"elapsedMs":started.elapsed().as_millis()}),
        )
    })();
    let _ = child.kill();
    let _ = child.wait();
    match result {
        Ok(report) => crate::write_json(path, &report),
        Err(e) => {
            let _ = crate::write_json(path, &serde_json::json!({"success":false,"error":e}));
            Err(e)
        }
    }
}

/// Opens only this executable's temporary test windows, never user applications.
pub fn workspace_run(path: &Path) -> Result<(), String> {
    let exe = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .to_string_lossy()
        .into_owned();
    let title = format!(
        "DisplayProLite workspace verification {}",
        std::process::id()
    );
    let slot = crate::layouts::SavedWindow {
        handle: 0,
        pid: 0,
        exe: exe.clone(),
        class: "Static".into(),
        title: title.clone(),
        rect: [80, 80, 600, 400],
        maximized: false,
        minimized: false,
        launch: None,
        restore_only: false,
    };
    // Windows reports the built-in class name as Static.
    let mut second = slot.clone();
    second.rect = [700, 80, 1220, 400];
    if let Some(m) = crate::display::enumerate(false)?
        .iter()
        .find(|m| m.enabled && !m.primary)
    {
        second.rect = [m.x + 80, m.y + 80, m.x + 600, m.y + 400];
    }
    let profile = crate::Profile {
        id: "test-workspace".into(),
        name: "Synthetic test".into(),
        monitors: vec![],
        windows: vec![slot, second],
        launches: vec![crate::workspace::LaunchCommand {
            exe: exe.clone(),
            args: vec!["--test-window".into(), title.clone()],
            working_dir: String::new(),
        }],
        startup: None,
    };
    let result = (|| {
        let first = crate::workspace::open(&profile)?;
        if first.launched != 2 || first.restored != 2 || !first.skipped.is_empty() {
            return Err(format!(
                "Initial workspace result: launched {}, restored {}, skipped {:?}",
                first.launched, first.restored, first.skipped
            ));
        }
        let again = crate::workspace::open(&profile)?;
        if again.launched != 0 || again.restored != 2 || !again.skipped.is_empty() {
            return Err("Reopening the workspace did not reuse its two windows.".into());
        }
        let current = crate::layouts::capture()?;
        let closing = current
            .iter()
            .find(|w| w.exe.eq_ignore_ascii_case(&exe) && w.title.starts_with(&title))
            .ok_or("No test window to close")?;
        unsafe {
            let _ = PostMessageW(
                Some(HWND(closing.handle as *mut _)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            );
        }
        std::thread::sleep(Duration::from_millis(200));
        let missing = crate::workspace::open(&profile)?;
        if missing.launched != 1 || missing.restored != 2 || !missing.skipped.is_empty() {
            return Err(format!(
                "Missing-window refill failed: launched {}, restored {}, skipped {:?}",
                missing.launched, missing.restored, missing.skipped
            ));
        }
        let mut minimized = profile.clone();
        minimized.windows[1].minimized = true;
        let minimized_report = crate::workspace::open(&minimized)?;
        if minimized_report.launched != 0
            || minimized_report.restored != 2
            || !minimized_report.skipped.is_empty()
        {
            return Err("Minimized-window placement failed.".into());
        }
        let normal_report = crate::workspace::open(&profile)?;
        if normal_report.launched != 0
            || normal_report.restored != 2
            || !normal_report.skipped.is_empty()
        {
            return Err("Restore from minimized state failed.".into());
        }
        Ok(
            serde_json::json!({"success":true,"checks":["restore on a second monitor when available","minimize and restore normal geometry without relaunching","survive delayed app self-resize and title changes","replace exactly one missing window","launch two missing app windows with arguments","assign duplicate-title windows to separate slots","restore and verify exact positions and sizes","repeat open/load reuses windows without launching duplicates"],"launched":first.launched,"restored":first.restored}),
        )
    })();
    if let Ok(windows) = crate::layouts::capture() {
        for window in windows
            .into_iter()
            .filter(|w| w.exe.eq_ignore_ascii_case(&exe) && w.title.starts_with(&title))
        {
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(window.handle as *mut _)),
                    WM_CLOSE,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    }
    match result {
        Ok(report) => crate::write_json(path, &report),
        Err(e) => {
            let _ = crate::write_json(path, &serde_json::json!({"success":false,"error":e}));
            Err(e)
        }
    }
}
