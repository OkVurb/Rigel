#!/usr/bin/env node
/**
 * Send one GA4 event over the Measurement Protocol.
 *
 * This is the server-side channel, not the browser tag: gtag.js in `web/index.html` never sees the
 * API secret and does not need it. The secret authenticates *this* script, and anyone holding it
 * can forge events into the property, so it is read from the environment (or a file outside the
 * repository) and is never written to disk by this script, printed, or committed.
 *
 *   GA_API_SECRET=… node scripts/ga-event.mjs sign_up --params '{"method":"google"}'
 *   GA_API_SECRET=… node scripts/ga-event.mjs page_view --client user-42
 *
 * Flags:
 *   --client <id>   a stable id for the person or machine; random when omitted
 *   --params <json> extra event parameters
 *   --measurement   override the measurement id (default G-XCN9DVHGV7)
 *   --dry-run       print the request that would be sent, with the secret redacted
 *
 * The endpoint answers 204 on a valid hit. A 4xx almost always means a bad measurement id or a
 * secret that belongs to another property; Google validates the pair, not the event.
 */
import { randomUUID } from "node:crypto";
import { readFileSync } from "node:fs";

const DEFAULTS = { measurement: "G-XCN9DVHGV7" };

function arg(name, fallback = undefined) {
  const index = process.argv.indexOf(`--${name}`);
  return index === -1 ? fallback : process.argv[index + 1];
}

const event = process.argv[2];
if (!event || event.startsWith("--")) {
  console.error("usage: GA_API_SECRET=… node scripts/ga-event.mjs <event-name> [--client id] [--params json]");
  process.exit(2);
}

const secretFile = arg("secret-file");
const secret = secretFile
  ? readFileSync(secretFile, "utf8").trim()
  : (process.env.GA_API_SECRET || "").trim();
if (!secret) {
  console.error(
    "No API secret. Set GA_API_SECRET, or pass --secret-file with a path outside the repository.\n" +
      "Create one in Google Analytics → Admin → Data streams → your stream → Measurement Protocol API secrets.",
  );
  process.exit(2);
}

const measurement = arg("measurement", process.env.GA_MEASUREMENT_ID || DEFAULTS.measurement);
const clientId = arg("client", randomUUID());

let params = {};
const raw = arg("params");
if (raw) {
  try {
    params = JSON.parse(raw);
  } catch {
    console.error("--params must be a JSON object");
    process.exit(2);
  }
}

const url = `https://www.google-analytics.com/mp/collect?measurement_id=${encodeURIComponent(
  measurement,
)}&api_secret=${encodeURIComponent(secret)}`;

const body = {
  client_id: clientId,
  events: [{ name: event, params: { ...params, engagement_time_msec: 1 } }],
};

if (process.argv.includes("--dry-run")) {
  console.log(`POST ${url.replace(secret, "«secret»")}`);
  console.log(JSON.stringify(body, null, 2));
  process.exit(0);
}

const response = await fetch(url, {
  method: "POST",
  headers: { "Content-Type": "application/json" },
  body: JSON.stringify(body),
});

// The response body is empty by design; the status is the whole answer.
console.log(`${event} → ${response.status} ${response.statusText || ""}`.trim());
if (response.status !== 204 && response.status !== 200) {
  console.error(await response.text());
  process.exit(1);
}
console.log(`  measurement_id ${measurement}, client_id ${clientId}`);
console.log("  Realtime in Google Analytics should show it within a minute.");
