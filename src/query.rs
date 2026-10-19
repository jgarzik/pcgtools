//
// query.rs -- object filters
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! Filters for selecting objects from a dataset.
//!
//! A condition is one of:
//!
//! | Form | Matches objects that |
//! |---|---|
//! | `TAG` | have the tag |
//! | `!TAG` | lack the tag |
//! | `TAG=value` | have the tag with exactly this value |
//! | `TAG~regex` | have the tag with a value matching the regex |
//! | `TYPE=x` | have type x (case-insensitive, any of the `.`-list) |
//!
//! Tags of sub-lines (class levels, kit lines) are searched as well.

use crate::dataset::Object;
use globset::GlobMatcher;
use regex::Regex;

#[derive(Debug, Clone)]
pub enum Cond {
    Has(String),
    Lacks(String),
    Equals(String, String),
    Matches(String, Regex),
    Type(String),
}

impl Cond {
    pub fn parse(s: &str) -> Result<Cond, String> {
        if let Some(t) = s.strip_prefix('!') {
            return Ok(Cond::Lacks(t.to_string()));
        }
        if let Some((t, re)) = s.split_once('~') {
            let r = Regex::new(re).map_err(|e| format!("bad regex '{}': {}", re, e))?;
            return Ok(Cond::Matches(t.to_string(), r));
        }
        if let Some((t, v)) = s.split_once('=') {
            if t == "TYPE" {
                return Ok(Cond::Type(v.to_string()));
            }
            return Ok(Cond::Equals(t.to_string(), v.to_string()));
        }
        if s.is_empty() {
            return Err("empty condition".into());
        }
        Ok(Cond::Has(s.to_string()))
    }

    pub fn test(&self, o: &Object) -> bool {
        let mut vals = all_values(o);
        match self {
            Cond::Has(t) => vals.any(|(tag, _)| tag == t),
            Cond::Lacks(t) => !vals.any(|(tag, _)| tag == t),
            Cond::Equals(t, v) => vals.any(|(tag, val)| tag == t && val == v),
            Cond::Matches(t, r) => vals.any(|(tag, val)| tag == t && r.is_match(val)),
            Cond::Type(t) => o.is_type(t),
        }
    }
}

/// (tag, value) pairs of an object, its sub-lines, and its children.
fn all_values(o: &Object) -> Box<dyn Iterator<Item = (&str, &str)> + '_> {
    let own = o.tokens.iter().map(|t| (t.tag.as_str(), t.value.as_str()));
    let subs = o
        .sublines
        .iter()
        .flat_map(|s| s.tokens.iter().map(|t| (t.tag.as_str(), t.value.as_str())));
    let kids = o.children.iter().flat_map(all_values);
    Box::new(own.chain(subs).chain(kids))
}

/// A conjunction of conditions plus an optional key/name glob.
#[derive(Debug, Clone, Default)]
pub struct Query {
    pub name: Option<GlobMatcher>,
    pub conds: Vec<Cond>,
}

impl Query {
    pub fn new(name_glob: Option<&str>, conds: &[String]) -> Result<Query, String> {
        let name = match name_glob {
            Some(g) => Some(
                globset::GlobBuilder::new(g)
                    .case_insensitive(true)
                    .build()
                    .map_err(|e| e.to_string())?
                    .compile_matcher(),
            ),
            None => None,
        };
        let conds = conds
            .iter()
            .map(|c| Cond::parse(c))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Query { name, conds })
    }

    pub fn matches(&self, o: &Object) -> bool {
        if let Some(g) = &self.name {
            if !g.is_match(&o.key) && !g.is_match(&o.name) {
                return false;
            }
        }
        self.conds.iter().all(|c| c.test(o))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataset::{Origin, Tok};
    use crate::lst::kind::FileKind;

    fn obj() -> Object {
        let o = Origin { file: 0, line: 1 };
        let tok = |t: &str, v: &str| Tok {
            tag: t.into(),
            value: v.into(),
            origin: o,
        };
        Object {
            kind: FileKind::Spell,
            head_tag: None,
            name: "Magic Spark".into(),
            key: "Magic Spark".into(),
            category: None,
            tokens: vec![
                tok("SCHOOL", "Evocation"),
                tok("TYPE", "Arcane.Divine"),
                tok("CLASSES", "Tinker=1|Mage=2"),
            ],
            sublines: vec![],
            children: vec![],
            source: vec![],
            origin: o,
            history: vec![],
        }
    }

    fn q(name: Option<&str>, conds: &[&str]) -> bool {
        let conds: Vec<String> = conds.iter().map(|s| s.to_string()).collect();
        Query::new(name, &conds).unwrap().matches(&obj())
    }

    #[test]
    fn conditions() {
        assert!(q(None, &[]));
        assert!(q(None, &["SCHOOL"]));
        assert!(!q(None, &["DESC"]));
        assert!(q(None, &["!DESC"]));
        assert!(q(None, &["SCHOOL=Evocation"]));
        assert!(!q(None, &["SCHOOL=evocation"]));
        assert!(q(None, &["CLASSES~Mage=[0-9]"]));
        assert!(q(None, &["TYPE=divine"]));
        assert!(!q(None, &["TYPE=Div"]));
        assert!(q(None, &["SCHOOL", "TYPE=Arcane", "!DESC"]));
    }

    #[test]
    fn names() {
        assert!(q(Some("magic*"), &[]));
        assert!(q(Some("*Spark"), &[]));
        assert!(!q(Some("Fire*"), &[]));
    }

    #[test]
    fn bad() {
        assert!(Cond::parse("X~(").is_err());
        assert!(Cond::parse("").is_err());
    }
}
