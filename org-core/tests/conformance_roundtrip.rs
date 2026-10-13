use std::fs;
use std::path::{Path, PathBuf};

use org_core::{IndexedDocument, Settings};

fn org_files(root: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).expect("read fixture directory") {
        let entry = entry.expect("read fixture entry");
        let path = entry.path();
        if path.is_dir() {
            org_files(&path, files);
        } else if path.extension().and_then(|value| value.to_str()) == Some("org") {
            files.push(path);
        }
    }
}

#[test]
fn pinned_fixture_corpus_round_trips_byte_for_byte() {
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test-utils/fixtures");
    let mut files = Vec::new();
    org_files(&fixture_root, &mut files);
    if let Some(value) = std::env::var_os("AGD_CONFORMANCE_CORPUS") {
        let root = PathBuf::from(value);
        let root = if root.is_absolute() {
            root
        } else {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(root)
        };
        org_files(&root, &mut files);
    }
    files.sort();
    assert!(!files.is_empty(), "fixture corpus must not be empty");

    for path in files {
        let original = fs::read(&path).expect("read fixture");
        let document = IndexedDocument::parse(path.clone(), original.clone(), &Settings::default())
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(document.source().as_bytes(), original, "{}", path.display());
    }
}
