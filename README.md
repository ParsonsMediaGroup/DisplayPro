# DisplayProLite

**Version: 0.2.0 — workspace launch preview.** Public source repository: [ParsonsMediaGroup/DisplayPro](https://github.com/ParsonsMediaGroup/DisplayPro).

A local Windows desktop utility built with Tauri 2, Rust, TypeScript and native Windows APIs. Source lives in this project folder; no account or server is required.

## Use case

DisplayProLite is for multi-monitor Windows users who want to switch between work/gaming/design layouts, restore application positions, assign photo/video screensavers to different monitors, and configure independent taskbars without a large desktop-management suite. Its design prioritizes low idle activity and explicit actions.

The Tauri dashboard sends commands to a Rust backend. Native Windows APIs enumerate display modes, stage and apply monitor changes, capture window placements, and register global hotkeys. Profiles are local JSON. Optional taskbars are native Win32/GDI appbars in a separate process, updated by coalesced window events. Folder screensavers use separate media windows only while active, with Windows providing the idle timeout. The dashboard webview is destroyed when its window closes.

## Dashboard

- **Monitor Configuration:** detected monitors, supported resolution/Hz combinations, primary display, enable/disable, and desktop X/Y placement. Apply changes with a 25-second automatic rollback. A separate helper restores the previous setup even if the dashboard crashes. Profiles are rejected when their connected monitor identities no longer match.
- **Window Positions:** capture selected open windows into named profiles, optionally including the display setup. Restore normal, maximized and minimized placements. Closed or ambiguous windows are reported and skipped. Windows stranded by a disconnected monitor are moved back into view. Use **Open and load** to launch missing applications and restore the selected layout.
- **Screensavers:** choose folders separately for each connected screen, include subfolders, shuffle, fit/fill and photo timing. Supports JPG/JPEG, PNG, WebP, BMP, GIF and silent MP4/M4V/WebM playback using installed Windows codecs. Unassigned screens stay black. Videos run to completion. Right arrow or Space advances all screens; mouse movement or Escape exits.
- **Hotkeys:** shortcuts for specific profiles, next monitor profile, next window layout, next screensaver image/video, start screensaver, start folder screensaver, sleep displays, and opening the dashboard. Conflicting shortcuts produce an error without intentionally discarding existing bindings. F12 is reserved by Windows.

The tray’s Monitor Configuration submenu lists saved monitor profiles. Window Positions lists window profiles for dock selection. The tray and native dock have an **Open and load window profile: [selected name]** action. The dock chevron opens profile choices; the remaining menu includes Screensavers, Hotkeys, Taskbars and Quit. Closing the dashboard destroys its webview; the native tray and registered hotkeys remain. Reopen using the tray or the program shortcut. Automatic startup is opt-in.

## Save, load and launch workspaces

The Displays page shows the profile matching the currently detected monitor layout (or Custom / unsaved), a saved-profile selector, **Load profile**, and **Save monitor profile**. Load previews settings; **Test display changes** applies them with rollback. Saving over an existing profile requires an explicit overwrite confirmation. Saving monitor settings preserves that profile’s window slots and launch commands.

In Window Positions, capture open windows or add an executable. Set each app’s quantity and edit each window’s X/Y, width/height, and normal/maximized/minimized state. The scaled layout preview shows the saved rectangles. Quantities correspond to window slots; duplicated slots initially share geometry, which you can edit. Load a profile in the editor to revise it, then save (with confirmation for overwrites).

**Use in dock** selects the master button’s profile. **Open and load** reuses matching open windows, launches missing slots, waits up to 15 seconds per new window, and restores positions. The entire launch phase is capped at 120 seconds; repeated clicks while a launch is active are rejected. A final placement check reports apps that enforce minimum sizes, refuse movement, or do not create another window. It never closes extra application windows. Same-title windows use stable handle ordering when no unique title match exists; this does not restore document contents or browser tabs. **Restore open windows only** retains conservative matching and skips ambiguous windows.

Per-app arguments are JSON arrays, passed directly to the executable without shell parsing. Single-instance apps often need a new-window argument; launching an executable cannot force an app to support multiple windows. Under Advanced startup script / command, supply an interpreter executable, its argument array (including a local script path), and optional working directory. This optional command runs once and must finish successfully before app slots launch. Only scripts you intentionally configure should be used; imported commands never run merely from importing or saving settings. Saved executable paths, arguments and scripts are private configuration, excluded from Git.

Window loading does not change display modes. Apply the monitor profile separately when needed. Placement uses Windows window-placement coordinates and app constraints; elevated apps may require matching privileges.

## Per-monitor taskbars

Enable bars under **Taskbars**, independently choose left/right/top/bottom and auto-hide, then **Save & apply taskbars**. Pin running applications or browse to an `.exe`; pins are stored separately for each screen. Running windows are filtered to their own monitor and grouped by executable. Hover a group to choose a window by title; click a single-window group to activate or minimize it. Hover menus are title lists, not live thumbnail previews. Full-screen foreground applications suppress bars on the covered monitor.

These are custom DisplayProLite bars; Windows native taskbars remain managed by Windows. The dashboard links to Windows taskbar settings so you can disable native secondary bars or change native auto-hide. Only one auto-hide appbar can own a particular monitor edge; conflicts are reported. Bars start disabled until you configure them. Executable pins are supported; packaged-app identity/shortcut launchers are not yet implemented.

## First use

1. Open DisplayProLite. Refresh the detected monitor setup.
2. In **Profiles & windows**, save a baseline profile before experimenting. Capture and select any windows you want to restore.
3. Adjust monitor settings under **Displays**. Use **Test display changes**; select **Keep changes** while the countdown is visible.
4. Under **Screensaver**, browse to each screen's media folder, check its contents, save, then preview. Preview never locks the workstation. Use **Use as Windows screensaver** to register the local `.scr` copy for Windows' normal idle timeout. This is optional; preview and start-now work without registration.
5. Assign your shortcuts under **Hotkeys & settings**. Choose one modifier plus a letter, digit or F1–F11. The app must be running in the tray for global shortcuts to work. Folder screensavers also have built-in Right/Space controls.

The Windows screensaver timeout and require-sign-in setting remain visible below the per-screen folders. The standard Windows screensaver dialog can select other `.scr` files. Embedded miniature preview in that Windows dialog is not implemented; use the dashboard's full-screen Preview button instead.

## Resource design

- No Explorer DLL injection, global keyboard hooks, continuous window polling, or idle-input polling.
- Optional taskbars use native GDI appbar windows in a separate small process. They subscribe to out-of-process Windows accessibility events; updates are coalesced during window movement. No webview is used for the taskbars. They exit when the parent utility exits.
- Global shortcuts use `RegisterHotKey` and a blocking Windows message loop.
- The dashboard webview exists only while the control window is open.
- Per-monitor media windows run in a separate process only during the screensaver. Media decoding consumes additional memory/GPU/CPU while active, especially video and very large photos.
- The rollback helper runs only during a pending display change.
- Media folders are scanned once per screensaver run, not continuously. Folder scans avoid symlinks, limit recursion depth to 20, and reject libraries over 20,000 media files.

See `verification/` for measured checks and resource results from this computer. Idle measurements are short samples, not a guarantee of zero CPU under every workload.

## Settings and backup

Use **Hotkeys & settings → Export settings / Import settings** to transfer profiles, shortcuts, per-screen media assignments, taskbar layouts and pins. Exported files end in `.displayprolite-settings.json` and are ignored by Git, including at nested paths. Store imported files in the ignored `settings-imports/` directory. Imports validate their format, back up previous local configuration, register shortcuts with conflict checks, and stop existing custom taskbars so you can review/apply imported settings. Importing does not switch monitor modes or alter Windows' screensaver/security settings. Environment variables and private keys are never included in exports.

The tracked-file audit also rejects a settings export renamed to another JSON filename. Keep backups and imports private: they contain window titles and local folder/application paths.

- `%APPDATA%\DisplayProLite\profiles.json`: profiles and hotkeys.
- `%APPDATA%\DisplayProLite\photos.json`: media folders and screen assignments.
- `%APPDATA%\DisplayProLite\taskbars.json`: per-screen taskbars and pins.
- `%APPDATA%\DisplayProLite\pending.json`: temporary display rollback snapshot; do not delete during an active change.
- `%LOCALAPPDATA%\DisplayProLite\DisplayProLite.scr`: optional installed screensaver copy. After rebuilding/updating, press **Use as Windows screensaver** to update this copy.

Back up the three configuration JSON files along with this source project. They contain local application/window titles and folder paths; they stay on this computer. `scripts/Backup-Settings.ps1` copies the configuration files into `settings-backup/` in this project on demand. Exclude `node_modules/`, `dist/` and `src-tauri/target/` from backup if desired; they are reproducible build output. Keep `package-lock.json` and `src-tauri/Cargo.lock`.

## Build

Requires Node.js, Rust MSVC, Microsoft C++ Build Tools, and WebView2 (already present on this PC).

```powershell
npm ci
npm run tauri:dev
npm run tauri:build
```

Release executable: `src-tauri/target/release/displayprolite.exe`. NSIS installer: `src-tauri/target/release/bundle/nsis/`.

```powershell
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run test:dashboard
python scripts/audit-public-files.py
```

The release executable also accepts `--self-test-workspace <output.json>` to launch two temporary test windows, verify exact placement, and check that repeating Open and load does not create duplicates.

The release executable accepts `--diagnose <output.json>` for a read-only inventory and `--self-test <output.json>` for native checks. Self-test creates, moves and restores only its own temporary window, tests current display modes with `CDS_TEST` (does not apply them), verifies hotkey conflicts and checks JSON persistence.

## Practical boundaries

Run at the same privilege level as applications whose windows you want to move; elevated/protected apps may reject a normal user's placement requests. Monitor profiles currently use Windows' extended-desktop mode API; HDR, variable-refresh settings, display mirroring, rotation editing and per-monitor DPI scaling are not controls in this version. Saved profiles retain the current orientation. Changing monitor connections may require recapturing profiles and folder assignments. Network folders must be accessible at playback time.

This is a focused local utility, not a replacement for every DisplayFusion feature. Keep DisplayFusion closed while validating the replacement to avoid competing window/taskbar integrations.

## Environment configuration and public repository policy

No API keys are required. Each Windows account uses its own `%APPDATA%\DisplayProLite` settings by default. To isolate another instance, set the backend environment variable `DISPLAYPROLITE_DATA_DIR` to an absolute folder path. A portable executable loads `.env.local` beside itself; debug builds also load the project-local file. Existing OS environment variables take precedence. The reader accepts simple `KEY=value` entries and single/double-quoted values; it does not interpolate variables.

Copy `.env.example` to `.env.local` if you need private overrides. Never commit `.env.local`, credentials, imported/exported settings or diagnostics. Future integrations must read secrets in Rust, not from `VITE_*` frontend variables. The optional installed `.scr` stores only its settings-directory pointer in its private local application folder so it can find the same instance settings.

`AGENTS.md` defines the public-repository rules. `.gitignore` excludes instance settings, environment files, signing material, diagnostics, test screenshots, dependencies and builds. Run the public-file audit and manually inspect staged changes before pushing; pattern scanning alone cannot prove the absence of secrets.

## Validation status for 0.2.0

Native smoke checks cover mode validation without applying changes, exact restore of a dedicated temporary window, hotkey registration/conflicts, and profile persistence. The workspace test launches two synthetic app windows, verifies both exact placements, then reloads the profile and checks that no duplicate windows launch. Rust tests cover layout validation, window matching, taskbar monitor filtering/pinning, media filtering, local environment parsing and settings bundle validation. The headless Edge dashboard test uses synthetic Windows responses and does not exercise actual native controls.

Interactive native dock menu/hover/auto-hide checks, media playback across actual screens, and end-to-end display rollback still require interactive acceptance testing. This preview should not be described as fully production-validated. Local verification output is intentionally excluded from this public repository.
