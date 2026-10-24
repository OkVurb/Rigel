#!/usr/bin/env node
/**
 * Write the signed `latest.json` a tagged release publishes.
 *
 * The payload is the exact byte sequence `rigel-core::update::UpdateManifest::signed_bytes`
 * verifies: three lines, then the optional MSI URL and hash as two more lines. The private key is
 * a 32-byte hex seed in RIGEL_UPDATE_SIGNING_KEY (a GitHub Actions secret); only the matching
 * public key is committed in crates/rigel-app/resources/update-pubkey.txt.
 *
 * Usage: node scripts/sign-update-manifest.mjs [output.json]
 */

import { createPrivateKey, sign } from "node:crypto";
import { writeFileSync } from "node:fs";

const output = process.argv[2] ?? "latest.json";
const version = process.env.RIGEL_UPDATE_VERSION?.trim();
const url = process.env.RIGEL_UPDATE_URL?.trim();
const sha256 = process.env.RIGEL_UPDATE_SHA256?.trim().toLowerCase();
const msiUrl = process.env.RIGEL_UPDATE_MSI_URL?.trim() || null;
const msiSha256 = process.env.RIGEL_UPDATE_MSI_SHA256?.trim().toLowerCase() || null;
const notes = process.env.RIGEL_UPDATE_NOTES?.trim() || null;
const privateKey = process.env.RIGEL_UPDATE_SIGNING_KEY?.trim();

if (!version || !url || !sha256) {
  throw new Error("RIGEL_UPDATE_VERSION, RIGEL_UPDATE_URL and RIGEL_UPDATE_SHA256 are required");
}
if (!privateKey) {
  throw new Error("RIGEL_UPDATE_SIGNING_KEY is not set; configure the release signing secret");
}
if (!/^[0-9a-f]{64}$/.test(sha256)) {
  throw new Error("RIGEL_UPDATE_SHA256 must be 64 hex characters");
}
if (Boolean(msiUrl) !== Boolean(msiSha256)) {
  throw new Error("RIGEL_UPDATE_MSI_URL and RIGEL_UPDATE_MSI_SHA256 must be set together");
}
if (msiSha256 && !/^[0-9a-f]{64}$/.test(msiSha256)) {
  throw new Error("RIGEL_UPDATE_MSI_SHA256 must be 64 hex characters");
}
if (!/^[0-9a-f]{64}$/.test(privateKey)) {
  throw new Error("RIGEL_UPDATE_SIGNING_KEY must be a 32-byte hex seed");
}

const lines = [version, url, sha256];
if (msiUrl && msiSha256) lines.push(msiUrl, msiSha256);
const payload = lines.join("\n");

// PKCS#8 wraps an Ed25519 private key as a fixed prefix plus the 32-byte seed.
const prefix = Buffer.from("302e020100300506032b657004220420", "hex");
const key = createPrivateKey({
  key: Buffer.concat([prefix, Buffer.from(privateKey, "hex")]),
  format: "der",
  type: "pkcs8",
});
const signature = sign(null, Buffer.from(payload, "utf8"), key).toString("hex");

const manifest = {
  version,
  url,
  sha256,
  signature,
  notes,
  published_at: Math.floor(Date.now() / 1000),
  ...(msiUrl && msiSha256 ? { msi_url: msiUrl, msi_sha256: msiSha256 } : {}),
};

writeFileSync(output, `${JSON.stringify(manifest, null, 2)}\n`, "utf8");
console.log(`wrote ${output} for ${version}`);
