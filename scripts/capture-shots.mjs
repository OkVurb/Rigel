#!/usr/bin/env node
/**
 * Capture the current build.
 *
 * The renderer runs in a browser preview with sample rows (`ui/src/mock.ts`), which is exactly what
 * the Playwright suite already drives — so these are photographs of the shipped build, not mockups
 * drawn for a README. Every still is taken at a 1440x900 viewport with a 2x device scale, and the
 * tour records the same pages in motion for the GIF and the ad.
 *
 * Usage: node scripts/capture-shots.mjs
 *   expects `npm run preview` in ui/ on http://127.0.0.1:4173 (the Playwright config's port)
 */
import { createRequire } from "node:module";
import { existsSync, mkdirSync, readdirSync, renameSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";

const require = createRequire(new URL("../ui/package.json", import.meta.url));
const { chromium } = require("playwright");

const BASE = process.env.RIGEL_PREVIEW_URL ?? "http://127.0.0.1:4173";
const SHOTS = resolve("web/shots");
const VIDDIR = resolve("tmp/capture-video");
const PREFIX = "2026-";

mkdirSync(SHOTS, { recursive: true });
rmSync(VIDDIR, { recursive: true, force: true });
mkdirSync(VIDDIR, { recursive: true });

const taken = [];
const missed = [];

async function still(page, name) {
  try {
    await page.screenshot({ path: join(SHOTS, `${PREFIX}${name}.png`) });
    taken.push(name);
  } catch (error) {
    missed.push(`${name}: ${error.message}`);
  }
}

async function click(page, role, name, timeout = 6000) {
  await page.getByRole(role, { name, exact: false }).first().click({ timeout });
}

async function card(page, html, file) {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.setContent(html, { waitUntil: "load" });
  await page.waitForTimeout(400);
  await page.screenshot({ path: join("tmp", file) });
}

const CARD_CSS = `
  * { box-sizing: border-box; margin: 0; }
  body {
    width: 1440px; height: 900px; overflow: hidden;
    background: #08090b; color: #e8eaee;
    font-family: "Segoe UI", system-ui, sans-serif;
    display: grid; place-items: center; position: relative; isolation: isolate;
  }
  .aurora { position: absolute; inset: -20% -10% auto; height: 900px; filter: blur(80px); opacity: .55; }
  .a1 { position: absolute; width: 620px; height: 620px; left: 8%; top: -6%;
        border-radius: 50%; background: radial-gradient(circle, rgba(240,166,60,.9), transparent 65%); }
  .a2 { position: absolute; width: 520px; height: 520px; right: 6%; top: -2%;
        border-radius: 50%; background: radial-gradient(circle, rgba(226,96,59,.7), transparent 65%); }
  .a3 { position: absolute; width: 560px; height: 560px; left: 34%; top: 30%;
        border-radius: 50%; background: radial-gradient(circle, rgba(123,92,224,.6), transparent 68%); }
  .grid { position: absolute; inset: 0;
    background-image: linear-gradient(to right, rgba(255,255,255,.05) 1px, transparent 1px),
                      linear-gradient(to bottom, rgba(255,255,255,.05) 1px, transparent 1px);
    background-size: 64px 64px;
    mask-image: radial-gradient(ellipse 55% 50% at 50% 45%, #000 20%, transparent 78%); }
  .inner { position: relative; text-align: center; padding: 0 80px; }
  .eyebrow { color: #f0a63c; letter-spacing: .22em; font-size: 15px; text-transform: uppercase;
             font-weight: 650; margin-bottom: 34px; }
  h1 { font-size: 96px; line-height: 1.04; letter-spacing: -.035em; font-weight: 750; }
  .g { background: linear-gradient(100deg, #fff, #ffd9a6 38%, #f0a63c 68%, #ff9d7a 96%);
       -webkit-background-clip: text; background-clip: text; color: transparent; }
  .sub { margin-top: 30px; color: #9aa0a8; font-size: 24px; }
  .mark { position: absolute; left: 56px; top: 44px; display: flex; align-items: center; gap: 12px;
          font-size: 20px; font-weight: 650; color: #e8eaee; }
  .mark img { width: 34px; height: 34px; filter: drop-shadow(0 0 12px rgba(240,166,60,.5)); }
`;

const titleCard = `<!doctype html><html><head><style>${CARD_CSS}</style></head><body>
  <div class="aurora"><span class="a1"></span><span class="a2"></span><span class="a3"></span></div>
  <div class="grid"></div>
  <div class="mark"><img src="${BASE}/rigel.svg" alt="">Rigel</div>
  <div class="inner">
    <p class="eyebrow">Local-first · Windows</p>
    <h1>Several coding agents.<br><span class="g">One tree. One bill.</span></h1>
    <p class="sub">Codex · Claude Code · Cursor · Cline · and one native coordinator</p>
  </div>
</body></html>`;

const endCard = `<!doctype html><html><head><style>${CARD_CSS}</style></head><body>
  <div class="aurora"><span class="a1" style="left:24%"></span><span class="a2" style="right:18%"></span><span class="a3" style="left:40%;top:22%"></span></div>
  <div class="grid"></div>
  <div class="inner">
    <p class="eyebrow">MIT OR Apache-2.0</p>
    <h1><span class="g">Rigel</span></h1>
    <p class="sub">No telemetry · no account · the index stays on your disk<br>
      <span style="color:#e8eaee">github.com/OkVurb/Rigel</span></p>
  </div>
</body></html>`;

const browser = await chromium.launch();

/* ------------------------------- stills ------------------------------- */
try {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    deviceScaleFactor: 2,
    reducedMotion: "no-preference",
  });
  const page = await context.newPage();
  page.setDefaultTimeout(8000);
  await page.goto(BASE, { waitUntil: "networkidle" });
  await page.waitForTimeout(700);

  await still(page, "shelf");

  // The inspector is the point of the shelf: one row open beside the list.
  try {
    await page.getByRole("option", { name: /pdf/i }).first().click();
    await page.waitForTimeout(600);
    await still(page, "shelf-detail");
  } catch (error) {
    missed.push(`shelf-detail: ${error.message}`);
  }

  await click(page, "button", "Session");
  await page.waitForTimeout(700);
  await still(page, "session");

  await click(page, "button", "Chat");
  await page.waitForTimeout(600);
  await still(page, "chat");

  await click(page, "button", "Diff");
  await page.waitForTimeout(600);
  await still(page, "diff");

  await click(page, "button", "Terminal");
  await page.waitForTimeout(900);
  try {
    await page.locator(".terminal-surface").click({ timeout: 4000 });
    await page.keyboard.type("rigel doctor --project .", { delay: 28 });
    await page.keyboard.press("Enter");
    await page.waitForTimeout(600);
  } catch {
    /* the shell may need a beat; the still is still worth taking */
  }
  await still(page, "terminal");

  await click(page, "button", "Settings");
  await page.waitForTimeout(500);
  await still(page, "settings-agent");
  try {
    await page.getByRole("option", { name: /Harnesses/i }).first().click();
    await page.waitForTimeout(400);
    await still(page, "settings-harnesses");
  } catch (error) {
    missed.push(`settings-harnesses: ${error.message}`);
  }

  await click(page, "button", "Attention");
  await page.waitForTimeout(500);
  await still(page, "attention");

  await click(page, "button", "Shelf");
  await page.waitForTimeout(400);
  try {
    await page.keyboard.press("Control+k");
    await page.waitForTimeout(350);
    await page.keyboard.type("session", { delay: 40 });
    await page.waitForTimeout(350);
    await still(page, "palette");
    await page.keyboard.press("Escape");
  } catch (error) {
    missed.push(`palette: ${error.message}`);
  }

  await context.close();
} catch (error) {
  missed.push(`stills: ${error.message}`);
}

