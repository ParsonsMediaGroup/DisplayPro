# Changelog

## 0.2.1 ? 2026-11-09

- Load & apply now changes monitor profiles immediately with the existing confirmation countdown; Preview only remains available.
- Fix disabled-monitor mode fields and avoid isolated primary-detachment validation failures.
- Persist display-application errors and show them in a visible error banner.
- Add read-only saved-profile diagnostics and regression coverage.

## 0.2.0 — 2026-11-09

- Monitor-profile load/save controls, detected-profile indicator and explicit overwrite guard.
- Native dock chevron and tray submenus for selecting saved monitor and window profiles.
- Persistent selected window profile and a named Open and load master action.
- Workspace editor with app quantities, editable per-window geometry/state, layout preview, launch arguments and an optional startup command.
- Bounded on-demand missing-window launch, window reuse, exact-placement verification and failure reporting.
- Synthetic dashboard coverage and a native two-window launch/reuse self-test.

## 0.1.0 — 2026-11-09

Initial preview of DisplayProLite:

- Tauri dashboard with a Rust/Win32 backend and unload-on-close behavior.
- Monitor resolution, refresh rate, arrangement, primary-display settings and saved profiles.
- Timed display confirmation with a separate rollback helper.
- Saved window positions with matching, ambiguity reporting and off-screen recovery.
- Per-screen photo/video folders, shuffle, fit/fill and Windows screensaver integration.
- Global hotkeys, profile cycling and next screensaver media actions.
- Optional native per-monitor taskbars with edge placement, auto-hide, independent executable pins, monitor-local window grouping and hover selection lists.
- High-contrast app/tray icon.
- Private settings import/export and backend-only local environment configuration.
- Public-repository safeguards, documentation, unit checks and isolated dashboard tests.

Native interactive acceptance testing is still pending; see README validation status.
