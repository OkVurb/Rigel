//! The frontmatter parser, ported from `prototype/src/github.ts`.
//!
//! A SKILL.md starts with an optional `---` block holding `name`, `description`, sometimes
//! `license` and `allowed-tools`. The parser deliberately handles only the subset the
//! ecosystem actually uses — single-line values, quoted values, folded (`>`) and literal
//! (`|`) block scalars, and dash lists — and treats everything else as prose the rules can
//! scan. It never fails: a malformed header yields no metadata and the whole file as body.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Frontmatter {
    /// Flat string map. Block scalars and lists are joined with single spaces, which is all
    /// any consumer needs: `description` is read as prose, `allowed-tools` is split later.
    pub data: BTreeMap<String, String>,
    pub body: String,
}

impl Frontmatter {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.data.get(key).map(String::as_str)
    }

    /// `allowed-tools` is either comma- or space-separated in the wild.
    pub fn allowed_tools(&self) -> Option<Vec<String>> {
        let raw = self.get("allowed-tools")?;
        let tools: Vec<String> = raw
            .split([',', ' ', '\t', '\n'])
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(ToOwned::to_owned)
            .collect();
        if tools.is_empty() { None } else { Some(tools) }
    }
}

pub fn parse(src: &str) -> Frontmatter {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);

    let Some((header, body_start)) = split_header(src) else {
        return Frontmatter {
            data: BTreeMap::new(),
            body: src.to_string(),
        };
    };

    let mut data = BTreeMap::new();
    let mut key: Option<String> = None;
    let mut buffer: Vec<String> = Vec::new();

    let flush = |key: &mut Option<String>,
                 buffer: &mut Vec<String>,
                 data: &mut BTreeMap<String, String>| {
        if let Some(name) = key.take() {
            let value = buffer
                .iter()
                .map(|part| part.trim())
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            data.insert(name, value);
        }
        buffer.clear();
    };

    for line in header.lines() {
        let line = line.trim_end_matches('\r');

        if let Some((name, raw_value)) = split_key(line) {
            flush(&mut key, &mut buffer, &mut data);
            let value = raw_value.trim();
            if value.is_empty() || value == ">" || value == "|" {
                key = Some(name);
            } else {
                data.insert(name, unquote(value));
            }
            continue;
        }

        if key.is_some() && is_list_item(line) {
            buffer.push(strip_list_marker(line).to_string());
            continue;
        }

        if key.is_some() && line.starts_with([' ', '\t']) && !line.trim().is_empty() {
            buffer.push(line.trim().to_string());
            continue;
        }

        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }

        // A stray line ends any block in progress, exactly as the prototype did.
        flush(&mut key, &mut buffer, &mut data);
    }
    flush(&mut key, &mut buffer, &mut data);

    Frontmatter {
        data,
        body: src[body_start..].to_string(),
    }
}

/// Returns the text between the delimiters and the byte offset where the body starts.
fn split_header(src: &str) -> Option<(&str, usize)> {
    let first_end = src.find('\n')?;
    if src[..first_end].trim_end_matches('\r').trim() != "---" {
        return None;
    }

    let header_start = first_end + 1;
    let mut cursor = header_start;

    while cursor <= src.len() {
        let end = src[cursor..].find('\n').map_or(src.len(), |i| cursor + i);
        let line = src[cursor..end].trim_end_matches('\r').trim();
        if line == "---" {
            let body_start = if end < src.len() { end + 1 } else { end };
            return Some((&src[header_start..cursor], body_start));
        }
        if end == src.len() {
            break;
        }
        cursor = end + 1;
    }

    None
}

fn split_key(line: &str) -> Option<(String, &str)> {
    if line.starts_with([' ', '\t']) {
        return None;
    }
    let colon = line.find(':')?;
    let name = &line[..colon];
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    Some((name.to_string(), &line[colon + 1..]))
}

fn is_list_item(line: &str) -> bool {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed
        .strip_prefix('-')
        .or_else(|| trimmed.strip_prefix('*'))
    {
        return rest.starts_with(' ') || rest.starts_with('\t');
    }
    false
}

fn strip_list_marker(line: &str) -> &str {
    let trimmed = line.trim_start();
    trimmed[1..].trim_start()
}

fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' || first == b'\'') && first == last {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_shape_the_ecosystem_actually_ships() {
        let src = "---\nname: pdf-tools\ndescription: >\n  Extract text from scanned\n  documents and receipts.\nlicense: MIT\nallowed-tools: Read, Write\n---\n# PDF tools\nBody here.\n";
        let parsed = parse(src);

        assert_eq!(parsed.get("name"), Some("pdf-tools"));
        assert_eq!(
            parsed.get("description"),
            Some("Extract text from scanned documents and receipts.")
        );
        assert_eq!(parsed.get("license"), Some("MIT"));
        assert_eq!(
            parsed.allowed_tools(),
            Some(vec!["Read".to_string(), "Write".to_string()])
        );
        assert_eq!(parsed.body, "# PDF tools\nBody here.\n");
    }

    #[test]
    fn quotes_and_dash_lists_are_unwrapped() {
        let parsed =
            parse("---\nname: \"quoted name\"\nallowed-tools:\n  - Bash\n  - Read\n---\nbody");
        assert_eq!(parsed.get("name"), Some("quoted name"));
        assert_eq!(parsed.get("allowed-tools"), Some("Bash Read"));
        assert_eq!(parsed.body, "body");
    }

    #[test]
    fn a_file_without_frontmatter_is_all_body() {
        let parsed = parse("# Just a skill\n\nNo header.\n");
        assert!(parsed.data.is_empty());
        assert_eq!(parsed.body, "# Just a skill\n\nNo header.\n");
    }

    #[test]
    fn crlf_and_a_byte_order_mark_do_not_break_it() {
        let parsed = parse("\u{feff}---\r\nname: crlf\r\n---\r\nbody\r\n");
        assert_eq!(parsed.get("name"), Some("crlf"));
        assert_eq!(parsed.body, "body\r\n");
    }

    #[test]
    fn an_unterminated_header_is_treated_as_prose() {
        let src = "---\nname: never closed\n";
        let parsed = parse(src);
        assert!(parsed.data.is_empty());
        assert_eq!(parsed.body, src);
    }
}
