#![no_main]
//! A project `.mcp.json` body, fed arbitrary text.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = rigel_mcp::project_mcp::parse(text);
    }
});
