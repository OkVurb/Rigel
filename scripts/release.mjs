#!/usr/bin/env node
/**
 * Publish a release in one command: bump every manifest, record the changelog, commit, tag, push.
 *
 * Usage:
 *   node scripts/release.mjs patch
 *   node scripts/release.mjs minor
 *   node scripts/release.mjs major
 *   node scripts/release.mjs 0.2.0
 *   node scripts/release.mjs patch --dry-run
 *   node scripts/release.mjs patch --no-push
 *
 * The tag is what publishes the release: pushing `v<version>` starts
 * `.github/workflows/release.yml`, which builds the sidecar and both installers, signs
 * `latest.json`, and attaches them to the release. The version bump is committed before the
 * tag is pushed so a checkout of the tag reports the same version as the release page.
 *
 * The changelog entry is the commit subjects since the previous tag, oldest first. Merge and
 * prior release commits are left out because they name no change a reader can use.
 */

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const USAGE = `usage: node scripts/release.mjs <patch|minor|major|x.y.z> [--dry-run] [--no-push] [--allow-branch]

  patch, minor, major  bump that part of the current version
  x.y.z                use an explicit version; it must be newer than the current one
  --dry-run            print the version, changelog and commands without changing anything
  --no-push            commit and tag locally, then stop before pushing
  --allow-branch       release from a branch other than main (rarely what you want)`;

function fail(message) {
  console.error(`release: ${message}`);
  process.exit(1);
}

function git(args, allowFailure = false) {
  const result = spawnSync("git", args, { cwd: ROOT, encoding: "utf8" });
  if (result.error) fail(`could not run git: ${result.error.message}`);
  if (result.status !== 0 && !allowFailure) {
    const detail = (result.stderr || result.stdout || "").trim();
    fail(`git ${args.join(" ")} failed${detail ? `:\n${detail}` : ""}`);
  }
  return result;
}

function run(command, args) {
  const result = spawnSync(command, args, { cwd: ROOT, encoding: "utf8" });
  if (result.error) fail(`could not run ${command}: ${result.error.message}`);
  if (result.status !== 0) {
    const detail = (result.stderr || result.stdout || "").trim();
    fail(`${command} ${args.join(" ")} failed${detail ? `:\n${detail}` : ""}`);
  }
  return result.stdout;
}

/* ------------------------------------------------------------------ version */

function current_version() {
  const cargo = readFileSync(path.join(ROOT, "Cargo.toml"), "utf8");
  const match = /\[workspace\.package\][\s\S]*?\nversion = "([^"]+)"/.exec(cargo);
  if (!match) fail("Cargo.toml has no [workspace.package] version");
  return match[1];
}

function numeric(version) {
  const parts = version.split("-")[0].split(".").map((part) => Number(part));
  if (parts.length !== 3 || parts.some((part) => !Number.isInteger(part))) {
    fail(`${version} is not a three-part version`);
  }
  return parts;
}

function is_newer(candidate, current) {
  const [aMajor, aMinor, aPatch] = numeric(candidate);
  const [bMajor, bMinor, bPatch] = numeric(current);
  if (aMajor !== bMajor) return aMajor > bMajor;
  if (aMinor !== bMinor) return aMinor > bMinor;
  return aPatch > bPatch;
}

function next_version(current, requested) {
  if (/^\d+\.\d+\.\d+$/.test(requested)) return requested;
  const [major, minor, patch] = numeric(current);
  if (requested === "major") return `${major + 1}.0.0`;
  if (requested === "minor") return `${major}.${minor + 1}.0`;
  if (requested === "patch") return `${major}.${minor}.${patch + 1}`;
  fail(`expected patch, minor, major or an explicit x.y.z version, not ${requested}`);
}

/* ----------------------------------------------------------------- changelog */

