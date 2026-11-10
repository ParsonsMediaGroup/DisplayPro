#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod display;
mod dock;
mod environment;
mod hotkeys;
mod layouts;
mod photos;
mod screensaver;
mod settings;
mod smoke;
mod taskbars;
mod workspace;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    os::windows::{fs::OpenOptionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};

#[derive(Clone, Serialize, Deserialize)]
struct Profile {
    id: String,
    name: String,
    monitors: Vec<display::Monitor>,
    windows: Vec<layouts::SavedWindow>,
    #[serde(default)]
    launches: Vec<workspace::LaunchCommand>,
    #[serde(default)]
    startup: Option<workspace::LaunchCommand>,
}
#[derive(Clone, Serialize, Deserialize, Default)]
struct Config {
    #[serde(default)]
    profiles: Vec<Profile>,
    #[serde(default)]
    hotkeys: Vec<hotkeys::Binding>,
    #[serde(default)]
    selected_window_profile: Option<String>,
}
struct State {
    config: Mutex<Config>,
    dir: PathBuf,
    transaction: Mutex<()>,
    hotkeys: hotkeys::Hotkeys,
    startup_error: Mutex<Option<String>>,
    requested_page: Mutex<Option<String>>,
    last_window_profile: Mutex<Option<String>>,
}
#[derive(Serialize, Deserialize)]
struct Pending {
    id: String,
    deadline: u64,
    before: Vec<display::Monitor>,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn data_dir() -> PathBuf {
    if let Some(path) = environment::configured_dir().or_else(environment::screensaver_dir) {
        return path;
    }
    PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default()).join("DisplayProLite")
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let temp = path.with_extension("tmp");
    fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    fs::rename(temp, path).map_err(|e| e.to_string())
}
fn save_config(state: &State, new: &Config) -> Result<(), String> {
    write_json(&state.dir.join("profiles.json"), new)?;
    dock::refresh_bars();
    Ok(())
}
fn show(app: &tauri::AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        let window = handle.get_webview_window("main").or_else(|| {
            tauri::WebviewWindowBuilder::from_config(&handle, &handle.config().app.windows[0])
                .ok()?
                .build()
                .ok()
        });
        if let Some(w) = window {
            let _ = w.show();
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
    });
}
fn disk_lock(dir: &Path) -> Result<fs::File, String> {
    for _ in 0..100 {
        match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .share_mode(0)
            .open(dir.join("display-transaction.lock"))
        {
            Ok(f) => return Ok(f),
            Err(e) if e.raw_os_error() == Some(32) => {
                std::thread::sleep(std::time::Duration::from_millis(100))
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("Another display operation is still finishing. Try again shortly.".into())
}
fn notify(app: &tauri::AppHandle, result: Result<String, String>) {
    let persisted = match &result {
        Ok(s) => serde_json::json!({"ok":true,"message":s}),
        Err(e) => serde_json::json!({"ok":false,"message":e}),
    };
    let _ = write_json(&data_dir().join("last-action.json"), &persisted);
    let _ = app.emit(
        "action-result",
        match result {
            Ok(s) => serde_json::json!({"ok":true,"message":s}),
            Err(e) => {
                show(app);
                serde_json::json!({"ok":false,"message":e})
            }
        },
    );
}
#[tauri::command]
fn get_state(state: tauri::State<State>) -> Result<serde_json::Value, String> {
    let config = state.config.lock().map_err(|e| e.to_string())?.clone();
    let pending = fs::read(state.dir.join("pending.json"))
        .ok()
        .and_then(|v| serde_json::from_slice::<Pending>(&v).ok());
    let last_action = fs::read(state.dir.join("last-action.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
    Ok(
        serde_json::json!({"lastAction":last_action,"config":config,"monitors":display::enumerate(true)?,"screensaver":screensaver::get()?,"photos":photos::load()?,"taskbars":taskbars::load()?,"requestedPage":state.requested_page.lock().unwrap().take(),"pending":pending,"dataDir":state.dir,"startupError":state.startup_error.lock().unwrap().clone(),"autostart":!screensaver::reg_read("Software\\Microsoft\\Windows\\CurrentVersion\\Run","DisplayProLite").is_empty()}),
    )
}
#[tauri::command]
fn capture_windows() -> Result<Vec<layouts::SavedWindow>, String> {
    layouts::capture()
}
#[tauri::command]
fn save_profile(
    app: tauri::AppHandle,
    state: tauri::State<State>,
    id: Option<String>,
    name: String,
    mut monitors: Vec<display::Monitor>,
    windows: Vec<layouts::SavedWindow>,
    launches: Option<Vec<workspace::LaunchCommand>>,
    startup: Option<workspace::LaunchCommand>,
    overwrite: Option<bool>,
) -> Result<(), String> {
    let launches = launches.unwrap_or_default();
    workspace::validate(&windows, &launches, &startup)?;
    let name = name.trim();
    if name.is_empty() || name.len() > 100 {
        return Err("Use a profile name between 1 and 100 characters.".into());
    }
    if monitors.is_empty() && windows.is_empty() {
        return Err("Capture displays or select windows first.".into());
    }
    if !monitors.is_empty() {
        display::normalize(&mut monitors);
        display::validate(&monitors)?;
    }
    for m in &mut monitors {
        m.modes.clear();
    }
    let mut config = state.config.lock().map_err(|e| e.to_string())?;
    let mut new = config.clone();
    if new.profiles.iter().any(|p| Some(&p.id) == id.as_ref()) && overwrite != Some(true) {
        return Err("Confirm overwrite before replacing an existing profile.".into());
    }
    if new
        .profiles
        .iter()
        .any(|p| p.name.eq_ignore_ascii_case(name) && Some(&p.id) != id.as_ref())
    {
        return Err("A profile with that name already exists. Load it and confirm overwrite, or choose a new name.".into());
    }
    let id = id.unwrap_or_else(|| {
        format!(
            "{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis()
        )
    });
    let p = Profile {
        id: id.clone(),
        name: name.to_string(),
        monitors,
        windows,
        launches,
        startup,
    };
    if let Some(slot) = new.profiles.iter_mut().find(|p| p.id == id) {
        *slot = p;
    } else {
        new.profiles.push(p);
    }
    if new.selected_window_profile.is_none()
        && new
            .profiles
            .iter()
            .any(|p| p.id == id && !p.windows.is_empty())
    {
        new.selected_window_profile = Some(id.clone());
    }
    save_config(&state, &new)?;
    *config = new;
    drop(config);
    dock::refresh_tray(&app)?;
    Ok(())
}
#[tauri::command]
fn delete_profile(
    app: tauri::AppHandle,
    state: tauri::State<State>,
    id: String,
) -> Result<(), String> {
    let mut config = state.config.lock().map_err(|e| e.to_string())?;
    let mut new = config.clone();
    new.profiles.retain(|p| p.id != id);
    if new.selected_window_profile.as_deref() == Some(&id) {
        new.selected_window_profile = None;
    }
    new.hotkeys
        .retain(|b| b.action != format!("display:{id}") && b.action != format!("windows:{id}"));
    state.hotkeys.update(new.hotkeys.clone())?;
    if let Err(e) = save_config(&state, &new) {
        let _ = state.hotkeys.update(config.hotkeys.clone());
        return Err(e);
    }
    *config = new;
    drop(config);
    dock::refresh_tray(&app)?;
    Ok(())
}
fn begin_display(
    app: &tauri::AppHandle,
    state: &State,
    mut monitors: Vec<display::Monitor>,
) -> Result<String, String> {
    let _lock = state.transaction.lock().map_err(|e| e.to_string())?;
    let _disk_guard = disk_lock(&state.dir)?;
    let path = state.dir.join("pending.json");
    if path.exists() {
        return Err("Keep or revert the pending display change first.".into());
    }
    display::normalize(&mut monitors);
    display::test(&monitors)?;
    let id = format!(
        "{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let pending = Pending {
        id: id.clone(),
        deadline: now() + 25,
        before: display::enumerate(false)?,
    };
    write_json(&path, &pending)?;
    // Independent helper survives a UI crash and owns the timed rollback.
    if let Err(e) = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .arg("--rollback-watchdog")
        .arg(&path)
        .arg(&id)
        .creation_flags(0x08000000)
        .spawn()
    {
        let _ = fs::remove_file(&path);
        return Err(e.to_string());
    }
    if let Err(e) = display::apply(&monitors) {
        match display::apply(&pending.before) {
            Ok(()) => {
                let _ = fs::remove_file(&path);
            }
            Err(r) => return Err(format!("{e}; rollback failed: {r}")),
        }
        return Err(e);
    }
    show(app);
    let _ = app.emit(
        "display-pending",
        serde_json::json!({"id":id,"deadline":pending.deadline}),
    );
    Ok(id)
}
#[tauri::command]
fn apply_displays(
    app: tauri::AppHandle,
    state: tauri::State<State>,
    monitors: Vec<display::Monitor>,
) -> Result<String, String> {
    let result = begin_display(&app, &state, monitors);
    match &result {
        Err(error) => notify(&app, Err(error.clone())),
        Ok(_) => notify(
            &app,
            Ok("Display profile applied. Keep or revert the change within 25 seconds.".into()),
        ),
    }
    result
}
#[tauri::command]
fn finish_display(state: tauri::State<State>, id: String, keep: bool) -> Result<(), String> {
    let _lock = state.transaction.lock().map_err(|e| e.to_string())?;
    let _disk_guard = disk_lock(&state.dir)?;
    let path = state.dir.join("pending.json");
    let p: Pending = serde_json::from_slice(
        &fs::read(&path).map_err(|_| "Display change already reverted or completed.")?,
    )
    .map_err(|e| e.to_string())?;
    if p.id != id {
        return Err("This confirmation has expired.".into());
    }
    if keep && now() >= p.deadline.saturating_sub(2) {
        return Err("Confirmation expired; the original layout is being restored.".into());
    }
    if !keep {
        display::apply(&p.before)?;
    }
    fs::remove_file(path).map_err(|e| e.to_string())
}
fn get_profile(state: &State, id: &str) -> Result<Profile, String> {
    state
        .config
        .lock()
        .map_err(|e| e.to_string())?
        .profiles
        .iter()
        .find(|p| p.id == id)
        .cloned()
        .ok_or("Profile not found.".into())
}
#[tauri::command]
fn restore_windows(
    state: tauri::State<State>,
    id: String,
) -> Result<layouts::RestoreReport, String> {
    layouts::restore(&get_profile(&state, &id)?.windows)
}
#[tauri::command]
fn save_hotkeys(state: tauri::State<State>, bindings: Vec<hotkeys::Binding>) -> Result<(), String> {
    let mut config = state.config.lock().map_err(|e| e.to_string())?;
    for b in &bindings {
        if [
            "show",
            "screensaver",
            "sleep",
            "photo-saver",
            "next-media",
            "next-display-profile",
            "next-window-profile",
        ]
        .contains(&b.action.as_str())
        {
            continue;
        }
        let Some((kind, id)) = b.action.split_once(':') else {
            return Err("Unknown hotkey action.".into());
        };
        if !["display", "windows"].contains(&kind) || !config.profiles.iter().any(|p| p.id == id) {
            return Err("Hotkey refers to a missing profile.".into());
        }
    }
    state.hotkeys.update(bindings.clone())?;
    let mut new = config.clone();
    new.hotkeys = bindings;
    if let Err(e) = save_config(&state, &new) {
        let _ = state.hotkeys.update(config.hotkeys.clone());
        return Err(e);
    }
    *config = new;
    Ok(())
}
#[tauri::command]
fn save_screensaver(saver: screensaver::Saver) -> Result<(), String> {
    screensaver::save(&saver)
}
#[tauri::command]
fn start_screensaver() -> Result<(), String> {
    screensaver::start()
}
#[tauri::command]
fn screensaver_settings() -> Result<(), String> {
    screensaver::settings()
}
#[tauri::command]
fn sleep_displays() {
    screensaver::sleep_displays()
}
#[tauri::command]
fn set_autostart(enabled: bool) -> Result<(), String> {
    let key = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    if enabled {
        screensaver::reg_write(
            key,
            "DisplayProLite",
            &format!(
                "\"{}\" --tray",
                std::env::current_exe()
                    .map_err(|e| e.to_string())?
                    .display()
            ),
        )
    } else {
        use windows::{core::PCWSTR, Win32::System::Registry::*};
        let k = display::wide(key);
        let n = display::wide("DisplayProLite");
        let e = unsafe {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(k.as_ptr()), PCWSTR(n.as_ptr()))
        };
        if e.0 == 0 || e.0 == 2 {
            Ok(())
        } else {
            Err(format!("Windows error {}", e.0))
        }
    }
}
fn cycle_displays(app: &tauri::AppHandle, state: &State) -> Result<String, String> {
    let profiles: Vec<_> = state
        .config
        .lock()
        .map_err(|e| e.to_string())?
        .profiles
        .iter()
        .filter(|p| !p.monitors.is_empty())
        .cloned()
        .collect();
    if profiles.is_empty() {
        return Err("Save a monitor profile first.".into());
    }
    let current = display::enumerate(false)?;
    let active = profiles.iter().position(|p| {
        p.monitors.len() == current.len()
            && p.monitors.iter().all(|a| {
                current.iter().any(|b| {
                    a.identity == b.identity
                        && a.device == b.device
                        && a.enabled == b.enabled
                        && (!a.enabled
                            || (a.primary == b.primary
                                && a.width == b.width
                                && a.height == b.height
                                && a.hz == b.hz
                                && a.x == b.x
                                && a.y == b.y))
                })
            })
    });
    let p = &profiles[active.map(|i| (i + 1) % profiles.len()).unwrap_or(0)];
    begin_display(app, state, p.monitors.clone())?;
    Ok(format!("Confirm monitor profile: {}", p.name))
}
fn cycle_windows(state: &State) -> Result<String, String> {
    let profiles: Vec<_> = state
        .config
        .lock()
        .map_err(|e| e.to_string())?
        .profiles
        .iter()
        .filter(|p| !p.windows.is_empty())
        .cloned()
        .collect();
    if profiles.is_empty() {
        return Err("Save a window layout first.".into());
    }
    let mut last = state
        .last_window_profile
        .lock()
        .map_err(|e| e.to_string())?;
    let active = profiles.iter().position(|p| Some(&p.id) == last.as_ref());
    let p = &profiles[active.map(|i| (i + 1) % profiles.len()).unwrap_or(0)];
    let report = layouts::restore(&p.windows)?;
    *last = Some(p.id.clone());
    Ok(format!(
        "{}: restored {} windows; {} skipped.",
        p.name,
        report.restored,
        report.skipped.len()
    ))
}
pub fn dispatch(app: &tauri::AppHandle, action: &str) {
    let state = app.state::<State>();
    if let Some(id) = action.strip_prefix("open-load:") {
        let result = get_profile(&state, id)
            .and_then(|p| workspace::open(&p))
            .map(|r| {
                format!(
                    "Found {} of {} windows; launched {}; positioned {}. {}",
                    r.found,
                    r.requested,
                    r.launched,
                    r.restored,
                    r.skipped.join("; ")
                )
            });
        show(app);
        notify(app, result);
        return;
    }
    if let Some(id) = action.strip_prefix("select-window:") {
        notify(
            app,
            dock::select(&app, id.to_string()).map(|_| "Window profile selected.".into()),
        );
        return;
    }
    if let Some(page) = action.strip_prefix("page:") {
        *state.requested_page.lock().unwrap() = Some(page.into());
        show(app);
        let _ = app.emit("navigate", page);
        return;
    }
    let result = match action {
        "next-media" => photos::next_media().map(|_| "Advanced screensaver media.".into()),
        "photo-saver" => {
            photos::start_photo_saver(false).map(|_| "Folder screensaver started.".into())
        }
        "next-display-profile" => cycle_displays(app, &state),
        "next-window-profile" => cycle_windows(&state),
        "show" => {
            show(app);
            Ok("DisplayProLite opened.".into())
        }
        "screensaver" => screensaver::start().map(|_| "Screensaver started.".into()),
        "sleep" => {
            screensaver::sleep_displays();
            Ok("Displays sent to sleep.".into())
        }
        _ => {
            if let Some((kind, id)) = action.split_once(':') {
                get_profile(&state, id).and_then(|p| {
                    if kind == "display" {
                        begin_display(app, &state, p.monitors)
                            .map(|_| "Confirm your display changes.".into())
                    } else {
                        layouts::restore(&p.windows).map(|r| {
                            format!(
                                "Restored {} windows; {} skipped.",
                                r.restored,
                                r.skipped.len()
                            )
                        })
                    }
                })
            } else {
                Err("Unknown action".into())
            }
        }
    };
    notify(app, result);
}
fn watchdog(path: &Path, id: &str) {
    loop {
        let Ok(bytes) = fs::read(path) else { return };
        let Ok(p) = serde_json::from_slice::<Pending>(&bytes) else {
            return;
        };
        if p.id != id {
            return;
        }
        if now() >= p.deadline {
            let Ok(_guard) = disk_lock(path.parent().unwrap()) else {
                return;
            };
            let Ok(bytes) = fs::read(path) else { return };
            let Ok(p) = serde_json::from_slice::<Pending>(&bytes) else {
                return;
            };
            if p.id != id {
                return;
            }
            if display::apply(&p.before).is_ok() {
                let _ = fs::remove_file(path);
            }
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}
fn main() {
    let env_error = environment::load().err();
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "--taskbars") {
        let parent = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);
        if let Err(e) = taskbars::run(parent) {
            let _ = write_json(
                &data_dir().join("taskbars-ready.json"),
                &Result::<(), String>::Err(e),
            );
        }
        return;
    }
    if args.get(1).is_some_and(|s| s == "--test-window") {
        if let Some(title) = args.get(2) {
            let _ = smoke::test_window(title);
        }
        return;
    }
    if args.get(1).is_some_and(|s| s == "--self-test-workspace") {
        if let Some(path) = args.get(2) {
            if smoke::workspace_run(Path::new(path)).is_err() {
                std::process::exit(1);
            }
        }
        return;
    }
    if args.get(1).is_some_and(|s| s == "--check-profiles") {
        if let Some(path) = args.get(2) {
            let result = (|| -> Result<serde_json::Value, String> {
                let config: Config = serde_json::from_slice(
                    &fs::read(data_dir().join("profiles.json")).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
                Ok(serde_json::json!(config.profiles.iter().filter(|p|!p.monitors.is_empty()).map(|p|serde_json::json!({"name":p.name,"validation":display::test(&p.monitors)})).collect::<Vec<_>>()))
            })();
            let _ = write_json(Path::new(path), &result);
        }
        return;
    }
    if args.get(1).is_some_and(|s| s == "--self-test") {
        if let Some(path) = args.get(2) {
            if smoke::run(Path::new(path)).is_err() {
                std::process::exit(1)
            }
        }
        return;
    }
    let mode = args
        .get(1)
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    if ["/s", "-s", "--photo-saver", "--photo-preview"].contains(&mode.as_str()) {
        let _ = fs::create_dir_all(data_dir());
        if let Err(e) = photos::run(mode == "--photo-preview") {
            let _ = fs::write(data_dir().join("screensaver-error.txt"), e);
        }
        return;
    }
    if mode.starts_with("/p") || mode.starts_with("-p") {
        return;
    }
    if args.get(1).is_some_and(|s| s == "--rollback-watchdog") {
        if let (Some(path), Some(id)) = (args.get(2), args.get(3)) {
            watchdog(Path::new(path), id)
        }
        return;
    }
    // Read-only diagnostics used to validate the packaged executable.
    if args.get(1).is_some_and(|s| s == "--diagnose") {
        if let Some(path) = args.get(2) {
            let result = serde_json::json!({"monitors":display::enumerate(true),"windows":layouts::capture(),"screensaver":screensaver::get()});
            let _ = write_json(Path::new(path), &result);
        }
        return;
    }
    let dir = data_dir();
    fs::create_dir_all(&dir).expect("create settings directory");
    let _guard = match fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .share_mode(0)
        .open(dir.join("instance.lock"))
    {
        Ok(f) => f,
        Err(_) => {
            use windows::{
                core::w,
                Win32::{Foundation::CloseHandle, System::Threading::*},
            };
            unsafe {
                if let Ok(event) =
                    OpenEventW(EVENT_MODIFY_STATE, false, w!("Local\\DisplayProLite-Show"))
                {
                    let _ = SetEvent(event);
                    let _ = CloseHandle(event);
                }
            }
            return;
        }
    };
    let mut startup_error = env_error;
    let config = match fs::read(dir.join("profiles.json")) {
        Ok(bytes) => match serde_json::from_slice::<Config>(&bytes) {
            Ok(c) => c,
            Err(e) => {
                let backup = dir.join(format!("profiles-unreadable-{}.json", now()));
                let _ = fs::write(&backup, &bytes);
                startup_error = Some(format!(
                    "Settings could not be read ({e}). Original saved at {}.",
                    backup.display()
                ));
                Config::default()
            }
        },
        Err(_) => Config::default(),
    };
    if let Ok(bytes) = fs::read(dir.join("pending.json")) {
        if let Ok(p) = serde_json::from_slice::<Pending>(&bytes) {
            if display::apply(&p.before).is_ok() {
                let _ = fs::remove_file(dir.join("pending.json"));
            } else {
                startup_error=Some("Previous display rollback needs attention. Reconnect the original monitors and restart.".into());
            }
        }
    }
    tauri::Builder::default()
        .setup(move |app| {
            use windows::{
                core::w,
                Win32::{
                    Foundation::{HANDLE, WAIT_OBJECT_0},
                    System::Threading::*,
                },
            };
            let wake =
                unsafe { CreateEventW(None, false, false, w!("Local\\DisplayProLite-Show")) }?;
            let raw = wake.0 as usize;
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                while unsafe { WaitForSingleObject(HANDLE(raw as *mut _), INFINITE) }
                    == WAIT_OBJECT_0
                {
                    show(&app_handle);
                }
            });
            let hotkeys = hotkeys::Hotkeys::start(app.handle().clone());
            if let Err(e) = taskbars::start() {
                startup_error = Some(e);
            }
            if let Err(e) = hotkeys.update(config.hotkeys.clone()) {
                startup_error = Some(e)
            }
            app.manage(State {
                config: Mutex::new(config),
                dir,
                transaction: Mutex::new(()),
                hotkeys,
                startup_error: Mutex::new(startup_error),
                requested_page: Mutex::new(None),
                last_window_profile: Mutex::new(None),
            });
            let open = MenuItem::with_id(
                app,
                "page:displays",
                "Monitor Configuration",
                true,
                None::<&str>,
            )?;
            let positions =
                MenuItem::with_id(app, "page:profiles", "Window Positions", true, None::<&str>)?;
            let saver =
                MenuItem::with_id(app, "page:screensaver", "Screensavers", true, None::<&str>)?;
            let keys = MenuItem::with_id(app, "page:hotkeys", "Hotkeys", true, None::<&str>)?;
            let bars = MenuItem::with_id(app, "page:taskbars", "Taskbars", true, None::<&str>)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Quit DisplayProLite", true, None::<&str>)?;
            let menu = Menu::with_items(
                app,
                &[&open, &positions, &saver, &keys, &bars, &separator, &quit],
            )?;
            let mut tray = TrayIconBuilder::with_id("main")
                .tooltip("DisplayProLite")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    if event.id.as_ref() == "quit" {
                        app.exit(0)
                    } else {
                        let app = app.clone();
                        let action = event.id.as_ref().to_string();
                        std::thread::spawn(move || dispatch(&app, &action));
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show(tray.app_handle())
                    }
                });
            if let Ok(icon) =
                tauri::image::Image::from_bytes(include_bytes!("../icons/tray-16.png"))
            {
                tray = tray.icon(icon);
            }
            tray.build(app)?;
            dock::refresh_tray(app.handle())?;
            dock::listen(app.handle().clone())?;
            if !args.iter().any(|s| s == "--tray") {
                show(app.handle());
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            dock::select_window_profile,
            dock::open_window_profile,
            get_state,
            capture_windows,
            save_profile,
            delete_profile,
            apply_displays,
            finish_display,
            restore_windows,
            save_hotkeys,
            save_screensaver,
            start_screensaver,
            screensaver_settings,
            sleep_displays,
            set_autostart,
            photos::get_photo_config,
            photos::save_photo_config,
            photos::pick_photo_folder,
            photos::validate_photo_folder,
            photos::start_photo_saver,
            photos::install_photo_saver,
            settings::export_settings,
            settings::import_settings,
            taskbars::get_taskbars,
            taskbars::save_taskbars,
            taskbars::pick_taskbar_app,
            taskbars::open_windows_taskbar_settings
        ])
        .build(tauri::generate_context!())
        .expect("DisplayProLite failed")
        .run(|_app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                taskbars::stop();
            }
            if let tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
            }
        });
}
