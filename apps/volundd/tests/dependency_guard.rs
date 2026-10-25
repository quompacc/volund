//! Guards for dependency advisories that are excepted because VÖLUND does not
//! reach the affected API (see docs/security/advisory-exceptions.md).

use std::fs;
use std::path::Path;

fn rust_sources(directory: &Path, files: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(directory).expect("read source directory") {
        let path = entry.expect("source entry").path();
        if path.is_dir() {
            rust_sources(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let text = fs::read_to_string(&path).expect("read source file");
            files.push((path.display().to_string(), text));
        }
    }
}

#[test]
fn zip_extraction_helpers_stay_unused() {
    // RUSTSEC-2026-0367/0368/0369 affect only zip's bulk extract functions.
    let mut files = Vec::new();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    rust_sources(&root.join("src"), &mut files);
    rust_sources(&root.join("../../crates"), &mut files);
    assert!(!files.is_empty(), "no Rust sources found");
    for (path, text) in files {
        for forbidden in [".extract(", "ZipStreamReader", "extract_unwrapped_root_dir"] {
            assert!(
                !text.contains(forbidden),
                "{path} uses {forbidden}; review RUSTSEC-2026-0367/0368/0369 first"
            );
        }
    }
}
