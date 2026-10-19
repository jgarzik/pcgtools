//
// lint.rs -- data file checks
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! Static checks on LST files that do not need a loaded dataset:
//! unknown and deprecated tags, malformed prerequisites and bonuses,
//! unparseable formulas, separator mistakes, encoding problems.

use crate::catalog::{Catalog, Status};
use crate::error::{Diagnostic, Diagnostics};
use crate::gamemode::GameModes;
use crate::lst::grammar::{classify, Role};
use crate::lst::kind::FileKind;
use crate::lst::line::{is_tag_name, LineKind, LstFile};
use crate::pcc::Campaign;
use crate::token::bonus::Bonus;
use crate::token::pre::Prereq;
use std::path::Path;

/// Which checks to run.
#[derive(Debug, Clone)]
pub struct LintOptions {
    pub unknown_tags: bool,
    pub deprecated: bool,
    pub formulas: bool,
}

impl Default for LintOptions {
    fn default() -> Self {
        LintOptions {
            unknown_tags: true,
            deprecated: true,
            formulas: true,
        }
    }
}

/// Check a value for leading, trailing, or doubled separators.
pub fn separator_problem(value: &str, sep: char) -> Option<&'static str> {
    if value.starts_with(sep) {
        Some("starts with a separator")
    } else if value.ends_with(sep) {
        Some("ends with a separator")
    } else if value.contains(&format!("{}{}", sep, sep)) {
        Some("contains an empty field")
    } else {
        None
    }
}

/// Does this token look like two tokens joined by a space instead of
/// a TAB (e.g. `COST:5 WT:1`)?
pub fn looks_space_joined(value: &str) -> bool {
    value.split(' ').skip(1).any(|w| {
        w.split_once(':').is_some_and(|(t, v)| {
            t.len() >= 2 && is_tag_name(t) && !v.is_empty() && Catalog::get().known_anywhere(t)
        })
    })
}

/// Lint one LST file of a known kind.
pub fn lint_lst(path: &Path, kind: FileKind, lf: &LstFile, opts: &LintOptions) -> Diagnostics {
    let cat = Catalog::get();
    let mut diags = Diagnostics::default();
    let mut d = |sev: Diagnostic, line: usize| diags.push(sev.at(path, line));

    if lf.lines.iter().any(|l| l.latin1) {
        d(Diagnostic::warning("file is not valid UTF-8"), 0);
    }
    if lf.bom {
        d(Diagnostic::info("file starts with a byte-order mark"), 1);
    }
    let has_data = lf.lines.iter().any(|l| l.kind == LineKind::Record);
    if has_data
        && kind != FileKind::DataTable
        && !lf.lines.iter().any(|l| l.kind == LineKind::Source)
    {
        d(Diagnostic::info("file has no SOURCE header line"), 0);
    }
    if kind == FileKind::DataTable {
        return diags;
    }

    for rec in lf.records() {
        if rec.kind() != LineKind::Record {
            continue;
        }
        let line_no = rec.line_no();
        let head = rec.head().unwrap_or("");
        let line_tag = match classify(kind, head) {
            Role::Object { head_tag, .. } => head_tag,
            Role::Child { head_tag, .. } => Some(head_tag),
            Role::Sub { tag, .. } | Role::ChildSub { tag, .. } => Some(tag),
        };

        for t in rec.tokens() {
            let line = t.line + 1;
            let v = t.value.unwrap_or("");
            if t.value.is_none() && !t.tag.starts_with('.') {
                d(
                    Diagnostic::warning(format!(
                        "token '{}' has no ':' (missing TAB or colon?)",
                        t.tag
                    )),
                    line,
                );
                continue;
            }

            if opts.unknown_tags || opts.deprecated {
                match cat.check(kind, line_tag.as_deref(), t.tag) {
                    None if opts.unknown_tags => d(
                        Diagnostic::warning(format!("unknown tag '{}' for {}", t.tag, kind)),
                        line,
                    ),
                    Some(e) if opts.deprecated && e.status == Status::Deprecated => d(
                        Diagnostic::info(format!("deprecated tag '{}'", t.tag)),
                        line,
                    ),
                    _ => {}
                }
            }

            if looks_space_joined(v) {
                d(
                    Diagnostic::warning(format!(
                        "'{}:{}' looks like two tokens joined by a space",
                        t.tag, v
                    )),
                    line,
                );
            }

            if Prereq::is_pre_tag(t.tag) {
                if let Err(e) = Prereq::from_token(t.tag, v) {
                    d(Diagnostic::error(format!("bad prerequisite: {}", e)), line);
                }
            } else if t.tag == "BONUS" {
                match Bonus::parse(v) {
                    Err(e) => d(Diagnostic::error(e), line),
                    Ok(b) => {
                        if let Some(m) = separator_problem(v, '|') {
                            d(Diagnostic::warning(format!("BONUS value {}", m)), line);
                        }
                        if opts.formulas {
                            if let Err(e) = b.expr() {
                                d(
                                    Diagnostic::warning(format!(
                                        "BONUS formula '{}': {}",
                                        b.formula, e
                                    )),
                                    line,
                                );
                            }
                        }
                    }
                }
            } else if matches!(t.tag, "ABILITY" | "ADD" | "AUTO" | "CHOOSE" | "DEFINE") {
                if let Some(m) = separator_problem(v, '|') {
                    d(Diagnostic::warning(format!("{} value {}", t.tag, m)), line);
                }
            }
        }
        let _ = line_no;
    }
    diags
}

