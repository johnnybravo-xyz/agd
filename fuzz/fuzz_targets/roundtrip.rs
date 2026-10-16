#![no_main]

use std::path::PathBuf;

use libfuzzer_sys::fuzz_target;
use agd_core::{IndexedDocument, Settings};

fuzz_target!(|data: &[u8]| {
    if std::str::from_utf8(data).is_err() {
        return;
    }
    if let Ok(document) = IndexedDocument::parse(
        PathBuf::from("fuzz.org"),
        data.to_vec(),
        &Settings::default(),
    ) {
        assert_eq!(document.source().as_bytes(), data);
    }
});
