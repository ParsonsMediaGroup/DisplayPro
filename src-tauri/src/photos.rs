use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
};
use tauri::{Emitter, Manager};
#[derive(Clone, Serialize, Deserialize)]
pub struct Assignment {
    pub identity: String,
    pub device: String,
    pub folder: String,
    pub fit: String,
    pub recursive: bool,
    pub shuffle: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct PhotoConfig {
    pub seconds: u32,
    pub screens: Vec<Assignment>,
}
impl Default for PhotoConfig {
    fn default() -> Self {
        Self {
            seconds: 30,
            screens: vec![],
        }
    }
}
#[derive(Clone, Serialize)]
pub struct Payload {
    files: Vec<String>,
    fit: String,
    shuffle: bool,
    seconds: u32,
    error: Option<String>,
}
struct Playback {
    screens: HashMap<String, Payload>,
    lock_on_exit: bool,
}
pub fn load() -> Result<PhotoConfig, String> {
    match fs::read(crate::data_dir().join("photos.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(PhotoConfig::default()),
        Err(e) => Err(e.to_string()),
    }
}
#[tauri::command]
pub fn get_photo_config() -> Result<PhotoConfig, String> {
    load()
}
#[tauri::command]
pub fn save_photo_config(config: PhotoConfig) -> Result<(), String> {
    if !(5..=3600).contains(&config.seconds) {
        return Err("Slide duration must be between 5 and 3600 seconds.".into());
    }
    for s in &config.screens {
        if !["contain", "cover"].contains(&s.fit.as_str()) {
            return Err("Unknown image fit.".into());
        }
        if !s.folder.is_empty() && !Path::new(&s.folder).is_dir() {
            return Err(format!("Folder not available: {}", s.folder));
        }
    }
    crate::write_json(&crate::data_dir().join("photos.json"), &config)
}
#[tauri::command]
pub async fn pick_photo_folder() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(|| unsafe {
        use windows::Win32::{System::Com::*, UI::Shell::*};
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|e| e.to_string())?;
        let result = (|| {
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| e.to_string())?;
            dialog
                .SetOptions(FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST)
                .map_err(|e| e.to_string())?;
            if let Err(e) = dialog.Show(None) {
                if e.code().0 as u32 == 0x800704C7 {
                    return Ok(None);
                }
                return Err(e.to_string());
            }
            let path = dialog
                .GetResult()
                .and_then(|i| i.GetDisplayName(SIGDN_FILESYSPATH))
                .map_err(|e| e.to_string())?;
            let text = path.to_string().map_err(|e| e.to_string());
            CoTaskMemFree(Some(path.0.cast()));
            text.map(Some)
        })();
        CoUninitialize();
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
pub fn is_media(p: &Path) -> bool {
    p.extension().is_some_and(|e| {
        [
            "jpg", "jpeg", "png", "webp", "bmp", "gif", "mp4", "webm", "m4v",
        ]
        .contains(&e.to_string_lossy().to_ascii_lowercase().as_str())
    })
}
pub fn files(folder: &Path, recursive: bool) -> Result<Vec<String>, String> {
    let mut result = vec![];
    let mut stack = vec![(folder.to_path_buf(), 0)];
    while let Some((path, depth)) = stack.pop() {
        let entries = fs::read_dir(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        for entry in entries.flatten() {
            let Ok(t) = entry.file_type() else { continue };
            if t.is_dir() && recursive && depth < 20 {
                stack.push((entry.path(), depth + 1));
            } else if t.is_file() && is_media(&entry.path()) {
                result.push(entry.path().to_string_lossy().into_owned());
                if result.len() >= 20000 {
                    return Err(
                        "Folder exceeds 20,000 media files. Choose a smaller folder.".into(),
                    );
                }
            }
        }
    }
    result.sort();
    Ok(result)
}
#[tauri::command]
pub async fn validate_photo_folder(folder: String, recursive: bool) -> Result<usize, String> {
    tauri::async_runtime::spawn_blocking(move || {
        files(Path::new(&folder), recursive).map(|v| v.len())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
fn photo_payload(
    window: tauri::WebviewWindow,
    state: tauri::State<Playback>,
) -> Result<Payload, String> {
    state
        .screens
        .get(window.label())
        .cloned()
        .ok_or("Unknown screen".into())
}
#[tauri::command]
fn stop_photos(app: tauri::AppHandle, state: tauri::State<Playback>) {
    if state.lock_on_exit {
        unsafe {
            let _ = windows::Win32::System::Shutdown::LockWorkStation();
        }
    }
    app.exit(0);
}
#[tauri::command]
fn advance_photos(app: tauri::AppHandle) {
    let _ = app.emit("next-media", ());
}
pub fn next_media() -> Result<(), String> {
    use windows::{
        core::w,
        Win32::{Foundation::CloseHandle, System::Threading::*},
    };
    unsafe {
        let event = OpenEventW(
            EVENT_MODIFY_STATE,
            false,
            w!("Local\\DisplayProLite-NextMedia"),
        )
        .map_err(|_| "Start the folder screensaver first.")?;
        let result = SetEvent(event).map_err(|e| e.to_string());
        let _ = CloseHandle(event);
        result
    }
}
#[tauri::command]
pub fn start_photo_saver(preview: bool) -> Result<(), String> {
    let config = load()?;
    if !config.screens.iter().any(|s| !s.folder.is_empty()) {
        return Err("Assign a photo folder to at least one screen first.".into());
    }
    Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .arg(if preview {
            "--photo-preview"
        } else {
            "--photo-saver"
        })
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}
#[tauri::command]
pub fn install_photo_saver() -> Result<String, String> {
    let config = load()?;
    if !config.screens.iter().any(|s| !s.folder.is_empty()) {
        return Err("Save your per-screen folders first.".into());
    }
    let dir = PathBuf::from(std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA missing")?)
        .join("DisplayProLite");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("DisplayProLite.scr");
    fs::write(
        dir.join("DisplayProLite.data-dir"),
        crate::data_dir().to_string_lossy().as_bytes(),
    )
    .map_err(|e| e.to_string())?;
    fs::copy(std::env::current_exe().map_err(|e| e.to_string())?, &path).map_err(|e| {
        format!("Could not update screensaver. Close any running preview and try again: {e}")
    })?;
    let mut s = crate::screensaver::get()?;
    s.path = path.to_string_lossy().into_owned();
    s.enabled = true;
    s.timeout = s.timeout.max(60);
    crate::screensaver::save(&s)?;
    Ok(s.path)
}
pub fn run(preview: bool) -> Result<(), String> {
    use std::os::windows::fs::OpenOptionsExt;
    let _guard = match fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .share_mode(0)
        .open(crate::data_dir().join("photos.lock"))
    {
        Ok(f) => f,
        Err(_) => return Ok(()),
    };
    let config = load()?;
    let monitors = crate::display::enumerate(false)?;
    let lock_on_exit = !preview && crate::screensaver::get()?.secure;
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            photo_payload,
            stop_photos,
            advance_photos
        ])
        .setup(move |app| {
            use windows::{
                core::w,
                Win32::{
                    Foundation::{HANDLE, WAIT_OBJECT_0},
                    System::Threading::*,
                },
            };
            let wake =
                unsafe { CreateEventW(None, false, false, w!("Local\\DisplayProLite-NextMedia")) }?;
            let raw = wake.0 as usize;
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                while unsafe { WaitForSingleObject(HANDLE(raw as *mut _), INFINITE) }
                    == WAIT_OBJECT_0
                {
                    let _ = handle.emit("next-media", ());
                }
            });
            let mut payloads = HashMap::new();
            let mut cache: HashMap<(String, bool), Result<Vec<String>, String>> = HashMap::new();
            for (i, m) in monitors.iter().filter(|m| m.enabled).enumerate() {
                let assignment = config
                    .screens
                    .iter()
                    .find(|s| s.identity == m.identity && s.device == m.device);
                let mut payload = Payload {
                    files: vec![],
                    fit: "contain".into(),
                    shuffle: true,
                    seconds: config.seconds.max(5),
                    error: None,
                };
                if let Some(a) = assignment.filter(|a| !a.folder.is_empty()) {
                    payload.fit = a.fit.clone();
                    payload.shuffle = a.shuffle;
                    // Only these explicitly configured folders are readable by the media renderer.
                    app.asset_protocol_scope()
                        .allow_directory(&a.folder, a.recursive)?;
                    let listing = cache
                        .entry((a.folder.clone(), a.recursive))
                        .or_insert_with(|| files(Path::new(&a.folder), a.recursive));
                    match listing {
                        Ok(v) => {
                            payload.files = v.clone();
                            if v.is_empty() {
                                payload.error =
                                    Some("No supported photos or videos in this folder.".into());
                            }
                        }
                        Err(e) => payload.error = Some(e.clone()),
                    }
                }
                let label = format!("photo-{i}");
                payloads.insert(label.clone(), payload);
            }
            app.manage(Playback {
                screens: payloads,
                lock_on_exit,
            });
            for (i, m) in monitors.iter().filter(|m| m.enabled).enumerate() {
                let w = tauri::WebviewWindowBuilder::new(
                    app,
                    format!("photo-{i}"),
                    tauri::WebviewUrl::App("index.html?saver=1".into()),
                )
                .title("DisplayProLite Screensaver")
                .visible(false)
                .decorations(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .background_color(tauri::utils::config::Color(0, 0, 0, 255))
                .build()?;
                w.set_position(tauri::PhysicalPosition::new(m.x, m.y))?;
                w.set_size(tauri::PhysicalSize::new(m.width, m.height))?;
                w.show()?;
                if m.primary {
                    w.set_focus()?;
                }
            }
            Ok(())
        })
        .on_window_event(|w, e| {
            if matches!(e, tauri::WindowEvent::CloseRequested { .. }) {
                if let Some(s) = w.app_handle().try_state::<Playback>() {
                    if s.lock_on_exit {
                        unsafe {
                            let _ = windows::Win32::System::Shutdown::LockWorkStation();
                        }
                    }
                }
                w.app_handle().exit(0);
            }
        })
        .run(tauri::generate_context!())
        .map_err(|e| e.to_string())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn media_extensions_are_constrained() {
        assert!(is_media(Path::new("photo.JPG")));
        assert!(is_media(Path::new("clip.mp4")));
        assert!(!is_media(Path::new("script.svg")));
        assert!(!is_media(Path::new("program.exe")));
    }
}
