//! On-demand workspace launcher. No background polling when idle.
use crate::layouts::{self, SavedWindow};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::Path,
    process::Command,
    sync::Mutex,
    time::{Duration, Instant},
};

static RUNNING: Mutex<()> = Mutex::new(());
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LaunchCommand {
    pub exe: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub working_dir: String,
}
#[derive(Serialize)]
pub struct Report {
    pub requested: usize,
    pub found: usize,
    pub launched: usize,
    pub restored: usize,
    pub skipped: Vec<String>,
}
pub fn validate(
    windows: &[SavedWindow],
    commands: &[LaunchCommand],
    startup: &Option<LaunchCommand>,
) -> Result<(), String> {
    if windows.len() > 100 || commands.len() > 100 {
        return Err("A workspace supports up to 100 window slots and launch commands.".into());
    }
    for w in windows {
        let [l, t, r, b] = w.rect;
        if w.rect.iter().any(|v| v.unsigned_abs() > 100000) || r <= l || b <= t {
            return Err(
                "Window sizes must be positive and coordinates within 100,000 pixels.".into(),
            );
        }
    }
    let mut seen = HashSet::new();
    for c in commands
        .iter()
        .chain(startup.iter())
        .chain(windows.iter().filter_map(|w| w.launch.as_ref()))
    {
        if c.exe.trim().is_empty()
            || !Path::new(&c.exe).is_absolute()
            || !c.exe.to_lowercase().ends_with(".exe")
            || c.args.len() > 100
            || c.args.iter().any(|a| a.len() > 32768 || a.contains('\0'))
            || c.working_dir.contains('\0')
        {
            return Err("Launch commands need an absolute .exe path and valid arguments. For scripts, use a script interpreter and pass the script path as an argument.".into());
        }
    }
    for c in commands {
        if !seen.insert(c.exe.to_lowercase()) {
            return Err("Only one launch command per application is supported.".into());
        }
    }
    Ok(())
}
fn launch(c: &LaunchCommand) -> Result<std::process::Child, String> {
    if !Path::new(&c.exe).is_file() {
        return Err(format!("Application not found: {}", c.exe));
    }
    let mut command = Command::new(&c.exe);
    command.args(&c.args);
    if !c.working_dir.is_empty() {
        command.current_dir(&c.working_dir);
    }
    command
        .spawn()
        .map_err(|e| format!("Could not launch {}: {e}", c.exe))
}
fn compatible(a: &SavedWindow, b: &SavedWindow) -> bool {
    if !a.exe.eq_ignore_ascii_case(&b.exe) || (!a.class.is_empty() && a.class != b.class) {
        return false;
    }
    if filename(&a.exe) == "pycharm64.exe" {
        let project = a.title.split(" \u{2013} ").next().unwrap_or("");
        return b.title == project || b.title.starts_with(&format!("{project} \u{2013} "));
    }
    true
}
// Exact matches first, then stable slot ordering for otherwise identical windows.
// Only the explicit Open and load action uses this fallback.
fn assign(saved: &[SavedWindow], current: &[SavedWindow]) -> Vec<Option<usize>> {
    let mut used = HashSet::new();
    let mut result = vec![None; saved.len()];
    for (i, s) in saved.iter().enumerate() {
        if let Some((j, w)) = current.iter().enumerate().find(|(_, w)| {
            !used.contains(&w.handle) && compatible(s, w) && w.handle == s.handle && w.pid == s.pid
        }) {
            result[i] = Some(j);
            used.insert(w.handle);
        }
    }
    for (i, s) in saved.iter().enumerate() {
        if result[i].is_some() {
            continue;
        }
        let exact: Vec<_> = current
            .iter()
            .enumerate()
            .filter(|(_, w)| !used.contains(&w.handle) && compatible(s, w) && w.title == s.title)
            .collect();
        if let Some((j, w)) = exact
            .iter()
            .find(|(_, w)| w.handle == s.handle && w.pid == s.pid)
            .or_else(|| {
                if exact.len() == 1 {
                    exact.first()
                } else {
                    None
                }
            })
        {
            result[i] = Some(*j);
            used.insert(w.handle);
        }
    }
    for (i, s) in saved
        .iter()
        .enumerate()
        .filter(|(i, _)| result[*i].is_none())
        .collect::<Vec<_>>()
    {
        if let Some((j, w)) = current
            .iter()
            .enumerate()
            .filter(|(_, w)| !used.contains(&w.handle) && compatible(s, w))
            .min_by_key(|(_, w)| {
                (
                    s.rect
                        .iter()
                        .zip(w.rect)
                        .map(|(a, b)| (*a as i64 - b as i64).unsigned_abs())
                        .sum::<u64>(),
                    w.handle,
                )
            })
        {
            result[i] = Some(j);
            used.insert(w.handle);
        }
    }
    result
}
fn filename(exe: &str) -> String {
    exe.rsplit(['\\', '/'])
        .next()
        .unwrap_or(exe)
        .to_ascii_lowercase()
}
pub fn default_command(w: &SavedWindow) -> Result<LaunchCommand, String> {
    let mut command = LaunchCommand {
        exe: w.exe.clone(),
        ..Default::default()
    };
    command.args=match filename(&w.exe).as_str(){
        "termius.exe"=>vec!["--new-window".into()],
        "firefox.exe" if w.class=="MozillaWindowClass" || w.class.is_empty()=>vec!["--new-window".into(),"about:blank".into()],
        "firefox.exe"=>return Err("This Firefox slot is a secondary/dialog window. Restore it while open, or configure its own launch command.".into()),
        "chrome.exe"|"msedge.exe"=>vec!["--new-window".into(),"about:blank".into()],
        "windowsterminal.exe"|"wt.exe"=>{
            if let Some(local)=std::env::var_os("LOCALAPPDATA") {let alias=std::path::PathBuf::from(local).join("Microsoft/WindowsApps/wt.exe");if alias.is_file(){command.exe=alias.to_string_lossy().into_owned();}}
            vec!["-w".into(),"new".into()]
        },
        "python.exe"|"pythonw.exe"|"powershell.exe"|"pwsh.exe"|"cmd.exe"=>return Err("A script or shell slot needs its own launch arguments; opening the interpreter alone would not restart the saved task.".into()),
        "pycharm64.exe"=>{
            let project=w.title.split(" – ").next().unwrap_or("").trim();
            let home=std::env::var_os("USERPROFILE").map(std::path::PathBuf::from);
            let path=home.and_then(|home| {
                if project.is_empty()||project==".."||project.contains(['\\','/',':']){return None;}
                [home.join("PycharmProjects").join(project),home.join("Desktop").join(project)].into_iter().find(|p|p.join(".idea").is_dir())
            });
            if let Some(path)=path {vec![path.to_string_lossy().into_owned()]}else{return Err("This PyCharm slot needs its project-folder argument. Configure it under this window's launch settings.".into());}
        },
        _=>vec![],
    };
    Ok(command)
}
fn bind_slots(saved: &[SavedWindow], current: &[SavedWindow], bound: &mut [Option<SavedWindow>]) {
    for entry in bound.iter_mut() {
        if let Some(old) = entry {
            if let Some(now) = current
                .iter()
                .find(|w| w.handle == old.handle && w.pid == old.pid && compatible(old, w))
            {
                *old = now.clone();
            } else {
                *entry = None;
            }
        }
    }
    let used: HashSet<_> = bound.iter().flatten().map(|w| w.handle).collect();
    let available: Vec<_> = current
        .iter()
        .filter(|w| !used.contains(&w.handle))
        .cloned()
        .collect();
    let missing: Vec<_> = bound
        .iter()
        .enumerate()
        .filter_map(|(i, w)| w.is_none().then_some(i))
        .collect();
    let desired: Vec<_> = missing.iter().map(|i| saved[*i].clone()).collect();
    for (i, found) in missing.into_iter().zip(assign(&desired, &available)) {
        if let Some(j) = found {
            bound[i] = Some(available[j].clone());
        }
    }
}
fn target_windows(saved: &[SavedWindow], bound: &[Option<SavedWindow>]) -> Vec<SavedWindow> {
    saved
        .iter()
        .zip(bound)
        .filter_map(|(s, live)| {
            live.as_ref().map(|live| {
                let mut w = s.clone();
                w.handle = live.handle;
                w.pid = live.pid;
                w.title = live.title.clone();
                w.class = live.class.clone();
                w
            })
        })
        .collect()
}
fn in_place(s: &SavedWindow, a: &SavedWindow) -> bool {
    s.handle == a.handle
        && s.pid == a.pid
        && s.rect == a.rect
        && s.maximized == a.maximized
        && s.minimized == a.minimized
}
pub fn open(profile: &crate::Profile) -> Result<Report, String> {
    crate::diagnostics::event(
        "workspace_start",
        serde_json::json!({"profile":profile.name,"requested":profile.windows.iter().map(crate::diagnostics::window).collect::<Vec<_>>(),"monitors":crate::display::enumerate(false).ok()}),
    );
    let result = open_inner(profile);
    crate::diagnostics::event(
        "workspace_result",
        serde_json::json!({"profile":profile.name,"result":result}),
    );
    let _ = crate::write_json(
        &crate::data_dir().join("last-workspace-report.json"),
        &serde_json::json!({"profile":profile.name,"time":crate::now(),"result":result}),
    );
    result
}
fn open_inner(profile: &crate::Profile) -> Result<Report, String> {
    let _guard = RUNNING
        .try_lock()
        .map_err(|_| "A window profile is already opening. Wait for it to finish.")?;
    validate(&profile.windows, &profile.launches, &profile.startup)?;
    let deadline = Instant::now() + Duration::from_secs(180);
    if let Some(c) = &profile.startup {
        let mut child = launch(c)?;
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                if !status.success() {
                    return Err("The startup command failed; window launching was stopped.".into());
                }
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                return Err("Startup command exceeded the 180-second limit.".into());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    let mut report = Report {
        requested: profile.windows.len(),
        found: 0,
        launched: 0,
        restored: 0,
        skipped: vec![],
    };
    let mut bound = vec![None; profile.windows.len()];
    let mut stalled = HashSet::new();
    let mut last_launch = None;
    bind_slots(&profile.windows, &layouts::capture()?, &mut bound);
    for (i, saved) in profile.windows.iter().enumerate() {
        bind_slots(&profile.windows, &layouts::capture()?, &mut bound);
        if bound[i].is_some() {
            continue;
        }
        if saved.restore_only {
            report
                .skipped
                .push(format!("{}: restore-only slot is not open.", saved.title));
            continue;
        }
        if Instant::now() >= deadline {
            report
                .skipped
                .push("Workspace launch reached its 180-second limit.".into());
            break;
        }
        if filename(&saved.exe) == "firefox.exe"
            && saved.class == "MozillaDialogClass"
            && saved.launch.is_none()
        {
            report.skipped.push(format!("{}: secondary Firefox windows need a per-window launch command or must already be open.",saved.title));
            continue;
        }
        let command = saved
            .launch
            .clone()
            .or_else(|| {
                profile
                    .launches
                    .iter()
                    .find(|c| c.exe.eq_ignore_ascii_case(&saved.exe))
                    .cloned()
            })
            .map(Ok)
            .unwrap_or_else(|| default_command(saved));
        let command = match command {
            Ok(c) => c,
            Err(e) => {
                report.skipped.push(format!("{}: {e}", saved.title));
                continue;
            }
        };
        let group = serde_json::to_string(&(&command, &saved.class)).map_err(|e| e.to_string())?;
        if stalled.contains(&group) {
            continue;
        }
        let before: HashSet<_> = layouts::capture()?
            .iter()
            .map(|w| (w.handle, w.pid))
            .collect();
        let child = match launch(&command) {
            Ok(child) => child,
            Err(e) => {
                report.skipped.push(e);
                stalled.insert(group);
                continue;
            }
        };
        let launched_pid = child.id();
        report.launched += 1;
        last_launch = Some(Instant::now());
        let until = (Instant::now() + Duration::from_secs(30)).min(deadline);
        while Instant::now() < until {
            std::thread::sleep(Duration::from_millis(250));
            let current = layouts::capture()?;
            if bound[i].is_none() {
                let used: HashSet<_> = bound.iter().flatten().map(|w| (w.handle, w.pid)).collect();
                if let Some(created) = current
                    .iter()
                    .filter(|w| {
                        !before.contains(&(w.handle, w.pid))
                            && !used.contains(&(w.handle, w.pid))
                            && compatible(saved, w)
                    })
                    .min_by_key(|w| (w.pid != launched_pid, w.handle))
                {
                    bound[i] = Some(created.clone());
                }
            }
            bind_slots(&profile.windows, &current, &mut bound);
            if bound[i].is_some() {
                break;
            }
        }
        if bound[i].is_none() {
            stalled.insert(group);
        }
    }
    // Keep slot-to-window bindings stable across title changes and startup resizes.
    // Stop after a stable placement interval, never poll while idle.
    let settle_until =
        (Instant::now() + Duration::from_secs(15)).min(deadline + Duration::from_secs(15));
    let mut stable_since = None;
    loop {
        let current = layouts::capture()?;
        bind_slots(&profile.windows, &current, &mut bound);
        let targets = target_windows(&profile.windows, &bound);
        let mismatched: Vec<_> = targets
            .iter()
            .filter(|s| !current.iter().any(|a| in_place(s, a)))
            .cloned()
            .collect();
        if mismatched.is_empty() {
            let since = stable_since.get_or_insert_with(Instant::now);
            let initialized = last_launch.is_none_or(|t| t.elapsed() >= Duration::from_secs(5));
            if initialized && since.elapsed() >= Duration::from_secs(3) {
                break;
            }
        } else {
            stable_since = None;
            let _ = layouts::restore(&mismatched)?;
        }
        if Instant::now() >= settle_until {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let actual = layouts::capture()?;
    bind_slots(&profile.windows, &actual, &mut bound);
    report.found = bound.iter().flatten().count();
    crate::diagnostics::event(
        "workspace_final",
        serde_json::json!({"profile":profile.name,"slots":profile.windows.iter().zip(&bound).map(|(saved, actual)|serde_json::json!({"requested":crate::diagnostics::window(saved),"actual":actual.as_ref().map(crate::diagnostics::window)})).collect::<Vec<_>>()}),
    );
    let targets = target_windows(&profile.windows, &bound);
    for s in &targets {
        if actual.iter().any(|a| in_place(s, a)) {
            report.restored += 1;
        } else {
            report.skipped.push(format!("{}: could not confirm the saved size/position; the app may constrain its bounds or require elevation.",s.title));
        }
    }
    for (s, b) in profile.windows.iter().zip(&bound) {
        if b.is_none() {
            report.skipped.push(format!(
                "{}: expected window did not appear; check this slot's launch settings.",
                s.title
            ));
        }
    }
    report.skipped.sort();
    report.skipped.dedup();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn w(h: usize, title: &str) -> SavedWindow {
        SavedWindow {
            handle: h,
            pid: 1,
            exe: "C:\\App.exe".into(),
            class: "App".into(),
            title: title.into(),
            rect: [0, 0, 800, 600],
            maximized: false,
            minimized: false,
            launch: None,
            restore_only: false,
        }
    }
    #[test]
    fn pycharm_matches_project_after_editor_title_changes() {
        let mut saved = w(1, "ExampleProject \u{2013} old.py");
        saved.exe = "C:\\Apps\\pycharm64.exe".into();
        let mut live = saved.clone();
        live.title = "ExampleProject \u{2013} new.py".into();
        assert!(compatible(&saved, &live));
        live.title = "AnotherProject \u{2013} new.py".into();
        assert!(!compatible(&saved, &live));
    }
    #[test]
    fn changing_titles_do_not_swap_existing_windows() {
        let saved = vec![w(1, "A"), w(2, "B")];
        let current = vec![w(1, "B"), w(2, "A")];
        assert_eq!(assign(&saved, &current), vec![Some(0), Some(1)]);
    }
    #[test]
    fn bindings_stay_stable_when_a_new_window_appears() {
        let saved = vec![w(11, "A"), w(12, "B")];
        let mut bound = vec![None, None];
        bind_slots(&saved, &[w(20, "untitled")], &mut bound);
        let original = bound[0].as_ref().unwrap().handle;
        bind_slots(&saved, &[w(10, "A"), w(20, "B")], &mut bound);
        assert_eq!(bound[0].as_ref().unwrap().handle, original);
        assert_eq!(bound[1].as_ref().unwrap().handle, 10);
    }
    #[test]
    fn known_apps_request_new_windows_and_scripts_require_context() {
        let mut window = w(1, "x");
        window.exe = "C:\\Apps\\Termius.exe".into();
        assert_eq!(default_command(&window).unwrap().args, vec!["--new-window"]);
        window.exe = "C:\\Apps\\firefox.exe".into();
        window.class = "MozillaWindowClass".into();
        assert_eq!(
            default_command(&window).unwrap().args,
            vec!["--new-window", "about:blank"]
        );
        window.class = "MozillaDialogClass".into();
        assert!(default_command(&window).is_err());
        window.exe = "C:\\Python\\python.exe".into();
        assert!(default_command(&window).is_err());
    }
    #[test]
    fn slots_reserve_exact_titles_before_fallback() {
        let saved = vec![w(10, "missing"), w(11, "B")];
        let current = vec![w(1, "B"), w(2, "A")];
        assert_eq!(assign(&saved, &current), vec![Some(1), Some(0)]);
    }
    #[test]
    fn slots_never_reuse_a_window() {
        assert_eq!(
            assign(&[w(9, "x"), w(10, "x")], &[w(1, "x")]),
            vec![Some(0), None]
        );
    }
    #[test]
    fn one_exact_window_is_reserved_for_its_slot() {
        assert_eq!(
            assign(&[w(9, "A"), w(10, "B")], &[w(1, "B")]),
            vec![None, Some(0)]
        );
    }
    #[test]
    fn old_profiles_load_without_launch_fields() {
        let p: crate::Profile =
            serde_json::from_str(r#"{"id":"old","name":"Old profile","monitors":[],"windows":[]}"#)
                .unwrap();
        assert!(p.launches.is_empty());
        assert!(p.startup.is_none());
    }
    #[test]
    fn script_commands_require_an_explicit_interpreter() {
        let c = LaunchCommand {
            exe: "C:\\Scripts\\work.ps1".into(),
            ..Default::default()
        };
        assert!(validate(&[], &[], &Some(c)).is_err());
    }
    #[test]
    fn invalid_geometry_is_rejected() {
        let mut x = w(1, "x");
        x.rect = [100, 0, 0, 10];
        assert!(validate(&[x], &[], &None).is_err());
    }
}
