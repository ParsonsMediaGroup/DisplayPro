use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tauri::Manager;
const SUFFIX: &str = ".displayprolite-settings.json";
#[derive(Serialize, Deserialize)]
struct Bundle {
    format: String,
    schema: u32,
    app_version: String,
    config: crate::Config,
    photos: crate::photos::PhotoConfig,
    taskbars: crate::taskbars::Config,
}
unsafe fn picker(save: bool) -> Result<Option<PathBuf>, String> {
    use windows::{
        core::w,
        Win32::{System::Com::*, UI::Shell::*},
    };
    CoInitializeEx(None, COINIT_APARTMENTTHREADED)
        .ok()
        .map_err(|e| e.to_string())?;
    let result = (|| {
        let dialog: IFileDialog = if save {
            let d: IFileSaveDialog = CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| e.to_string())?;
            d.SetDefaultExtension(w!("displayprolite-settings.json"))
                .map_err(|e| e.to_string())?;
            d.SetFileName(w!("DisplayProLite.displayprolite-settings.json"))
                .map_err(|e| e.to_string())?;
            windows::core::Interface::cast(&d).map_err(|e: windows::core::Error| e.to_string())?
        } else {
            let d: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| e.to_string())?;
            windows::core::Interface::cast(&d).map_err(|e: windows::core::Error| e.to_string())?
        };
        dialog
            .SetOptions(
                FOS_FORCEFILESYSTEM
                    | if save {
                        FOS_OVERWRITEPROMPT | FOS_PATHMUSTEXIST
                    } else {
                        FOS_FILEMUSTEXIST
                    },
            )
            .map_err(|e| e.to_string())?;
        if let Err(e) = dialog.Show(None) {
            if e.code().0 as u32 == 0x800704C7 {
                return Ok(None);
            }
            return Err(e.to_string());
        }
        let p = dialog
            .GetResult()
            .and_then(|i| i.GetDisplayName(SIGDN_FILESYSPATH))
            .map_err(|e| e.to_string())?;
        let text = p.to_string().map_err(|e| e.to_string());
        CoTaskMemFree(Some(p.0.cast()));
        Ok(Some(PathBuf::from(text?)))
    })();
    CoUninitialize();
    result
}
#[tauri::command]
pub async fn export_settings(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(mut path) = unsafe { picker(true) }? else {
            return Ok(None);
        };
        if !path
            .to_string_lossy()
            .to_ascii_lowercase()
            .ends_with(SUFFIX)
        {
            path = PathBuf::from(format!("{}{SUFFIX}", path.display()));
        }
        let state = app.state::<crate::State>();
        let bundle = Bundle {
            format: "DisplayProLite.settings".into(),
            schema: 1,
            app_version: env!("CARGO_PKG_VERSION").into(),
            config: state.config.lock().map_err(|e| e.to_string())?.clone(),
            photos: crate::photos::load()?,
            taskbars: crate::taskbars::load()?,
        };
        crate::write_json(&path, &bundle)?;
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(|e| e.to_string())?
}
fn validate(bundle: &Bundle) -> Result<(), String> {
    if bundle.format != "DisplayProLite.settings" || bundle.schema != 1 {
        return Err("Unsupported DisplayProLite settings format or schema.".into());
    }
    if bundle.config.profiles.len() > 200
        || bundle.config.hotkeys.len() > 100
        || bundle.photos.screens.len() > 32
        || bundle.taskbars.bars.len() > 32
    {
        return Err("Settings file exceeds supported limits.".into());
    }
    let mut ids = std::collections::HashSet::new();
    for p in &bundle.config.profiles {
        if p.name.trim().is_empty()
            || p.name.len() > 100
            || !ids.insert(&p.id)
            || p.windows.len() > 2000
        {
            return Err("Invalid or duplicate profile.".into());
        }
        if !p.monitors.is_empty() {
            crate::display::validate(&p.monitors)?;
        }
        crate::workspace::validate(&p.windows, &p.launches, &p.startup)?;
        for w in &p.windows {
            let [l, t, r, b] = w.rect;
            if w.rect.iter().any(|v| v.unsigned_abs() > 1_000_000) || r <= l || b <= t {
                return Err("Invalid saved window rectangle.".into());
            }
        }
    }
    if !(5..=3600).contains(&bundle.photos.seconds)
        || bundle
            .photos
            .screens
            .iter()
            .any(|s| !["contain", "cover"].contains(&s.fit.as_str()))
    {
        return Err("Invalid photo screensaver configuration.".into());
    }
    for b in &bundle.taskbars.bars {
        if !(32..=80).contains(&b.size)
            || !["top", "bottom", "left", "right"].contains(&b.edge.as_str())
            || b.pins.len() > 100
        {
            return Err("Invalid taskbar configuration.".into());
        }
    }
    for b in &bundle.config.hotkeys {
        crate::hotkeys::parse(&b.shortcut)?;
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
            return Err("Invalid hotkey action.".into());
        };
        if !["display", "windows", "open-load"].contains(&kind)
            || !bundle.config.profiles.iter().any(|p| {
                p.id == id
                    && if kind == "display" {
                        !p.monitors.is_empty()
                    } else {
                        !p.windows.is_empty()
                    }
            })
        {
            return Err("Hotkey refers to an unknown profile.".into());
        }
    }
    Ok(())
}
#[tauri::command]
pub async fn import_settings(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move||{let Some(path)=unsafe{picker(false)}? else{return Ok(None)};if fs::metadata(&path).map_err(|e|e.to_string())?.len()>8*1024*1024{return Err("Settings file exceeds 8 MB.".into())}let bytes=fs::read(&path).map_err(|e|e.to_string())?;let bundle:Bundle=serde_json::from_slice(&bytes).map_err(|_|"Could not read the DisplayProLite settings file.")?;validate(&bundle)?;let state=app.state::<crate::State>();let _guard=state.transaction.lock().map_err(|e|e.to_string())?;if state.dir.join("pending.json").exists(){return Err("Finish the pending display change before importing settings.".into())}let mut current=state.config.lock().map_err(|e|e.to_string())?;state.hotkeys.update(bundle.config.hotkeys.clone())?;
    let files=["profiles.json","photos.json","taskbars.json"];let before:Vec<_>=files.iter().map(|f|fs::read(state.dir.join(f)).ok()).collect();let backup=state.dir.join(format!("import-backup-{}",crate::now()));if let Err(e)=fs::create_dir_all(&backup){let _=state.hotkeys.update(current.hotkeys.clone());return Err(e.to_string())}for(i,name)in files.iter().enumerate(){if let Some(bytes)=&before[i]{if let Err(e)=fs::write(backup.join(name),bytes){let _=state.hotkeys.update(current.hotkeys.clone());return Err(e.to_string())}}}
    let write=(||{crate::write_json(&state.dir.join(files[0]),&bundle.config)?;crate::write_json(&state.dir.join(files[1]),&bundle.photos)?;crate::write_json(&state.dir.join(files[2]),&bundle.taskbars)?;Ok::<(),String>(())})();if let Err(e)=write{for(i,name)in files.iter().enumerate(){let path=state.dir.join(name);if let Some(bytes)=&before[i]{let _=fs::write(path,bytes);}else{let _=fs::remove_file(path);}}let _=state.hotkeys.update(current.hotkeys.clone());return Err(e)}*current=bundle.config;drop(current);crate::dock::refresh_tray(&app)?;crate::taskbars::stop();Ok(Some(format!("Settings imported. Previous settings backed up in {}. Review monitors/folders, then apply taskbars and screensaver settings as needed. Display modes were not changed.",backup.display())))
}).await.map_err(|e|e.to_string())?
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_unknown_bundle_schema() {
        let b = Bundle {
            format: "DisplayProLite.settings".into(),
            schema: 2,
            app_version: "0.1.0".into(),
            config: crate::Config::default(),
            photos: crate::photos::PhotoConfig::default(),
            taskbars: crate::taskbars::Config::default(),
        };
        assert!(validate(&b).is_err());
    }
    #[test]
    fn private_bundle_roundtrips() {
        let b = Bundle {
            format: "DisplayProLite.settings".into(),
            schema: 1,
            app_version: "0.1.0".into(),
            config: crate::Config::default(),
            photos: crate::photos::PhotoConfig::default(),
            taskbars: crate::taskbars::Config::default(),
        };
        let v: Bundle = serde_json::from_slice(&serde_json::to_vec(&b).unwrap()).unwrap();
        assert!(validate(&v).is_ok());
    }
}
