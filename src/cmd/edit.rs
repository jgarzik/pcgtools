//
// cmd/edit.rs -- edit and fmt subcommands
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use super::CmdResult;
use pcgtools::lst::edit::{apply, find_record, format, Edit};
use pcgtools::lst::LstFile;
use pcgtools::text::{Eol, TextFile, TextLine};
use pcgtools::Error;
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct EditArgs<'a> {
    pub file: &'a Path,
    pub object: &'a str,
    pub set: &'a [String],
    pub add: &'a [String],
    pub remove: &'a [String],
    pub rename: Option<&'a str>,
    pub delete: bool,
    pub any_op: bool,
    pub output: Option<&'a Path>,
    pub in_place: bool,
}

fn write_out(bytes: &[u8], output: Option<&Path>, in_place: Option<&Path>) -> pcgtools::Result<()> {
    let dest = output.or(in_place);
    match dest {
        Some(p) => std::fs::write(p, bytes).map_err(|e| Error::io(p, e)),
        None => std::io::stdout()
            .write_all(bytes)
            .map_err(|e| Error::io(Path::new("<stdout>"), e)),
    }
}

pub fn edit(a: EditArgs<'_>) -> CmdResult {
    let mut edits = Vec::new();
    for s in a.set {
        let (t, v) = Edit::token(s).map_err(Error::Other)?;
        edits.push(Edit::Set(t, v));
    }
    for s in a.add {
        let (t, v) = Edit::token(s).map_err(Error::Other)?;
        edits.push(Edit::Add(t, v));
    }
    for t in a.remove {
        edits.push(Edit::Remove(t.clone()));
    }
    if let Some(n) = a.rename {
        edits.push(Edit::Rename(n.to_string()));
    }
    if a.delete {
        edits.push(Edit::Delete);
    }
    if edits.is_empty() {
        return Err(Error::Other("no edits given".into()));
    }

    let mut lf = LstFile::read(a.file)?;
    let Some(lines) = find_record(&lf, a.object, a.any_op) else {
        eprintln!("{}: no record named '{}'", a.file.display(), a.object);
        return Ok(false);
    };
    for msg in apply(&mut lf, &lines, &edits) {
        eprintln!("{}: {}", a.object, msg);
    }
    write_out(&lf.encode(), a.output, a.in_place.then_some(a.file))?;
    Ok(true)
}

fn format_pcc(tf: &TextFile) -> TextFile {
    let mut lines: Vec<TextLine> = tf
        .lines
        .iter()
        .map(|l| TextLine {
            text: l.text.trim().to_string(),
            eol: Eol::Lf,
            latin1: false,
        })
        .collect();
    while lines.last().is_some_and(|l| l.text.is_empty()) {
        lines.pop();
    }
    TextFile { bom: false, lines }
}

/// Format files.  Returns false with --check if any file would change.
pub fn fmt(files: &[PathBuf], check: bool, write: bool) -> CmdResult {
    let mut clean = true;
    for f in files {
        let orig = std::fs::read(f).map_err(|e| Error::io(f, e))?;
        let tf = TextFile::decode(&orig);
        let is_pcc = f.extension().is_some_and(|e| e.eq_ignore_ascii_case("pcc"));
        let out = if is_pcc {
            format_pcc(&tf).encode()
        } else {
            format(&LstFile::from_text(&tf)).encode()
        };
        if out == orig {
            continue;
        }
        clean = false;
        if check {
            println!("{}", f.display());
        } else if write {
            std::fs::write(f, &out).map_err(|e| Error::io(f, e))?;
        } else {
            write_out(&out, None, None)?;
        }
    }
    Ok(!check || clean)
}
