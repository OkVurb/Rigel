#![no_main]
//! The unified-diff header parser, fed arbitrary text.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        // The parser is allowed to reject a malformed header by dropping it; the invariant the
        // fuzzer checks is that no input makes it panic.
        let _ = rigel_core::patches::parse_diff_hunks(text);
    }
});
