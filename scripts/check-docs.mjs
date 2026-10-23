#!/usr/bin/env node
// Fails when a document this repository publishes is no longer usable.
//
// GitHub renders the front door, the docs and the issue forms; nothing else tests
// them. A relative link that names no file, an anchor that names no heading, or an
// issue form with a field GitHub rejects fails here, offline, with no dependencies.
//
// The YAML reader below understands the subset issue forms are written in: block
// mappings, block sequences, plain and quoted scalars, `|` and `>` blocks, and
// one-line flow sequences. Anything else is reported as an error rather than
// skipped, so a form written in a construct this reader does not know fails the
// build instead of passing unexamined.

import { existsSync, readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const TEMPLATES = path.join(ROOT, ".github", "ISSUE_TEMPLATE");
const problems = [];
const report = (file, line, message) =>
  problems.push(line === null ? `${file}: ${message}` : `${file}:${line}: ${message}`);

// ---------------------------------------------------------------- YAML subset

class YamlReader {
  constructor(source, file) {
    this.file = file;
    this.lines = source.replace(/\r\n?/g, "\n").split("\n");
    this.i = 0;
  }

  error(message, line = this.i) {
    report(path.relative(ROOT, this.file), line + 1, message);
  }

  static blank(line) {
    return line.trim() === "" || line.trimStart().startsWith("#");
  }

  static indent(line) {
    return line.length - line.trimStart().length;
  }

  peek() {
    while (this.i < this.lines.length && YamlReader.blank(this.lines[this.i])) this.i++;
    return this.i < this.lines.length ? this.lines[this.i] : null;
  }

  parse() {
    return this.parseBlock(0);
  }

  parseBlock(minIndent) {
    const line = this.peek();
    if (line === null) return null;
    const indent = YamlReader.indent(line);
    if (indent < minIndent) return null;
    const trimmed = line.trimStart();
    return trimmed.startsWith("- ") || trimmed === "-"
      ? this.parseSequence(indent)
      : this.parseMapping(indent, {});
  }

  parseSequence(indent) {
    const out = [];
    for (;;) {
      const line = this.peek();
      if (line === null) break;
      const current = YamlReader.indent(line);
      if (current < indent) break;
      if (current > indent) {
        this.error(`unexpected indentation inside a sequence (${current} spaces)`);
        this.i++;
        continue;
      }
      const trimmed = line.trimStart();
      if (!trimmed.startsWith("-")) break;
      const body = trimmed.slice(1).trim();
      this.i++;
      if (body === "") {
        out.push(this.parseBlock(indent + 1));
        continue;
      }
      const inline = /^([A-Za-z0-9_.-]+):(?:\s+(.*))?$/.exec(body);
      if (!inline) {
        out.push(this.parseScalar(body));
        continue;
      }
      const item = {};
      item[inline[1]] = inline[2] === undefined ? null : this.parseScalar(inline[2].trim());
      out.push(this.parseMapping(indent + 2, item));
    }
    return out;
  }

  parseMapping(indent, target) {
    for (;;) {
      const line = this.peek();
      if (line === null) break;
      const current = YamlReader.indent(line);
      if (current < indent) break;
      if (current > indent) {
        this.error(`unexpected indentation (${current} spaces, expected ${indent})`);
        this.i++;
        continue;
      }
      const trimmed = line.trimStart();
      if (trimmed.startsWith("- ")) break;
      const entry = /^([A-Za-z0-9_.-]+):(?:\s+(.*))?$/.exec(trimmed);
      if (!entry) {
        this.error("expected `key: value`");
        this.i++;
        continue;
      }
      const key = entry[1];
      const rest = entry[2] === undefined ? "" : entry[2].trim();
      this.i++;
      if (/^[|>][+-]?$/.test(rest)) {
        target[key] = this.parseBlockScalar(indent, rest[0]);
      } else if (rest === "") {
        const next = this.peek();
        target[key] = next !== null && YamlReader.indent(next) > indent ? this.parseBlock(indent + 1) : null;
      } else {
        target[key] = this.parseScalar(rest);
      }
    }
    return target;
  }

  parseBlockScalar(keyIndent, style) {
    const collected = [];
    let blockIndent = null;
    while (this.i < this.lines.length) {
      const line = this.lines[this.i];
      if (line.trim() === "") {
        collected.push("");
        this.i++;
        continue;
      }
      const indent = YamlReader.indent(line);
      if (indent <= keyIndent) break;
      if (blockIndent === null) blockIndent = indent;
      collected.push(line.slice(Math.min(indent, blockIndent)));
      this.i++;
    }
    while (collected.length && collected[collected.length - 1] === "") collected.pop();
    const text = collected.join("\n");
    return style === "|" ? text : text.replace(/\s*\n\s*/g, " ").trim();
  }

  parseScalar(raw) {
    const value = raw.trim();
    if (value.length > 1 && value.startsWith('"') && value.endsWith('"')) {
      return value.slice(1, -1).replace(/\\"/g, '"').replace(/\\\\/g, "\\");
    }
    if (value.length > 1 && value.startsWith("'") && value.endsWith("'")) {
      return value.slice(1, -1).replace(/''/g, "'");
    }
    if (value.startsWith("[") && value.endsWith("]")) {
      const inner = value.slice(1, -1).trim();
      return inner === "" ? [] : inner.split(",").map((item) => this.parseScalar(item.trim()));
    }
    if (value === "true") return true;
    if (value === "false") return false;
    if (value === "null" || value === "~") return null;
    const comment = value.search(/\s#/);
    return comment === -1 ? value : value.slice(0, comment).trim();
  }
}

// ---------------------------------------------------------------- README

// The anchor GitHub derives from a heading. Emoji are stripped, the space they
// leave behind becomes a dash, and a repeated heading gains a numeric suffix.
function anchorOf(heading) {
  return heading
    .replace(/[\uFE0E\uFE0F]/g, "")
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\p{M} _-]/gu, "")
    .replace(/ /g, "-");
}

// Relative targets resolve against the file's own directory, which is what GitHub
// does: two documents in different folders can both say `qa/p2.md` and mean
// different things. An in-page anchor is checked against this file's headings; a
// link that names another document is checked for existence, not for its anchor.
function checkMarkdown(pathname) {
  const file = path.relative(ROOT, pathname).split(path.sep).join("/");
  const source = readFileSync(pathname, "utf8");
  const lines = source.replace(/\r\n?/g, "\n").split("\n");
  const anchors = new Map();
  let fence = null;

  lines.forEach((line) => {
    const trimmed = line.trimStart();
    if (/^(```|~~~)/.test(trimmed)) {
      fence = fence === null ? trimmed.slice(0, 3) : null;
      return;
    }
    if (fence !== null) return;
    const heading = /^#{1,6}\s+(.*?)\s*#*\s*$/.exec(line);
    if (!heading) return;
    const base = anchorOf(heading[1]);
    const seen = (anchors.get(base) ?? 0) + 1;
    anchors.set(base, seen);
    if (seen > 1) anchors.set(`${base}-${seen - 1}`, 1);
  });

  const checkTarget = (target, lineNumber) => {
    if (/^(https?:|mailto:)/i.test(target)) return;
    if (target.startsWith("#")) {
      const anchor = decodeURIComponent(target.slice(1));
      if (anchor !== "" && !anchors.has(anchor)) {
        report(file, lineNumber, `anchor ${target} names no heading`);
      }
      return;
    }
    const relative = decodeURIComponent(target.split("#")[0].split("?")[0]);
    if (relative !== "" && !existsSync(path.join(path.dirname(pathname), relative))) {
      report(file, lineNumber, `link target ${target} does not exist`);
    }
  };

  // Sections can be linked two ways, and the README uses both: markdown links in
  // the body, and raw anchors in the centred navigation row.
  fence = null;
  lines.forEach((line, index) => {
    const trimmed = line.trimStart();
    if (/^(```|~~~)/.test(trimmed)) {
      fence = fence === null ? trimmed.slice(0, 3) : null;
      return;
    }
    if (fence !== null) return;
    // A link written inside a code span is documentation, not a link.
    const scannable = line.replace(/`[^`]*`/g, "");
    const markdown = /!?\[[^\]]*\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g;
    for (let hit = markdown.exec(scannable); hit !== null; hit = markdown.exec(scannable)) {
      checkTarget(hit[1], index + 1);
    }
    const html = /(?:href|src)\s*=\s*"([^"]+)"/g;
    for (let hit = html.exec(scannable); hit !== null; hit = html.exec(scannable)) {
      checkTarget(hit[1], index + 1);
    }
  });
}

// ---------------------------------------------------------------- issue forms

const ELEMENT_TYPES = new Set(["markdown", "input", "textarea", "dropdown", "checkboxes"]);

function validateForm(file, form) {
  if (form === null || typeof form !== "object" || Array.isArray(form)) {
    report(file, null, "an issue form is a mapping at the top level");
    return;
  }
  for (const key of ["name", "description", "body"]) {
    if (!(key in form)) report(file, null, `missing \`${key}\``);
  }
  if (typeof form.name === "string" && form.name.trim() === "") report(file, null, "`name` is empty");
  if (typeof form.description === "string" && form.description.length > 200) {
    report(file, null, "`description` is longer than GitHub's 200 characters");
  }
  if (form.labels !== undefined && !Array.isArray(form.labels)) {
    report(file, null, "`labels` is a sequence of strings");
  }
  if (!Array.isArray(form.body) || form.body.length === 0) {
    report(file, null, "`body` is a non-empty sequence of elements");
    return;
  }

  const ids = new Set();
  form.body.forEach((element, index) => {
    const at = `body[${index}]`;
    if (element === null || typeof element !== "object" || Array.isArray(element)) {
      report(file, null, `${at} is not a mapping`);
      return;
    }
    const type = element.type;
    if (!ELEMENT_TYPES.has(type)) {
      report(file, null, `${at} has type ${JSON.stringify(type)}; GitHub accepts ${[...ELEMENT_TYPES].join(", ")}`);
      return;
    }
    if (type !== "markdown") {
      if (typeof element.id !== "string") {
        report(file, null, `${at} needs an \`id\``);
      } else if (!/^[a-zA-Z0-9_-]+$/.test(element.id)) {
        report(file, null, `${at} id ${JSON.stringify(element.id)} may only use letters, digits, - and _`);
      } else if (ids.has(element.id)) {
        report(file, null, `${at} repeats the id ${JSON.stringify(element.id)}`);
      } else {
        ids.add(element.id);
      }
    }
    const attributes = element.attributes;
    if (attributes === null || typeof attributes !== "object" || Array.isArray(attributes)) {
      report(file, null, `${at} needs \`attributes\``);
      return;
    }
    if (type === "markdown") {
      if (typeof attributes.value !== "string") report(file, null, `${at} is markdown and needs a \`value\``);
      if (element.validations !== undefined) report(file, null, `${at} is markdown; GitHub cannot mark it required`);
      return;
    }
    if (typeof attributes.label !== "string" || attributes.label.trim() === "") {
      report(file, null, `${at} needs a non-empty \`label\``);
    }
    if (type === "dropdown" || type === "checkboxes") {
      const options = attributes.options;
      if (!Array.isArray(options) || options.length === 0) {
        report(file, null, `${at} needs a non-empty \`options\``);
      } else if (type === "dropdown" && options.some((option) => typeof option !== "string")) {
        report(file, null, `${at} dropdown options are strings`);
      } else if (type === "checkboxes" && options.some((option) => !option || typeof option.label !== "string")) {
        report(file, null, `${at} every checkbox needs a \`label\``);
      }
    }
    if (element.validations !== undefined) {
      const validations = element.validations;
      if (validations === null || typeof validations !== "object" || Array.isArray(validations)) {
        report(file, null, `${at} \`validations\` is a mapping`);
        return;
      }
      const unknown = Object.keys(validations).filter((key) => key !== "required");
      if (unknown.length > 0) report(file, null, `${at} has unknown validations: ${unknown.join(", ")}`);
      if ("required" in validations && typeof validations.required !== "boolean") {
        report(file, null, `${at} \`required\` is true or false`);
      }
    }
  });
}

function validateConfig(file, config) {
  if (config === null || typeof config !== "object" || Array.isArray(config)) {
    report(file, null, "the template chooser config is a mapping");
    return;
  }
  if ("blank_issues_enabled" in config && typeof config.blank_issues_enabled !== "boolean") {
    report(file, null, "`blank_issues_enabled` is true or false");
  }
  const links = config.contact_links ?? [];
  if (!Array.isArray(links)) {
    report(file, null, "`contact_links` is a sequence");
    return;
  }
  links.forEach((link, index) => {
    const at = `contact_links[${index}]`;
    if (link === null || typeof link !== "object" || Array.isArray(link)) {
      report(file, null, `${at} is not a mapping`);
      return;
    }
    if (typeof link.name !== "string" || link.name.trim() === "") report(file, null, `${at} needs a \`name\``);
    if (typeof link.about !== "string" || link.about.trim() === "") report(file, null, `${at} needs an \`about\``);
    if (typeof link.url !== "string" || !/^https?:\/\//.test(link.url)) {
      report(file, null, `${at} needs an http(s) \`url\``);
    }
  });
}

function parseTemplates() {
  let entries;
  try {
    entries = readdirSync(TEMPLATES).sort();
  } catch {
    report(".github/ISSUE_TEMPLATE", null, "directory is missing");
    return;
  }
  const forms = entries.filter((name) => /\.ya?ml$/.test(name) && name !== "config.yml");
  if (forms.length === 0) report(".github/ISSUE_TEMPLATE", null, "no issue forms found");

  for (const name of entries) {
    if (!/\.ya?ml$/.test(name)) continue;
    const file = path.join(TEMPLATES, name);
    const shown = path.relative(ROOT, file).split(path.sep).join("/");
    if (name !== "config.yml" && !name.endsWith(".yml")) {
      report(shown, null, "GitHub reads issue forms only from a .yml file");
      continue;
    }
    let document;
    try {
      document = new YamlReader(readFileSync(file, "utf8"), file).parse();
    } catch (error) {
      report(shown, null, `could not be read: ${error.message}`);
      continue;
    }
    if (name === "config.yml") validateConfig(shown, document);
    else validateForm(shown, document);
  }
}

function markdownFiles(directory) {
  const found = [];
  const entries = readdirSync(directory, { withFileTypes: true })
    .sort((left, right) => left.name.localeCompare(right.name));
  for (const entry of entries) {
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) found.push(...markdownFiles(full));
    else if (entry.name.endsWith(".md")) found.push(full);
  }
  return found;
}

const documents = [path.join(ROOT, "README.md"), ...markdownFiles(path.join(ROOT, "docs"))];
for (const document of documents) checkMarkdown(document);
parseTemplates();

if (problems.length > 0) {
  console.error(`docs check: ${problems.length} problem${problems.length === 1 ? "" : "s"}\n`);
  for (const problem of problems) console.error(`  ${problem}`);
  process.exitCode = 1;
} else {
  console.log(`docs check: ${documents.length} documents and every issue form are valid`);
}
