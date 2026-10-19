//
// lst/edit.rs -- cell-level edits and canonical formatting
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! Edits change individual cells of a record and leave every other
//! byte of the file alone.  [`format()`] produces the canonical layout
//! instead.

use super::line::{split_token, Head, HeadOp, LineKind, LstFile, LstLine};
use crate::text::Eol;

/// One edit to apply to a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// replace every `TAG:` token with a single `TAG:value`
    /// (appended if the tag was absent)
    Set(String, String),
    /// append `TAG:value`
    Add(String, String),
    /// remove every token with this tag
    Remove(String),
    /// change the object name in the head, keeping any suffix
    Rename(String),
    /// delete the record and its continuation lines
    Delete,
}

impl Edit {
    /// Parse `TAG:value` for Set/Add.
    pub fn token(s: &str) -> Result<(String, String), String> {
        match s.split_once(':') {
            Some((t, v)) if !t.is_empty() => Ok((t.to_string(), v.to_string())),
            _ => Err(format!("expected TAG:value, got '{}'", s)),
        }
    }
}

/// Find the record lines (record line plus continuations) of the
/// object definition named `name` (case-insensitive), optionally
/// including modification heads.
pub fn find_record(lf: &LstFile, name: &str, any_op: bool) -> Option<Vec<usize>> {
    lf.records().into_iter().find_map(|r| {
        let h = Head::parse(r.head()?);
        let ok = h.name.eq_ignore_ascii_case(name) && (any_op || h.op == HeadOp::Define);
        ok.then(|| r.lines.clone())
    })
}

fn cell_tag(cell: &str) -> &str {
    split_token(cell).0
}

/// Apply edits to the record occupying `lines`.  Returns a description
/// of what changed.
pub fn apply(lf: &mut LstFile, lines: &[usize], edits: &[Edit]) -> Vec<String> {
    let mut log = Vec::new();
    let first = lines[0];
    for e in edits {
        match e {
            Edit::Set(tag, value) => {
                let mut placed = false;
                for &li in lines {
                    let line = &mut lf.lines[li];
                    let head = if li == first { line.head_cell() } else { None };
                    let mut i = 0;
                    while i < line.cells.len() {
                        if Some(i) != head && cell_tag(&line.cells[i]) == tag {
                            if placed {
                                line.cells.remove(i);
                                continue;
                            }
                            line.cells[i] = format!("{}:{}", tag, value);
                            placed = true;
                        }
                        i += 1;
                    }
                }
                if !placed {
                    lf.lines[*lines.last().unwrap()]
                        .cells
                        .push(format!("{}:{}", tag, value));
                }
                log.push(format!("set {}:{}", tag, value));
            }
            Edit::Add(tag, value) => {
                lf.lines[*lines.last().unwrap()]
                    .cells
                    .push(format!("{}:{}", tag, value));
                log.push(format!("added {}:{}", tag, value));
            }
            Edit::Remove(tag) => {
                let mut n = 0;
                for &li in lines {
                    let line = &mut lf.lines[li];
                    let head = if li == first { line.head_cell() } else { None };
                    let mut i = 0;
                    while i < line.cells.len() {
                        if Some(i) != head && cell_tag(&line.cells[i]) == tag {
                            line.cells.remove(i);
                            n += 1;
                        } else {
                            i += 1;
                        }
                    }
                }
                log.push(format!("removed {} {} token(s)", n, tag));
            }
            Edit::Rename(newname) => {
                let line = &mut lf.lines[first];
                if let Some(hc) = line.head_cell() {
                    let mut h = Head::parse(&line.cells[hc]);
                    let old = h.name.clone();
                    h.name = newname.clone();
                    line.cells[hc] = h.unparse();
                    log.push(format!("renamed '{}' to '{}'", old, newname));
                }
            }
            Edit::Delete => {
                log.push("deleted record".into());
            }
        }
    }
    if edits.contains(&Edit::Delete) {
        for &li in lines.iter().rev() {
            lf.lines.remove(li);
        }
    } else {
        // continuation lines emptied by removals are dropped
        for &li in lines[1..].iter().rev() {
            if lf.lines[li].cells.iter().all(|c| c.trim().is_empty()) {
                lf.lines.remove(li);
            }
        }
    }
    log
}

