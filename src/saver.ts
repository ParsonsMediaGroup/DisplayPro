import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
type Payload = {
  files: string[];
  fit: string;
  shuffle: boolean;
  seconds: number;
  error: string | null;
};
document.body.innerHTML =
  '<div id="media-stage"></div><div id="media-error"></div>';
const style = document.createElement("style");
style.textContent =
  "html,body{width:100%;height:100%;overflow:hidden;margin:0;background:black;cursor:none}#media-stage{position:fixed;inset:0;display:grid;place-items:center}#media-stage img,#media-stage video{width:100%;height:100%;position:absolute}#media-error{position:fixed;bottom:40px;left:40px;right:40px;color:#aaa;font:16px Segoe UI;text-align:center}";
document.head.append(style);
const stage = document.querySelector<HTMLDivElement>("#media-stage")!;
const error = document.querySelector<HTMLDivElement>("#media-error")!;
let timer: number | undefined;
let generation = 0;
let files: string[] = [];
let index = 0;
let failures = 0;
let payload: Payload;
let quitting = false;
const started = performance.now();
let mouse: { x: number; y: number } | null = null;
function quit() {
  if (quitting) return;
  quitting = true;
  void invoke("stop_photos");
}
function shuffle(items: string[]) {
  for (let i = items.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [items[i], items[j]] = [items[j], items[i]];
  }
}
async function next() {
  if (!files.length || quitting) return;
  if (timer) clearTimeout(timer);
  const token = ++generation;
  const path = files[index++ % files.length];
  if (index % files.length === 0 && payload.shuffle) shuffle(files);
  const video = /\.(mp4|m4v|webm)$/i.test(path);
  const media = document.createElement(video ? "video" : "img");
  media.style.objectFit = payload.fit;
  media.draggable = false;
  const fail = () => {
    if (token !== generation) return;
    failures++;
    if (failures >= files.length) {
      error.textContent =
        "Unable to display media from this folder. Check the files and installed video codecs.";
      return;
    }
    timer = window.setTimeout(() => void next(), 300);
  };
  media.onerror = fail;
  if (media instanceof HTMLVideoElement) {
    media.muted = true;
    media.autoplay = true;
    media.playsInline = true;
    media.preload = "metadata";
    media.onended = () => void next();
    media.onloadeddata = () => {
      if (token !== generation) {
        media.pause();
        media.removeAttribute("src");
        return;
      }
      failures = 0;
      error.textContent = "";
      stage.replaceChildren(media);
      void media.play().catch(fail);
    };
  } else {
    media.onload = () => {
      if (token !== generation) return;
      failures = 0;
      error.textContent = "";
      stage.replaceChildren(media);
      timer = window.setTimeout(() => void next(), payload.seconds * 1000);
    };
  }
  const old = stage.querySelector("video");
  if (old) {
    old.pause();
    old.removeAttribute("src");
    old.load();
  }
  media.src = convertFileSrc(path);
}
document.addEventListener("keydown", (e) => {
  if (
    ["Control", "Alt", "Shift", "Meta"].includes(e.key) ||
    e.ctrlKey ||
    e.altKey ||
    e.metaKey
  )
    return;
  e.preventDefault();
  if (e.key === "ArrowRight" || e.key === " ") {
    void invoke("advance_photos");
    return;
  }
  quit();
});
document.addEventListener("pointerdown", () => {
  if (performance.now() - started > 1000) quit();
});
document.addEventListener("wheel", () => quit());
document.addEventListener("pointermove", (e) => {
  if (performance.now() - started < 1200) {
    mouse = { x: e.screenX, y: e.screenY };
    return;
  }
  if (!mouse) {
    mouse = { x: e.screenX, y: e.screenY };
    return;
  }
  if (Math.abs(e.screenX - mouse.x) + Math.abs(e.screenY - mouse.y) > 12)
    quit();
});
try {
  payload = await invoke<Payload>("photo_payload");
  files = [...payload.files];
  if (payload.shuffle) shuffle(files);
  if (payload.error) error.textContent = payload.error;
  await listen("next-media", () => void next());
  void next();
} catch (e) {
  error.textContent = String(e);
}
