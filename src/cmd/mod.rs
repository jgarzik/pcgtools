//
// cmd/mod.rs -- CLI subcommand implementations
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

pub mod campaign;
pub mod misc;

use pcgtools::Diagnostics;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub type CmdResult = pcgtools::Result<bool>;

/// Print a value as pretty JSON.
pub fn print_json<T: serde::Serialize>(v: &T) {
    println!(
        "{}",
        serde_json::to_string_pretty(v).expect("serializable value")
    );
}

/// Print diagnostics to stderr, returning true if there were errors.
pub fn report(diags: &Diagnostics) -> bool {
    for d in &diags.items {
        eprintln!("{}", d);
    }
    diags.count(pcgtools::Severity::Error) > 0
}

fn has_ext(p: &Path, exts: &[&str]) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| exts.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// Expand directories into the data files below them.
pub fn data_files(paths: &[PathBuf], exts: &[&str]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            for ent in WalkDir::new(p).sort_by_file_name().into_iter().flatten() {
                if ent.file_type().is_file() && has_ext(ent.path(), exts) {
                    out.push(ent.into_path());
                }
            }
        } else {
            out.push(p.clone());
        }
    }
    out
}
