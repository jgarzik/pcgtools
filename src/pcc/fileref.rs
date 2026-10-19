//
// pcc/fileref.rs -- PCC file reference values
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! A PCC file-reference value names a file plus optional filters:
//!
//! ```text
//! path [ "|" suffix { "|" suffix } ]
//! suffix = "(INCLUDE:" item { "|" item } ")"
//!        | "(EXCLUDE:" item { "|" item } ")"
//!        | [ "!" ] "PRE" kind ":" body
//! ```

use crate::token::split::{split_top, unwrap_group};
use serde::Serialize;

/// Where a path is anchored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum PathBase {
    /// `@/...`: the data directory
    Data,
    /// `&/...`: the vendor data directory
    Vendor,
    /// `$/...`: the homebrew data directory
    Homebrew,
    /// `*/...`: homebrew, then vendor, then data
    Any,
    /// `scheme:...`: an absolute URI
    Uri,
    /// legacy `data/...` (or `/data/...`): the data directory's parent
    LegacyData,
    /// relative to the referring PCC file
    Local,
}

/// A path as written in a PCC file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PathSpec {
    pub base: PathBase,
    /// the path below the base, using `/` separators
    pub rel: String,
}

impl PathSpec {
    pub fn parse(raw: &str) -> PathSpec {
        let s = raw.trim().replace('\\', "/");
        let strip = |r: &str| r.strip_prefix('/').unwrap_or(r).to_string();
        let (base, rel) = match s.chars().next() {
            Some('@') => (PathBase::Data, strip(&s[1..])),
            Some('&') => (PathBase::Vendor, strip(&s[1..])),
            Some('$') => (PathBase::Homebrew, strip(&s[1..])),
            Some('*') => (PathBase::Any, strip(&s[1..])),
            _ if s.find(':').is_some_and(|i| i > 1) => (PathBase::Uri, s.clone()),
            _ => {
                let t = s.strip_prefix('/').unwrap_or(&s);
                if let Some(r) = t.strip_prefix("data/") {
                    (PathBase::LegacyData, r.to_string())
                } else {
                    (PathBase::Local, s.clone())
                }
            }
        };
        PathSpec { base, rel }
    }
}

/// An INCLUDE or EXCLUDE filter on a file reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Filter {
    Include(Vec<FilterItem>),
    Exclude(Vec<FilterItem>),
}

/// One filtered object key.  For ability files the items are grouped
/// by category: `CATEGORY=FEAT,Alertness,Dodge`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct FilterItem {
    pub category: Option<String>,
    pub key: String,
}

impl Filter {
    pub fn items(&self) -> &[FilterItem] {
        match self {
            Filter::Include(v) | Filter::Exclude(v) => v,
        }
    }

    /// Does this filter admit an object?
    pub fn admits(&self, category: Option<&str>, key: &str) -> bool {
        let hit = self.items().iter().any(|it| {
            it.key == key
                && match (&it.category, category) {
                    (Some(c), Some(oc)) => c.eq_ignore_ascii_case(oc),
                    (Some(_), None) => false,
                    (None, _) => true,
                }
        });
        match self {
            Filter::Include(_) => hit,
            Filter::Exclude(_) => !hit,
        }
    }
}

/// A parsed file reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileRef {
    /// the value exactly as written
    pub raw: String,
    pub path: PathSpec,
    pub filter: Option<Filter>,
    /// prerequisite suffixes, as written (e.g. `!PRECAMPAIGN:1,Foo`)
    pub prereqs: Vec<String>,
}

fn parse_items(body: &str) -> Vec<FilterItem> {
    let mut out = Vec::new();
    for part in split_top(body, '|') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(rest) = part.strip_prefix("CATEGORY=") {
            let mut it = rest.split(',');
            let cat = it.next().unwrap_or("").to_string();
            for k in it {
                out.push(FilterItem {
                    category: Some(cat.clone()),
                    key: k.to_string(),
                });
            }
        } else {
            out.push(FilterItem {
                category: None,
                key: part.to_string(),
            });
        }
    }
    out
}

