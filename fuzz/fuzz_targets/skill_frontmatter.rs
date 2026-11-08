#![no_main]
//! A SKILL.md header and body, fed arbitrary text.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let parsed = rigel_core::skills::frontmatter::parse(text);
        // The parser never fails, and the body must stay a subset of its input.
        assert!(parsed.body.len() <= text.len());
    }
});
