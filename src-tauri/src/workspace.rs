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
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct LaunchCommand {
    pub exe: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub working_dir: String,
}
#[derive(Serialize)]
pub struct Report {
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
    for c in commands.iter().chain(startup.iter()) {
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
    a.exe.eq_ignore_ascii_case(&b.exe) && (a.class.is_empty() || a.class == b.class)
}
// Exact matches first, then stable slot ordering for otherwise identical windows.
// Only the explicit Open and load action uses this fallback.
fn assign(saved: &[SavedWindow], current: &[SavedWindow]) -> Vec<Option<usize>> {
    let mut used = HashSet::new();
    let mut result = vec![None; saved.len()];
    for (i, s) in saved.iter().enumerate() {
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
            .min_by_key(|(_, w)| w.handle)
        {
            result[i] = Some(j);
            used.insert(w.handle);
        }
    }
    result
}
pub fn open(profile: &crate::Profile) -> Result<Report, String> {
    let _guard = RUNNING
        .try_lock()
        .map_err(|_| "A window profile is already opening. Wait for it to finish.")?;
    validate(&profile.windows, &profile.launches, &profile.startup)?;
    let deadline = Instant::now() + Duration::from_secs(120);
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
                return Err("Startup command exceeded the 120-second limit.".into());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    let mut report = Report {
        launched: 0,
        restored: 0,
        skipped: vec![],
    };
    let mut attempted = HashSet::new();
    loop {
        let current = layouts::capture()?;
        let mapping = assign(&profile.windows, &current);
        let Some(i) = mapping
            .iter()
            .enumerate()
            .find(|(i, m)| m.is_none() && !attempted.contains(i))
            .map(|(i, _)| i)
        else {
            break;
        };
        if Instant::now() >= deadline {
            report
                .skipped
                .push("Workspace launch reached its 120-second limit.".into());
            break;
        }
        attempted.insert(i);
        let saved = &profile.windows[i];
        let command = profile
            .launches
            .iter()
            .find(|c| c.exe.eq_ignore_ascii_case(&saved.exe))
            .cloned()
            .unwrap_or(LaunchCommand {
                exe: saved.exe.clone(),
                ..Default::default()
            });
        if let Err(e) = launch(&command) {
            report.skipped.push(e);
            continue;
        }
        report.launched += 1;
        let wait_until = (Instant::now() + Duration::from_secs(15)).min(deadline);
        loop {
            std::thread::sleep(Duration::from_millis(250));
            let now = layouts::capture()?;
            if assign(&profile.windows, &now)
                .iter()
                .filter(|m| m.is_some())
                .count()
                > mapping.iter().filter(|m| m.is_some()).count()
                || Instant::now() >= wait_until
            {
                break;
            }
        }
    }
    let current = layouts::capture()?;
    let mapping = assign(&profile.windows, &current);
    let mut targets = vec![];
    for (s, m) in profile.windows.iter().zip(mapping) {
        if let Some(i) = m {
            let mut w = s.clone();
            w.handle = current[i].handle;
            w.pid = current[i].pid;
            w.class = current[i].class.clone();
            w.title = current[i].title.clone();
            targets.push(w);
        } else {
            report.skipped.push(format!(
                "{}: no matching window appeared. This app may need new-window arguments.",
                s.title
            ));
        }
    }
    let mut restored = layouts::restore(&targets)?;
    // Apps may finish initializing their window bounds after they first appear.
    // Retry only requested slots, then report applications that constrain their size.
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(250));
        let actual = layouts::capture()?;
        let mismatched: Vec<_> = targets
            .iter()
            .filter(|s| {
                actual.iter().any(|a| {
                    a.handle == s.handle
                        && a.pid == s.pid
                        && (a.rect != s.rect
                            || a.maximized != s.maximized
                            || a.minimized != s.minimized)
                })
            })
            .cloned()
            .collect();
        if mismatched.is_empty() {
            break;
        }
        let _ = layouts::restore(&mismatched)?;
    }
    let actual = layouts::capture()?;
    restored.restored = 0;
    for s in &targets {
        if actual.iter().any(|a| {
            a.handle == s.handle
                && a.pid == s.pid
                && a.rect == s.rect
                && a.maximized == s.maximized
                && a.minimized == s.minimized
        }) {
            restored.restored += 1;
        } else {
            restored.skipped.push(format!("{}: the app constrained its placement, closed, or the saved display is unavailable.",s.title));
        }
    }
    report.restored = restored.restored;
    report.skipped.extend(restored.skipped);
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
        }
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
