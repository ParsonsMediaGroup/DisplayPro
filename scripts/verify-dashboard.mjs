import { chromium } from "playwright-core";
import { createServer } from "node:http";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import path from "node:path";
import assert from "node:assert/strict";

const root = path.resolve(import.meta.dirname, "..");
const output = path.join(root, "verification");
await mkdir(output, { recursive: true });
const monitors = [
  {
    device: "\\\\.\\DISPLAY1",
    identity: "mock-1",
    label: "Landscape monitor",
    primary: true,
    enabled: true,
    x: 0,
    y: 0,
    width: 2560,
    height: 1440,
    hz: 144,
    orientation: 0,
    bits: 32,
    modes: [
      { width: 2560, height: 1440, hz: 144 },
      { width: 1920, height: 1080, hz: 60 },
    ],
  },
  {
    device: "\\\\.\\DISPLAY2",
    identity: "mock-2",
    label: "Portrait monitor",
    primary: false,
    enabled: true,
    x: 2560,
    y: 0,
    width: 1080,
    height: 1920,
    hz: 60,
    orientation: 1,
    bits: 32,
    modes: [{ width: 1080, height: 1920, hz: 60 }],
  },
];
const fixture = {
  config: { profiles: [], hotkeys: [] },
  monitors,
  screensaver: {
    enabled: true,
    timeout: 600,
    secure: true,
    path: "C:\\Windows\\System32\\scrnsave.scr",
    available: ["C:\\Windows\\System32\\scrnsave.scr"],
  },
  photos: { seconds: 30, screens: [] },
  taskbars: { bars: [] },
  pending: null,
  dataDir: "C:\\Test\\DisplayProLite",
  startupError: null,
  requestedPage: null,
  autostart: false,
};
const server = createServer(async (req, res) => {
  try {
    const pathname = new URL(req.url, "http://localhost").pathname;
    const file = path.resolve(
      root,
      "dist",
      "." + (pathname === "/" ? "/index.html" : pathname),
    );
    if (!file.startsWith(path.join(root, "dist") + path.sep))
      throw Error("Invalid path");
    const body = await readFile(file);
    res.setHeader(
      "Content-Type",
      file.endsWith(".js")
        ? "text/javascript"
        : file.endsWith(".css")
          ? "text/css"
          : "text/html",
    );
    res.end(body);
  } catch {
    res.statusCode = 404;
    res.end("Not found");
  }
});
await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
let browser;
try {
  browser = await chromium.launch({
    executablePath:
      "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe",
    headless: true,
  });
  const page = await browser.newPage({
    viewport: { width: 1160, height: 900 },
    deviceScaleFactor: 1,
  });
  const errors = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.addInitScript((fixture) => {
    window.__calls = [];
    let callbacks = {};
    let next = 0;
    const state = structuredClone(fixture);
    window.__state = state;
    window.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main" },
      },
      transformCallback(fn) {
        const id = ++next;
        callbacks[id] = fn;
        return id;
      },
      unregisterCallback(id) {
        delete callbacks[id];
      },
      invoke: async (command, args = {}) => {
        window.__calls.push({ command, args });
        switch (command) {
          case "get_state":
            return structuredClone(state);
          case "plugin:event|listen":
            return ++next;
          case "plugin:event|unlisten":
            return;
          case "capture_windows":
            return [
              {
                handle: 1,
                pid: 1,
                exe: "C:\\Apps\\Editor.exe",
                class: "Editor",
                title: "Document A",
                rect: [20, 20, 800, 600],
                maximized: false,
                minimized: false,
              },
              {
                handle: 2,
                pid: 2,
                exe: "C:\\Apps\\Browser.exe",
                class: "Browser",
                title: "Browser",
                rect: [900, 20, 1500, 900],
                maximized: false,
                minimized: false,
              },
            ];
          case "save_profile": {
            const p = { ...args, id: args.id ?? "profile1" };
            const index = state.config.profiles.findIndex((v) => v.id === p.id);
            if (index < 0) state.config.profiles.push(p);
            else state.config.profiles[index] = p;
            return;
          }
          case "save_hotkeys":
            state.config.hotkeys = args.bindings;
            return;
          case "pick_photo_folder":
            return "C:\\TestPhotos";
          case "validate_photo_folder":
            return 2;
          case "save_photo_config":
            state.photos = args.config;
            return;
          case "save_taskbars":
            state.taskbars = args.config;
            return;
          case "apply_displays":
            state.pending = {
              id: "test-display",
              deadline: Math.floor(Date.now() / 1000) + 25,
            };
            return state.pending.id;
          case "finish_display":
            state.pending = null;
            return;
          case "restore_windows":
            return { restored: 2, skipped: [] };
          default:
            return null;
        }
      },
    };
  }, fixture);
  await page.goto(`http://127.0.0.1:${server.address().port}`);
  await page
    .getByRole("heading", { name: "Displays, in their place." })
    .waitFor();
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({
    path: path.join(output, "dashboard-displays.png"),
    fullPage: true,
  });
  await page
    .getByRole("button", { name: "Test display changes", exact: true })
    .click();
  await page
    .getByRole("heading", { name: "Keep this display setup?" })
    .waitFor();
  await page.getByRole("button", { name: "Keep changes", exact: true }).click();
  await page.getByRole("dialog").waitFor({ state: "hidden" });
  await page.locator('[data-page="profiles"]').click();
  await page.locator("#profile-name").fill("Verification workspace");
  await page.getByRole("button", { name: "Capture open windows" }).click();
  await page.locator('[data-window="0"]').waitFor();
  await page.locator('[data-window="1"]').uncheck();
  await page.getByRole("button", { name: "Save new profile" }).click();
  await page.getByRole("heading", { name: "Verification workspace" }).waitFor();
  assert.equal(
    await page.evaluate(() => window.__state.config.profiles[0].windows.length),
    1,
  );
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({
    path: path.join(output, "dashboard-profiles.png"),
    fullPage: true,
  });
  await page.locator('[data-page="hotkeys"]').click();
  await page.getByRole("button", { name: "Add common actions" }).click();
  assert.equal(await page.locator(".hotkey-row").count(), 4);
  await page.getByRole("button", { name: "Save shortcuts" }).click();
  await page.waitForFunction(() => window.__state.config.hotkeys.length === 4);
  assert.equal(
    await page.evaluate(() => window.__state.config.hotkeys[0].action),
    "next-media",
  );
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({
    path: path.join(output, "dashboard-hotkeys.png"),
    fullPage: true,
  });
  await page.locator('[data-page="screensaver"]').click();
  await page.locator('[data-action="photo-browse"]').first().click();
  await page.waitForFunction(
    () =>
      document.querySelector('[data-photo-field="folder"]').value ===
      "C:\\TestPhotos",
  );
  await page.locator('[data-photo-field="fit"]').first().selectOption("cover");
  await page.getByRole("button", { name: "Save folders", exact: true }).click();
  await page.waitForFunction(
    () => window.__state.photos.screens[0].folder === "C:\\TestPhotos",
  );
  assert.equal(
    await page.evaluate(() => window.__state.photos.screens[0].fit),
    "cover",
  );
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({
    path: path.join(output, "dashboard-screensavers.png"),
    fullPage: true,
  });
  await page.locator('[data-page="taskbars"]').click();
  await page.getByRole("button", { name: "Refresh running apps" }).click();
  await page.waitForFunction(
    () => document.querySelectorAll("#pin-app-0 option").length === 3,
  );
  await page.locator('[data-bar="0"][data-bar-field="enabled"]').check();
  await page
    .locator('[data-bar="0"][data-bar-field="edge"]')
    .selectOption("left");
  await page.locator("#pin-app-0").selectOption("C:\\Apps\\Editor.exe");
  await page.locator('[data-action="bars-pin-running"][data-id="0"]').click();
  await page.getByRole("button", { name: "Save & apply taskbars" }).click();
  await page.waitForFunction(
    () => window.__state.taskbars.bars[0].pins.length === 1,
  );
  const bar = await page.evaluate(() => window.__state.taskbars.bars[0]);
  assert.equal(bar.edge, "left");
  assert.equal(bar.enabled, true);
  assert.equal(bar.pins[0].path, "C:\\Apps\\Editor.exe");
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.screenshot({
    path: path.join(output, "dashboard-taskbars.png"),
    fullPage: true,
  });
  assert.deepEqual(errors, []);
  await writeFile(
    path.join(output, "dashboard-tests.json"),
    JSON.stringify(
      {
        success: true,
        environment:
          "Headless Edge, isolated mocked Windows commands; not a native desktop click-through",
        checks: [
          "dashboard loads",
          "display confirmation modal",
          "profile capture and selected window persistence",
          "common hotkey actions",
          "per-monitor photo folders and fit settings",
          "taskbar edge, enable and per-monitor pin settings",
          "all five dashboard screenshots",
          "no JavaScript errors",
        ],
      },
      null,
      2,
    ),
  );
  console.log("Dashboard tests passed; screenshots saved under verification/.");
} finally {
  await browser?.close();
  server.close();
}
