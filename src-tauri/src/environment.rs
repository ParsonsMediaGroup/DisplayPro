//! Small backend-only .env.local reader. Existing process variables win.
use std::{collections::HashSet, path::PathBuf};
pub fn load() -> Result<(), String> {
    let mut candidates = vec![];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join(".env.local"));
        }
    }
    if cfg!(debug_assertions) {
        if let Ok(dir) = std::env::current_dir() {
            candidates.push(dir.join(".env.local"));
            if dir.file_name().is_some_and(|s| s == "src-tauri") {
                if let Some(parent) = dir.parent() {
                    candidates.push(parent.join(".env.local"));
                }
            }
        }
    }
    let mut seen = HashSet::new();
    for path in candidates {
        if !path.is_file() || !seen.insert(path.clone()) {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|_| format!("Cannot read local environment file: {}", path.display()))?;
        for (index, line) in text.lines().enumerate() {
            if let Some((name, value)) = parse(line).map_err(|_| {
                format!(
                    "Invalid local environment entry at {} line {} (value omitted).",
                    path.display(),
                    index + 1
                )
            })? {
                if std::env::var_os(&name).is_none() {
                    std::env::set_var(name, value);
                }
            }
        }
    }
    Ok(())
}
fn parse(line: &str) -> Result<Option<(String, String)>, ()> {
    let line = line.trim().trim_start_matches('\u{feff}');
    if line.is_empty() || line.starts_with('#') {
        return Ok(None);
    }
    let line = line.strip_prefix("export ").unwrap_or(line);
    let (name, value) = line.split_once('=').ok_or(())?;
    let name = name.trim();
    if name.is_empty()
        || name.as_bytes()[0].is_ascii_digit()
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(());
    }
    let value = value.trim();
    let value = if value.starts_with('"') || value.starts_with('\'') {
        if value.len() < 2 || value.as_bytes()[0] != *value.as_bytes().last().ok_or(())? {
            return Err(());
        }
        &value[1..value.len() - 1]
    } else {
        value.split(" #").next().unwrap_or(value).trim_end()
    };
    Ok(Some((name.into(), value.into())))
}
pub fn configured_dir() -> Option<PathBuf> {
    std::env::var_os("DISPLAYPROLITE_DATA_DIR")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}
pub fn screensaver_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    if !exe.extension()?.eq_ignore_ascii_case("scr") {
        return None;
    }
    let text = std::fs::read_to_string(exe.parent()?.join("DisplayProLite.data-dir")).ok()?;
    let path = PathBuf::from(text.trim());
    path.is_absolute().then_some(path)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_environment_parsing_preserves_windows_paths() {
        assert_eq!(
            parse("ROOT=C:\\Example\\private").unwrap().unwrap().1,
            "C:\\Example\\private"
        );
        assert_eq!(
            parse("NAME=\"hello world\"").unwrap().unwrap().1,
            "hello world"
        );
        assert!(parse("# private config").unwrap().is_none());
        assert!(parse("BAD NAME=value").is_err());
    }
}