function changelog_entry(version) {
  const previous = git(["describe", "--tags", "--abbrev=0", "HEAD"], true);
  const range = previous.status === 0 ? `${previous.stdout.trim()}..HEAD` : "HEAD";
  const subjects = git(["log", "--reverse", "--no-merges", "--pretty=format:%s", range])
    .stdout.split(/\r?\n/)
    .map((subject) => subject.trim())
    .filter((subject) => subject !== "")
    .filter((subject) => !/^chore\(release\):/.test(subject))
    .filter((subject) => !/^Merge /.test(subject));

  if (subjects.length === 0) {
    const since = previous.status === 0 ? ` since ${previous.stdout.trim()}` : "";
    fail(`nothing to release: no commits${since}`);
  }

  // The release day as the machine cutting it sees it: UTC would date an evening release
  // tomorrow, which is a confusing heading for a changelog.
  const now = new Date();
  const date = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(
    now.getDate(),
  ).padStart(2, "0")}`;
  const entry = [`## ${version} — ${date}`, "", ...subjects.map((subject) => `- ${subject}`)];
  return { entry: entry.join("\n"), subjects, previous: previous.stdout.trim() };
}

function write_changelog(entry) {
  const file = path.join(ROOT, "CHANGELOG.md");
  if (!existsSync(file)) {
    writeFileSync(
      file,
      `# Changelog\n\nEvery release, newest first. The heading is the version its \`v*\` tag publishes.\n\n${entry}\n`,
      "utf8",
    );
    return;
  }
  const existing = readFileSync(file, "utf8").replace(/\r\n/g, "\n");
  const first = existing.indexOf("\n## ");
  if (first === -1) {
    writeFileSync(file, `${existing.trimEnd()}\n\n${entry}\n`, "utf8");
    return;
  }
  // Newest first: the new entry sits directly under the title, above the last release. The
  // slice ends on the newline that opens the old heading, so trim it before adding the space
  // this entry needs; otherwise every release grows a blank line at the top of the file.
  const head = existing.slice(0, first).trimEnd();
  const tail = existing.slice(first + 1);
  writeFileSync(file, `${head}\n\n${entry}\n\n${tail}`, "utf8");
}

/* --------------------------------------------------------------- the other files */

/// `set-release-version.mjs` is the single place that rewrites the three manifests, so the
/// workflow's stamp and this script cannot disagree about what a release version touches.
function stamp_manifests(version) {
  run(process.execPath, [path.join(ROOT, "scripts", "set-release-version.mjs"), version]);
}

/// npm records the package version in the lock's root and again under `packages[""]`. Editing
/// the JSON keeps the existing line endings, which is why this is not re-serialised wholesale.
function stamp_package_lock(version) {
  const file = path.join(ROOT, "ui", "package-lock.json");
  if (!existsSync(file)) return;
  const text = readFileSync(file, "utf8");
  const lock = JSON.parse(text);
  const eol = text.includes("\r\n") ? "\r\n" : "\n";
  lock.version = version;
  if (lock.packages && lock.packages[""]) lock.packages[""].version = version;
  writeFileSync(file, `${JSON.stringify(lock, null, 2).replace(/\n/g, eol)}${eol}`, "utf8");
}

/// A workspace version bump makes Cargo.lock stale. `cargo metadata` refreshes just the lock,
/// without building anything or resolving a new dependency.
function stamp_cargo_lock() {
  const result = spawnSync("cargo", ["metadata", "--format-version", "1"], {
    cwd: ROOT,
    stdio: "ignore",
    timeout: 180_000,
  });
  if (result.error || result.status !== 0) {
    fail(
      "could not refresh Cargo.lock with `cargo metadata`; the version files are already bumped, " +
        "so fix cargo and rerun, or discard the bump with `git checkout -- Cargo.toml " +
        "crates/rigel-app/tauri.conf.json ui/package.json`",
    );
  }
}

/* ---------------------------------------------------------------------- main */

const argv = process.argv.slice(2);
if (argv.includes("--help") || argv.includes("-h")) {
  console.log(USAGE);
  process.exit(0);
}

const flags = new Set();
const positional = [];
for (const argument of argv) {
  if (argument.startsWith("--")) flags.add(argument);
  else positional.push(argument);
}
const known = new Set(["--dry-run", "--no-push", "--allow-branch"]);
for (const flag of flags) {
  if (!known.has(flag)) fail(`unknown option ${flag}\n\n${USAGE}`);
}
if (positional.length !== 1) fail(USAGE);

const dryRun = flags.has("--dry-run");
const noPush = flags.has("--no-push");
const allowBranch = flags.has("--allow-branch");

process.chdir(ROOT);

