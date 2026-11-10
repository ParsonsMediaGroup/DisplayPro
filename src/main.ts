import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./style.css";
import {
  setTaskbars,
  taskbarSettings,
  taskbarAction,
  type TaskbarConfig,
} from "./taskbar-settings";
import {
  photoSettings,
  photoAction,
  setPhotos,
  type PhotoConfig,
} from "./photo-settings";

type Mode = { width: number; height: number; hz: number };
type Monitor = Mode & {
  device: string;
  identity: string;
  label: string;
  primary: boolean;
  enabled: boolean;
  x: number;
  y: number;
  orientation: number;
  bits: number;
  modes: Mode[];
};
type SavedWindow = {
  handle: number;
  pid: number;
  exe: string;
  class: string;
  title: string;
  rect: number[];
  maximized: boolean;
  minimized: boolean;
};
type Profile = {
  id: string;
  name: string;
  monitors: Monitor[];
  windows: SavedWindow[];
};
type Binding = { shortcut: string; action: string };
type Saver = {
  enabled: boolean;
  timeout: number;
  secure: boolean;
  path: string;
  available: string[];
};
type Pending = { id: string; deadline: number };
type Snapshot = {
  taskbars: TaskbarConfig;
  photos: PhotoConfig;
  requestedPage: string | null;
  config: { profiles: Profile[]; hotkeys: Binding[] };
  monitors: Monitor[];
  screensaver: Saver;
  pending: Pending | null;
  dataDir: string;
  startupError: string | null;
  autostart: boolean;
};
let data: Snapshot;
let draft: Monitor[] = [];
let windows: SavedWindow[] = [];
let chosen = new Set<number>();
let bindings: Binding[] = [];
let page = "displays";
let editing: string | null = null;
let profileName = "";
let includeDisplays = true;
let pending: Pending | null = null;
let countdown: number | undefined;
let busy = false;
const root = document.querySelector<HTMLDivElement>("#app")!;
const esc = (s: unknown) =>
  String(s ?? "").replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ]!,
  );
