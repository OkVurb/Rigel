#!/usr/bin/env node
// Can advertising pay for cloud agent runs? Arithmetic, not vibes.
//
// A display impression is worth fractions of a cent; an agent run costs dimes. The point of this
// script is to show the ratio before anyone builds a queue, a sandbox and a billing system on top
// of a number that never worked.
//
// usage:
//   node scripts/cloud-economics.mjs
//   node scripts/cloud-economics.mjs --cpm 8 --run 0.05 --per-session 3

const args = process.argv.slice(2);
const value = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i === -1 ? fallback : Number(args[i + 1]);
};

const CPM = value("cpm", 4); // dollars per thousand impressions
const RUN_USD = value("run", 0.18); // one agent run, all tokens
const PER_SESSION = value("per-session", 2); // ad slots a user actually sees per session
const FILL = value("fill", 0.6); // share of slots an ad network actually fills

const perImpression = CPM / 1000;
const impressionsPerRun = RUN_USD / (perImpression * FILL);
const runsPerThousand = 1000 / impressionsPerRun;

const money = (n) => `$${n.toFixed(4)}`;

console.log(`CPM $${CPM} · fill ${(FILL * 100).toFixed(0)}% · run $${RUN_USD.toFixed(2)}`);
console.log(`one impression earns ${money(perImpression * FILL)}`);
console.log(`one run needs ${impressionsPerRun.toFixed(1)} impressions`);
console.log(`1,000 impressions fund ${runsPerThousand.toFixed(1)} runs`);
console.log(
  `at ${PER_SESSION} slots per session, 1,000 impressions fund ${(
    runsPerThousand * PER_SESSION
  ).toFixed(1)} user sessions`,
);

console.log("\nbreak-even impressions per run");
console.log("run cost | CPM $1 | CPM $4 | CPM $12");
for (const run of [0.05, 0.18, 0.6]) {
  const cells = [1, 4, 12].map((cpm) => ((run / ((cpm / 1000) * FILL)) | 0).toString().padStart(7));
  console.log(`$${run.toFixed(2)}`.padEnd(9) + "|" + cells.join("|"));
}

console.log("\nwhat the same money buys as a sponsor, not an impression");
// A sponsor buying "1,000 runs" is a single line item a human can approve, and it does not
// require tracking anybody. These are the numbers to take to a sponsor instead of a CPM.
for (const price of [500, 2000, 5000]) {
  console.log(`$${price}/month → ${(price / RUN_USD).toFixed(0)} runs (${(price / RUN_USD / 30).toFixed(0)}/day)`);
}
