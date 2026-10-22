# Index sweep — the 5,000-skill floor

Recorded 22 Oct 2026 (run started 21 Oct), Windows 11, unauthenticated networks.
`rigel skills sweep` in its first real run, against a fresh index (0 skills).

## Command and result

```
rigel skills sweep --skillhub-pages 150 --skillsmp-pages 1
```

| Stage | Rows |
|---|---|
| SkillHub list pages walked (100 cards/request, first 150 pages) | 14,741 |
| SkillsMP search pages walked (50 rows/request, 20 broad queries) | 1,000, then stopped (https://skillsmp.com/api/v1/skills/search?q=claude&limit=50&sortBy=stars&page=5 is rate limited) |
| Same-skill rows collapsed by the metadata dedupe pass | −825 |
| **Unique skills in the index** | **9,996** |

The target was a 5,000-skill floor; 9,996 is just under double it, from two metadata
sources alone. `skills stats` after the run:

```
skills      9996
with body   0
clusters    0
blobs       121 (38668 KB)
```

Search over the full 9,996 rows answers instantaneously (SQLite FTS5, metadata-only
rows are tiny compared to fetched bodies).

## What the sweep does

`skills sweep` is `skills sync` without the single-page limits and without the
chatty printing:

1. Walks SkillHub list pages (`--skillhub-pages × 100 cards` each).
2. Runs each SkillsMP query (`--skillsmp-queries`, 20 broad defaults) up to
   `--skillsmp-pages × 50 rows` per query, clamped 1..50.
3. Ingests rows quietly (no per-page logging; one line per marketplace at the end).
4. Recomputes the SimHash duplicate clusters exactly once.
5. Runs the metadata dedupe pass and prints the count.

Stop summary per marketplace is one line: rows, pages, and the URL that stopped it.

## Notes

- **SkillsMP is aggressive about rate limiting.** 5 pages in, it returns 429s. The
  per-query page depth, not the query count, is what triggers it; a sweep that asks
  for one page per query (the default 20 queries × 50 rows = 1,000) is the reliable
  path. `skills status` then reports the source as `broken` with the last 429 URL,
  which is honest but means the row count for SkillsMP costs more than it looks.
- **SkillHub carries the bulk.** 100 cards/page × 14,741 rows in 150 requests, no
  rate limit hit.
- The dedupe pass collapses rows with identical source+slug. 825 duplicates were
  cross-marketplace copies (the same skill published under both names).
- Everything was unauthenticated. `RIGEL_GITHUB_TOKEN` does not change these two
  marketplaces; setting it only raises GitHub's repo-walk quota.
