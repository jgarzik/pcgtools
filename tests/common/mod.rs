// Shared helpers for integration tests.
//
// Corpus tests read real PCGen data from outside the repository.  They
// run only when PCGEN_ROOT is set (e.g. PCGEN_ROOT=~/tmp/repo/pcg) and
// silently pass otherwise.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Root of the PCGen source checkouts, if configured.
pub fn pcgen_root() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os("PCGEN_ROOT")?);
    if root.is_dir() {
        Some(root)
    } else {
        None
    }
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
