import { invoke } from "@tauri-apps/api/core";
export type Assignment = {
  identity: string;
  device: string;
  folder: string;
  fit: string;
  recursive: boolean;
  shuffle: boolean;
};
export type PhotoConfig = { seconds: number; screens: Assignment[] };
type Display = {
  identity: string;
  device: string;
  label: string;
  enabled: boolean;
};
let config: PhotoConfig = { seconds: 30, screens: [] };
let screens: Display[] = [];
const e = (s: unknown) =>
  String(s ?? "").replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ]!,
  );
export function setPhotos(value: PhotoConfig, monitors: Display[]) {
  config = structuredClone(value);
  screens = monitors.filter((m) => m.enabled);
  for (const m of screens) {
    if (
      !config.screens.some(
        (s) => s.identity === m.identity && s.device === m.device,
      )
    )
      config.screens.push({
        identity: m.identity,
        device: m.device,
        folder: "",
        fit: "contain",
        recursive: true,
        shuffle: true,
      });
  }
}
function assignment(m: Display) {
  return config.screens.find(
    (s) => s.identity === m.identity && s.device === m.device,
  )!;
}
export function photoSettings() {
  return `<section class="card"><div class="section-head"><div><h2>Folders for each screen</h2><p class="help">Photos and muted videos. Each monitor gets its own folder.</p></div><span class="badge">Only runs while active</span></div>${screens
    .map((m, i) => {
      const s = assignment(m);
      return `<div class="photo-assignment" data-photo-row="${i}"><div class="section-head"><h2>${i + 1}. ${e(m.label)}</h2><small class="muted">${e(m.device)}</small></div><div class="row"><label class="grow">Folder<input data-photo="${i}" data-photo-field="folder" placeholder="No folder — show black" value="${e(s.folder)}"></label><button data-action="photo-browse" data-id="${i}">Browse…</button></div><div class="row"><label>Image fit<select data-photo="${i}" data-photo-field="fit"><option value="contain" ${s.fit === "contain" ? "selected" : ""}>Fit · show entire image</option><option value="cover" ${s.fit === "cover" ? "selected" : ""}>Fill · crop edges</option></select></label><label class="check"><input type="checkbox" data-photo="${i}" data-photo-field="recursive" ${s.recursive ? "checked" : ""}> Include subfolders</label><label class="check"><input type="checkbox" data-photo="${i}" data-photo-field="shuffle" ${s.shuffle ? "checked" : ""}> Shuffle</label><button data-action="photo-check" data-id="${i}">Check folder</button></div></div>`;
    })
    .join(
      "",
    )}<div class="row"><label>Photo duration (seconds)<input id="photo-seconds" type="number" min="5" max="3600" value="${config.seconds}"></label><p class="help grow">JPG, PNG, WebP, BMP, GIF, MP4, M4V and WebM. Videos play to the end, then advance. Windows’ installed codecs determine video support.</p></div><div class="toolbar"><button data-action="photo-save">Save folders</button><button data-action="photo-preview">Preview on all screens</button><button class="accent" data-action="photo-install">Use as Windows screensaver</button></div><p class="help">Preview exits on mouse movement or Escape. Right arrow / Space advances all screens. Automatic start uses the Windows timeout below; DisplayProLite does not poll for inactivity.</p></section>`;
}
document.addEventListener("input", (event) => {
  const t = event.target as HTMLInputElement;
  if (t.id === "photo-seconds") config.seconds = Number(t.value);
  if (t.dataset.photo !== undefined && t.dataset.photoField === "folder")
    assignment(screens[Number(t.dataset.photo)]).folder = t.value;
});
document.addEventListener("change", (event) => {
  const t = event.target as HTMLInputElement;
  if (t.dataset.photo === undefined) return;
  const s = assignment(screens[Number(t.dataset.photo)]);
  if (t.dataset.photoField === "fit") s.fit = t.value;
  if (t.dataset.photoField === "recursive") s.recursive = t.checked;
  if (t.dataset.photoField === "shuffle") s.shuffle = t.checked;
});
export async function photoAction(
  action: string,
  id: string | undefined,
  status: (s: string, error?: boolean) => void,
) {
  switch (action) {
    case "photo-browse": {
      const folder = await invoke<string | null>("pick_photo_folder");
      if (folder) {
        assignment(screens[Number(id)]).folder = folder;
        const input = document.querySelector<HTMLInputElement>(
          `input[data-photo="${id}"][data-photo-field="folder"]`,
        );
        if (input) input.value = folder;
      }
      break;
    }
    case "photo-check": {
      const s = assignment(screens[Number(id)]);
      if (!s.folder) {
        status("Choose a folder first.", true);
        break;
      }
      const n = await invoke<number>("validate_photo_folder", {
        folder: s.folder,
        recursive: s.recursive,
      });
      status(`${n} supported photos/videos found.`, n === 0);
      break;
    }
    case "photo-save":
      await invoke("save_photo_config", { config });
      status("Per-screen folders saved.");
      break;
    case "photo-preview":
      await invoke("save_photo_config", { config });
      await invoke("start_photo_saver", { preview: true });
      status("Preview started. Move the mouse or press Escape to exit.");
      break;
    case "photo-install":
      await invoke("save_photo_config", { config });
      await invoke("install_photo_saver");
      status(
        "DisplayProLite is now your Windows screensaver. Use Refresh below to load its current settings.",
      );
      break;
  }
}
