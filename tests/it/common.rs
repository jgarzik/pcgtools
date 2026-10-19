// Shared helpers for integration tests.
//
// Corpus tests read real PCGen data from outside the repository.  They
// run only when PCGEN_ROOT is set (e.g. PCGEN_ROOT=~/tmp/repo/pcg) and
// silently pass otherwise.

use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Whether PCGEN_REQUIRE (a comma-separated list) names `what`: a
/// required input that is missing fails the test instead of skipping it.
pub fn required(what: &str) -> bool {
    std::env::var("PCGEN_REQUIRE")
        .is_ok_and(|v| v.split(',').any(|x| x.trim().eq_ignore_ascii_case(what)))
}

/// Note (or, when required, fail on) a test skipped for missing input.
pub fn skip(what: &str, var: &str) {
    assert!(
        !required(what),
        "{} is required (PCGEN_REQUIRE) but not set or not a directory",
        var
    );
    eprintln!("skipped: {} not set", var);
}

/// Root of the PCGen source checkouts, if configured.
pub fn pcgen_root() -> Option<PathBuf> {
    let root = std::env::var_os("PCGEN_ROOT")
        .map(PathBuf::from)
        .filter(|r| r.is_dir());
    if root.is_none() {
        skip("root", "PCGEN_ROOT");
    }
    root
}

/// Path below PCGEN_ROOT, if it exists.
pub fn corpus_path(rel: &str) -> Option<PathBuf> {
    let p = pcgen_root()?.join(rel);
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

/// Fixture path inside this repository.
pub fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

/// All files below `dir` with one of the given extensions.
pub fn files_with_ext(dir: &Path, exts: &[&str]) -> Vec<PathBuf> {
    WalkDir::new(dir)
        .sort_by_file_name()
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| exts.iter().any(|w| w.eq_ignore_ascii_case(x)))
                .unwrap_or(false)
        })
        .map(|e| e.into_path())
        .collect()
}

/// The standard corpus directories that hold PCC/LST data.
pub fn data_dirs() -> Vec<PathBuf> {
    ["pcgen/data", "pcgen-newsources", "pcgen/system"]
        .iter()
        .filter_map(|r| corpus_path(r))
        .collect()
}
