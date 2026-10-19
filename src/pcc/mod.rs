//
// pcc/mod.rs -- PCC campaign files
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! A PCC file holds one `TAG:value` per line.  Unlike LST files,
//! lines are not split on TAB; leading and trailing whitespace is
//! ignored.

pub mod fileref;
pub mod resolve;

use crate::error::{Diagnostic, Diagnostics, Result};
use crate::lst::kind::FileKind;
use crate::text::TextFile;
use fileref::FileRef;
use resolve::{find_existing, DataDirs};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// One `TAG:value` line of a PCC file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PccEntry {
    /// 1-based line number
    pub line: usize,
    /// tag was written with a leading `!`
    pub negated: bool,
    pub tag: String,
    pub value: String,
}

impl PccEntry {
    /// The tag as written, including any `!`.
    pub fn full_tag(&self) -> String {
        if self.negated {
            format!("!{}", self.tag)
        } else {
            self.tag.clone()
        }
    }
}

/// A PCC file, kept losslessly alongside its parsed entries.
#[derive(Debug, Clone, Serialize)]
pub struct PccFile {
    pub path: PathBuf,
    #[serde(skip)]
    pub text: TextFile,
    pub entries: Vec<PccEntry>,
    pub diags: Diagnostics,
}

impl PccFile {
    pub fn parse(path: &Path, text: TextFile) -> PccFile {
        let mut entries = Vec::new();
        let mut diags = Diagnostics::default();
        for (i, l) in text.lines.iter().enumerate() {
            let s = l.text.trim();
            if s.is_empty() || s.starts_with('#') {
                continue;
            }
            match s.split_once(':') {
                Some((tag, value)) if !tag.is_empty() => {
                    let (negated, tag) = match tag.strip_prefix('!') {
                        Some(t) => (true, t),
                        None => (false, tag),
                    };
                    entries.push(PccEntry {
                        line: i + 1,
                        negated,
                        tag: tag.to_string(),
                        value: value.trim().to_string(),
                    });
                }
                _ => diags.push(
                    Diagnostic::error(format!("PCC line is not TAG:value: '{}'", s))
                        .at(path, i + 1),
                ),
            }
        }
        PccFile {
            path: path.to_path_buf(),
            text,
            entries,
            diags,
        }
    }

    pub fn read(path: &Path) -> Result<PccFile> {
        Ok(PccFile::parse(path, TextFile::read(path)?))
    }

    pub fn from_text(path: &Path, s: &str) -> PccFile {
        PccFile::parse(path, TextFile::from_text(s))
    }
}

/// A file referenced by a campaign.
#[derive(Debug, Clone, Serialize)]
pub struct CampaignFile {
    pub kind: FileKind,
    pub fref: FileRef,
    /// resolved filesystem path, if it could be resolved and exists
    pub path: Option<PathBuf>,
    pub line: usize,
}

/// The parsed meaning of a PCC file.
#[derive(Debug, Clone, Serialize)]
pub struct Campaign {
    pub path: PathBuf,
    /// identity used by PRECAMPAIGN and PCG files (KEY, else CAMPAIGN)
    pub key: String,
    /// display name (CAMPAIGN tag)
    pub name: String,
    pub gamemodes: Vec<String>,
    pub booktypes: Vec<String>,
    /// load rank, 0..9, default 9
    pub rank: i32,
    /// top-level prerequisites (`PRECAMPAIGN`, `!PRECAMPAIGN`, ...)
    pub prereqs: Vec<String>,
    /// data files, in file order
    pub files: Vec<CampaignFile>,
    /// included campaign files (`PCC:`), resolved where possible
    pub includes: Vec<(FileRef, Option<PathBuf>)>,
    /// files excluded globally by LSTEXCLUDE
    pub lst_excludes: Vec<PathBuf>,
    /// all other tags, in order
    pub meta: Vec<(String, String)>,
    pub diags: Diagnostics,
}

