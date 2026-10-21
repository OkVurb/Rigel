//! Fingerprinting and near-duplicate clustering.
//!
//! `docs/skill-intelligence.md` asks for three independent signals; Phase 2 ships the one
//! that needs no extra storage: a 64-bit SimHash over the body once markdown syntax, code
//! fences and whitespace are stripped. Two rows join a cluster when their fingerprints are
//! within [`HAMMING_THRESHOLD`] bits, and clusters are connected components of that relation.
//!
//! The UI never hides a row because of this (P2.6): clustering adds a "3 variants" line and a
//! disclosure under the row, because a ranking you cannot inspect is a ranking nobody can
//! falsify.
//!
//! Candidate search is exact, not sampled: with at most 3 differing bits, splitting the
//! fingerprint into 4 bands means at least one band is identical (pigeonhole), so bucketing
//! by band finds every pair below the threshold without an O(n²) sweep.

use std::collections::{BTreeMap, HashMap};

use sha2::{Digest, Sha256};

pub const HAMMING_THRESHOLD: u32 = 3;
const BANDS: usize = 4;

/// 64-bit SimHash over the normalised body's tokens.
///
/// Unigrams, not shingles: near-duplicate skills are renames, reformats and small edits, and
/// a shingled fingerprint moves too far when one sentence is appended — which would make the
/// threshold meaningful only for byte-identical files.
pub fn simhash(body: &str) -> u64 {
    let tokens = normalise(body);
    if tokens.is_empty() {
        return 0;
    }

    let mut weights = [0_i32; 64];
    for token in &tokens {
        let hash = hash64(token);
        for (bit, weight) in weights.iter_mut().enumerate() {
            if hash & (1 << bit) != 0 {
                *weight += 1;
            } else {
                *weight -= 1;
            }
        }
    }

    let mut fingerprint = 0_u64;
    for (bit, weight) in weights.iter().enumerate() {
        if *weight > 0 {
            fingerprint |= 1 << bit;
        }
    }
    fingerprint
}

/// Lowercase, markdown-stripped tokens. Code fences are dropped whole: a shared boilerplate
/// snippet should not make two unrelated skills look like variants.
fn normalise(body: &str) -> Vec<String> {
    let mut out = String::with_capacity(body.len());
    let mut in_fence = false;

    for line in body.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        // HTML comments and common markdown punctuation carry no meaning for a fingerprint.
        let line = strip_html_comments(line);
        for c in line.chars() {
            if c.is_alphanumeric() || c.is_whitespace() {
                out.push(c.to_ascii_lowercase());
            } else {
                out.push(' ');
            }
        }
        out.push('\n');
    }

    out.split_whitespace().map(ToOwned::to_owned).collect()
}

