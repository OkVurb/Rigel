#!/usr/bin/env node
// Both Windows bundles must be Rigel, not stock Tauri/NSIS/WiX. This check is used by CI and by
// the release workflow, so losing the installer art or the amber pointer fails before publishing.

import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const app = join(root, "crates", "rigel-app");
const configPath = join(app, "tauri.conf.json");
const checkDist = process.argv.includes("--dist");

const passed = [];
const failed = [];

function check(condition, message) {
  if (condition) passed.push(message);
  else failed.push(message);
}

function read(path) {
  try {
    return readFileSync(path);
  } catch {
    return null;
  }
}

function insideApp(path) {
  const tail = relative(app, path);
  return tail !== "" && !tail.startsWith("..") && !isAbsolute(tail);
}

function checkArt(label, configured, width, height) {
  if (typeof configured !== "string" || configured.length === 0) {
    check(false, `${label} is configured`);
    return;
  }
  const path = resolve(app, configured);
  check(insideApp(path), `${label} stays inside crates/rigel-app`);
  const bytes = read(path);
  check(bytes !== null && bytes.length > 1024, `${label} exists and has content`);
  if (!bytes || bytes.length < 54) return;

  const bmp = bytes.toString("ascii", 0, 2) === "BM";
  const actualWidth = bytes.readInt32LE(18);
  const actualHeight = Math.abs(bytes.readInt32LE(22));
  const depth = bytes.readUInt16LE(28);
  check(bmp, `${label} is a BMP`);
  check(
    actualWidth === width && actualHeight === height,
    `${label} is ${width}x${height} (found ${actualWidth}x${actualHeight})`,
  );
  check(depth === 24, `${label} is 24-bit (found ${depth}-bit)`);
}

function checkIcon(label, configured) {
  if (typeof configured !== "string" || configured.length === 0) {
    check(false, `${label} is configured`);
    return;
  }
  const path = resolve(app, configured);
  check(insideApp(path), `${label} stays inside crates/rigel-app`);
  const bytes = read(path);
  check(bytes !== null && bytes.length > 1024, `${label} exists and has content`);
  if (!bytes || bytes.length < 6) return;
  check(
    bytes[0] === 0 && bytes[1] === 0 && bytes[2] === 1 && bytes[3] === 0,
    `${label} is a Windows icon`,
  );
}

check(existsSync(configPath), "tauri.conf.json exists");
if (!existsSync(configPath)) {
  for (const message of failed) console.error(`  FAIL  ${message}`);
  process.exit(1);
}

let config;
try {
  config = JSON.parse(readFileSync(configPath, "utf8"));
} catch (error) {
  console.error(`  FAIL  tauri.conf.json is valid JSON: ${error.message}`);
  process.exit(1);
}

const bundle = config.bundle ?? {};
const windows = bundle.windows ?? {};
const wix = windows.wix ?? {};
const nsis = windows.nsis ?? {};
const targets = new Set(bundle.targets ?? []);

check(bundle.active === true, "bundling is active");
check(targets.has("msi") && targets.has("nsis"), "both MSI and NSIS targets are enabled");
check(
  windows.webviewInstallMode?.type === "downloadBootstrapper",
  "the WebView2 bootstrapper is configured",
);
check(
  typeof config.build?.beforeBuildCommand === "string" &&
    config.build.beforeBuildCommand.includes("ui run build"),
  "the UI is rebuilt before bundling",
);

checkArt("WiX banner", wix.bannerPath, 493, 58);
checkArt("WiX dialog", wix.dialogImagePath, 493, 312);
checkArt("NSIS header", nsis.headerImage, 150, 57);
checkArt("NSIS sidebar", nsis.sidebarImage, 164, 314);
checkIcon("NSIS installer icon", nsis.installerIcon);
checkIcon("NSIS uninstaller icon", nsis.uninstallerIcon);
checkIcon("bundle icon", (bundle.icon ?? [])[0]);
check(
  nsis.installerIcon === (bundle.icon ?? [])[0] &&
    nsis.uninstallerIcon === (bundle.icon ?? [])[0],
  "setup, uninstaller and bundle share Rigel's icon",
);

const cursor = read(join(root, "ui", "src", "cursor.tsx"))?.toString("utf8") ?? "";
const main = read(join(root, "ui", "src", "main.tsx"))?.toString("utf8") ?? "";
const shell = read(join(root, "ui", "src", "shell.css"))?.toString("utf8") ?? "";
check(main.includes("<Cursor />"), "the Rigel pointer is mounted in the main window");
check(cursor.includes('className="rigel-pointer"'), "the pointer component renders .rigel-pointer");
check(
  cursor.includes("rigel-pointer-dot") && cursor.includes("rigel-pointer-ring"),
  "the pointer keeps its dot and ring",
);
check(
  shell.includes("html.rigel-cursor") && shell.includes(".rigel-pointer-dot"),
  "the shell styles the pointer and hides the native cursor",
);

if (checkDist) {
  const dist = join(root, "ui", "dist");
  const assets = join(dist, "assets");
  check(existsSync(join(dist, "index.html")), "ui/dist contains the built index.html");
  if (existsSync(assets)) {
    const files = readdirSync(assets);
    const js = files
      .filter((name) => name.endsWith(".js"))
      .map((name) => read(join(assets, name))?.toString("utf8") ?? "")
      .join("\n");
    const css = files
      .filter((name) => name.endsWith(".css"))
      .map((name) => read(join(assets, name))?.toString("utf8") ?? "")
      .join("\n");
    check(js.includes("rigel-pointer"), "the built UI contains the Rigel pointer");
    check(css.includes("rigel-pointer") && css.includes("rigel-cursor"), "the built CSS hides the native cursor");
  } else {
    check(false, "ui/dist/assets exists");
  }
}

for (const message of passed) console.log(`  ok    ${message}`);
for (const message of failed) console.error(`  FAIL  ${message}`);
if (failed.length > 0) {
  console.error(`installer branding: ${failed.length} check(s) failed`);
  process.exit(1);
}
console.log(`installer branding: ${passed.length} check(s) passed${checkDist ? " (built UI)" : ""}`);
