use serde::{Deserialize, Serialize};
use std::{
    sync::{mpsc, Mutex},
    time::Duration,
};
use windows::Win32::{
    Foundation::{LPARAM, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Binding {
    pub shortcut: String,
    pub action: String,
}
pub struct Hotkeys {
    thread: u32,
    tx: Mutex<mpsc::Sender<(Vec<Binding>, mpsc::Sender<Result<(), String>>)>>,
}
pub fn parse(value: &str) -> Result<(HOT_KEY_MODIFIERS, u32), String> {
    let mut flags = MOD_NOREPEAT;
    let mut key = None;
    for part in value.split('+').map(|s| s.trim().to_ascii_uppercase()) {
        match part.as_str() {
            "CTRL" | "CONTROL" => flags |= MOD_CONTROL,
            "ALT" => flags |= MOD_ALT,
            "SHIFT" => flags |= MOD_SHIFT,
            "WIN" | "SUPER" => flags |= MOD_WIN,
            _ => {
                if key.is_some() {
                    return Err("Use one letter, digit, or F1–F11 key.".into());
                }
                let named = match part.as_str() {
                    "LEFT" | "ARROWLEFT" => Some(0x25),
                    "RIGHT" | "ARROWRIGHT" => Some(0x27),
                    "UP" | "ARROWUP" => Some(0x26),
                    "DOWN" | "ARROWDOWN" => Some(0x28),
                    "SPACE" => Some(0x20),
                    "HOME" => Some(0x24),
                    "END" => Some(0x23),
                    "PAGEUP" => Some(0x21),
                    "PAGEDOWN" => Some(0x22),
                    _ => None,
                };
                let vk = if let Some(v) = named {
                    v
                } else if part.len() == 1 && part.as_bytes()[0].is_ascii_alphanumeric() {
                    part.as_bytes()[0] as u32
                } else if part.starts_with('F') {
                    let n = part[1..]
                        .parse::<u32>()
                        .map_err(|_| "Invalid function key")?;
                    if !(1..=11).contains(&n) {
                        return Err("Use F1–F11; Windows reserves F12.".into());
                    }
                    0x70 + n - 1
                } else {
                    return Err(
                        "Use Ctrl, Alt, Shift or Win plus a letter, digit or F1–F11.".into(),
                    );
                };
                key = Some(vk);
            }
        }
    }
    if flags.0 == MOD_NOREPEAT.0 {
        return Err("Include at least one modifier (Ctrl, Alt, Shift, Win).".into());
    }
    Ok((flags, key.ok_or("Choose a key")?))
}
fn register(bindings: &[Binding]) -> Result<(), String> {
    for (i, b) in bindings.iter().enumerate() {
        let (flags, key) = parse(&b.shortcut)?;
        if let Err(e) = unsafe { RegisterHotKey(None, i as i32 + 1, flags, key) } {
            for n in 0..i {
                unsafe {
                    let _ = UnregisterHotKey(None, n as i32 + 1);
                }
            }
            return Err(format!("{} is unavailable: {}", b.shortcut, e));
        }
    }
    Ok(())
}
fn unregister(bindings: &[Binding]) {
    for i in 0..bindings.len() {
        unsafe {
            let _ = UnregisterHotKey(None, i as i32 + 1);
        }
    }
}
impl Hotkeys {
    pub fn start(app: tauri::AppHandle) -> Self {
        let (tx, rx) = mpsc::channel::<(Vec<Binding>, mpsc::Sender<Result<(), String>>)>();
        let (ready, r) = mpsc::channel();
        std::thread::spawn(move || {
            let mut msg = MSG::default();
            unsafe {
                let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
                let _ = ready.send(GetCurrentThreadId());
            }
            let mut active: Vec<Binding> = vec![];
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
                if msg.message == WM_APP + 7 {
                    while let Ok((new, reply)) = rx.try_recv() {
                        unregister(&active);
                        let result = register(&new);
                        if result.is_ok() {
                            active = new
                        } else {
                            let _ = register(&active);
                        }
                        let _ = reply.send(result);
                    }
                } else if msg.message == WM_HOTKEY {
                    if let Some(b) = active.get(msg.wParam.0.saturating_sub(1)) {
                        let app = app.clone();
                        let action = b.action.clone();
                        std::thread::spawn(move || {
                            crate::dispatch(&app, &action);
                        });
                    }
                }
            }
            unregister(&active);
        });
        Self {
            thread: r.recv().expect("hotkey thread"),
            tx: Mutex::new(tx),
        }
    }
    pub fn update(&self, bindings: Vec<Binding>) -> Result<(), String> {
        let mut seen = std::collections::HashSet::new();
        for b in &bindings {
            let (f, k) = parse(&b.shortcut)?;
            if !seen.insert((f.0, k)) {
                return Err("Two actions use the same shortcut.".into());
            }
        }
        let (tx, rx) = mpsc::channel();
        self.tx
            .lock()
            .map_err(|e| e.to_string())?
            .send((bindings, tx))
            .map_err(|e| e.to_string())?;
        unsafe {
            PostThreadMessageW(self.thread, WM_APP + 7, WPARAM(0), LPARAM(0))
                .map_err(|e| e.to_string())?;
        }
        rx.recv_timeout(Duration::from_secs(5))
            .map_err(|e| e.to_string())?
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_and_rejects_unsafe_keys() {
        assert_eq!(parse("Ctrl+Alt+1").unwrap().1, 49);
        assert!(parse("F12").is_err());
        assert!(parse("Ctrl+A+B").is_err());
        assert!(parse("A").is_err());
    }
}
