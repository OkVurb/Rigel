#!/usr/bin/env node
/**
 * Verify Rigel Cloud against a live Firebase project, end to end, without a human reading a
 * runbook each time.
 *
 *   RIGEL_FIREBASE_PROJECT_ID=... RIGEL_FIREBASE_API_KEY=...
 *   RIGEL_CLOUD_EMAIL=you+rigeltest@example.com RIGEL_CLOUD_PASSWORD=...
 *   node scripts/cloud-verify.mjs
 *
 * The first live verification was done by hand, which meant every release re-derived the same
 * command sequence and every failure looked like a new one. This is that sequence with the
 * assertions attached: account creation, the duplicate refusal, the reset answer that must not
 * reveal whether an address exists, a push/pull round trip across two throwaway data
 * directories, a tombstone, and the sync clocks. Two directories stand in for two machines; the
 * keychain session is shared, which is exactly what a second install would see.
 *
 * Two things stay human because a script cannot do them: an emailed link is either waited for
 * (`--wait-verified <seconds>`) or reported as skipped, and the rules playground is not touched.
 * Nothing here runs in CI: it needs a live project, real mail and a real password.
 *
 * The account is expected to be disposable. Unless `--keep` is passed, the run deletes
 * `users/<uid>` from Firestore and signs out; the Firebase Authentication user itself has no
 * CLI delete, so the summary names the console page for it. Cleanup runs even after a failed
 * step, so a failure does not leave the account's documents behind.
 *
 * The Firebase session is one OS keychain entry shared by every data directory — that sharing is
 * what lets directory B reuse directory A's sign-in — so the run signs this machine out of Rigel
 * Cloud when it starts and again at cleanup. Sign in again afterwards.
 */
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
// `/tmp/` is gitignored, so a verification run leaves no repository state behind.
const WORK = path.join(ROOT, "tmp", "cloud-verify");
const DIRECTORY_A = path.join(WORK, "a");
const DIRECTORY_B = path.join(WORK, "b");

const USAGE = `Verify Rigel Cloud against a live Firebase project.

  node scripts/cloud-verify.mjs [options]

Required, from the environment or a flag:
  RIGEL_FIREBASE_PROJECT_ID     --project <id>
  RIGEL_FIREBASE_API_KEY        --api-key <key>     the public web API key
  RIGEL_CLOUD_EMAIL             --email <address>   a disposable test account
  RIGEL_CLOUD_PASSWORD                              read from the environment only

Optional:
  RIGEL_BIN                     a prebuilt CLI to test instead of rebuilding
  RIGEL_CLOUD_NEW_PASSWORD      after opening a reset link, proves the new password
  --repo <owner/repo>           the shelf fixture        (default anthropics/skills)
  --limit <n>                   rows to index             (default 3)
  --wait-verified <seconds>     poll for a clicked link   (default 0: report skipped)
  --skip-mail                   skip verification and reset mail
  --clean-cloud                 delete users/<uid> before seeding
  --keep                        keep the directories, the documents and the session
  --json                        one JSON summary instead of the step lines
  -h, --help`;

const HELP = new Set(["-h", "--help"]);

class Skip extends Error {}

function parseArgs(list) {
  const options = {
    project: process.env.RIGEL_FIREBASE_PROJECT_ID ?? "",
    apiKey: process.env.RIGEL_FIREBASE_API_KEY ?? "",
    email: process.env.RIGEL_CLOUD_EMAIL ?? "",
    password: process.env.RIGEL_CLOUD_PASSWORD ?? "",
    newPassword: process.env.RIGEL_CLOUD_NEW_PASSWORD ?? "",
    repo: "anthropics/skills",
    limit: 3,
    waitVerified: 0,
    skipMail: false,
    cleanCloud: false,
    keep: false,
    json: false,
    help: false,
  };
  for (let index = 0; index < list.length; index += 1) {
    const arg = list[index];
    const value = () => {
      const next = list[index + 1];
      if (next === undefined) throw new Error(`${arg} needs a value`);
      index += 1;
      return next;
    };
    switch (arg) {
      case "--project":
        options.project = value();
        break;
      case "--api-key":
        options.apiKey = value();
        break;
      case "--email":
        options.email = value();
        break;
      case "--repo":
        options.repo = value();
        break;
      case "--limit":
        options.limit = Number(value());
        break;
      case "--wait-verified":
        options.waitVerified = Number(value());
        break;
      case "--skip-mail":
        options.skipMail = true;
        break;
      case "--clean-cloud":
        options.cleanCloud = true;
        break;
      case "--keep":
        options.keep = true;
        break;
      case "--json":
        options.json = true;
        break;
      default:
        if (HELP.has(arg)) options.help = true;
        else throw new Error(`unknown option ${arg}`);
    }
  }
  if (!Number.isInteger(options.limit) || options.limit < 1 || options.limit > 20) {
    throw new Error("--limit must be a whole number between 1 and 20");
  }
  // A negative wait means "do not poll", which is the same as the default.
  if (!Number.isFinite(options.waitVerified) || options.waitVerified < 0) {
    throw new Error("--wait-verified must be a number of seconds");
  }
  return options;
}

