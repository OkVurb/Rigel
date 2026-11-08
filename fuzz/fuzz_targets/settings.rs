#![no_main]
//! The update settings file, fed arbitrary text.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = rigel_core::update::parse_settings(text);
    }
});
