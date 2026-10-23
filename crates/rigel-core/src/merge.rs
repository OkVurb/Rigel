//! A line-level three-way merge (D79).
//!
//! Three versions of one file: `base`, the content the claimant started from; `ours`, the
//! claimant's version; `theirs`, the version another agent wrote over it. The merge is the shape
//! git uses — a region neither side changed passes through, a change on one side wins where the
//! other side left the region alone, an edit both sides made to the same lines becomes a conflict
//! block a human edits.
//!
//! Nothing here writes. The result is text, and the caller hands it to [`crate::PatchPipeline`],
//! which is the only writer (§8.4). That split is why a merge can be shown, edited and thrown
//! away without touching the working tree.
//!
//! Line endings are normalised for comparison and re-emitted in `ours`'s style, so a CRLF file
//! does not turn into a mixed-ending one the first time an agent's hunk crosses a conflict.

use serde::Serialize;

/// The opening line of a conflict block.
pub const OURS_MARKER: &str = "<<<<<<< ours";
/// The line between the two sides.
pub const SEPARATOR: &str = "=======";
/// The closing line of a conflict block.
pub const THEIRS_MARKER: &str = ">>>>>>> theirs";

/// Larger inputs are refused rather than merged with a quadratic table the size of the file.
/// The cap is per side, in lines; a source file that needs more is a file to merge by hand.
pub const MAX_LINES: usize = 4000;
/// The LCS table is `(base + 1) × (other + 1)` cells of `u32`; four million is 16 MB.
const MAX_CELLS: usize = 4_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MergeResult {
    /// The merged text. Conflict blocks are marked with [`OURS_MARKER`], [`SEPARATOR`] and
    /// [`THEIRS_MARKER`].
    pub merged: String,
    /// How many blocks need a human.
    pub conflicts: usize,
    /// `conflicts == 0`: the merge can be written without reading it.
    pub clean: bool,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MergeError {
    #[error("a side has {0} lines, more than the {MAX_LINES} this merge will align; pick a side instead")]
    TooLarge(usize),
}

/// Three-way merge `ours` and `theirs` over their common `base`.
pub fn three_way(base: &str, ours: &str, theirs: &str) -> Result<MergeResult, MergeError> {
    let base = Text::parse(base);
    let ours = Text::parse(ours);
    let theirs = Text::parse(theirs);

    // Emit the style `ours` was written in; if it had no line at all, take either neighbour.
    let newline = if ours.crlf || (ours.lines.is_empty() && (theirs.crlf || base.crlf)) {
        "\r\n"
    } else {
        "\n"
    };

    // A line is stable when both sides still have it at the same place: it cannot conflict.
    let ours_match = matches(&base.lines, &ours.lines)?;
    let theirs_match = matches(&base.lines, &theirs.lines)?;
    let mut ours_at = vec![None; base.lines.len()];
    for (base_index, ours_index) in ours_match {
        ours_at[base_index] = Some(ours_index);
    }
    let mut theirs_at = vec![None; base.lines.len()];
    for (base_index, theirs_index) in theirs_match {
        theirs_at[base_index] = Some(theirs_index);
    }

    let mut out: Vec<String> = Vec::new();
    let mut conflicts = 0;
    let mut tail = base.trailing_newline;
    let mut base_pos = 0;
    let mut ours_pos = 0;
    let mut theirs_pos = 0;

    loop {
        let next_stable = (base_pos..base.lines.len())
            .find(|&index| ours_at[index].is_some() && theirs_at[index].is_some());
        let (base_end, ours_end, theirs_end) = match next_stable {
            Some(index) => (
                index,
                ours_at[index].expect("stable index has an ours line"),
                theirs_at[index].expect("stable index has a theirs line"),
            ),
            None => (base.lines.len(), ours.lines.len(), theirs.lines.len()),
        };

        let base_region = &base.lines[base_pos..base_end];
        let ours_region = &ours.lines[ours_pos..ours_end];
        let theirs_region = &theirs.lines[theirs_pos..theirs_end];

        if ours_region == theirs_region {
            // Both sides agree, whether or not they agree with the base.
            if !ours_region.is_empty() {
                out.extend_from_slice(ours_region);
                tail = ours.trailing_newline;
            }
        } else if ours_region == base_region {
            if !theirs_region.is_empty() {
                out.extend_from_slice(theirs_region);
                tail = theirs.trailing_newline;
            }
        } else if theirs_region == base_region {
            if !ours_region.is_empty() {
                out.extend_from_slice(ours_region);
                tail = ours.trailing_newline;
            }
        } else {
            out.push(OURS_MARKER.to_string());
            out.extend_from_slice(ours_region);
            out.push(SEPARATOR.to_string());
            out.extend_from_slice(theirs_region);
            out.push(THEIRS_MARKER.to_string());
            conflicts += 1;
            tail = true;
        }

        match next_stable {
            Some(index) => {
                out.push(base.lines[index].clone());
                tail = base.trailing_newline;
                base_pos = index + 1;
                ours_pos = ours_end + 1;
                theirs_pos = theirs_end + 1;
            }
            None => break,
        }
    }

    // A final newline is not a line, so a stable tail would otherwise keep the base's. When both
    // sides agree about it, theirs is the answer.
    if ours.trailing_newline == theirs.trailing_newline && !out.is_empty() {
        tail = ours.trailing_newline;
    }

    // An empty merged file is a real answer (both sides deleted everything); it does not get a
    // newline just because one side ended with one.
    let merged = if out.is_empty() {
        String::new()
    } else {
        let mut merged = out.join(newline);
        if tail {
            merged.push_str(newline);
        }
        merged
    };

    Ok(MergeResult {
        merged,
        conflicts,
        clean: conflicts == 0,
    })
}

/// One file as lines, without their terminators, plus what the last line ended with.
#[derive(Debug)]
struct Text {
    lines: Vec<String>,
    trailing_newline: bool,
    crlf: bool,
}

impl Text {
    fn parse(text: &str) -> Self {
        if text.is_empty() {
            return Self {
                lines: Vec::new(),
                trailing_newline: false,
                crlf: false,
            };
        }
        let trailing_newline = text.ends_with('\n');
        let body = if trailing_newline {
            &text[..text.len() - 1]
        } else {
            text
        };
        let mut crlf = false;
        let lines = body
            .split('\n')
            .map(|line| match line.strip_suffix('\r') {
                Some(stripped) => {
                    crlf = true;
                    stripped.to_string()
                }
                None => line.to_string(),
            })
            .collect();
        Self {
            lines,
            trailing_newline,
            crlf,
        }
    }
}

/// The equal lines shared by `a` and `b`, in order, as `(a index, b index)` pairs.
///
/// The common prefix and suffix are matched directly; only the middle goes through the LCS
/// table, which is what keeps an ordinary edit cheap.
fn matches(a: &[String], b: &[String]) -> Result<Vec<(usize, usize)>, MergeError> {
    if a.len() > MAX_LINES || b.len() > MAX_LINES {
        return Err(MergeError::TooLarge(a.len().max(b.len())));
    }

    let mut prefix = 0;
    while prefix < a.len() && prefix < b.len() && a[prefix] == b[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < a.len() - prefix
        && suffix < b.len() - prefix
        && a[a.len() - 1 - suffix] == b[b.len() - 1 - suffix]
    {
        suffix += 1;
    }

    let mut pairs: Vec<(usize, usize)> = (0..prefix).map(|index| (index, index)).collect();
    let a_mid = &a[prefix..a.len() - suffix];
    let b_mid = &b[prefix..b.len() - suffix];
    let (rows, columns) = (a_mid.len(), b_mid.len());

    if rows > 0 && columns > 0 {
        let cells = (rows + 1).saturating_mul(columns + 1);
        if cells > MAX_CELLS {
            return Err(MergeError::TooLarge(a.len().max(b.len())));
        }
        // `table[i * columns + j]` is the LCS length of the two suffixes from (i, j).
        let width = columns + 1;
        let mut table = vec![0u32; cells];
        for i in (0..rows).rev() {
            for j in (0..columns).rev() {
                table[i * width + j] = if a_mid[i] == b_mid[j] {
                    table[(i + 1) * width + j + 1] + 1
                } else {
                    table[(i + 1) * width + j].max(table[i * width + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < rows && j < columns {
            if a_mid[i] == b_mid[j] {
                pairs.push((prefix + i, prefix + j));
                i += 1;
                j += 1;
            } else if table[(i + 1) * width + j] >= table[i * width + j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    }

    for index in 0..suffix {
        pairs.push((a.len() - suffix + index, b.len() - suffix + index));
    }
    Ok(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merge(base: &str, ours: &str, theirs: &str) -> MergeResult {
        three_way(base, ours, theirs).expect("small file")
    }

    const BASE: &str = "one\ntwo\nthree\nfour\nfive\n";

    #[test]
    fn edits_to_different_lines_merge_without_a_conflict() {
        let ours = "ONE\ntwo\nthree\nfour\nfive\n";
        let theirs = "one\ntwo\nthree\nfour\nFIVE\n";
        let result = merge(BASE, ours, theirs);
        assert_eq!(result.merged, "ONE\ntwo\nthree\nfour\nFIVE\n");
        assert_eq!(result.conflicts, 0);
        assert!(result.clean);
    }

    #[test]
    fn the_same_edit_on_both_sides_is_not_a_conflict() {
        let both = "one\nTWO\nthree\nfour\nfive\n";
        let result = merge(BASE, both, both);
        assert_eq!(result.merged, both);
        assert_eq!(result.conflicts, 0);
    }

    #[test]
    fn edits_to_the_same_line_become_one_conflict_block() {
        let ours = "one\nTWO from ours\nthree\nfour\nfive\n";
        let theirs = "one\nTWO from theirs\nthree\nfour\nfive\n";
        let result = merge(BASE, ours, theirs);
        assert_eq!(result.conflicts, 1);
        assert!(!result.clean);
        assert_eq!(
            result.merged,
            "one\n<<<<<<< ours\nTWO from ours\n=======\nTWO from theirs\n>>>>>>> theirs\nthree\nfour\nfive\n"
        );
    }

    #[test]
    fn a_side_that_matches_base_yields_to_the_other() {
        // ours deleted a line, theirs did not touch it: the deletion wins.
        let ours = "one\nthree\nfour\nfive\n";
        let result = merge(BASE, ours, BASE);
        assert_eq!(result.merged, ours);
        assert_eq!(result.conflicts, 0);

        // ours added a line, theirs did not touch that place: the addition wins.
        let ours = "one\ntwo\ninserted\nthree\nfour\nfive\n";
        let result = merge(BASE, ours, BASE);
        assert_eq!(result.merged, ours);
        assert_eq!(result.conflicts, 0);
    }

    #[test]
    fn insertions_at_the_same_place_conflict() {
        let ours = "one\ntwo\nfrom ours\nthree\nfour\nfive\n";
        let theirs = "one\ntwo\nfrom theirs\nthree\nfour\nfive\n";
        let result = merge(BASE, ours, theirs);
        assert_eq!(result.conflicts, 1);
        assert!(result.merged.contains("from ours"));
        assert!(result.merged.contains("from theirs"));
        assert!(result.merged.contains(OURS_MARKER));
    }

    #[test]
    fn a_missing_final_newline_is_kept() {
        let result = merge("a\nb", "a\nB", "a\nb");
        assert_eq!(result.merged, "a\nB");

        let result = merge("a\nb", "a\nb\n", "a\nb\n");
        assert_eq!(result.merged, "a\nb\n");
    }

    #[test]
    fn crlf_is_preserved_and_not_mixed_with_lf() {
        let base = "one\r\ntwo\r\nthree\r\n";
        let ours = "ONE\r\ntwo\r\nthree\r\n";
        let theirs = "one\r\ntwo\r\nTHREE\r\n";
        let result = merge(base, ours, theirs);
        assert_eq!(result.merged, "ONE\r\ntwo\r\nTHREE\r\n");
        assert!(!result.merged.contains("\n\n"), "{:?}", result.merged);
    }

    #[test]
    fn whole_file_rewrites_are_one_conflict_not_a_hundred() {
        let ours = "alpha\nbeta\n";
        let theirs = "gamma\ndelta\n";
        let result = merge(BASE, ours, theirs);
        assert_eq!(result.conflicts, 1);
    }

    #[test]
    fn a_file_too_large_to_align_is_refused_rather_than_guessed() {
        let big: String = (0..=MAX_LINES).map(|line| format!("{line}\n")).collect();
        assert_eq!(
            three_way(&big, &big, &big),
            Err(MergeError::TooLarge(MAX_LINES + 1))
        );
    }
}