fn strip_html_comments(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

fn hash64(input: &str) -> u64 {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    u64::from_be_bytes(digest[..8].try_into().expect("sha256 is 32 bytes"))
}

pub fn hamming(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Cluster id assigned to one fingerprint. `None` means "no fingerprint, own row".
///
/// The id is derived from the smallest member id in the cluster rather than from the
/// fingerprint, so re-fingerprinting a body after an edit does not renumber the cluster the
/// user is looking at unless membership actually changed.
pub fn cluster_id(member_ids: &[&str]) -> String {
    let smallest = member_ids.iter().min().copied().unwrap_or_default();
    format!("cl-{}", &hash_hex(smallest)[..12])
}

fn hash_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Assign every `(id, fingerprint)` pair a cluster id.
pub fn assign(rows: &[(String, u64)]) -> BTreeMap<String, String> {
    let mut parent: Vec<usize> = (0..rows.len()).collect();

    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }

    let mut buckets: Vec<HashMap<u64, Vec<usize>>> = vec![HashMap::new(); BANDS];
    for (index, (_, fingerprint)) in rows.iter().enumerate() {
        for (band, bucket) in buckets.iter_mut().enumerate() {
            bucket
                .entry(band_of(*fingerprint, band))
                .or_default()
                .push(index);
        }
    }

    for bucket in &buckets {
        for members in bucket.values() {
            for i in 0..members.len() {
                for j in (i + 1)..members.len() {
                    let (a, b) = (members[i], members[j]);
                    if hamming(rows[a].1, rows[b].1) <= HAMMING_THRESHOLD {
                        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                        if ra != rb {
                            parent[ra] = rb;
                        }
                    }
                }
            }
        }
    }

    let mut groups: HashMap<usize, Vec<String>> = HashMap::new();
    for (index, (id, _)) in rows.iter().enumerate() {
        let root = find(&mut parent, index);
        groups.entry(root).or_default().push(id.clone());
    }

    let mut assigned = BTreeMap::new();
    for members in groups.values() {
        let refs: Vec<&str> = members.iter().map(String::as_str).collect();
        let id = cluster_id(&refs);
        for member in members {
            assigned.insert(member.clone(), id.clone());
        }
    }
    assigned
}

fn band_of(fingerprint: u64, band: usize) -> u64 {
    let width = 64 / BANDS as u32;
    let shift = (band as u32) * width;
    (fingerprint >> shift) & ((1 << width) - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = "# Code review\n\nRead the diff, name the risk, propose the smallest fix.\n\nCheck for tests and error handling before style.\n";

    /// A body of the length real skills ship. Fingerprints over twenty tokens are noisy:
    /// changing one word can move five bits, which says nothing about the algorithm.
    const LONG: &str = "# Code review\n\nRead the diff before you read the description, because the description is usually aspirational and the diff is what will actually run. Name the specific risk you see instead of describing the general area, and propose the smallest change that removes it. Check that new branches have tests, that error paths return something a caller can act on, and that the change does not quietly widen a public interface. Leave style to the formatter. If the change is correct but hard to follow, say which part you had to read twice and why. Never approve a change you have not understood, and never reject one because it is long. Finish with the one thing that would make you reject it if someone else wrote it.\n";

    #[test]
    fn the_same_body_fingerprints_the_same_way() {
        assert_eq!(simhash(BODY), simhash(BODY));
        assert_ne!(
            simhash(BODY),
            simhash("# Deploy\n\nUpload the artefact to production.")
        );
    }

    #[test]
    fn formatting_changes_do_not_move_the_fingerprint() {
        let reformatted = "# Code review\nRead  the diff, name the risk,\npropose the smallest fix.\nCheck for tests and error handling before style.";
        assert_eq!(simhash(BODY), simhash(reformatted));

        let recased = BODY.to_uppercase();
        assert_eq!(simhash(BODY), simhash(&recased));
    }

    #[test]
    fn code_fences_are_ignored() {
        let with_fence = format!("{BODY}\n```bash\nrm -rf /\n```\n");
        assert_eq!(simhash(BODY), simhash(&with_fence));
    }

    #[test]
    fn an_edited_copy_lands_in_the_same_cluster() {
        // One word changed and one sentence appended: what a maintained fork looks like.
        let edited = format!(
            "{}Always link the line numbers you mean.\n",
            LONG.replace("quietly widen", "silently widen")
        );
        let distance = hamming(simhash(LONG), simhash(&edited));
        assert!(
            distance <= HAMMING_THRESHOLD,
            "distance {distance} exceeds the threshold"
        );

        let rows = vec![
            ("a".to_string(), simhash(LONG)),
            ("b".to_string(), simhash(&edited)),
            (
                "c".to_string(),
                simhash("# Deploy\n\nTag the release, then upload the artefact to production."),
            ),
        ];
        let assigned = assign(&rows);

        assert_eq!(assigned["a"], assigned["b"]);
        assert_ne!(assigned["a"], assigned["c"]);
        assert_eq!(assigned["a"], cluster_id(&["a", "b"]));
    }

    #[test]
    fn every_row_gets_an_assignment_even_with_no_fingerprint() {
        let rows = vec![("solo".to_string(), 0_u64), ("other".to_string(), u64::MAX)];
        let assigned = assign(&rows);
        assert_eq!(assigned.len(), 2);
        assert_ne!(assigned["solo"], assigned["other"]);
    }

    #[test]
    fn threshold_matches_the_documented_three_bits() {
        assert_eq!(hamming(0b1011, 0b1011), 0);
        assert_eq!(hamming(0b1011, 0b1110), 2);
        assert_eq!(hamming(0b1011, 0b0101), 3);
        assert_eq!(HAMMING_THRESHOLD, 3);
    }
}
