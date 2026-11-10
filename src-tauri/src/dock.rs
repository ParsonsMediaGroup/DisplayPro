//! Profile controls shared by the tray and native dock.
use crate::{Config, State};
use std::{fs, io::Write};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    Manager,
};
use windows::{
    core::{w, BOOL},
    Win32::{Foundation::*, System::Threading::*, UI::WindowsAndMessaging::*},
};
pub const REFRESH_DOCK: u32 = WM_APP + 120;
pub fn disk_config() -> Config {
    fs::read(crate::data_dir().join("profiles.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
pub fn selected_name(c: &Config) -> String {
    c.profiles
        .iter()
        .find(|p| Some(&p.id) == c.selected_window_profile.as_ref())
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "Choose a profile".into())
}
pub fn select(app: &tauri::AppHandle, id: String) -> Result<(), String> {
    let state = app.state::<State>();
    let mut config = state.config.lock().map_err(|e| e.to_string())?;
    if !config
        .profiles
        .iter()
        .any(|p| p.id == id && !p.windows.is_empty())
    {
        return Err("Select a saved profile with window slots.".into());
    }
    let mut new = config.clone();
    new.selected_window_profile = Some(id);
    crate::save_config(&state, &new)?;
    *config = new;
    drop(config);
    refresh_tray(app)?;
    Ok(())
}
#[tauri::command]
pub fn select_window_profile(app: tauri::AppHandle, id: String) -> Result<(), String> {
    select(&app, id)
}
#[tauri::command]
pub async fn open_window_profile(
    app: tauri::AppHandle,
    id: String,
) -> Result<crate::workspace::Report, String> {
    tauri::async_runtime::spawn_blocking(move || {
        select(&app, id.clone())?;
        let state = app.state::<State>();
        crate::workspace::open(&crate::get_profile(&state, &id)?)
    })
    .await
    .map_err(|e| e.to_string())?
}
pub fn refresh_tray(app: &tauri::AppHandle) -> Result<(), String> {
    let c = app
        .state::<State>()
        .config
        .lock()
        .map_err(|e| e.to_string())?
        .clone();
    let make = || -> tauri::Result<Menu<tauri::Wry>> {
        let menu = Menu::new(app)?;
        let displays = Submenu::new(app, "Monitor Configuration", true)?;
        displays.append(&MenuItem::with_id(
            app,
            "page:displays",
            "Open monitor editor",
            true,
            None::<&str>,
        )?)?;
        for p in c.profiles.iter().filter(|p| !p.monitors.is_empty()) {
            displays.append(&MenuItem::with_id(
                app,
                format!("display:{}", p.id),
                &p.name,
                true,
                None::<&str>,
            )?)?;
        }
        menu.append(&displays)?;
        let windows = Submenu::new(app, "Window Positions", true)?;
        windows.append(&MenuItem::with_id(
            app,
            "page:profiles",
            "Open window profile editor",
            true,
            None::<&str>,
        )?)?;
        for p in c.profiles.iter().filter(|p| !p.windows.is_empty()) {
            windows.append(&MenuItem::with_id(
                app,
                format!("select-window:{}", p.id),
                format!(
                    "{}{}",
                    if c.selected_window_profile.as_ref() == Some(&p.id) {
                        "✓ "
                    } else {
                        ""
                    },
                    p.name
                ),
                true,
                None::<&str>,
            )?)?;
        }
        menu.append(&windows)?;
        menu.append(&MenuItem::with_id(
            app,
            format!(
                "open-load:{}",
                c.selected_window_profile.as_deref().unwrap_or("")
            ),
            format!("Open and load window profile: {}", selected_name(&c)),
            c.selected_window_profile.is_some(),
            None::<&str>,
        )?)?;
        for (id, label) in [
            ("page:screensaver", "Screensavers"),
            ("page:hotkeys", "Hotkeys"),
            ("page:taskbars", "Taskbars"),
        ] {
            menu.append(&MenuItem::with_id(app, id, label, true, None::<&str>)?)?;
        }
        menu.append(&PredefinedMenuItem::separator(app)?)?;
        menu.append(&MenuItem::with_id(
            app,
            "quit",
            "Quit DisplayProLite",
            true,
            None::<&str>,
        )?)?;
        Ok(menu)
    };
    if let Some(tray) = app.tray_by_id("main") {
        tray.set_menu(Some(make().map_err(|e| e.to_string())?))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
unsafe extern "system" fn refresh(hwnd: HWND, _: LPARAM) -> BOOL {
    let mut class = [0u16; 80];
    let n = GetClassNameW(hwnd, &mut class);
    if String::from_utf16_lossy(&class[..n.max(0) as usize]) == "DisplayProLiteNativeBar" {
        let _ = PostMessageW(Some(hwnd), REFRESH_DOCK, WPARAM(0), LPARAM(0));
    }
    BOOL(1)
}
pub fn refresh_bars() {
    unsafe {
        let _ = EnumWindows(Some(refresh), LPARAM(0));
    }
}
pub fn request(action: &str) -> Result<(), String> {
    let path = crate::data_dir().join("dock-request.json");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| "Another dock action is pending. Try again shortly.")?;
    file.write_all(&serde_json::to_vec(action).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    drop(file);
    let result = unsafe {
        let event = OpenEventW(
            EVENT_MODIFY_STATE,
            false,
            w!("Local\\DisplayProLite-DockAction"),
        )
        .map_err(|e| e.to_string());
        let event = match event {
            Ok(event) => event,
            Err(e) => {
                let _ = fs::remove_file(&path);
                return Err(e);
            }
        };
        let result = SetEvent(event).map_err(|e| e.to_string());
        let _ = CloseHandle(event);
        result
    };
    if result.is_err() {
        let _ = fs::remove_file(&path);
    }
    result
}
pub fn listen(app: tauri::AppHandle) -> Result<(), String> {
    // Discard requests from an earlier session; never execute them on startup.
    let _ = fs::remove_file(crate::data_dir().join("dock-request.json"));
    let event = unsafe { CreateEventW(None, false, false, w!("Local\\DisplayProLite-DockAction")) }
        .map_err(|e| e.to_string())?;
    let raw = event.0 as usize;
    std::thread::spawn(move || {
        while unsafe { WaitForSingleObject(HANDLE(raw as *mut _), INFINITE) } == WAIT_OBJECT_0 {
            let path = crate::data_dir().join("dock-request.json");
            let action = fs::read(&path)
                .ok()
                .filter(|b| b.len() < 4096)
                .and_then(|b| serde_json::from_slice::<String>(&b).ok());
            let _ = fs::remove_file(path);
            if let Some(action) = action {
                if ["display:", "select-window:", "open-load:", "page:"]
                    .iter()
                    .any(|p| action.starts_with(p))
                {
                    let app = app.clone();
                    std::thread::spawn(move || crate::dispatch(&app, &action));
                }
            }
        }
    });
    Ok(())
}
