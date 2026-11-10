import { invoke } from "@tauri-apps/api/core";
type Pin = { path: string; label: string };
type Bar = {
  identity: string;
  device: string;
  enabled: boolean;
  edge: string;
  autohide: boolean;
  size: number;
  pins: Pin[];
};
export type TaskbarConfig = { bars: Bar[] };
type Display = {
  identity: string;
  device: string;
  label: string;
  enabled: boolean;
  primary: boolean;
};
let config: TaskbarConfig = { bars: [] };
let monitors: Display[] = [];
let apps: { exe: string; title: string }[] = [];
const esc = (s: unknown) =>
  String(s ?? "").replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ]!,
  );
export function setTaskbars(c: TaskbarConfig, m: Display[]) {
  config = structuredClone(c);
  monitors = m.filter((x) => x.enabled);
  for (const screen of monitors) {
    if (
      !config.bars.some(
        (b) => b.identity === screen.identity && b.device === screen.device,
      )
    )
      config.bars.push({
        identity: screen.identity,
        device: screen.device,
        enabled: false,
        edge: "bottom",
        autohide: false,
        size: 48,
        pins: [],
      });
  }
}
function bar(i: number) {
  return config.bars.find(
    (b) =>
      b.identity === monitors[i].identity && b.device === monitors[i].device,
  )!;
}
export function taskbarSettings() {
  return `<div class="heading"><div><p class="eyebrow">A PLACE FOR EVERY APP</p><h1>Taskbars, per screen.</h1><p>Only this monitor’s running windows, plus the apps you pin here.</p></div><button data-action="bars-refresh-apps">Refresh running apps</button></div><section class="card"><h2>Independent native taskbars</h2><p class="help">Choose an edge and auto-hide separately for each display. Windows from the same app are grouped; hover to choose a window. Pinning keeps an app visible even when it has no windows on that screen.</p><p class="help">These are DisplayProLite taskbars. Windows’ own taskbar remains separately managed. Use Windows settings to disable its secondary taskbars or enable auto-hide where desired.</p><button data-action="bars-windows-settings">Windows taskbar settings ↗</button></section>${monitors
    .map((m, i) => {
      const b = bar(i);
      return `<section class="card"><div class="section-head"><div><h2>${i + 1}. ${esc(m.label)}${m.primary ? " · Primary" : ""}</h2><small class="muted">${esc(m.device)}</small></div><label class="check"><input type="checkbox" data-bar="${i}" data-bar-field="enabled" ${b.enabled ? "checked" : ""}> Enable taskbar</label></div><div class="row"><label>Position<select data-bar="${i}" data-bar-field="edge">${["bottom", "top", "left", "right"].map((v) => `<option value="${v}" ${b.edge === v ? "selected" : ""}>${v[0].toUpperCase() + v.slice(1)}</option>`).join("")}</select></label><label>Size (pixels)<select data-bar="${i}" data-bar-field="size">${[32, 40, 48, 56, 64, 80].map((v) => `<option value="${v}" ${b.size === v ? "selected" : ""}>${v} px</option>`).join("")}</select></label><label class="check"><input type="checkbox" data-bar="${i}" data-bar-field="autohide" ${b.autohide ? "checked" : ""}> Auto-hide</label></div><h2 class="subheading">Pinned on this screen</h2><div class="pin-list">${b.pins.map((p, n) => `<div class="pin-row"><div class="grow"><strong>${esc(p.label)}</strong><small>${esc(p.path)}</small></div><button data-action="bars-pin-up" data-id="${i}" data-pin="${n}" ${n === 0 ? "disabled" : ""} aria-label="Move pin up">↑</button><button data-action="bars-unpin" data-id="${i}" data-pin="${n}">Unpin</button></div>`).join("") || '<p class="help">No pinned apps yet. Running apps still appear automatically.</p>'}</div><div class="row"><label class="grow">Pin a running app<select id="pin-app-${i}"><option value="">Select an app…</option>${apps.map((a) => `<option value="${esc(a.exe)}">${esc(a.exe.split("\\").pop())}</option>`).join("")}</select></label><button data-action="bars-pin-running" data-id="${i}">Pin app</button><button data-action="bars-browse" data-id="${i}">Browse .exe…</button></div></section>`;
    })
    .join(
      "",
    )}<div class="action-bar"><p>Taskbars update on window events. No continuous window polling.</p><button class="accent" data-action="bars-save">Save & apply taskbars</button></div>`;
}
document.addEventListener("change", (event) => {
  const e = event.target as HTMLInputElement;
  if (e.dataset.bar === undefined) return;
  const b = bar(Number(e.dataset.bar));
  switch (e.dataset.barField) {
    case "enabled":
      b.enabled = e.checked;
      break;
    case "autohide":
      b.autohide = e.checked;
      break;
    case "edge":
      b.edge = e.value;
      break;
    case "size":
      b.size = Number(e.value);
      break;
  }
});
function pin(i: number, path: string) {
  const b = bar(i);
  if (!b.pins.some((p) => p.path.toLowerCase() === path.toLowerCase()))
    b.pins.push({
      path,
      label: (path.split("\\").pop() ?? path).replace(/\.exe$/i, ""),
    });
}
export async function taskbarAction(
  action: string,
  target: HTMLElement,
  status: (s: string, error?: boolean) => void,
  render: () => void,
) {
  const i = Number(target.dataset.id);
  switch (action) {
    case "bars-refresh-apps": {
      const windows =
        await invoke<{ exe: string; title: string }[]>("capture_windows");
      apps = [
        ...new Map(windows.map((w) => [w.exe.toLowerCase(), w])).values(),
      ].sort((a, b) => a.exe.localeCompare(b.exe));
      render();
      status(`Loaded ${apps.length} running applications.`);
      break;
    }
    case "bars-windows-settings":
      await invoke("open_windows_taskbar_settings");
      break;
    case "bars-pin-running": {
      const path = document.querySelector<HTMLSelectElement>(
        `#pin-app-${i}`,
      )!.value;
      if (!path) {
        status("Refresh running apps, then choose an app to pin.", true);
        return;
      }
      pin(i, path);
      render();
      break;
    }
    case "bars-browse": {
      const path = await invoke<string | null>("pick_taskbar_app");
      if (path) {
        pin(i, path);
        render();
      }
      break;
    }
    case "bars-unpin":
      bar(i).pins.splice(Number(target.dataset.pin), 1);
      render();
      break;
    case "bars-pin-up": {
      const pins = bar(i).pins;
      const n = Number(target.dataset.pin);
      if (n > 0) [pins[n - 1], pins[n]] = [pins[n], pins[n - 1]];
      render();
      break;
    }
    case "bars-save":
      await invoke("save_taskbars", { config });
      status("Taskbar configuration saved and applied.");
      break;
  }
}