impl FileRef {
    /// Parse a file reference value.  Returns the reference plus any
    /// problems found.
    pub fn parse(raw: &str) -> (FileRef, Vec<String>) {
        let mut errs = Vec::new();
        let parts = split_top(raw.trim(), '|');
        let path = PathSpec::parse(parts[0]);
        if parts[0].trim().is_empty() {
            errs.push("empty file path".to_string());
        }
        let mut filter = None;
        let mut prereqs = Vec::new();
        for suf in &parts[1..] {
            let suf = suf.trim();
            if let Some(inner) = unwrap_group(suf, '(', ')') {
                let f = if let Some(b) = inner.strip_prefix("INCLUDE:") {
                    Filter::Include(parse_items(b))
                } else if let Some(b) = inner.strip_prefix("EXCLUDE:") {
                    Filter::Exclude(parse_items(b))
                } else {
                    errs.push(format!("unknown file filter '{}'", suf));
                    continue;
                };
                if filter.is_some() {
                    errs.push("multiple INCLUDE/EXCLUDE filters".to_string());
                }
                filter = Some(f);
            } else if suf.starts_with("PRE") || suf.starts_with("!PRE") {
                prereqs.push(suf.to_string());
            } else {
                errs.push(format!("unrecognized file suffix '{}'", suf));
            }
        }
        (
            FileRef {
                raw: raw.to_string(),
                path,
                filter,
                prereqs,
            },
            errs,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths() {
        let p = PathSpec::parse("@/fantasy/core/spells.lst");
        assert_eq!(p.base, PathBase::Data);
        assert_eq!(p.rel, "fantasy/core/spells.lst");
        assert_eq!(PathSpec::parse("*/_universal/x.lst").base, PathBase::Any);
        assert_eq!(PathSpec::parse("&x.lst").base, PathBase::Vendor);
        assert_eq!(PathSpec::parse("$/x.lst").base, PathBase::Homebrew);
        assert_eq!(PathSpec::parse("http://foo/x.lst").base, PathBase::Uri);
        let p = PathSpec::parse("..\\common\\x.lst");
        assert_eq!(p.base, PathBase::Local);
        assert_eq!(p.rel, "../common/x.lst");
        let p = PathSpec::parse("/data/d20/x.lst");
        assert_eq!(p.base, PathBase::LegacyData);
        assert_eq!(p.rel, "d20/x.lst");
    }

    #[test]
    fn filters() {
        let (r, e) = FileRef::parse("eq.lst|(INCLUDE:Sword (Long)|Club)");
        assert!(e.is_empty());
        let f = r.filter.unwrap();
        assert!(f.admits(None, "Club"));
        assert!(f.admits(None, "Sword (Long)"));
        assert!(!f.admits(None, "Dagger"));

        let (r, e) = FileRef::parse(
            "ab.lst|(EXCLUDE:CATEGORY=FEAT,Dodge,Mobility|CATEGORY=Special Ability,Evasion)",
        );
        assert!(e.is_empty());
        let f = r.filter.unwrap();
        assert_eq!(f.items().len(), 3);
        assert!(!f.admits(Some("FEAT"), "Dodge"));
        assert!(f.admits(Some("Special Ability"), "Dodge"));

        let (r, e) = FileRef::parse("sp.lst|!PRECAMPAIGN:1,Some Book");
        assert!(e.is_empty());
        assert_eq!(r.prereqs, vec!["!PRECAMPAIGN:1,Some Book"]);

        let (_, e) = FileRef::parse("sp.lst|junk");
        assert_eq!(e.len(), 1);
    }

    #[test]
    fn path_table() {
        let cases: &[(&str, PathBase, &str)] = &[
            ("@/a/b.lst", PathBase::Data, "a/b.lst"),
            ("@a/b.lst", PathBase::Data, "a/b.lst"),
            ("@\\a\\b.lst", PathBase::Data, "a/b.lst"),
            ("&/v.lst", PathBase::Vendor, "v.lst"),
            ("$/h.lst", PathBase::Homebrew, "h.lst"),
            ("*/x/y.lst", PathBase::Any, "x/y.lst"),
            ("file:/abs/x.lst", PathBase::Uri, "file:/abs/x.lst"),
            (
                "https://example.com/x.lst",
                PathBase::Uri,
                "https://example.com/x.lst",
            ),
            ("data/x.lst", PathBase::LegacyData, "x.lst"),
            ("/data/x.lst", PathBase::LegacyData, "x.lst"),
            ("x.lst", PathBase::Local, "x.lst"),
            ("sub/x.lst", PathBase::Local, "sub/x.lst"),
            ("../x.lst", PathBase::Local, "../x.lst"),
            ("  x.lst  ", PathBase::Local, "x.lst"),
            (
                "Player's Guide/x.lst",
                PathBase::Local,
                "Player's Guide/x.lst",
            ),
        ];
        for (raw, base, rel) in cases {
            let p = PathSpec::parse(raw);
            assert_eq!(&p.base, base, "{}", raw);
            assert_eq!(p.rel, *rel, "{}", raw);
        }
    }

    #[test]
    fn suffix_table() {
        // (value, filter item count, include?, prereq count, error count)
        let cases: &[(&str, usize, bool, usize, usize)] = &[
            ("a.lst", 0, false, 0, 0),
            ("a.lst|(INCLUDE:X)", 1, true, 0, 0),
            ("a.lst|(INCLUDE:X|Y|Z)", 3, true, 0, 0),
            ("a.lst|(EXCLUDE:X|Y)", 2, false, 0, 0),
            (
                "a.lst|(INCLUDE:CATEGORY=FEAT,A,B|CATEGORY=Special,C)",
                3,
                true,
                0,
                0,
            ),
            (
                "a.lst|(INCLUDE:Pick (Heavy)|Sword (Long, Bastard))",
                2,
                true,
                0,
                0,
            ),
            ("a.lst|PRECAMPAIGN:1,X", 0, false, 1, 0),
            ("a.lst|!PRECAMPAIGN:1,X|PRECAMPAIGN:1,Y", 0, false, 2, 0),
            ("a.lst|(INCLUDE:X)|PRECAMPAIGN:1,Y", 1, true, 1, 0),
            ("a.lst|(INCLUDE:X)|(EXCLUDE:Y)", 1, false, 0, 1),
            ("a.lst|(OTHER:X)", 0, false, 0, 1),
            ("a.lst|junk", 0, false, 0, 1),
            ("", 0, false, 0, 1),
        ];
        for (v, items, include, pres, errs) in cases {
            let (r, e) = FileRef::parse(v);
            assert_eq!(e.len(), *errs, "{} {:?}", v, e);
            assert_eq!(r.prereqs.len(), *pres, "{}", v);
            let n = r.filter.as_ref().map(|f| f.items().len()).unwrap_or(0);
            assert_eq!(n, *items, "{}", v);
            if *items > 0 {
                assert_eq!(
                    matches!(r.filter, Some(Filter::Include(_))),
                    *include,
                    "{}",
                    v
                );
            }
            assert_eq!(r.raw, *v);
        }
    }

    #[test]
    fn admits_categories() {
        let (r, _) = FileRef::parse("a.lst|(INCLUDE:CATEGORY=FEAT,Dodge|Plain)");
        let f = r.filter.unwrap();
        assert!(f.admits(Some("feat"), "Dodge"));
        assert!(!f.admits(None, "Dodge"));
        assert!(f.admits(None, "Plain"));
        assert!(f.admits(Some("Anything"), "Plain"));
        assert!(!f.admits(Some("FEAT"), "dodge"));
    }
}
