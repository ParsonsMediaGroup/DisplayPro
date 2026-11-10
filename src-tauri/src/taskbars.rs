//! Optional native GDI appbars. No browser renderers or injected DLL hooks.
use crate::{
    display::{wide, Monitor},
    layouts::SavedWindow,
};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs,
    mem::size_of,
    os::windows::process::CommandExt,
    path::Path,
    process::Command,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    time::Duration,
};
use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::*,
        Graphics::Gdi::*,
        System::{LibraryLoader::GetModuleHandleW, Threading::*},
        UI::{Accessibility::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
    },
};
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Pin {
    pub path: String,
    pub label: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct BarConfig {
    pub identity: String,
    pub device: String,
    pub enabled: bool,
    pub edge: String,
    pub autohide: bool,
    pub size: u32,
    pub pins: Vec<Pin>,
}
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub bars: Vec<BarConfig>,
}
#[derive(Clone)]
struct Group {
    path: String,
    label: String,
    windows: Vec<SavedWindow>,
    pinned: bool,
}
struct Bar {
    hwnd: HWND,
    config: BarConfig,
    monitor: Monitor,
    full: RECT,
    hidden: bool,
    fullscreen: bool,
    groups: Vec<Group>,
    icons: HashMap<String, HICON>,
    hover: Option<usize>,
    positioning: bool,
    dock_config: crate::Config,
}
static THREAD: AtomicU32 = AtomicU32::new(0);
static DIRTY: AtomicBool = AtomicBool::new(false);
const WM_MOUSEHOVER: u32 = 0x02A1;
const WM_MOUSELEAVE: u32 = 0x02A3;
const REFRESH: u32 = WM_APP + 101;
const ACTION: u32 = WM_APP + 102;
const REPOSITION: u32 = WM_APP + 103;
const POINTER: u32 = WM_APP + 104;
const HOVER: u32 = WM_APP + 105;
const LEAVE: u32 = WM_APP + 106;
const APPBAR: u32 = WM_APP + 107;
const HIDE: u32 = WM_APP + 108;
pub fn load() -> Result<Config, String> {
    match fs::read(crate::data_dir().join("taskbars.json")) {
        Ok(b) => serde_json::from_slice(&b).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e.to_string()),
    }
}
pub fn stop() {
    unsafe {
        if let Ok(e) = OpenEventW(
            EVENT_MODIFY_STATE,
            false,
            w!("Local\\DisplayProLite-StopTaskbars"),
        ) {
            let _ = SetEvent(e);
            let _ = CloseHandle(e);
        }
    }
}
pub fn start() -> Result<(), String> {
    let config = load()?;
    if !config.bars.iter().any(|b| b.enabled) {
        return Ok(());
    }
    let ready = crate::data_dir().join("taskbars-ready.json");
    let _ = fs::remove_file(&ready);
    let mut child = Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .args(["--taskbars", &std::process::id().to_string()])
        .creation_flags(0x08000000)
        .spawn()
        .map_err(|e| e.to_string())?;
    for _ in 0..100 {
        if let Ok(bytes) = fs::read(&ready) {
            let result: Result<(), String> =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            return result;
        }
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            return Err("Taskbar process stopped during startup.".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    stop();
    Err("Taskbar startup timed out.".into())
}
#[tauri::command]
pub fn get_taskbars() -> Result<Config, String> {
    load()
}
#[tauri::command]
pub async fn save_taskbars(config: Config) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        for b in &config.bars {
            if !["left", "right", "top", "bottom"].contains(&b.edge.as_str())
                || !(32..=80).contains(&b.size)
            {
                return Err("Invalid taskbar edge or size.".into());
            }
            for p in &b.pins {
                if !Path::new(&p.path).is_file() || !p.path.to_ascii_lowercase().ends_with(".exe") {
                    return Err(format!(
                        "Pinned application must be an existing .exe: {}",
                        p.path
                    ));
                }
            }
        }
        let old = load()?;
        stop();
        for _ in 0..80 {
            let stopped = unsafe {
                OpenEventW(
                    EVENT_MODIFY_STATE,
                    false,
                    w!("Local\\DisplayProLite-StopTaskbars"),
                )
            };
            match stopped {
                Err(_) => break,
                Ok(e) => unsafe {
                    let _ = CloseHandle(e);
                },
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        crate::write_json(&crate::data_dir().join("taskbars.json"), &config)?;
        if let Err(e) = start() {
            stop();
            std::thread::sleep(Duration::from_millis(150));
            let _ = crate::write_json(&crate::data_dir().join("taskbars.json"), &old);
            let _ = start();
            return Err(e);
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn open_windows_taskbar_settings() -> Result<(), String> {
    unsafe {
        let result = ShellExecuteW(
            None,
            w!("open"),
            w!("ms-settings:taskbar"),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
        if result.0 as isize <= 32 {
            Err("Windows taskbar settings could not be opened.".into())
        } else {
            Ok(())
        }
    }
}
#[tauri::command]
pub async fn pick_taskbar_app() -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(|| unsafe {
        use windows::Win32::System::Com::*;
        CoInitializeEx(None, COINIT_APARTMENTTHREADED)
            .ok()
            .map_err(|e| e.to_string())?;
        let result = (|| {
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| e.to_string())?;
            dialog
                .SetOptions(FOS_FORCEFILESYSTEM | FOS_FILEMUSTEXIST)
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
            let value = p.to_string().map_err(|e| e.to_string());
            CoTaskMemFree(Some(p.0.cast()));
            let value = value?;
            if !value.to_ascii_lowercase().ends_with(".exe") {
                return Err(
                    "Choose the application's .exe file, or pin it from the running-app list."
                        .into(),
                );
            }
            Ok(Some(value))
        })();
        CoUninitialize();
        result
    })
    .await
    .map_err(|e| e.to_string())?
}
fn edge(c: &BarConfig) -> u32 {
    match c.edge.as_str() {
        "left" => ABE_LEFT,
        "top" => ABE_TOP,
        "right" => ABE_RIGHT,
        _ => ABE_BOTTOM,
    }
}
fn vertical(c: &BarConfig) -> bool {
    c.edge == "left" || c.edge == "right"
}
fn monitor_rect(m: &Monitor) -> RECT {
    RECT {
        left: m.x,
        top: m.y,
        right: m.x + m.width as i32,
        bottom: m.y + m.height as i32,
    }
}
fn bar_rect(c: &BarConfig, mut r: RECT, size: i32) -> RECT {
    match c.edge.as_str() {
        "left" => r.right = r.left + size,
        "right" => r.left = r.right - size,
        "top" => r.bottom = r.top + size,
        _ => r.top = r.bottom - size,
    }
    r
}
unsafe fn position(b: &mut Bar) -> Result<(), String> {
    if b.fullscreen {
        let _ = ShowWindow(b.hwnd, SW_HIDE);
        return Ok(());
    }
    if b.positioning {
        return Ok(());
    }
    b.positioning = true;
    let mut data = APPBARDATA {
        cbSize: size_of::<APPBARDATA>() as u32,
        hWnd: b.hwnd,
        uEdge: edge(&b.config),
        rc: monitor_rect(&b.monitor),
        ..Default::default()
    };
    if !b.config.autohide {
        data.rc = bar_rect(&b.config, data.rc, b.config.size as i32);
        SHAppBarMessage(ABM_QUERYPOS, &mut data);
        data.rc = bar_rect(&b.config, data.rc, b.config.size as i32);
        if data.rc != b.full {
            SHAppBarMessage(ABM_SETPOS, &mut data);
        }
        b.full = data.rc;
    } else {
        b.full = bar_rect(&b.config, data.rc, b.config.size as i32);
    }
    let r = if b.hidden {
        bar_rect(&b.config, b.full, 3)
    } else {
        b.full
    };
    let result = SetWindowPos(
        b.hwnd,
        Some(HWND_TOPMOST),
        r.left,
        r.top,
        r.right - r.left,
        r.bottom - r.top,
        SWP_NOACTIVATE | SWP_SHOWWINDOW,
    )
    .map_err(|e| e.to_string());
    b.positioning = false;
    result
}
fn groups(pins: &[Pin], device: &str, windows: &[(SavedWindow, String)]) -> Vec<Group> {
    let mut out: Vec<Group> = pins
        .iter()
        .map(|p| Group {
            path: p.path.clone(),
            label: p.label.clone(),
            windows: vec![],
            pinned: true,
        })
        .collect();
    for (w, screen) in windows {
        if screen != device {
            continue;
        }
        if let Some(group) = out.iter_mut().find(|g| g.path.eq_ignore_ascii_case(&w.exe)) {
            group.windows.push(w.clone())
        } else {
            out.push(Group {
                path: w.exe.clone(),
                label: Path::new(&w.exe)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                windows: vec![w.clone()],
                pinned: false,
            })
        }
    }
    out
}
unsafe fn scan() -> Result<Vec<(SavedWindow, String)>, String> {
    let mut result = vec![];
    for window in crate::layouts::capture()? {
        let monitor = MonitorFromWindow(HWND(window.handle as *mut _), MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(monitor, (&mut info as *mut MONITORINFOEXW).cast()).as_bool() {
            let len = info.szDevice.iter().position(|&v| v == 0).unwrap_or(32);
            result.push((window, String::from_utf16_lossy(&info.szDevice[..len])));
        }
    }
    Ok(result)
}
unsafe fn update(b: &mut Bar, windows: &[(SavedWindow, String)]) {
    let foreground = GetForegroundWindow();
    let mut bounds = RECT::default();
    let monitor = monitor_rect(&b.monitor);
    let full = windows
        .iter()
        .any(|(w, _)| w.handle == foreground.0 as usize)
        && GetWindowRect(foreground, &mut bounds).is_ok()
        && bounds.left <= monitor.left
        && bounds.top <= monitor.top
        && bounds.right >= monitor.right
        && bounds.bottom >= monitor.bottom;
    if full != b.fullscreen {
        b.fullscreen = full;
        if full {
            let _ = ShowWindow(b.hwnd, SW_HIDE);
        } else {
            let _ = position(b);
        }
    }
    b.groups = groups(&b.config.pins, &b.config.device, windows);
    let wanted: HashSet<_> = b.groups.iter().map(|g| g.path.clone()).collect();
    b.icons.retain(|path, icon| {
        if wanted.contains(path) {
            true
        } else {
            let _ = DestroyIcon(*icon);
            false
        }
    });
    for path in wanted {
        if !b.icons.contains_key(&path) {
            let name = wide(&path);
            let mut icon = HICON::default();
            ExtractIconExW(PCWSTR(name.as_ptr()), 0, None, Some(&mut icon), 1);
            if !icon.is_invalid() {
                b.icons.insert(path, icon);
            }
        }
    }
    let _ = InvalidateRect(Some(b.hwnd), None, false);
}
unsafe fn draw(b: &Bar) {
    let mut paint = PAINTSTRUCT::default();
    let dc = BeginPaint(b.hwnd, &mut paint);
    let mut client = RECT::default();
    let _ = GetClientRect(b.hwnd, &mut client);
    let bg = CreateSolidBrush(COLORREF(0x00251b15));
    FillRect(dc, &client, bg);
    let _ = DeleteObject(bg.into());
    let _ = SetBkMode(dc, TRANSPARENT);
    let _ = SetTextColor(dc, COLORREF(0x00f2eae6));
    let font = GetStockObject(DEFAULT_GUI_FONT);
    let previous = SelectObject(dc, font);
    let size = b.config.size as i32;
    let controls = if vertical(&b.config) {
        size * 4
    } else {
        size + 320
    };
    let mut chevron = RECT {
        left: 0,
        top: 0,
        right: size,
        bottom: size,
    };
    let mut glyph = ">".encode_utf16().collect::<Vec<_>>();
    DrawTextW(
        dc,
        &mut glyph,
        &mut chevron,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
    let mut button = if vertical(&b.config) {
        RECT {
            left: 2,
            top: size,
            right: size - 2,
            bottom: controls,
        }
    } else {
        RECT {
            left: size,
            top: 2,
            right: controls,
            bottom: size - 2,
        }
    };
    let mut title = format!(
        "Open and load window profile: {}",
        crate::dock::selected_name(&b.dock_config)
    )
    .encode_utf16()
    .collect::<Vec<_>>();
    DrawTextW(
        dc,
        &mut title,
        &mut button,
        if vertical(&b.config) {
            DT_CENTER | DT_WORDBREAK | DT_END_ELLIPSIS
        } else {
            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS
        },
    );
    let foreground = GetForegroundWindow().0 as usize;
    for (i, g) in b.groups.iter().enumerate() {
        let (x, y) = if vertical(&b.config) {
            (0, controls + i as i32 * size)
        } else {
            (controls + i as i32 * size, 0)
        };
        if x >= client.right || y >= client.bottom {
            break;
        }
        let mut r = RECT {
            left: x + 3,
            top: y + 3,
            right: x + size - 3,
            bottom: y + size - 3,
        };
        let active = g.windows.iter().any(|w| w.handle == foreground);
        if active || b.hover == Some(i) {
            let fill = CreateSolidBrush(COLORREF(if active { 0x00463925 } else { 0x0045382d }));
            FillRect(dc, &r, fill);
            let _ = DeleteObject(fill.into());
        }
        let icon_size = (size - 16).min(32);
        let pad = (size - icon_size) / 2;
        if let Some(icon) = b.icons.get(&g.path) {
            let _ = DrawIconEx(
                dc,
                x + pad,
                y + pad,
                *icon,
                icon_size,
                icon_size,
                0,
                None,
                DI_NORMAL,
            );
        } else {
            let mut text: Vec<u16> = g
                .label
                .chars()
                .take(2)
                .collect::<String>()
                .encode_utf16()
                .collect();
            DrawTextW(
                dc,
                &mut text,
                &mut r,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
        }
        if !g.windows.is_empty() {
            let brush = CreateSolidBrush(COLORREF(if active { 0x00c5e983 } else { 0x008c7966 }));
            let line = if vertical(&b.config) {
                RECT {
                    left: x + 1,
                    top: y + size / 3,
                    right: x + 4,
                    bottom: y + 2 * size / 3,
                }
            } else {
                RECT {
                    left: x + size / 3,
                    top: y + size - 4,
                    right: x + 2 * size / 3,
                    bottom: y + size - 1,
                }
            };
            FillRect(dc, &line, brush);
            let _ = DeleteObject(brush.into());
        }
        if g.windows.len() > 1 {
            let mut count = g
                .windows
                .len()
                .to_string()
                .encode_utf16()
                .collect::<Vec<_>>();
            let mut badge = RECT {
                left: x + size - 17,
                top: y + 1,
                right: x + size - 2,
                bottom: y + 16,
            };
            DrawTextW(dc, &mut count, &mut badge, DT_CENTER | DT_SINGLELINE);
        } else if g.pinned && g.windows.is_empty() {
            let brush = CreateSolidBrush(COLORREF(0x00706050));
            let dot = RECT {
                left: x + size / 2 - 1,
                top: y + size - 5,
                right: x + size / 2 + 2,
                bottom: y + size - 2,
            };
            FillRect(dc, &dot, brush);
            let _ = DeleteObject(brush.into());
        }
    }
    SelectObject(dc, previous);
    let _ = EndPaint(b.hwnd, &paint);
}
unsafe fn master_menu(b: &Bar) -> Option<String> {
    let menu = CreatePopupMenu().ok()?;
    let label = wide(
        &format!(
            "Open and load window profile: {}",
            crate::dock::selected_name(&b.dock_config)
        )
        .replace('&', "&&"),
    );
    let _ = AppendMenuW(menu, MF_STRING, 1, PCWSTR(label.as_ptr()));
    let mut point = POINT::default();
    let _ = GetCursorPos(&mut point);
    let n = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_NONOTIFY,
        point.x,
        point.y,
        None,
        b.hwnd,
        None,
    )
    .0;
    let _ = DestroyMenu(menu);
    if n == 1 {
        Some(
            b.dock_config
                .selected_window_profile
                .as_ref()
                .map(|id| format!("open-load:{id}"))
                .unwrap_or_else(|| "page:profiles".into()),
        )
    } else {
        None
    }
}
unsafe fn profile_menu(b: &Bar) -> Option<String> {
    let menu = CreatePopupMenu().ok()?;
    let mut actions = vec![];
    let mut add = |label: String, action: String| {
        actions.push(action);
        let label = wide(&label.replace('&', "&&"));
        let _ = AppendMenuW(menu, MF_STRING, actions.len(), PCWSTR(label.as_ptr()));
    };
    add("Monitor Configuration…".into(), "page:displays".into());
    for p in b
        .dock_config
        .profiles
        .iter()
        .filter(|p| !p.monitors.is_empty())
    {
        add(
            format!("Monitor profile: {}", p.name),
            format!("display:{}", p.id),
        );
    }
    add("Window profile editor…".into(), "page:profiles".into());
    for p in b
        .dock_config
        .profiles
        .iter()
        .filter(|p| !p.windows.is_empty())
    {
        add(
            format!(
                "{}Window profile: {}",
                if b.dock_config.selected_window_profile.as_ref() == Some(&p.id) {
                    "✓ "
                } else {
                    ""
                },
                p.name
            ),
            format!("select-window:{}", p.id),
        );
    }
    let mut point = POINT::default();
    let _ = GetCursorPos(&mut point);
    let n = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_NONOTIFY,
        point.x,
        point.y,
        None,
        b.hwnd,
        None,
    )
    .0 as usize;
    let _ = DestroyMenu(menu);
    n.checked_sub(1).and_then(|i| actions.get(i).cloned())
}
unsafe fn selected_window(w: &SavedWindow) {
    let hwnd = HWND(w.handle as *mut _);
    if IsWindow(Some(hwnd)).as_bool() {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindowAsync(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}
unsafe fn launch(path: &str) {
    let p = wide(path);
    ShellExecuteW(
        None,
        w!("open"),
        PCWSTR(p.as_ptr()),
        PCWSTR::null(),
        PCWSTR::null(),
        SW_SHOWNORMAL,
    );
}
unsafe fn choose(b: &Bar, index: usize, hover: bool) {
    let Some(g) = b.groups.get(index) else { return };
    if hover && g.windows.is_empty() {
        return;
    }
    if !hover && g.windows.len() == 1 {
        let w = &g.windows[0];
        if GetForegroundWindow().0 as usize == w.handle {
            let _ = ShowWindowAsync(HWND(w.handle as *mut _), SW_MINIMIZE);
        } else {
            selected_window(w);
        }
        return;
    }
    if !hover && g.windows.is_empty() {
        launch(&g.path);
        return;
    }
    let Ok(menu) = CreatePopupMenu() else { return };
    for (i, w) in g.windows.iter().enumerate() {
        let text = wide(&w.title.replace('&', "&&"));
        let _ = AppendMenuW(menu, MF_STRING, i + 1, PCWSTR(text.as_ptr()));
    }
    if g.pinned {
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let label = wide(&format!("Open new {}", g.label));
        let _ = AppendMenuW(menu, MF_STRING, 10000, PCWSTR(label.as_ptr()));
    }
    let mut point = POINT::default();
    let _ = GetCursorPos(&mut point);
    let result = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_NONOTIFY,
        point.x,
        point.y,
        None,
        b.hwnd,
        None,
    )
    .0 as usize;
    let _ = DestroyMenu(menu);
    if result == 10000 {
        launch(&g.path)
    } else if result > 0 {
        if let Some(w) = g.windows.get(result - 1) {
            selected_window(w)
        }
    }
}
unsafe fn send(msg: u32, hwnd: HWND, value: isize) {
    let _ = PostThreadMessageW(
        THREAD.load(Ordering::Relaxed),
        msg,
        WPARAM(hwnd.0 as usize),
        LPARAM(value),
    );
}
unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const RefCell<Bar>;
    match msg {
        crate::dock::REFRESH_DOCK => {
            send(crate::dock::REFRESH_DOCK, hwnd, 0);
            return LRESULT(0);
        }
        WM_PAINT => {
            if !ptr.is_null() {
                if let Ok(b) = (*ptr).try_borrow() {
                    draw(&b);
                    return LRESULT(0);
                }
            }
        }
        WM_ERASEBKGND => return LRESULT(1),
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_LBUTTONUP => {
            send(ACTION, hwnd, lp.0);
            return LRESULT(0);
        }
        WM_MOUSEMOVE => {
            send(POINTER, hwnd, lp.0);
            return LRESULT(0);
        }
        WM_MOUSEHOVER => {
            send(HOVER, hwnd, lp.0);
            return LRESULT(0);
        }
        WM_MOUSELEAVE => {
            send(LEAVE, hwnd, 0);
            return LRESULT(0);
        }
        WM_TIMER => {
            let _ = KillTimer(Some(hwnd), wp.0);
            if wp.0 == 1 {
                send(REFRESH, HWND::default(), 1)
            } else {
                send(HIDE, hwnd, 0)
            }
            return LRESULT(0);
        }
        WM_DISPLAYCHANGE => {
            send(REPOSITION, HWND::default(), 0);
            return LRESULT(0);
        }
        APPBAR => {
            if wp.0 == ABN_POSCHANGED as usize {
                send(REPOSITION, hwnd, 0)
            }
            return LRESULT(0);
        }
        WM_CLOSE => {
            PostQuitMessage(0);
            return LRESULT(0);
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, wp, lp)
}
unsafe extern "system" fn changed(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _thread: u32,
    _time: u32,
) {
    if object != 0 || child != 0 {
        return;
    }
    if event != EVENT_OBJECT_DESTROY && GetAncestor(hwnd, GA_ROOT) != hwnd {
        return;
    }
    if !DIRTY.swap(true, Ordering::Relaxed) {
        let _ = PostThreadMessageW(
            THREAD.load(Ordering::Relaxed),
            REFRESH,
            WPARAM(0),
            LPARAM(0),
        );
    }
}
unsafe fn cleanup(bars: &[Box<RefCell<Bar>>]) {
    for cell in bars {
        let b = cell.borrow();
        let mut d = APPBARDATA {
            cbSize: size_of::<APPBARDATA>() as u32,
            hWnd: b.hwnd,
            uEdge: edge(&b.config),
            rc: monitor_rect(&b.monitor),
            ..Default::default()
        };
        if b.config.autohide {
            d.lParam = LPARAM(0);
            SHAppBarMessage(ABM_SETAUTOHIDEBAREX, &mut d);
        }
        SHAppBarMessage(ABM_REMOVE, &mut d);
        for icon in b.icons.values() {
            let _ = DestroyIcon(*icon);
        }
        SetWindowLongPtrW(b.hwnd, GWLP_USERDATA, 0);
        let _ = DestroyWindow(b.hwnd);
    }
}
pub fn run(parent: u32) -> Result<(), String> {
    unsafe {
        THREAD.store(GetCurrentThreadId(), Ordering::Relaxed);
        let mut msg = MSG::default();
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
        let stop_event = CreateEventW(None, false, false, w!("Local\\DisplayProLite-StopTaskbars"))
            .map_err(|e| e.to_string())?;
        let raw = stop_event.0 as usize;
        let tid = THREAD.load(Ordering::Relaxed);
        std::thread::spawn(move || {
            let event = HANDLE(raw as *mut _);
            if let Ok(process) = OpenProcess(PROCESS_SYNCHRONIZE, false, parent) {
                WaitForMultipleObjects(&[event, process], false, INFINITE);
                let _ = CloseHandle(process);
            } else {
                WaitForSingleObject(event, INFINITE);
            }
            let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
        });
        let instance = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: w!("DisplayProLiteNativeBar"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return Err("Could not register native taskbar window.".into());
        }
        let config = load()?;
        let monitors = crate::display::enumerate(false)?;
        let mut bars: Vec<Box<RefCell<Bar>>> = vec![];
        let init = (|| {
            for c in config.bars.iter().filter(|b| b.enabled) {
                let Some(m) = monitors
                    .iter()
                    .find(|m| m.enabled && m.identity == c.identity && m.device == c.device)
                else {
                    continue;
                };
                let title = wide(&format!("DisplayProLite Taskbar {}", m.device));
                let hwnd = CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
                    w!("DisplayProLiteNativeBar"),
                    PCWSTR(title.as_ptr()),
                    WS_POPUP,
                    m.x,
                    m.y,
                    10,
                    10,
                    None,
                    None,
                    Some(instance.into()),
                    None,
                )
                .map_err(|e| e.to_string())?;
                let cell = Box::new(RefCell::new(Bar {
                    hwnd,
                    config: c.clone(),
                    monitor: m.clone(),
                    full: RECT::default(),
                    hidden: c.autohide,
                    fullscreen: false,
                    groups: vec![],
                    icons: HashMap::new(),
                    hover: None,
                    positioning: false,
                    dock_config: crate::dock::disk_config(),
                }));
                SetWindowLongPtrW(
                    hwnd,
                    GWLP_USERDATA,
                    (&*cell as *const RefCell<Bar>) as isize,
                );
                bars.push(cell);
                let mut d = APPBARDATA {
                    cbSize: size_of::<APPBARDATA>() as u32,
                    hWnd: hwnd,
                    uCallbackMessage: APPBAR,
                    uEdge: edge(c),
                    rc: monitor_rect(m),
                    ..Default::default()
                };
                if SHAppBarMessage(ABM_NEW, &mut d) == 0 {
                    return Err("Windows rejected appbar registration.".into());
                }
                if c.autohide {
                    d.lParam = LPARAM(1);
                    if SHAppBarMessage(ABM_SETAUTOHIDEBAREX, &mut d) == 0 {
                        return Err(format!("Another auto-hide taskbar already owns the {} edge of {}. Choose another edge or turn off its auto-hide.",c.edge,m.label));
                    }
                }
                position(&mut bars.last().unwrap().borrow_mut())?;
            }
            if bars.is_empty() {
                return Err("No enabled taskbar matches a connected monitor.".into());
            }
            let windows = scan()?;
            for b in &bars {
                update(&mut b.borrow_mut(), &windows)
            }
            Ok(())
        })();
        if let Err(e) = init {
            cleanup(&bars);
            return Err(e);
        }
        let events = [
            (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
            (EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE),
            (EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_NAMECHANGE),
            (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
        ];
        let hooks: Vec<_> = events
            .iter()
            .map(|&(a, b)| {
                SetWinEventHook(
                    a,
                    b,
                    None,
                    Some(changed),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                )
            })
            .collect();
        if hooks.iter().any(|h| h.0.is_null()) {
            for hook in hooks {
                let _ = UnhookWinEvent(hook);
            }
            cleanup(&bars);
            return Err("Windows could not register taskbar window-event notifications.".into());
        }
        crate::write_json(
            &crate::data_dir().join("taskbars-ready.json"),
            &Result::<(), String>::Ok(()),
        )?;
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            match msg.message {
                crate::dock::REFRESH_DOCK => {
                    let c = crate::dock::disk_config();
                    for cell in &bars {
                        let mut b = cell.borrow_mut();
                        b.dock_config = c.clone();
                        let _ = InvalidateRect(Some(b.hwnd), None, false);
                    }
                }
                REFRESH => {
                    if msg.lParam.0 == 0 {
                        SetTimer(Some(bars[0].borrow().hwnd), 1, 250, None);
                    } else {
                        DIRTY.store(false, Ordering::Relaxed);
                        if let Ok(windows) = scan() {
                            for b in &bars {
                                update(&mut b.borrow_mut(), &windows)
                            }
                        }
                    }
                }
                REPOSITION => {
                    let updated = if msg.wParam.0 == 0 {
                        crate::display::enumerate(false).ok()
                    } else {
                        None
                    };
                    for cell in &bars {
                        let mut b = cell.borrow_mut();
                        if let Some(monitors) = &updated {
                            if let Some(m) = monitors.iter().find(|m| {
                                m.enabled
                                    && m.device == b.config.device
                                    && m.identity == b.config.identity
                            }) {
                                b.monitor = m.clone();
                            } else {
                                let _ = ShowWindow(b.hwnd, SW_HIDE);
                                continue;
                            }
                        }
                        if msg.wParam.0 == 0 || msg.wParam.0 == b.hwnd.0 as usize {
                            let _ = position(&mut b);
                        }
                    }
                }
                ACTION | POINTER | HOVER | LEAVE | HIDE => {
                    if let Some(cell) = bars
                        .iter()
                        .find(|c| c.borrow().hwnd.0 as usize == msg.wParam.0)
                    {
                        let mut b = cell.borrow_mut();
                        let x = (msg.lParam.0 as u16) as i16 as i32;
                        let y = ((msg.lParam.0 >> 16) as u16) as i16 as i32;
                        let coordinate = if vertical(&b.config) { y } else { x };
                        let controls = if vertical(&b.config) {
                            b.config.size as i32 * 4
                        } else {
                            b.config.size as i32 + 320
                        };
                        let index = if coordinate >= controls {
                            (coordinate - controls) as usize / b.config.size as usize
                        } else if coordinate < b.config.size as i32 {
                            usize::MAX
                        } else {
                            usize::MAX - 1
                        };
                        if msg.message == HOVER
                            && coordinate >= b.config.size as i32
                            && coordinate < controls
                        {
                            if let Some(action) = master_menu(&b) {
                                let _ = crate::dock::request(&action);
                            }
                            continue;
                        }
                        if msg.message == ACTION && coordinate < controls {
                            let action = if coordinate < b.config.size as i32 {
                                profile_menu(&b)
                            } else {
                                b.dock_config
                                    .selected_window_profile
                                    .as_ref()
                                    .map(|id| format!("open-load:{id}"))
                                    .or(Some("page:profiles".into()))
                            };
                            if let Some(action) = action {
                                if let Err(e) = crate::dock::request(&action) {
                                    let text = wide(&e);
                                    MessageBoxW(
                                        Some(b.hwnd),
                                        PCWSTR(text.as_ptr()),
                                        w!("DisplayProLite"),
                                        MB_OK | MB_ICONINFORMATION,
                                    );
                                }
                            }
                            continue;
                        }
                        match msg.message {
                            ACTION => choose(&b, index, false),
                            HOVER => choose(&b, index, true),
                            POINTER => {
                                if b.hidden {
                                    b.hidden = false;
                                    let _ = position(&mut b);
                                }
                                if b.hover != Some(index) {
                                    b.hover = Some(index);
                                    let _ = InvalidateRect(Some(b.hwnd), None, false);
                                    let mut track = TRACKMOUSEEVENT {
                                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                                        dwFlags: TME_LEAVE | TME_HOVER,
                                        hwndTrack: b.hwnd,
                                        dwHoverTime: 450,
                                    };
                                    let _ = TrackMouseEvent(&mut track);
                                }
                            }
                            LEAVE => {
                                b.hover = None;
                                let _ = InvalidateRect(Some(b.hwnd), None, false);
                                if b.config.autohide {
                                    SetTimer(Some(b.hwnd), 2, 600, None);
                                }
                            }
                            HIDE => {
                                let mut p = POINT::default();
                                let _ = GetCursorPos(&mut p);
                                if p.x < b.full.left
                                    || p.x >= b.full.right
                                    || p.y < b.full.top
                                    || p.y >= b.full.bottom
                                {
                                    b.hidden = true;
                                    let _ = position(&mut b);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
        for hook in hooks {
            let _ = UnhookWinEvent(hook);
        }
        cleanup(&bars);
        let _ = CloseHandle(stop_event);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn window(h: usize, exe: &str) -> SavedWindow {
        SavedWindow {
            handle: h,
            pid: 1,
            exe: exe.into(),
            class: "App".into(),
            title: h.to_string(),
            rect: [0, 0, 100, 100],
            maximized: false,
            minimized: false,
            launch: None,
            restore_only: false,
        }
    }
    #[test]
    fn groups_only_this_monitors_windows_but_keeps_pins() {
        let windows = vec![
            (window(1, "a.exe"), "screen1".into()),
            (window(2, "a.exe"), "screen1".into()),
            (window(3, "b.exe"), "screen2".into()),
        ];
        let pins = vec![Pin {
            path: "b.exe".into(),
            label: "B".into(),
        }];
        let g = groups(&pins, "screen1", &windows);
        assert_eq!(g.len(), 2);
        assert!(g[0].pinned);
        assert!(g[0].windows.is_empty());
        assert_eq!(g[1].windows.len(), 2);
    }
}