/// Canonical layout: tokens separated by a single TAB with no
/// surrounding whitespace, LF line ends, no trailing blank lines, and
/// a final newline.  Comments and blank lines between records are
/// kept; continuation lines are kept with a single leading TAB.
pub fn format(lf: &LstFile) -> LstFile {
    let mut out = LstFile {
        bom: false,
        lines: Vec::new(),
    };
    for l in &lf.lines {
        let cells: Vec<String> = match l.kind {
            LineKind::Blank => vec![String::new()],
            LineKind::Comment => vec![l.text().trim_end().to_string()],
            LineKind::Continuation => std::iter::once(String::new())
                .chain(
                    l.cells
                        .iter()
                        .map(|c| c.trim())
                        .filter(|c| !c.is_empty())
                        .map(str::to_string),
                )
                .collect(),
            LineKind::Source | LineKind::Record => l
                .cells
                .iter()
                .map(|c| c.trim())
                .filter(|c| !c.is_empty())
                .map(str::to_string)
                .collect(),
        };
        out.lines.push(LstLine {
            kind: l.kind,
            cells,
            eol: Eol::Lf,
            latin1: false,
        });
    }
    while out.lines.last().is_some_and(|l| l.kind == LineKind::Blank) {
        out.lines.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const F: &str = "# header\r\nSOURCELONG:B\r\nSpoon\t\tTYPE:Goods\t\tCOST:1\tCOST:2\r\n\tWT:1\r\nKnife\tTYPE:Tool\r\n";

    fn edit(edits: &[Edit]) -> String {
        let mut lf = LstFile::parse(F);
        let lines = find_record(&lf, "spoon", false).unwrap();
        apply(&mut lf, &lines, edits);
        String::from_utf8(lf.encode()).unwrap()
    }

    #[test]
    fn find() {
        let lf = LstFile::parse("A.MOD\tX:1\nA\tX:2\n");
        assert_eq!(find_record(&lf, "a", false), Some(vec![1]));
        assert_eq!(find_record(&lf, "a", true), Some(vec![0]));
        assert_eq!(find_record(&lf, "b", true), None);
    }

    #[test]
    fn set_replaces_and_dedupes() {
        assert_eq!(
            edit(&[Edit::Set("COST".into(), "5".into())]),
            "# header\r\nSOURCELONG:B\r\nSpoon\t\tTYPE:Goods\t\tCOST:5\r\n\tWT:1\r\nKnife\tTYPE:Tool\r\n"
        );
    }

    #[test]
    fn set_appends_when_absent() {
        assert_eq!(
            edit(&[Edit::Set("DESC".into(), "Shiny".into())]),
            "# header\r\nSOURCELONG:B\r\nSpoon\t\tTYPE:Goods\t\tCOST:1\tCOST:2\r\n\tWT:1\tDESC:Shiny\r\nKnife\tTYPE:Tool\r\n"
        );
    }

    #[test]
    fn remove_drops_empty_continuation() {
        assert_eq!(
            edit(&[Edit::Remove("WT".into())]),
            "# header\r\nSOURCELONG:B\r\nSpoon\t\tTYPE:Goods\t\tCOST:1\tCOST:2\r\nKnife\tTYPE:Tool\r\n"
        );
    }

    #[test]
    fn rename_and_delete() {
        assert!(edit(&[Edit::Rename("Ladle".into())]).contains("\r\nLadle\t\tTYPE:Goods"));
        assert_eq!(
            edit(&[Edit::Delete]),
            "# header\r\nSOURCELONG:B\r\nKnife\tTYPE:Tool\r\n"
        );
    }

    #[test]
    fn rename_keeps_suffix() {
        let mut lf = LstFile::parse("CATEGORY=FEAT|Old.MOD\tX:1\n");
        let lines = find_record(&lf, "old", true).unwrap();
        apply(&mut lf, &lines, &[Edit::Rename("New".into())]);
        assert_eq!(lf.encode(), b"CATEGORY=FEAT|New.MOD\tX:1\n");
    }

    #[test]
    fn untouched_bytes_preserved() {
        let mut lf = LstFile::parse(F);
        let lines = find_record(&lf, "knife", false).unwrap();
        apply(&mut lf, &lines, &[Edit::Add("COST".into(), "3".into())]);
        let out = String::from_utf8(lf.encode()).unwrap();
        assert_eq!(out, F.replace("TYPE:Tool\r\n", "TYPE:Tool\tCOST:3\r\n"));
    }

    #[test]
    fn token_parse() {
        assert_eq!(Edit::token("A:b:c"), Ok(("A".into(), "b:c".into())));
        assert!(Edit::token("nocolon").is_err());
        assert!(Edit::token(":x").is_err());
    }

    #[test]
    fn formatting() {
        let lf = LstFile::parse("\u{feff}# c  \r\nA\t\t B:1 \tC:2\t\n   \tD:3\n\n\n");
        let out = String::from_utf8(format(&lf).encode()).unwrap();
        assert_eq!(out, "# c\nA\tB:1\tC:2\n\tD:3\n");
        // idempotent
        let again = format(&LstFile::parse(&out));
        assert_eq!(String::from_utf8(again.encode()).unwrap(), out);
    }
}