const clone = <T>(v: T): T => structuredClone(v);
const short = (path: string) => path.split("\\").pop() ?? path;
function status(message: string, error = false) {
  const el = document.querySelector<HTMLElement>("#status");
  if (el) {
    el.textContent = message;
    el.classList.toggle("error", error);
  }
}
async function task(fn: () => Promise<void>) {
  if (busy) return;
  busy = true;
  root.classList.add("busy");
  try {
    await fn();
  } catch (e) {
    status(String(e), true);
  } finally {
    busy = false;
    root.classList.remove("busy");
  }
}
async function refresh(reset = true) {
  data = await invoke<Snapshot>("get_state");
  setPhotos(data.photos, data.monitors);
  setTaskbars(data.taskbars, data.monitors);
  if (data.requestedPage) page = data.requestedPage;
  if (reset) {
    draft = clone(data.monitors);
    bindings = clone(data.config.hotkeys);
  }
  pending = data.pending;
  render();
  if (pending) showPending(pending);
  if (data.startupError) status(data.startupError, true);
}
function monitorDiagram() {
  const active = draft.filter((m) => m.enabled);
  if (!active.length)
    return '<div class="empty">Enable a display to preview this layout.</div>';
  const minX = Math.min(...active.map((m) => m.x));
  const minY = Math.min(...active.map((m) => m.y));
  const w = Math.max(...active.map((m) => m.x + m.width)) - minX;
  const h = Math.max(...active.map((m) => m.y + m.height)) - minY;
  const scale = Math.min(680 / Math.max(w, 1), 190 / Math.max(h, 1));
  return `<div class="diagram" style="height:${Math.max(180, h * scale + 36)}px"><div class="canvas" style="width:${w * scale}px;height:${h * scale}px">${draft.map((m, i) => (m.enabled ? `<div class="monitor ${m.primary ? "primary" : ""}" style="left:${(m.x - minX) * scale}px;top:${(m.y - minY) * scale}px;width:${m.width * scale}px;height:${m.height * scale}px"><b>${i + 1}</b><span>${m.width} × ${m.height}</span><small>${m.hz} Hz${m.primary ? " · Primary" : ""}</small></div>` : "")).join("")}</div></div>`;
}
function modeOptions(m: Monitor) {
  const modes = [...m.modes];
  if (
    !modes.some(
      (x) => x.width === m.width && x.height === m.height && x.hz === m.hz,
    )
  )
    modes.unshift(m);
  return modes
    .map(
      (v) =>
        `<option value="${v.width},${v.height},${v.hz}" ${v.width === m.width && v.height === m.height && v.hz === m.hz ? "selected" : ""}>${v.width} × ${v.height} · ${v.hz} Hz</option>`,
    )
    .join("");
}
function displays() {
  return `<div class="heading"><div><p class="eyebrow">YOUR WORKSPACE</p><h1>Displays, in their place.</h1><p>Set resolution, refresh rate and arrangement. Save the setup you like.</p></div><button data-action="refresh">↻ Refresh</button></div><section class="card topology"><div class="section-head"><h2>Monitor arrangement</h2><span class="badge">${draft.filter((m) => m.enabled).length} active displays</span></div>${monitorDiagram()}<div class="toolbar"><span>Position in desktop pixels. Negative values are supported.</span><button data-action="arrange">Arrange side by side</button></div></section><div class="display-grid">${draft.map((m, i) => `<section class="card display-card"><div class="section-head"><div class="display-title"><span class="number">${i + 1}</span><div><h2>${esc(m.label)}</h2><small>${esc(m.device)}</small></div></div><label class="check"><input type="checkbox" data-monitor="${i}" data-field="enabled" ${m.enabled ? "checked" : ""}> Enabled</label></div><label>Resolution & refresh rate<select data-monitor="${i}" data-field="mode" ${!m.enabled ? "disabled" : ""}>${modeOptions(m)}</select></label><div class="row"><label>X position<input type="number" step="1" min="-100000" max="100000" data-monitor="${i}" data-field="x" value="${m.x}"></label><label>Y position<input type="number" step="1" min="-100000" max="100000" data-monitor="${i}" data-field="y" value="${m.y}"></label></div><label class="check"><input type="radio" name="primary" data-monitor="${i}" data-field="primary" ${m.primary ? "checked" : ""} ${!m.enabled ? "disabled" : ""}> Primary display</label></section>`).join("")}</div><div class="action-bar"><p>Changes revert after 25 seconds unless you keep them.</p><button class="accent" data-action="apply">Test display changes</button></div>`;
}
function profiles() {
  return `<div class="heading"><div><p class="eyebrow">READY WHEN YOU ARE</p><h1>Workspace profiles.</h1><p>Save a monitor setup, window positions, or both.</p></div><button data-action="new-profile">+ New profile</button></div><section class="card"><div class="section-head"><h2>${editing ? "Edit profile" : "Create a profile"}</h2><span class="badge">${chosen.size} windows selected</span></div><div class="row"><label class="grow">Profile name<input id="profile-name" maxlength="100" placeholder="Work, gaming, design…" value="${esc(profileName)}"></label><label class="check"><input id="include-displays" type="checkbox" ${includeDisplays ? "checked" : ""}> Include display setup</label></div><p class="help">Display settings come from the Displays tab. Capture windows to save their current positions; only open, identifiable windows can be restored.</p><div class="toolbar"><button data-action="capture">Capture open windows</button>${windows.length ? '<button data-action="select-all">Select all</button><button data-action="select-none">Select none</button>' : ""}</div>${windows.length ? `<div class="window-list">${windows.map((w, i) => `<label class="window-row"><input type="checkbox" data-window="${i}" ${chosen.has(i) ? "checked" : ""}><div><strong>${esc(w.title)}</strong><small>${esc(short(w.exe))} · ${w.rect[2] - w.rect[0]} × ${w.rect[3] - w.rect[1]}${w.maximized ? " · Maximized" : ""}${w.minimized ? " · Minimized" : ""}</small></div></label>`).join("")}</div>` : '<div class="empty">Capture your open windows to include a window layout.</div>'}<div class="toolbar right"><button class="accent" data-action="save-profile">${editing ? "Save profile changes" : "Save new profile"}</button></div></section><h2 class="subheading">Saved profiles <span class="muted">${data.config.profiles.length}</span></h2><div class="profile-grid">${data.config.profiles.map((p) => `<section class="card profile"><div class="section-head"><h2>${esc(p.name)}</h2><button class="text-button" data-action="edit-profile" data-id="${esc(p.id)}">Edit</button></div><p>${p.monitors.length} displays <span>·</span> ${p.windows.length} windows</p><div class="toolbar">${p.monitors.length ? `<button data-action="apply-profile" data-id="${esc(p.id)}">Apply displays</button>` : ""}${p.windows.length ? `<button data-action="restore" data-id="${esc(p.id)}">Restore windows</button>` : ""}<button class="text-button danger" data-action="delete" data-id="${esc(p.id)}">Delete</button></div></section>`).join("") || '<div class="empty">Your saved setups will appear here.</div>'}</div>`;
}
function screensaver() {
  const s = data.screensaver;
  return `<div class="heading"><div><p class="eyebrow">WHEN YOU STEP AWAY</p><h1>Screensaver control.</h1><p>Use Windows’ screensaver system with a few simple controls.</p></div></div>${photoSettings()}<section class="card saver"><div class="section-head"><h2>Windows screensaver</h2><label class="check"><input id="saver-enabled" type="checkbox" ${s.enabled ? "checked" : ""}> Enabled</label></div><label>Screensaver<select id="saver-path"><option value="">None selected</option>${s.available.map((p) => `<option value="${esc(p)}" ${p === s.path ? "selected" : ""}>${esc(short(p))}</option>`).join("")}</select></label><div class="row"><label>Start after (minutes)<input id="saver-minutes" type="number" min="1" max="1440" value="${Math.max(1, Math.round(s.timeout / 60))}"></label><label class="check"><input id="saver-secure" type="checkbox" ${s.secure ? "checked" : ""}> Require sign-in on resume</label></div><p class="help">For photo folders, custom .scr files, and screensaver-specific options, open Windows settings. Refresh here after making changes there.</p><div class="toolbar"><button data-action="saver-settings">Windows screensaver settings ↗</button><button data-action="refresh">Refresh</button></div><div class="toolbar right"><button class="accent" data-action="save-saver">Save screensaver settings</button></div></section><div class="profile-grid"><section class="card"><h2>Start now</h2><p>Launch the selected screensaver immediately.</p><button data-action="start-saver">Start screensaver</button></section><section class="card"><h2>Turn off displays</h2><p>Send displays to sleep. Mouse or keyboard activity wakes them.</p><button data-action="sleep">Sleep displays</button></section></div>`;
}
function actionOptions(selected: string) {
  const options: [string, string][] = [
    ["show", "Open DisplayProLite"],
    ["screensaver", "Start screensaver"],
    ["sleep", "Sleep displays"],
    ["next-media", "Next screensaver image / video"],
    ["photo-saver", "Start per-screen folder screensaver"],
    ["next-display-profile", "Next monitor profile"],
    ["next-window-profile", "Next window layout"],
  ];
  for (const p of data.config.profiles) {
    if (p.monitors.length)
      options.push([`display:${p.id}`, `Displays → ${p.name}`]);
    if (p.windows.length)
      options.push([`windows:${p.id}`, `Windows → ${p.name}`]);
  }
  return options
    .map(
      ([v, l]) =>
        `<option value="${esc(v)}" ${v === selected ? "selected" : ""}>${esc(l)}</option>`,
    )
    .join("");
}
function hotkeys() {
  return `<div class="heading"><div><p class="eyebrow">ONE SHORTCUT AWAY</p><h1>Your controls, on demand.</h1><p>Global shortcuts work while DisplayProLite is in the tray.</p></div><div class="toolbar"><button data-action="common-hotkeys">Add common actions</button><button data-action="add-hotkey">+ Add shortcut</button></div></div><section class="card"><div class="section-head"><h2>Global hotkeys</h2><span class="badge">Registered with Windows</span></div><p class="help">Click a shortcut field and press your combination, or type Ctrl+Alt+1. Use a letter, digit, or F1–F11. Conflicts are reported when you save.</p>${bindings.map((b, i) => `<div class="hotkey-row"><label>Shortcut<input class="hotkey-input" data-binding="${i}" data-kind="shortcut" value="${esc(b.shortcut)}" placeholder="Ctrl+Alt+1"></label><label class="grow">Action<select data-binding="${i}" data-kind="action">${actionOptions(b.action)}</select></label><button data-action="remove-hotkey" data-id="${i}" aria-label="Remove shortcut">×</button></div>`).join("") || '<div class="empty">Add a shortcut for a profile, screensaver, or the app.</div>'}<div class="toolbar right"><button class="accent" data-action="save-hotkeys">Save shortcuts</button></div></section><section class="card"><h2>Startup & background operation</h2><label class="check"><input id="autostart" type="checkbox" ${data.autostart ? "checked" : ""}> Start DisplayProLite in the tray when I sign in</label><p class="help">Closing the window keeps hotkeys available in the tray. Use the tray menu to quit. Monitor and window scans run only when requested.</p><div class="toolbar"><button data-action="export-settings">Export settings</button><button data-action="import-settings">Import settings</button></div><p class="help">Exports contain local profiles, folders, pins and hotkeys. Keep them private. Environment files and keys are never included. Import replaces saved configuration and backs up the previous files; review settings before applying them.</p><div class="data-path"><small>Profiles and settings</small><code>${esc(data.dataDir)}</code></div></section>`;
}
function render() {
  root.innerHTML = `<aside><div class="brand"><span class="brand-icon">▱</span><div>DisplayPro<span>Lite</span><small>YOUR DESKTOP, SIMPLIFIED</small></div></div><nav>${[
    ["displays", "▣", "Displays"],
    ["profiles", "▤", "Profiles & windows"],
    ["screensaver", "☾", "Screensaver"],
    ["hotkeys", "⌘", "Hotkeys & settings"],
    ["taskbars", "\u25a5", "Taskbars"],
  ]
    .map(
      ([id, icon, name]) =>
        `<button class="nav ${page === id ? "selected" : ""}" data-page="${id}"><span>${icon}</span>${name}</button>`,
    )
    .join(
      "",
    )}</nav><div class="sidebar-bottom"><span class="dot"></span> Ready on demand<small>DisplayProLite · v0.1.0</small></div></aside><main>${page === "displays" ? displays() : page === "profiles" ? profiles() : page === "screensaver" ? screensaver() : page === "taskbars" ? taskbarSettings() : hotkeys()}<footer id="status" role="status" aria-live="polite">Ready. Changes happen only when you apply them.</footer></main><dialog id="confirm-display"><div class="dialog-icon">▣</div><h2>Keep this display setup?</h2><p>The previous layout returns automatically in <strong id="seconds">25</strong> seconds.</p><div class="toolbar right"><button data-action="revert">Revert</button><button class="accent" data-action="keep">Keep changes</button></div></dialog><dialog id="delete-dialog"><h2>Delete this profile?</h2><p>Its saved layout and assigned shortcuts will be removed.</p><div class="toolbar right"><button data-action="cancel-delete">Cancel</button><button class="danger" id="confirm-delete" data-action="confirm-delete">Delete profile</button></div></dialog>`;
}
function showPending(p: Pending) {
  pending = p;
  const dialog = document.querySelector<HTMLDialogElement>("#confirm-display")!;
  if (!dialog.open) dialog.showModal();
  dialog.oncancel = (e) => {
    e.preventDefault();
    void task(() => finish(false));
  };
  if (countdown) clearInterval(countdown);
  const tick = () => {
    const remaining = Math.max(0, p.deadline - Math.floor(Date.now() / 1000));
    const e = document.querySelector("#seconds");
    if (e) e.textContent = String(remaining);
    const keep = document.querySelector<HTMLButtonElement>(
      '[data-action="keep"]',
    );
    if (keep) keep.disabled = remaining <= 2;
    if (!remaining) {
      clearInterval(countdown);
      countdown = undefined;
      dialog.close();
      pending = null;
      setTimeout(
        () =>
          void task(async () => {
            await refresh();
            status(
              "Display confirmation expired. The original layout was restored; inspect the current displays.",
            );
          }),
        1500,
      );
    }
  };
  tick();
  countdown = window.setInterval(tick, 250);
}
async function finish(keep: boolean) {
  if (!pending) return;
  await invoke("finish_display", { id: pending.id, keep });
  if (countdown) clearInterval(countdown);
  countdown = undefined;
  pending = null;
  await refresh();
  status(
    keep
      ? "Display setup kept. Save a profile to reuse it."
      : "Original display setup restored.",
  );
}
async function apply(monitors: Monitor[]) {
  const id = await invoke<string>("apply_displays", { monitors });
  const next = await invoke<Snapshot>("get_state");
  showPending(
    next.pending ?? { id, deadline: Math.floor(Date.now() / 1000) + 25 },
  );
}
root.addEventListener("click", (event) => {
  const b = (event.target as HTMLElement).closest<HTMLElement>(
    "[data-page],[data-action]",
  );
  if (!b) return;
  if (b.dataset.page) {
    if (pending) return;
    page = b.dataset.page;
    render();
    return;
  }
  void task(async () => {
    const id = b.dataset.id;
    if (b.dataset.action?.startsWith("bars-")) {
      await taskbarAction(b.dataset.action, b, status, render);
      return;
    }
    if (b.dataset.action?.startsWith("photo-")) {
      await photoAction(b.dataset.action, id, status);
      return;
    }
    switch (b.dataset.action) {
      case "export-settings": {
        const path = await invoke<string | null>("export_settings");
        if (path) status("Private settings exported to " + path);
        break;
      }
      case "import-settings": {
        const message = await invoke<string | null>("import_settings");
        if (message) {
          editing = null;
          profileName = "";
          windows = [];
          chosen.clear();
          await refresh();
          status(message);
        }
        break;
      }
      case "refresh":
        await refresh();
        status("Current Windows settings refreshed.");
        break;
      case "arrange": {
        let x = 0;
        for (const m of draft.filter((m) => m.enabled)) {
          m.x = x;
          m.y = 0;
          x += m.width;
        }
        render();
        break;
      }
      case "apply":
        await apply(draft);
        break;
      case "keep":
        await finish(true);
        break;
      case "revert":
        await finish(false);
        break;
      case "capture":
        windows = await invoke<SavedWindow[]>("capture_windows");
        chosen = new Set(windows.map((_, i) => i));
        render();
        status(
          `Captured ${windows.length} windows. Uncheck any you don't want to restore.`,
        );
        break;
      case "select-all":
        chosen = new Set(windows.map((_, i) => i));
        render();
        break;
      case "select-none":
        chosen.clear();
        render();
        break;
      case "new-profile":
        editing = null;
        profileName = "";
        windows = [];
        chosen.clear();
        includeDisplays = true;
        draft = clone(data.monitors);
        render();
        break;
      case "save-profile":
        await invoke("save_profile", {
          id: editing,
          name: profileName,
          monitors: includeDisplays ? draft : [],
          windows: windows.filter((_, i) => chosen.has(i)),
        });
        editing = null;
        profileName = "";
        await refresh(false);
        status("Profile saved locally. Assign a shortcut on the Hotkeys tab.");
        break;
      case "edit-profile": {
        const p = data.config.profiles.find((p) => p.id === id)!;
        editing = p.id;
        profileName = p.name;
        includeDisplays = !!p.monitors.length;
        draft = p.monitors.length
          ? p.monitors.map((m) => ({
              ...clone(m),
              modes:
                data.monitors.find(
                  (c) => c.identity === m.identity && c.device === m.device,
                )?.modes ?? [],
            }))
          : clone(data.monitors);
        windows = clone(p.windows);
        chosen = new Set(windows.map((_, i) => i));
        render();
        status(
          "Profile loaded for editing. Use Displays to adjust its monitor setup.",
        );
        break;
      }
      case "apply-profile":
        await apply(data.config.profiles.find((p) => p.id === id)!.monitors);
        break;
      case "restore": {
        const report = await invoke<{ restored: number; skipped: string[] }>(
          "restore_windows",
          { id },
        );
        status(
          `Restored ${report.restored} windows.${report.skipped.length ? " Skipped: " + report.skipped.join("; ") : ""}`,
          !!report.skipped.length,
        );
        break;
      }
      case "delete":
        document.querySelector<HTMLElement>("#confirm-delete")!.dataset.id = id;
        document
          .querySelector<HTMLDialogElement>("#delete-dialog")!
          .showModal();
        break;
      case "cancel-delete":
        document.querySelector<HTMLDialogElement>("#delete-dialog")!.close();
        break;
      case "confirm-delete":
        await invoke("delete_profile", { id });
        if (editing === id) {
          editing = null;
          profileName = "";
        }
        await refresh();
        status("Profile deleted.");
        break;
      case "saver-settings":
        await invoke("screensaver_settings");
        break;
      case "save-saver": {
        const saver = {
          ...data.screensaver,
          enabled:
            document.querySelector<HTMLInputElement>("#saver-enabled")!.checked,
          path: document.querySelector<HTMLSelectElement>("#saver-path")!.value,
          timeout:
            Number(
              document.querySelector<HTMLInputElement>("#saver-minutes")!.value,
            ) * 60,
          secure:
            document.querySelector<HTMLInputElement>("#saver-secure")!.checked,
        };
        await invoke("save_screensaver", { saver });
        await refresh();
        status("Windows screensaver settings saved.");
        break;
      }
      case "start-saver":
        await invoke("start_screensaver");
        break;
      case "sleep":
        await invoke("sleep_displays");
        break;
      case "common-hotkeys":
        for (const [shortcut, action] of [
          ["Ctrl+Alt+Right", "next-media"],
          ["Ctrl+Alt+P", "next-display-profile"],
          ["Ctrl+Alt+W", "next-window-profile"],
          ["Ctrl+Alt+S", "photo-saver"],
        ]) {
          if (!bindings.some((b) => b.action === action))
            bindings.push({ shortcut, action });
        }
        render();
        status(
          "Suggested shortcuts added. Review them, then Save shortcuts to activate.",
        );
        break;
      case "add-hotkey":
        bindings.push({ shortcut: "", action: "show" });
        render();
        break;
      case "remove-hotkey":
        bindings.splice(Number(id), 1);
        render();
        break;
      case "save-hotkeys":
        await invoke("save_hotkeys", { bindings });
        await refresh(false);
        status("Global shortcuts saved and active.");
        break;
    }
  });
});
root.addEventListener("input", (event) => {
  const e = event.target as HTMLInputElement;
  if (e.id === "profile-name") profileName = e.value;
  if (e.dataset.binding !== undefined && e.dataset.kind === "shortcut")
    bindings[Number(e.dataset.binding)].shortcut = e.value;
});
root.addEventListener("change", (event) => {
  const e = event.target as HTMLInputElement;
  if (e.dataset.monitor !== undefined) {
    const m = draft[Number(e.dataset.monitor)];
    switch (e.dataset.field) {
      case "mode": {
        const [width, height, hz] = e.value.split(",").map(Number);
        Object.assign(m, { width, height, hz });
        break;
      }
      case "enabled":
        m.enabled = e.checked;
        if (m.enabled && (!m.width || !m.height || !m.hz) && m.modes.length)
          Object.assign(m, m.modes[0]);
        if (!m.enabled && m.primary) {
          m.primary = false;
          const next = draft.find((x) => x.enabled);
          if (next) next.primary = true;
        }
        break;
      case "primary":
        for (const c of draft) c.primary = c === m;
        break;
      case "x":
        m.x = Number(e.value);
        break;
      case "y":
        m.y = Number(e.value);
        break;
    }
    render();
  }
  if (e.dataset.window !== undefined) {
    const i = Number(e.dataset.window);
    if (e.checked) chosen.add(i);
    else chosen.delete(i);
  }
  if (e.id === "include-displays") includeDisplays = e.checked;
  if (e.dataset.binding !== undefined && e.dataset.kind === "action")
    bindings[Number(e.dataset.binding)].action = e.value;
  if (e.id === "autostart")
    void task(async () => {
      try {
        await invoke("set_autostart", { enabled: e.checked });
        data.autostart = e.checked;
        status(
          e.checked
            ? "Starts in the tray at sign-in."
            : "Automatic startup disabled.",
        );
      } catch (error) {
        e.checked = !e.checked;
        throw error;
      }
    });
});
root.addEventListener("keydown", (event) => {
  const e = event.target as HTMLInputElement;
  if (!e.classList.contains("hotkey-input")) return;
  const key =
    event.key === " " ? "SPACE" : event.key.toUpperCase().replace("ARROW", "");
  if (!event.ctrlKey && !event.altKey && !event.metaKey) return;
  if (
    !/^[A-Z0-9]$|^F([1-9]|10|11)$|^(LEFT|RIGHT|UP|DOWN|SPACE|HOME|END|PAGEUP|PAGEDOWN)$/.test(
      key,
    )
  )
    return;
  event.preventDefault();
  const value = [
    event.ctrlKey ? "Ctrl" : "",
    event.altKey ? "Alt" : "",
    event.shiftKey ? "Shift" : "",
    event.metaKey ? "Win" : "",
    key,
  ]
    .filter(Boolean)
    .join("+");
  e.value = value;
  bindings[Number(e.dataset.binding)].shortcut = value;
});
async function boot() {
  root.innerHTML = '<div class="loading">Loading DisplayProLite…</div>';
  try {
    await refresh();
    await listen<Pending>("display-pending", (e) => showPending(e.payload));
    await listen<string>("navigate", (e) => {
      page = e.payload;
      render();
      if (pending) showPending(pending);
    });
    await listen<{ ok: boolean; message: string }>("action-result", (e) =>
      status(e.payload.message, !e.payload.ok),
    );
  } catch (e) {
    root.innerHTML = `<div class="loading"><h1>Could not load Windows settings</h1><p>${esc(e)}</p><button id="retry">Retry</button></div>`;
    document
      .querySelector("#retry")
      ?.addEventListener("click", () => void boot());
  }
}
if (new URLSearchParams(location.search).has("saver")) {
  void import("./saver");
} else {
  void boot();
}
