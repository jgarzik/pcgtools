//
// cmd/misc.rs -- parse, roundtrip, dump subcommands
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use super::{data_files, print_json, CmdResult};
use pcgtools::lst::{Head, LstFile};
use pcgtools::pcc::PccFile;
use pcgtools::text::TextFile;
use serde_json::json;
use std::path::{Path, PathBuf};

pub fn roundtrip(paths: &[PathBuf]) -> CmdResult {
    let mut ok = true;
    let files = data_files(paths, &["pcc", "lst", "pcg"]);
    for f in &files {
        let bytes = std::fs::read(f).map_err(|e| pcgtools::Error::io(f, e))?;
        if TextFile::decode(&bytes).encode() != bytes {
            println!("MISMATCH {}", f.display());
            ok = false;
        }
    }
    println!("{} files checked", files.len());
    Ok(ok)
}

fn is_ext(p: &Path, ext: &str) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

pub fn parse(file: &Path) -> CmdResult {
    if is_ext(file, "pcc") {
        let pcc = PccFile::read(file)?;
        print_json(&pcc);
        return Ok(true);
    }

    let lf = LstFile::read(file)?;
    let recs: Vec<serde_json::Value> = lf
        .records()
        .iter()
        .map(|r| {
            let toks: Vec<serde_json::Value> = r
                .tokens()
                .iter()
                .map(|t| json!({"tag": t.tag, "value": t.value}))
                .collect();
            json!({
                "line": r.line_no(),
                "kind": r.kind(),
                "head": r.head().map(Head::parse),
                "tokens": toks,
            })
        })
        .collect();
    print_json(&recs);
    Ok(true)
}
