#!/usr/bin/env node
// Enforces the 200-item contract of docs/roadmap-0.3.md, and its agreement with the
// Wave 1 sprint. Wired into `node scripts/check-docs.mjs`, so a future edit cannot
// silently drop an item, reuse an ID, lose the wave/priority/size metadata, repeat a
// title, move an item into a wave the sprint does not cover, shrink the research
// table, or stop citing Hallmark's items.
//
// Run alone to see only the roadmap result:
//   node scripts/check-roadmap.mjs
//   node scripts/check-roadmap.mjs --roadmap /tmp/roadmap-broken.md

import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

export const EXPECTED_ITEMS = 200;
export const MIN_REPOSITORIES = 20;
export const MIN_HALLMARK_CITATIONS = 8;

// `N. **W# · P# · S/M/L · Title.** body` — the one shape every item must keep.
const ITEM_HEAD = /^(\d+)\. \*\*(W[1-6]) · (P[0-2]) · ([SML]) · (.+?)\.\*\* ([\s\S]*)$/;
const CODE_SPAN = /`([^`]+)`/g;
const ARTIFACT_SHAPE = /\/|\.(?:rs|ts|tsx|js|mjs|cjs|toml|json|ya?ml|css|scss|md|lock|html)$/i;
const COMMAND_SHAPE = /^(?:rigel|npm|cargo|node|npx|gh)\b/;

function textLines(source) {
  return source.replace(/\r\n?/g, "\n").split("\n");
}

// An item owns its first line and every indented line after it, up to a blank line,
// a heading or a table row. Joined with spaces, so the metadata regex sees one string.
function itemBlocks(lines) {
  const blocks = [];
  let current = null;
  const flush = () => {
    if (current !== null) blocks.push(current);
    current = null;
  };
  lines.forEach((line, index) => {
    if (/^\d+\. /.test(line)) {
      flush();
      current = { line: index + 1, text: line };
      return;
    }
    if (current === null) return;
    if (line.trim() === "" || /^#{1,6} /.test(line) || line.startsWith("|")) {
      flush();
      return;
    }
    current.text += ` ${line.trim()}`;
  });
  flush();
  return blocks;
}

// `3–8, 10, 22–23` and `3, 4, 22–23` both expand to a set of IDs.
function expandIds(cell) {
  const found = new Set();
  for (const piece of String(cell).split(/[,;]/)) {
    const range = /(\d{1,3})\s*[–—-]\s*(\d{1,3})/.exec(piece);
    if (range !== null) {
      for (let n = Number(range[1]); n <= Number(range[2]); n += 1) found.add(n);
      continue;
    }
    const single = /\d{1,3}/.exec(piece);
    if (single !== null) found.add(Number(single[0]));
  }
  return found;
}

function sectionRange(lines, heading) {
  const start = lines.findIndex((line) => heading.test(line));
  if (start === -1) return null;
  let end = lines.length;
  for (let i = start + 1; i < lines.length; i += 1) {
    if (/^## /.test(lines[i])) {
      end = i;
      break;
    }
  }
  return { start, end };
}

const titleKey = (title) => title.toLowerCase().replace(/[^a-z0-9]+/g, " ").trim();

export function checkRoadmap({
  root = ROOT,
  roadmap = "docs/roadmap-0.3.md",
  sprint = "docs/sprint-0.3-wave-1.md",
  report = () => {},
} = {}) {
  const shown = (file) => path.relative(root, file).split(path.sep).join("/") || file;
  const roadmapFile = path.resolve(root, roadmap);
  const roadmapName = shown(roadmapFile);
  const sprintFile = path.resolve(root, sprint);
  const sprintName = shown(sprintFile);

  const empty = { items: 0, uniqueIds: 0, w1: 0, sprint: 0, repositories: 0, hallmark: 0 };

  let source;
  try {
    source = readFileSync(roadmapFile, "utf8");
  } catch {
    report(roadmapName, null, "the roadmap is missing");
    return empty;
  }
  const lines = textLines(source);
  const blocks = itemBlocks(lines);

  const items = [];
  const byId = new Map();
  const titles = new Map();

  for (const block of blocks) {
    const head = ITEM_HEAD.exec(block.text);
    if (head === null) {
      report(
        roadmapName,
        block.line,
        "item is not `N. **W# · P# · S/M/L · Title.** …` with the wave, priority and size metadata",
      );
      continue;
    }
    const [, idText, wave, priority, size, title, body] = head;
    const id = Number(idText);
    const item = { id, wave, priority, size, title: title.trim(), body, line: block.line };
    items.push(item);

    const earlier = byId.get(id);
    if (earlier !== undefined) {
      report(roadmapName, block.line, `repeats item ID ${id} (first used at line ${earlier.line})`);
    } else {
      byId.set(id, item);
    }

    const key = titleKey(item.title);
    if (titles.has(key)) {
      report(roadmapName, block.line, `title "${item.title}" repeats item ${titles.get(key)}`);
    } else {
      titles.set(key, id);
    }

    if (item.title.length > 72) {
      report(roadmapName, block.line, `item ${id} title is longer than 72 characters`);
    }
    if (!/— done when/i.test(body)) {
      report(roadmapName, block.line, `item ${id} has no "— done when …" signal`);
    }
    const artifacts = [...body.matchAll(CODE_SPAN)].map((hit) => hit[1].trim());
    if (!artifacts.some((name) => ARTIFACT_SHAPE.test(name) || COMMAND_SHAPE.test(name))) {
      report(
        roadmapName,
        block.line,
        `item ${id} names no backticked file, crate, workflow, command or screen`,
      );
    }
  }

  if (items.length !== EXPECTED_ITEMS) {
    report(
      roadmapName,
      null,
      `has ${items.length} numbered items; exactly ${EXPECTED_ITEMS} are required`,
    );
  }
  for (let id = 1; id <= EXPECTED_ITEMS; id += 1) {
    if (!byId.has(id)) report(roadmapName, null, `item ${id} is missing; IDs must run 1–${EXPECTED_ITEMS}`);
  }
  const outOfOrder = items.findIndex((item, index) => item.id !== index + 1);
  if (outOfOrder !== -1) {
    report(
      roadmapName,
      items[outOfOrder].line,
      `item ${items[outOfOrder].id} is out of order; IDs must be strictly sequential from 1`,
    );
  }

  // Waves must agree with the item metadata in both directions, inside this document.
  const order = sectionRange(lines, /^## Order of work/);
  const waveLists = new Map();
  if (order === null) {
    report(roadmapName, null, 'no "Order of work" section to compare wave membership against');
  } else {
    for (let i = order.start + 1; i < order.end; i += 1) {
      const bullet = /^- \*\*Wave (\d)\b[^*]*\*\*(.*)$/.exec(lines[i]);
      if (bullet === null) continue;
      waveLists.set(`W${bullet[1]}`, { line: i + 1, ids: expandIds(bullet[2]) });
    }
    for (let wave = 1; wave <= 6; wave += 1) {
      if (!waveLists.has(`W${wave}`)) report(roadmapName, null, `"Order of work" has no Wave ${wave} bullet`);
    }
    for (const item of items) {
      const listed = waveLists.get(item.wave);
      if (listed !== undefined && !listed.ids.has(item.id)) {
        report(roadmapName, item.line, `item ${item.id} says ${item.wave} but "Order of work" does not list it there`);
      }
    }
    for (const [wave, listed] of waveLists) {
      for (const id of listed.ids) {
        const item = byId.get(id);
        if (item === undefined) {
          report(roadmapName, listed.line, `"Order of work" lists item ${id}, which does not exist`);
        } else if (item.wave !== wave) {
          report(roadmapName, item.line, `"Order of work" lists item ${id} under ${wave} but the item says ${item.wave}`);
        }
      }
    }
  }

  // The Wave 1 set and the sprint's work-package table must be the same set.
  let sprintIds = new Set();
  try {
    const sprintLines = textLines(readFileSync(sprintFile, "utf8"));
    for (const line of sprintLines) {
      if (!/^\| *\d+\. /.test(line)) continue;
      const cells = line.split("|").map((cell) => cell.trim());
      for (const id of expandIds(cells[2] ?? "")) sprintIds.add(id);
    }
  } catch {
    report(sprintName, null, "the Wave 1 sprint document is missing");
    sprintIds = new Set();
  }
  if (sprintIds.size === 0) {
    report(sprintName, null, "the work-package table lists no roadmap item IDs");
  }
  const w1 = new Set(items.filter((item) => item.wave === "W1").map((item) => item.id));
  for (const id of sprintIds) {
    if (!w1.has(id)) {
      const item = byId.get(id);
      report(roadmapName, null, `the sprint contains item ${id} but the roadmap assigns it to ${item === undefined ? "no wave" : item.wave}`);
    }
  }
  for (const id of w1) {
    if (!sprintIds.has(id)) {
      report(roadmapName, byId.get(id)?.line ?? null, `item ${id} claims W1 but no sprint work package contains it`);
    }
  }

  // Research: at least 20 repositories, each with a URL, a thing to borrow and items.
  let repositories = 0;
  const repos = sectionRange(lines, /^## .*repositor/i);
  if (repos === null) {
    report(roadmapName, null, "no repository research section");
  } else {
    for (let i = repos.start + 1; i < repos.end; i += 1) {
      if (!/^\| *\[/.test(lines[i])) continue;
      repositories += 1;
      const cells = lines[i].split("|").map((cell) => cell.trim());
      if (!/\[[^\]]+\]\(https:\/\/github\.com\/[^)\s]+\)/.test(cells[1] ?? "")) {
        report(roadmapName, i + 1, "repository row has no GitHub URL");
      }
      if ((cells[2] ?? "") === "") {
        report(roadmapName, i + 1, "repository row does not say what to borrow");
      }
      if (expandIds(cells[3] ?? "").size === 0) {
        report(roadmapName, i + 1, "repository row names no roadmap item");
      }
    }
    if (repositories < MIN_REPOSITORIES) {
      report(roadmapName, null, `lists ${repositories} repositories; at least ${MIN_REPOSITORIES} are required`);
    }
  }

  // Hallmark: the doctrine section must cite concrete items, not just claim influence.
  let hallmark = 0;
  const influence = sectionRange(lines, /^## .*Hallmark/);
  if (influence === null) {
    report(roadmapName, null, "no Hallmark section");
  } else {
    const cited = new Set();
    for (let i = influence.start + 1; i < influence.end; i += 1) {
      for (const hit of lines[i].matchAll(/\bitems? (\d{1,3})(?:\s*[–-]\s*(\d{1,3}))?/g)) {
        const from = Number(hit[1]);
        const to = hit[2] === undefined ? from : Number(hit[2]);
        for (let id = from; id <= to; id += 1) cited.add(id);
      }
    }
    for (const id of cited) {
      if (!byId.has(id)) report(roadmapName, null, `the Hallmark section cites item ${id}, which does not exist`);
    }
    hallmark = cited.size;
    if (hallmark < MIN_HALLMARK_CITATIONS) {
      report(roadmapName, null, `the Hallmark section cites ${hallmark} items; at least ${MIN_HALLMARK_CITATIONS} are required`);
    }
  }

  return {
    items: items.length,
    uniqueIds: byId.size,
    w1: w1.size,
    sprint: sprintIds.size,
    repositories,
    hallmark,
  };
}

if (process.argv[1] !== undefined && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const argument = (name) => {
    const index = process.argv.indexOf(name);
    return index === -1 ? undefined : process.argv[index + 1];
  };
  const problems = [];
  const info = checkRoadmap({
    roadmap: argument("--roadmap") ?? "docs/roadmap-0.3.md",
    sprint: argument("--sprint") ?? "docs/sprint-0.3-wave-1.md",
    report: (file, line, message) =>
      problems.push(line === null ? `${file}: ${message}` : `${file}:${line}: ${message}`),
  });
  if (problems.length > 0) {
    console.error(`roadmap check: ${problems.length} problem${problems.length === 1 ? "" : "s"}\n`);
    for (const problem of problems) console.error(`  ${problem}`);
    process.exitCode = 1;
  } else {
    console.log(
      `roadmap check: ${info.items} items, IDs 1–${info.items} unique, ` +
        `${info.w1} Wave 1 items match ${info.sprint} sprint packages, ` +
        `${info.repositories} repositories, ${info.hallmark} Hallmark citations`,
    );
  }
}