// ------------------------------------------------------------------ the CLI

let BINARY = "";

function binaryPath() {
  if (process.env.RIGEL_BIN) return process.env.RIGEL_BIN;
  return path.join(ROOT, "target", "debug", process.platform === "win32" ? "rigel.exe" : "rigel");
}

function buildBinary() {
  BINARY = binaryPath();
  if (process.env.RIGEL_BIN) {
    if (!existsSync(BINARY)) {
      throw new Error(`RIGEL_BIN points at ${process.env.RIGEL_BIN}, which does not exist`);
    }
    return;
  }
  // Build every run: an old `target/debug` binary would verify the previous commit, not this one.
  const build = spawnSync("cargo", ["build", "-q", "-p", "rigel-cli"], {
    cwd: ROOT,
    encoding: "utf8",
    stdio: ["ignore", "inherit", "inherit"],
  });
  if (build.error) throw new Error(`could not run cargo: ${build.error.message}`);
  if (build.status !== 0) throw new Error(`cargo build -p rigel-cli failed with exit ${build.status}`);
  if (!existsSync(BINARY)) throw new Error(`the build produced no binary at ${BINARY}`);
}

/** The first JSON value in a stream; the CLI prints only JSON under `--json`, but cargo does not. */
function parseJson(text) {
  const start = text.search(/[[{]/);
  if (start === -1) return null;
  try {
    return JSON.parse(text.slice(start));
  } catch {
    return null;
  }
}

/**
 * Run the CLI in one data directory. `stdin` is how a password reaches it: never an argument,
 * where it would sit in the process list.
 */
function rigel(args, { dir, stdin = "", allowFailure = false } = {}) {
  const full = [...(dir ? ["--data-dir", dir] : []), ...args];
  const run = spawnSync(BINARY, full, {
    encoding: "utf8",
    input: stdin,
    maxBuffer: 16 << 20,
    env: process.env,
  });
  if (run.error) throw new Error(`could not run ${BINARY}: ${run.error.message}`);
  const stdout = run.stdout ?? "";
  const stderr = run.stderr ?? "";
  const json = parseJson(stdout);
  if (run.status !== 0 && !allowFailure) {
    const message = json?.error || stderr.trim() || stdout.trim() || `exit ${run.status}`;
    throw new Error(message);
  }
  return { code: run.status, stdout, stderr, json };
}

function firebase(args) {
  // The CLI is a .cmd shim on Windows, and Node will not spawn one without a shell; passing an
  // args array alongside `shell` is deprecated, so the command goes through as one quoted line.
  const command = ["firebase", ...args]
    .map((part) => (/^[A-Za-z0-9_@./:=+-]+$/.test(part) ? part : `"${part.replaceAll('"', '\\"')}"`))
    .join(" ");
  const run = spawnSync(command, { encoding: "utf8", maxBuffer: 16 << 20, shell: true });
  const stdout = run.stdout ?? "";
  const stderr = run.stderr ?? "";
  if (run.error || run.status !== 0) {
    return { ok: false, code: run.status, stdout, stderr, json: null };
  }
  return { ok: true, code: 0, stdout, stderr, json: parseJson(stdout) };
}

// ------------------------------------------------------------------ the steps

function expectEqual(actual, expected, field) {
  if (actual !== expected) {
    throw new Error(`${field}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

function expectNumber(value, field) {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new Error(`${field}: expected a number, got ${JSON.stringify(value)}`);
  }
}

function status(dir) {
  return rigel(["cloud", "status", "--json"], { dir }).json;
}

/** The setup mistakes a live run hits first, translated into the page that fixes each one. */
function setupHint(text, project) {
  const consoleUrl = `https://console.firebase.google.com/project/${project}`;
  if (/CONFIGURATION_NOT_FOUND|OPERATION_NOT_ALLOWED|sign-in is disabled/i.test(text)) {
    return `enable Email/Password at ${consoleUrl}/authentication/providers`;
  }
  if (/Firestore is not enabled|database.*does not exist|NOT_FOUND.*firestore/i.test(text)) {
    return `create the Firestore database at ${consoleUrl}/firestore`;
  }
  if (/API key not valid|API_KEY_INVALID/i.test(text)) {
    return "copy the Web API key from Project settings; an HTTP-referrer restriction breaks a desktop client";
  }
  if (/email or password is wrong|INVALID_LOGIN_CREDENTIALS|INVALID_PASSWORD/i.test(text)) {
    return "the account exists with a different password: put its password in RIGEL_CLOUD_PASSWORD, or use a fresh address";
  }
  return "";
}

function stats(dir) {
  return rigel(["skills", "stats", "--json"], { dir }).json;
}

const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    console.log(USAGE);
    return 0;
  }

  const state = {
    uid: "",
    seeded: 0,
    firestore: "unknown",
    configured: false,
    results: [],
    stopped: "",
  };

  const width = 24;
  const say = (text) => {
    if (!options.json) console.log(text);
  };

  async function step(label, run, { force = false } = {}) {
    if (state.stopped !== "" && !force) {
      state.results.push({ name: label, status: "skip", detail: `not run: ${state.stopped} failed` });
      return;
    }
    try {
      const detail = await run();
      state.results.push({ name: label, status: "pass", detail: detail ?? "" });
      say(`  ${label.padEnd(width)} ok${detail ? `   ${detail}` : ""}`);
    } catch (error) {
      if (error instanceof Skip) {
        state.results.push({ name: label, status: "skip", detail: error.message });
        say(`  ${label.padEnd(width)} skip ${error.message}`);
        return;
      }
      state.results.push({
        name: label,
        status: "fail",
        detail: error.message,
        hint: error.hint ?? "",
      });
      state.stopped = label;
      say(`  ${label.padEnd(width)} FAIL ${error.message}`);
      if (error.hint) say(`  ${"".padEnd(width)}      ${error.hint}`);
    }
  }

  say(`Rigel Cloud verification against ${options.project || "(no project)"}`);

  await step("preflight", () => {
    const missing = [];
    if (!options.project) missing.push("RIGEL_FIREBASE_PROJECT_ID");
    if (!options.apiKey) missing.push("RIGEL_FIREBASE_API_KEY");
    if (!options.email) missing.push("RIGEL_CLOUD_EMAIL");
    if (!options.password) missing.push("RIGEL_CLOUD_PASSWORD");
    if (missing.length > 0) {
      throw Object.assign(new Error(`set ${missing.join(", ")}`), {
        hint: "see docs/cloud.md, Verifying a live project",
      });
    }
    buildBinary();
    const version = spawnSync(BINARY, ["--version"], { encoding: "utf8" });
    return (version.stdout ?? "").trim() || BINARY;
  });

  await step("configure", () => {
    rmSync(WORK, { recursive: true, force: true });
    mkdirSync(DIRECTORY_A, { recursive: true });
    mkdirSync(DIRECTORY_B, { recursive: true });
    for (const dir of [DIRECTORY_A, DIRECTORY_B]) {
      // The web API key on stdin keeps it out of this process's argument list.
      rigel(["cloud", "configure", "--project", options.project], { dir, stdin: options.apiKey });
    }
    // A crashed run can leave the shared keychain session signed in; start from a known state.
    rigel(["cloud", "logout", "--json"], { dir: DIRECTORY_A, allowFailure: true });
    const current = status(DIRECTORY_A);
    expectEqual(current.configured, true, "configured");
    expectEqual(current.project_id, options.project, "project_id");
    expectEqual(current.signed_in, false, "signed_in");
    state.configured = true;
    return "cloud.json in both directories, signed out";
  });

  await step("account", () => {
    const signUp = rigel(["cloud", "sign-up", "--email", options.email, "--json"], {
      dir: DIRECTORY_A,
      stdin: options.password,
      allowFailure: true,
    });
    let answer = signUp;
    let how = "created";
    if (signUp.code !== 0) {
      const refusal = signUp.json?.error ?? signUp.stderr;
      // A repeatable run reuses its account; only an unrelated refusal is a failure.
      if (!/already has an account|EMAIL_EXISTS/i.test(refusal)) {
        throw Object.assign(new Error(`sign-up: ${refusal.trim()}`), {
          hint: setupHint(refusal, options.project),
        });
      }
      try {
        answer = rigel(["cloud", "login", "--email", options.email, "--json"], {
          dir: DIRECTORY_A,
          stdin: options.password,
        });
      } catch (error) {
        error.hint = setupHint(error.message, options.project);
        throw error;
      }
      how = "signed in";
    }
    expectEqual(answer.json?.signed_in, true, "signed_in");
    expectEqual(answer.json?.project_id, options.project, "project_id");
    if (typeof answer.json?.uid !== "string" || answer.json.uid === "") {
      throw new Error("sign-up and sign-in both returned no uid");
    }
    state.uid = answer.json.uid;
    const current = status(DIRECTORY_A);
    expectEqual(current.uid, state.uid, "status uid");
    expectEqual(current.email, options.email, "status email");
    return `${how} as ${options.email} (${state.uid})`;
  });

  await step("duplicate", () => {
    const again = rigel(["cloud", "sign-up", "--email", options.email, "--json"], {
      dir: DIRECTORY_A,
      stdin: options.password,
      allowFailure: true,
    });
    if (again.code === 0) {
      throw new Error("a second sign-up for the same address succeeded and made a duplicate account");
    }
    const refusal = again.json?.error ?? again.stderr;
    if (!/already has an account|EMAIL_EXISTS/i.test(refusal)) {
      throw new Error(`expected a duplicate refusal, got: ${refusal.trim()}`);
    }
    const current = status(DIRECTORY_A);
    expectEqual(current.signed_in, true, "the refused sign-up kept the session");
    return "refused, session intact";
  });

  await step("email verification", async () => {
    if (options.skipMail) throw new Skip("skipped with --skip-mail");
    if (status(DIRECTORY_A).email_verified) return "already verified";
    const sent = rigel(["cloud", "verify", "--json"], { dir: DIRECTORY_A }).json;
    expectEqual(sent?.sent, true, "sent");
    if (options.waitVerified === 0) {
      throw new Skip(`sent to ${sent.email}; open the link and pass --wait-verified <seconds>`);
    }
    const deadline = Date.now() + options.waitVerified * 1000;
    let waited = 0;
    while (Date.now() < deadline) {
      await sleep(Math.min(5000, Math.max(0, deadline - Date.now())));
      waited += 5;
      const check = rigel(["cloud", "verify", "--check", "--json"], { dir: DIRECTORY_A }).json;
      if (check?.email_verified === true) {
        return `verified after ${waited}s`;
      }
    }
    throw new Error(
      `still unverified after ${options.waitVerified}s — open the link, then run again`,
    );
  });

  await step("password reset", () => {
    if (options.skipMail) throw new Skip("skipped with --skip-mail");
    const unknown = `rigel-verify-${Date.now()}@example.com`;
    const known = rigel(["cloud", "reset", "--email", options.email], { dir: DIRECTORY_A });
    const stranger = rigel(["cloud", "reset", "--email", unknown], { dir: DIRECTORY_A });
    // The page must not be an account oracle: the wording may not differ by address.
    const wording = (answer, address) => answer.stdout.split(address).join("<address>").trim();
    if (wording(known, options.email) !== wording(stranger, unknown)) {
      throw new Error("the reset answer differs for a known and an unknown address");
    }
    if (!/if an account exists/i.test(known.stdout)) {
      throw new Error(`unexpected reset answer: ${known.stdout.trim()}`);
    }
    if (!options.newPassword) {
      throw new Skip("sent; set RIGEL_CLOUD_NEW_PASSWORD after opening the link to check the change");
    }
    rigel(["cloud", "logout", "--json"], { dir: DIRECTORY_A });
    const stale = rigel(["cloud", "login", "--email", options.email, "--json"], {
      dir: DIRECTORY_A,
      stdin: options.password,
      allowFailure: true,
    });
    if (stale.code === 0) {
      throw new Error("the pre-reset password still signs in — open the link first");
    }
    rigel(["cloud", "login", "--email", options.email, "--json"], {
      dir: DIRECTORY_A,
      stdin: options.newPassword,
    });
    options.password = options.newPassword;
    return "link sent, wording identical, old password refused, new password signs in";
  });

  await step("firestore", () => {
    const probe = firebase(["firestore:databases:list", "--project", options.project, "--json"]);
    if (!probe.ok) {
      state.firestore = "unknown";
      // The CLI prints a JSON blob on stdout, so the first line alone can be just "{".
      const lines = `${probe.stderr}\n${probe.stdout}`
        .split("\n")
        .map((line) => line.trim())
        // Skip the JSON wrapper: one of its fragments reads worse than the bare exit code.
        .filter(
          (line) =>
            line !== "" && !/^[[\]{}]/.test(line) && !line.startsWith('"') && !line.endsWith(","),
        );
      const why = (probe.json?.error?.message || lines[0] || `exit ${probe.code}`).slice(0, 140);
      throw new Skip(`the Firebase CLI could not answer (${why}); the push below is the real test`);
    }
    const databases = Array.isArray(probe.json) ? probe.json : (probe.json?.result ?? []);
    const present = databases.some(
      (database) => typeof database?.name === "string" && database.name.endsWith("/databases/(default)"),
    );
    if (!present) {
      state.firestore = "missing";
      const error = new Error("no (default) Firestore database in this project");
      error.hint = `create it at https://console.firebase.google.com/project/${options.project}/firestore`;
      throw error;
    }
    state.firestore = "present";
    return "the (default) database exists";
  });

  await step("clean cloud", () => {
    if (!options.cleanCloud) throw new Skip("left as it is; pass --clean-cloud to reset it");
    if (state.firestore === "missing") throw new Skip("no database to clean");
    const run = firebase([
      "firestore:delete",
      "--project",
      options.project,
      "--recursive",
      `users/${state.uid}`,
      "--force",
    ]);
    if (!run.ok) {
      throw new Error(`firestore:delete failed: ${(run.stderr || run.stdout).trim()}`);
    }
    return `users/${state.uid} removed`;
  });

  await step("seed shelf", () => {
    const report = rigel(
      ["skills", "index", options.repo, "--limit", String(options.limit), "--json"],
      { dir: DIRECTORY_A },
    ).json;
    if (!report || typeof report.with_body !== "number") {
      throw new Error("`skills index` printed an unexpected report");
    }
    const current = stats(DIRECTORY_A);
    expectEqual(current.skills, options.limit, "skills");
    expectEqual(current.with_body, options.limit, "with_body");
    state.seeded = current.skills;
    return `${current.skills} rows with bodies, from ${options.repo}`;
  });

  await step("push", () => {
    const report = rigel(["cloud", "push", "--json"], { dir: DIRECTORY_A }).json;
    expectEqual(report?.uid, state.uid, "uid");
    expectEqual(report?.pushed, state.seeded, "pushed");
    expectEqual(report?.deleted, 0, "deleted");
    expectEqual(report?.truncated, false, "truncated");
    return `${report.pushed} row(s) in ${report.batches} batch(es)`;
  });

  await step("pull", () => {
    const report = rigel(["cloud", "pull", "--json"], { dir: DIRECTORY_B }).json;
    if (report?.received !== state.seeded) {
      const error = new Error(
        `received ${report?.received} row(s), expected ${state.seeded} — the account holds documents from another run`,
      );
      error.hint = "use a disposable account, or pass --clean-cloud";
      throw error;
    }
    expectEqual(report.written, state.seeded, "written");
    expectEqual(report.tombstones, 0, "tombstones");
    const current = stats(DIRECTORY_B);
    expectEqual(current.skills, state.seeded, "second directory skills");
    // The guarantee under test: metadata crossed the wire, bodies did not.
    expectEqual(current.with_body, 0, "second directory with_body");
    expectEqual(status(DIRECTORY_B).signed_in, true, "shared session in the second directory");
    return `${report.written} row(s) written, 0 bodies`;
  });

  await step("tombstone", () => {
    const answer = rigel(["skills", "search", "--limit", String(options.limit), "--json"], {
      dir: DIRECTORY_A,
    }).json;
    // `skills search --json` wraps its rows: { query, count, order, results }.
    const hits = answer?.results;
    if (!Array.isArray(hits) || hits.length !== state.seeded) {
      const seen = Array.isArray(hits) ? `${hits.length} results` : JSON.stringify(answer?.count ?? null);
      throw new Error(`expected ${state.seeded} search results, got ${seen}`);
    }
    const removed = hits[0].id;
    rigel(["skills", "remove", removed, "--json"], { dir: DIRECTORY_A });
    const pushed = rigel(["cloud", "push", "--json"], { dir: DIRECTORY_A }).json;
    expectEqual(pushed?.pushed, state.seeded - 1, "pushed");
    expectEqual(pushed?.deleted, 1, "deleted");
    const pulled = rigel(["cloud", "pull", "--json"], { dir: DIRECTORY_B }).json;
    expectEqual(pulled?.tombstones, 1, "tombstones");
    expectEqual(pulled?.deleted, 1, "deleted");
    const current = stats(DIRECTORY_B);
    expectEqual(current.skills, state.seeded - 1, "second directory skills");
    return `1 row removed locally, 1 tombstone, 1 row deleted on the other side`;
  });

  await step("sync clocks", () => {
    const first = status(DIRECTORY_A);
    const second = status(DIRECTORY_B);
    expectNumber(first.last_push_at, "last_push_at");
    expectNumber(second.last_pull_at, "last_pull_at");
    expectEqual(first.last_sync_at, first.last_push_at, "the push directory's last_sync_at");
    expectEqual(second.last_sync_at, second.last_pull_at, "the pull directory's last_sync_at");
    return "each directory stamps its own direction, and last_sync_at follows it";
  });

  await step(
    "cleanup",
    () => {
      if (options.keep) {
        throw new Skip(`kept ${path.relative(ROOT, WORK)} and the signed-in session`);
      }
      if (!state.configured || !BINARY || !existsSync(BINARY)) {
        // Preflight failed, so nothing was configured and there is no session to sign out.
        rmSync(WORK, { recursive: true, force: true });
        return "the run never configured a directory; local directories removed";
      }
      rigel(["cloud", "logout", "--json"], { dir: DIRECTORY_A, allowFailure: true });
      let removed = "local directories removed";
      if (state.uid && state.firestore !== "missing") {
        const run = firebase([
          "firestore:delete",
          "--project",
          options.project,
          "--recursive",
          `users/${state.uid}`,
          "--force",
        ]);
        removed = run.ok
          ? `users/${state.uid} removed, local directories removed`
          : "local directories removed; the Firestore subtree is still there";
      }
      rmSync(WORK, { recursive: true, force: true });
      return removed;
    },
    // A failed step stops the checks, not the cleanup: never leave the account's subtree behind.
    { force: true },
  );

  const failed = state.results.filter((result) => result.status === "fail");
  const skipped = state.results.filter((result) => result.status === "skip");
  const passed = state.results.filter((result) => result.status === "pass");

  if (options.json) {
    console.log(
      JSON.stringify(
        {
          ok: failed.length === 0,
          project_id: options.project,
          email: options.email,
          uid: state.uid,
          checks: state.results,
        },
        null,
        2,
      ),
    );
  } else {
    console.log(
      `\n${passed.length} passed · ${skipped.length} skipped · ${failed.length} failed`,
    );
    if (state.uid && !options.keep && !options.project.startsWith("demo-")) {
      console.log(
        `The test account stays in Firebase Authentication: delete it at\n  https://console.firebase.google.com/project/${options.project}/authentication/users`,
      );
    }
    if (state.uid && options.keep) {
      console.log(
        `Left for inspection: ${path.relative(ROOT, WORK)}, users/${state.uid} in Firestore, and the keychain session.`,
      );
    }
  }
  return failed.length === 0 ? 0 : 1;
}

try {
  process.exitCode = await main();
} catch (error) {
  console.error(`rigel cloud verify: ${error.message}`);
  process.exitCode = 2;
}