/// Check that each campaign's GAMEMODE values name a known game mode.
pub fn lint_gamemodes<'a>(
    campaigns: impl IntoIterator<Item = &'a Campaign>,
    modes: &GameModes,
) -> Diagnostics {
    let mut diags = Diagnostics::default();
    for c in campaigns {
        if c.gamemodes.is_empty() && !c.name.is_empty() {
            diags.push(Diagnostic::warning("campaign has no GAMEMODE").at(&c.path, 0));
        }
        for g in &c.gamemodes {
            if modes.find(g).is_none() {
                diags
                    .push(Diagnostic::warning(format!("unknown game mode '{}'", g)).at(&c.path, 0));
            }
        }
    }
    diags
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lint(kind: FileKind, s: &str) -> Vec<String> {
        let lf = LstFile::parse(s);
        lint_lst(Path::new("t.lst"), kind, &lf, &LintOptions::default())
            .items
            .into_iter()
            .map(|d| format!("{}:{}", d.line, d.message))
            .collect()
    }

    #[test]
    fn clean() {
        let out = lint(
            FileKind::Spell,
            "SOURCELONG:X\tSOURCESHORT:X\nSpark\tSCHOOL:Evocation\tBONUS:VAR|X|1+2\tPREVARGT:X,1\n",
        );
        assert!(out.is_empty(), "{:?}", out);
    }

    #[test]
    fn problems() {
        let out = lint(
            FileKind::Spell,
            "SOURCELONG:X\nSpark\tSCHOOLX:Evo\tCOST:5 WT:1\tBONUS:VAR|X\tPREMULT:1,PREA:1,x\tBONUS:VAR|X|1+\tADD:FEAT||x\tNOCOLON\n",
        );
        let joined = out.join("\n");
        assert!(joined.contains("unknown tag 'SCHOOLX'"), "{}", joined);
        assert!(joined.contains("joined by a space"), "{}", joined);
        assert!(joined.contains("BONUS needs"), "{}", joined);
        assert!(joined.contains("bad prerequisite"), "{}", joined);
        assert!(joined.contains("BONUS formula"), "{}", joined);
        assert!(
            joined.contains("ADD value contains an empty field"),
            "{}",
            joined
        );
        assert!(joined.contains("has no ':'"), "{}", joined);
    }

    #[test]
    fn source_header() {
        let out = lint(FileKind::Spell, "Spark\tSCHOOL:Evocation\n");
        assert_eq!(out, vec!["0:file has no SOURCE header line"]);
    }

    #[test]
    fn separators() {
        assert_eq!(
            separator_problem("|a", '|'),
            Some("starts with a separator")
        );
        assert_eq!(separator_problem("a|", '|'), Some("ends with a separator"));
        assert_eq!(
            separator_problem("a||b", '|'),
            Some("contains an empty field")
        );
        assert_eq!(separator_problem("a|b", '|'), None);
    }
}