/* -------------------------------- tour -------------------------------- */
let video = null;
try {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    deviceScaleFactor: 1,
    recordVideo: { dir: VIDDIR, size: { width: 1440, height: 900 } },
    reducedMotion: "no-preference",
  });
  const page = await context.newPage();
  page.setDefaultTimeout(8000);
  await page.goto(BASE, { waitUntil: "networkidle" });
  await page.waitForTimeout(1600);

  // Shelf: open one row so the inspector animates in.
  await page.getByRole("option", { name: /pdf/i }).first().click().catch(() => {});
  await page.waitForTimeout(2200);

  // Palette: the keyboard path through the app.
  await page.keyboard.press("Control+k");
  await page.waitForTimeout(700);
  await page.keyboard.type("session", { delay: 55 });
  await page.waitForTimeout(900);
  await page.keyboard.press("Enter");
  await page.waitForTimeout(2400);

  // Session: scroll the transcript like a reader.
  await page.mouse.move(900, 600);
  await page.mouse.wheel(0, 420);
  await page.waitForTimeout(1800);
  await page.mouse.wheel(0, 420);
  await page.waitForTimeout(1600);

  await click(page, "button", "Chat").catch(() => {});
  await page.waitForTimeout(2600);

  await click(page, "button", "Diff").catch(() => {});
  await page.waitForTimeout(2800);

  await click(page, "button", "Terminal").catch(() => {});
  await page.waitForTimeout(1200);
  await page.locator(".terminal-surface").click().catch(() => {});
  await page.keyboard.type("rigel doctor", { delay: 85 });
  await page.keyboard.press("Enter");
  await page.waitForTimeout(2200);

  await click(page, "button", "Settings").catch(() => {});
  await page.waitForTimeout(1000);
  await page.getByRole("option", { name: /Harnesses/i }).first().click().catch(() => {});
  await page.waitForTimeout(2200);

  await click(page, "button", "Shelf").catch(() => {});
  await page.waitForTimeout(1400);

  const path = await page.video().path();
  await context.close();
  video = join(VIDDIR, "tour.webm");
  renameSync(path, video);
} catch (error) {
  missed.push(`tour: ${error.message}`);
}

/* -------------------------------- cards -------------------------------- */
let cards = false;
try {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    deviceScaleFactor: 1,
  });
  const page = await context.newPage();
  await card(page, titleCard, "ad-title.png");
  await card(page, endCard, "ad-end.png");
  await context.close();
  cards = true;
} catch (error) {
  missed.push(`cards: ${error.message}`);
}

await browser.close();

console.log(`still at ${BASE}`);
console.log(`  captured ${taken.length}: ${taken.join(", ")}`);
console.log(`  video: ${video ?? "none"}`);
console.log(`  cards: ${cards ? "tmp/ad-title.png, tmp/ad-end.png" : "none"}`);
if (missed.length) {
  console.log("  skipped:");
  for (const line of missed) console.log(`    ${line}`);
}
if (video && !existsSync(video)) process.exitCode = 1;
const files = existsSync(SHOTS) ? readdirSync(SHOTS).filter((f) => f.startsWith(PREFIX)) : [];
console.log(`  files on disk: ${files.length}`);
