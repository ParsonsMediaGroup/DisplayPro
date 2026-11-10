export type WindowSlot = {
  handle: number;
  pid: number;
  exe: string;
  class: string;
  title: string;
  rect: number[];
  maximized: boolean;
  minimized: boolean;
};
export type LaunchCommand = {
  exe: string;
  args: string[];
  working_dir: string;
};
const esc = (s: unknown) =>
  String(s ?? "").replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ]!,
  );
export function layoutPreview(windows: WindowSlot[], chosen: Set<number>) {
  const selected = windows.filter((_, i) => chosen.has(i));
  if (!selected.length)
    return '<p class="help">Select or add windows to preview their saved layout.</p>';
  const x = Math.min(...selected.map((w) => w.rect[0])),
    y = Math.min(...selected.map((w) => w.rect[1]));
  const width = Math.max(...selected.map((w) => w.rect[2])) - x,
    height = Math.max(...selected.map((w) => w.rect[3])) - y;
  return `<svg class="window-preview" viewBox="${x - 40} ${y - 40} ${width + 80} ${height + 80}" role="img" aria-label="Saved window layout">${selected.map((w, i) => `<g><rect x="${w.rect[0]}" y="${w.rect[1]}" width="${w.rect[2] - w.rect[0]}" height="${w.rect[3] - w.rect[1]}" fill="hsl(${(i * 67) % 360} 45% 25% / .65)" stroke="hsl(${(i * 67) % 360} 60% 75%)" stroke-width="4"/><text x="${w.rect[0] + 16}" y="${w.rect[1] + 42}" fill="white" font-size="32">${i + 1}: ${esc(w.exe.split("\\").pop())}</text></g>`).join("")}</svg>`;
}
export function windowEditor(
  windows: WindowSlot[],
  chosen: Set<number>,
  commands: LaunchCommand[],
  startup: LaunchCommand | null,
) {
  const apps = [...new Set(windows.map((w) => w.exe))];
  return `<h3>Applications and quantities</h3><p class="help">Each selected window is one slot. Open and load reuses open windows and launches missing ones. For browsers or other single-instance apps, supply their new-window argument. No app launches until you click Open and load.</p>
  <div class="toolbar"><button data-action="add-app">+ Add application</button></div>
  ${apps
    .map((exe, i) => {
      const cmd = commands.find(
        (c) => c.exe.toLowerCase() === exe.toLowerCase(),
      );
      return `<section class="launch-app"><strong>${esc(exe.split("\\").pop())}</strong><small>${esc(exe)}</small><div class="row"><label>Quantity<input type="number" min="0" max="30" data-quantity="${esc(exe)}" value="${windows.filter((w, j) => w.exe === exe && chosen.has(j)).length}"></label><label class="grow">Arguments (JSON array)<input data-command="${esc(exe)}" data-command-field="args" value="${esc(JSON.stringify(cmd?.args ?? []))}" placeholder='["--new-window"]'></label></div><label>Working folder (optional)<input data-command="${esc(exe)}" data-command-field="working_dir" value="${esc(cmd?.working_dir ?? "")}"></label></section>`;
    })
    .join("")}
  <h3>Window layout</h3>${layoutPreview(windows, chosen)}
  <div class="window-list">${windows.map((w, i) => `<section class="window-slot"><label class="check"><input type="checkbox" data-window="${i}" ${chosen.has(i) ? "checked" : ""}><strong>${i + 1}. ${esc(w.title)}</strong></label><div class="slot-fields">${["X", "Y", "Width", "Height"].map((label, j) => `<label>${label}<input type="number" step="1" min="${j < 2 ? -100000 : 1}" max="100000" data-slot="${i}" data-geometry="${j}" value="${j < 2 ? w.rect[j] : w.rect[j] - w.rect[j - 2]}"></label>`).join("")}<label>State<select data-slot="${i}" data-state="true"><option value="normal" ${!w.maximized && !w.minimized ? "selected" : ""}>Normal</option><option value="maximized" ${w.maximized ? "selected" : ""}>Maximized</option><option value="minimized" ${w.minimized ? "selected" : ""}>Minimized</option></select></label></div></section>`).join("")}</div>
  <details class="startup-editor"><summary>Advanced startup script / command</summary><p class="help">Optional: runs once before apps open and must finish successfully. Enter an interpreter executable (for example powershell.exe’s full path), then arguments such as ["-NoProfile", "-File", "C:\\\\Scripts\\\\work.ps1"]. The entire operation has a 120-second limit. Commands run as your Windows user. Imported commands run only when you explicitly open a profile.</p><label>Executable<input id="startup-exe" value="${esc(startup?.exe ?? "")}"></label><label>Arguments (JSON array)<input id="startup-args" value="${esc(JSON.stringify(startup?.args ?? []))}"></label><label>Working folder<input id="startup-dir" value="${esc(startup?.working_dir ?? "")}"></label></details>`;
}
export function resizeQuantity(
  windows: WindowSlot[],
  chosen: Set<number>,
  exe: string,
  count: number,
) {
  if (!Number.isInteger(count) || count < 0 || count > 30)
    throw new Error("Use a quantity from 0 to 30.");
  const indices = windows.flatMap((w, i) => (w.exe === exe ? [i] : []));
  const original = windows[indices[0]];
  if (!original) return;
  for (const i of indices) chosen.delete(i);
  for (let n = 0; n < count; n++) {
    if (n < indices.length) chosen.add(indices[n]);
    else {
      windows.push({
        ...structuredClone(original),
        handle: 0,
        pid: 0,
        title: `${exe.split("\\").pop()} · window ${n + 1}`,
      });
      chosen.add(windows.length - 1);
    }
  }
}