const dirty = git(["status", "--porcelain"]).stdout.trim();
if (dirty !== "") fail("the working tree has changes; commit or stash them before releasing");

const branch = git(["rev-parse", "--abbrev-ref", "HEAD"]).stdout.trim();
if (branch !== "main" && !allowBranch) {
  fail(`releases are cut from main, not ${branch}; check out main or pass --allow-branch`);
}

const current = current_version();
const version = next_version(current, positional[0]);
if (!is_newer(version, current)) fail(`${version} is not newer than the current ${current}`);
const tag = `v${version}`;

const localTag = git(["rev-parse", "-q", "--verify", `refs/tags/${tag}`], true);
if (localTag.status === 0) fail(`tag ${tag} already exists`);

if (!noPush) {
  // 2 is ls-remote's "no matching ref"; anything else means the check itself failed.
  const remoteTag = git(["ls-remote", "--exit-code", "--tags", "origin", `refs/tags/${tag}`], true);
  if (remoteTag.status === 0) fail(`tag ${tag} already exists on origin`);
  if (remoteTag.status !== 2) {
    const detail = (remoteTag.stderr || remoteTag.stdout || "").trim();
    fail(`could not check origin for ${tag}${detail ? `:\n${detail}` : ""}`);
  }
}

const { entry, subjects } = changelog_entry(version);

console.log(`release: ${current} -> ${version} (${tag})`);
console.log(`release: ${subjects.length} change${subjects.length === 1 ? "" : "s"} since the last tag`);
for (const subject of subjects) console.log(`  - ${subject}`);

if (dryRun) {
  console.log("release: dry run, nothing changed");
  console.log("release: would bump Cargo.toml, crates/rigel-app/tauri.conf.json, ui/package.json,");
  console.log("release:   Cargo.lock and ui/package-lock.json, and write the CHANGELOG.md entry");
  if (noPush) {
    console.log(`release: would commit "chore(release): ${version}" and tag ${tag}, without pushing`);
  } else {
    console.log(
      `release: would commit "chore(release): ${version}", tag ${tag}, and push ${branch} and the tag`,
    );
  }
  process.exit(0);
}

if (spawnSync("cargo", ["--version"], { cwd: ROOT, stdio: "ignore" }).status !== 0) {
  fail("cargo is not on PATH; the workspace version cannot be stamped");
}

stamp_manifests(version);
stamp_package_lock(version);
stamp_cargo_lock();
write_changelog(entry);

const tracked = [
  "Cargo.toml",
  "Cargo.lock",
  "crates/rigel-app/tauri.conf.json",
  "ui/package.json",
  "ui/package-lock.json",
  "CHANGELOG.md",
].filter((file) => existsSync(path.join(ROOT, file)));
git(["add", "--", ...tracked]);
git([
  "commit",
  "-m",
  `chore(release): ${version}`,
  "-m",
  "Bump the workspace, Tauri and UI versions and record the release notes in CHANGELOG.md.",
]);
git(["tag", "-a", tag, "-m", `Rigel ${version}`]);

if (noPush) {
  console.log(`release: committed and tagged ${tag}; nothing pushed`);
  console.log(`release: publish it with \`git push origin HEAD\` then \`git push origin ${tag}\``);
  process.exit(0);
}

git(["push", "origin", "HEAD"]);
const pushed = git(["push", "origin", tag], true);
if (pushed.status !== 0) {
  const detail = (pushed.stderr || pushed.stdout || "").trim();
  fail(
    `the branch is pushed but the tag is not; publish it with \`git push origin ${tag}\`${
      detail ? `\n${detail}` : ""
    }`,
  );
}

console.log(`release: pushed ${branch} and ${tag}`);

const remote = git(["remote", "get-url", "origin"], true);
const slug =
  remote.status === 0
    ? /github\.com[:/]([^/]+)\/([^/.]+?)(?:\.git)?$/.exec(remote.stdout.trim())
    : null;
if (slug) {
  const base = `https://github.com/${slug[1]}/${slug[2]}`;
  console.log(`release: workflow  ${base}/actions/workflows/release.yml`);
  console.log(`release: release   ${base}/releases/tag/${tag}`);
} else {
  console.log("release: the release workflow builds, signs and publishes the tag");
}
