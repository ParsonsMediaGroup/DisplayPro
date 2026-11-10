use std::{
    fs::{self, OpenOptions},
    io::Write,
    sync::Mutex,
};

static LOG_LOCK: Mutex<()> = Mutex::new(());

// Action-driven only; retain at most two approximately 2 MiB logs.
// Never log launch arguments, environment variables, or script contents.
pub fn event(kind: &str, detail: serde_json::Value) {
    let Ok(_guard) = LOG_LOCK.lock() else { return };
    let dir = crate::data_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("window-placement.log");
    if fs::metadata(&path).is_ok_and(|m| m.len() >= 2 * 1024 * 1024) {
        let previous = dir.join("window-placement.previous.log");
        let _ = fs::remove_file(&previous);
        if fs::rename(&path, previous).is_err() {
            return;
        }
    }
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let entry = serde_json::json!({"time_ms":time,"version":env!("CARGO_PKG_VERSION"),"thread":format!("{:?}",std::thread::current().id()),"event":kind,"detail":detail});
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{entry}");
    }
}

pub fn window(w: &crate::layouts::SavedWindow) -> serde_json::Value {
    serde_json::json!({"handle":w.handle,"pid":w.pid,"exe":w.exe,"title":w.title,"class":w.class,"normal_rect":w.rect,"width":w.rect[2]-w.rect[0],"height":w.rect[3]-w.rect[1],"maximized":w.maximized,"minimized":w.minimized})
}
