/**
 * D141: a small line diff for the changed-pin card.
 *
 * Skill bodies are prose, not source, and the card only needs an honest reading of what moved.
 * The LCS is bounded: past a point the middle is shown as a replaced block with a gap marker
 * rather than allocating a matrix for a book-sized body.
 */

export type SkillDiffLine = {
  kind: "same" | "add" | "del" | "gap";
  text: string;
  oldLine: number | null;
  newLine: number | null;
};

const MAX_CELLS = 250_000;

export function bodyDiff(before: string, after: string): SkillDiffLine[] {
  const a = before.split("\n");
  const b = after.split("\n");

  let start = 0;
  while (start < a.length && start < b.length && a[start] === b[start]) start += 1;

  let endA = a.length;
  let endB = b.length;
  while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
    endA -= 1;
    endB -= 1;
  }

  const rows: SkillDiffLine[] = [];
  for (let index = 0; index < start; index += 1) {
    rows.push({
      kind: "same",
      text: a[index],
      oldLine: index + 1,
      newLine: index + 1,
    });
  }

  const n = endA - start;
  const m = endB - start;
  if (n * m > MAX_CELLS) {
    for (let index = 0; index < n; index += 1) {
      rows.push({
        kind: "del",
        text: a[start + index],
        oldLine: start + index + 1,
        newLine: null,
      });
    }
    for (let index = 0; index < m; index += 1) {
      rows.push({
        kind: "add",
        text: b[start + index],
        oldLine: null,
        newLine: start + index + 1,
      });
    }
  } else if (n > 0 || m > 0) {
    // dp[i][j] is the length of the longest common suffix of a[start+i..endA] and
    // b[start+j..endB]. Uint32Array keeps a large matrix compact.
    const width = m + 1;
    const dp = new Uint32Array((n + 1) * width);
    for (let i = n - 1; i >= 0; i -= 1) {
      for (let j = m - 1; j >= 0; j -= 1) {
        dp[i * width + j] =
          a[start + i] === b[start + j]
            ? dp[(i + 1) * width + j + 1] + 1
            : Math.max(dp[(i + 1) * width + j], dp[i * width + j + 1]);
      }
    }

    let i = 0;
    let j = 0;
    while (i < n && j < m) {
      if (a[start + i] === b[start + j]) {
        rows.push({
          kind: "same",
          text: a[start + i],
          oldLine: start + i + 1,
          newLine: start + j + 1,
        });
        i += 1;
        j += 1;
      } else if (dp[(i + 1) * width + j] >= dp[i * width + j + 1]) {
        rows.push({
          kind: "del",
          text: a[start + i],
          oldLine: start + i + 1,
          newLine: null,
        });
        i += 1;
      } else {
        rows.push({
          kind: "add",
          text: b[start + j],
          oldLine: null,
          newLine: start + j + 1,
        });
        j += 1;
      }
    }
    while (i < n) {
      rows.push({
        kind: "del",
        text: a[start + i],
        oldLine: start + i + 1,
        newLine: null,
      });
      i += 1;
    }
    while (j < m) {
      rows.push({
        kind: "add",
        text: b[start + j],
        oldLine: null,
        newLine: start + j + 1,
      });
      j += 1;
    }
  }

  if (a.length - endA > 0 || b.length - endB > 0) {
    rows.push({
      kind: "gap",
      text: `${Math.max(a.length - endA, b.length - endB)} unchanged line(s) follow`,
      oldLine: null,
      newLine: null,
    });
    for (let index = 0; index < a.length - endA; index += 1) {
      rows.push({
        kind: "same",
        text: a[endA + index],
        oldLine: endA + index + 1,
        newLine: endB + index + 1,
      });
    }
  }

  return rows;
}