impl Campaign {
    pub fn from_pcc(pcc: &PccFile, dirs: &DataDirs) -> Campaign {
        let path = pcc.path.clone();
        let mut c = Campaign {
            path: path.clone(),
            key: String::new(),
            name: String::new(),
            gamemodes: Vec::new(),
            booktypes: Vec::new(),
            rank: 9,
            prereqs: Vec::new(),
            files: Vec::new(),
            includes: Vec::new(),
            lst_excludes: Vec::new(),
            meta: Vec::new(),
            diags: pcc.diags.clone(),
        };
        let mut key = None;

        let resolve = |fref: &FileRef, diags: &mut Diagnostics, line: usize| {
            let found = dirs
                .resolve(&fref.path, &path)
                .and_then(|p| find_existing(&p));
            if found.is_none() {
                diags.push(
                    Diagnostic::warning(format!("referenced file not found: {}", fref.raw))
                        .at(&path, line),
                );
            }
            found
        };

        for e in &pcc.entries {
            if e.tag.starts_with("PRE") {
                c.prereqs.push(format!("{}:{}", e.full_tag(), e.value));
                continue;
            }
            if let Some(kind) = FileKind::from_tag(&e.tag) {
                let (fref, errs) = FileRef::parse(&e.value);
                for m in errs {
                    c.diags.push(Diagnostic::error(m).at(&path, e.line));
                }
                if !kind.allows_filters() && (fref.filter.is_some() || !fref.prereqs.is_empty()) {
                    c.diags.push(
                        Diagnostic::warning(format!("{} file entries may not be filtered", kind))
                            .at(&path, e.line),
                    );
                }
                let p = resolve(&fref, &mut c.diags, e.line);
                c.files.push(CampaignFile {
                    kind,
                    fref,
                    path: p,
                    line: e.line,
                });
                continue;
            }
            match e.tag.as_str() {
                "CAMPAIGN" => c.name = e.value.clone(),
                "KEY" => key = Some(e.value.clone()),
                "GAMEMODE" => c.gamemodes = split_list(&e.value),
                "BOOKTYPE" => c.booktypes = split_list(&e.value),
                "RANK" => match e.value.parse() {
                    Ok(r) => c.rank = r,
                    Err(_) => c.diags.push(
                        Diagnostic::error(format!("bad RANK '{}'", e.value)).at(&path, e.line),
                    ),
                },
                "PCC" => {
                    let (fref, errs) = FileRef::parse(&e.value);
                    for m in errs {
                        c.diags.push(Diagnostic::error(m).at(&path, e.line));
                    }
                    let p = resolve(&fref, &mut c.diags, e.line);
                    c.includes.push((fref, p));
                }
                "LSTEXCLUDE" => {
                    for part in e.value.split('|') {
                        let (fref, _) = FileRef::parse(part);
                        if let Some(p) = resolve(&fref, &mut c.diags, e.line) {
                            c.lst_excludes.push(p);
                        }
                    }
                }
                _ => {}
            }
            c.meta.push((e.full_tag(), e.value.clone()));
        }
        if c.name.is_empty() {
            c.diags
                .push(Diagnostic::info("campaign has no CAMPAIGN tag").at(&path, 0));
        }
        c.key = key.unwrap_or_else(|| c.name.clone());
        c
    }

    /// First value of a metadata tag.
    pub fn meta(&self, tag: &str) -> Option<&str> {
        self.meta
            .iter()
            .find(|(t, _)| t == tag)
            .map(|(_, v)| v.as_str())
    }

    /// Is this campaign usable under the given game mode?
    pub fn supports_gamemode(&self, mode: &str) -> bool {
        self.gamemodes.iter().any(|g| g.eq_ignore_ascii_case(mode))
    }
}

fn split_list(s: &str) -> Vec<String> {
    s.split('|')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PCC: &str = "\
# A test campaign
CAMPAIGN:Book of Tests
KEY:BoT
GAMEMODE:Basic|Heroic
BOOKTYPE:Core Rules|Setting
RANK:3
!PRECAMPAIGN:1,Other Book
SOURCELONG:Book of Tests
SPELL:spells.lst|(EXCLUDE:Fireball)
ABILITY:@/common/feats.lst|PRECAMPAIGN:1,BOOKTYPE=Core Rules
PCC:sub/sub.pcc
LSTEXCLUDE:old.lst|older.lst
not a tag line
";

    #[test]
    fn parse_campaign() {
        let pcc = PccFile::from_text(Path::new("/x/book.pcc"), PCC);
        assert_eq!(pcc.entries.len(), 11);
        assert_eq!(pcc.diags.len(), 1);
        assert!(pcc.entries[5].negated);
        assert_eq!(pcc.text.encode(), PCC.as_bytes());

        let dirs = DataDirs {
            data: Some(PathBuf::from("/x")),
            ..Default::default()
        };
        let c = Campaign::from_pcc(&pcc, &dirs);
        assert_eq!(c.name, "Book of Tests");
        assert_eq!(c.key, "BoT");
        assert_eq!(c.gamemodes, vec!["Basic", "Heroic"]);
        assert_eq!(c.booktypes, vec!["Core Rules", "Setting"]);
        assert_eq!(c.rank, 3);
        assert_eq!(c.prereqs, vec!["!PRECAMPAIGN:1,Other Book"]);
        assert_eq!(c.files.len(), 2);
        assert_eq!(c.files[0].kind, FileKind::Spell);
        assert_eq!(c.files[1].fref.prereqs.len(), 1);
        assert_eq!(c.includes.len(), 1);
        assert!(c.supports_gamemode("heroic"));
        assert_eq!(c.meta("SOURCELONG"), Some("Book of Tests"));
    }
}
