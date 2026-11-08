#!/usr/bin/env node
// The fast CI gate: one version in the three build files, and a website that never advertises a
// release newer than the tree. `scripts/lib/versions.mjs` holds the rule so the release check
// cannot disagree with this one.

import { assertVersions } from "./lib/versions.mjs";

try {
  const { version, note } = assertVersions(process.cwd());
  if (note) console.log(note);
  console.log(`version ${version} agrees across the build files`);
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
