# DisplayProLite contributor instructions

This is a **public GitHub repository**. Treat every tracked file, commit, branch,
tag, issue, pull request, and release artifact as publicly readable.

## Private data and credentials

- Never commit API keys, passwords, access/refresh tokens, private keys,
  certificates with private material, signing passwords, connection strings,
  account credentials, or personal application configuration.
- Never read credential stores merely to discover or copy secrets. Use the
  computer's existing Git credential helper for authorized GitHub operations;
  do not print, export, place in a remote URL, or commit its credentials.
- `.env.local`, `.env`, and all instance environment files are private and must
  remain ignored. Only `.env.example`, with comments and nonsecret placeholders,
  belongs in Git. OS environment variables take precedence over local files.
- Secrets must remain in the Rust backend. Never put secrets into `VITE_*`
  variables, frontend source, generated JavaScript, screenshots or build output.
- Never commit actual profiles, window titles, application inventories, photo
  folder paths, pinned-app data, settings backups, imported/exported settings,
  personal diagnostics, or screenshots of a real user's desktop.
- Use `settings-imports/`, `settings-exports/`, `settings-backup/`, or `instance/`
  for private data under this project. Settings exports use the suffix
  `.displayprolite-settings.json`; keep that pattern ignored at every depth.
- Do not force-add ignored files. Do not weaken `.gitignore` to make a publish
  succeed. Keep it maintained when new private/generated file types are added.
- Before every public push, inspect `git status` and the staged diff, then run
  `python scripts/audit-public-files.py`. The scanner is an additional check,
  not a guarantee; manually review newly added files and sample data too.
- If a real secret was committed, stop publishing, notify the owner without
  repeating the secret, and arrange revocation/rotation. Do not assume deleting
  it in a later commit removes it from history.

## Build and maintenance

- Keep version information consistent in `package.json`, Cargo package metadata,
  `tauri.conf.json`, the dashboard and README/changelog. Update lockfiles with
  intentional dependency changes.
- Keep the README accurate about the use case, architecture, setup, privacy,
  resource behavior, feature boundaries, and validation actually completed.
- Run TypeScript/Vite build, relevant Rust tests, and the isolated dashboard
  tests when changing these surfaces. Do not claim native interaction was tested
  when only mocks or unit tests were used.
- Preserve timed display rollback and independently test its recovery behavior.
  Do not change a user's display modes as an incidental test.
- Performance is a product requirement: use blocking native event loops, unload
  the dashboard webview when closed, and bound active-task work. Taskbars must
  remain native windows; no per-taskbar webview engines or injected Explorer DLLs.
- Test data must be synthetic and portable. Build output, dependencies and
  machine-specific diagnostics stay out of Git.

## Local private configuration

The default settings location is `%APPDATA%\DisplayProLite`, which naturally
belongs to each Windows user. `DISPLAYPROLITE_DATA_DIR` can select a different
absolute directory for an instance. A portable build reads `.env.local` beside
its executable; development also supports the project-local `.env.local`.
Do not copy one person's environment or settings into public defaults.
