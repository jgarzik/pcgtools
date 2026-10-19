//
// token/pre.rs -- PRExxx prerequisite tokens
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! ```text
//! prereq = [ "!" ] "PRE" kind ":" body ;
//! body   = arg { "," arg }                       (most kinds)
//!        | count "," "[" prereq "]" { "," "[" prereq "]" }   (PREMULT)
//! ```
//!
//! Commas nested in `[]` or `()` do not split arguments.  The meaning
//! of the arguments depends on the kind; this module only captures the
//! structure, losslessly.

use super::split::{split_top, unwrap_group};
use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum PreBody {
    /// PREMULT: count plus bracketed sub-prerequisites
    Mult { count: String, subs: Vec<Prereq> },
    /// any other kind: the comma-separated arguments
    Args(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Prereq {
    pub negated: bool,
    /// the part after `PRE`, e.g. `ABILITY`, `VARGTEQ`, `MULT`
    pub kind: String,
    pub body: PreBody,
}

impl Prereq {
    /// Is this tag a prerequisite tag (`PRExxx` or `!PRExxx`)?
    pub fn is_pre_tag(tag: &str) -> bool {
        let t = tag.strip_prefix('!').unwrap_or(tag);
        t.len() > 3 && t.starts_with("PRE")
    }

    /// Parse from a separate tag and value.
    pub fn from_token(tag: &str, value: &str) -> Result<Prereq, String> {
        let (negated, t) = match tag.strip_prefix('!') {
            Some(t) => (true, t),
            None => (false, tag),
        };
        let kind = t
            .strip_prefix("PRE")
            .filter(|k| !k.is_empty())
            .ok_or_else(|| format!("not a prerequisite tag: {}", tag))?;
        let body = if kind == "MULT" {
            let parts = split_top(value, ',');
            let count = parts[0].to_string();
            let mut subs = Vec::new();
            for p in &parts[1..] {
                let inner = unwrap_group(p.trim(), '[', ']')
                    .ok_or_else(|| format!("PREMULT item not in brackets: {}", p))?;
                subs.push(Prereq::parse(inner)?);
            }
            if subs.is_empty() {
                return Err("PREMULT with no sub-prerequisites".into());
            }
            PreBody::Mult { count, subs }
        } else {
            PreBody::Args(
                split_top(value, ',')
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            )
        };
        Ok(Prereq {
            negated,
            kind: kind.to_string(),
            body,
        })
    }

    /// Parse `[!]PRExxx:body`.
    pub fn parse(s: &str) -> Result<Prereq, String> {
        let (tag, value) = s
            .split_once(':')
            .ok_or_else(|| format!("prerequisite has no ':': {}", s))?;
        Prereq::from_token(tag, value)
    }

    pub fn tag(&self) -> String {
        format!("{}PRE{}", if self.negated { "!" } else { "" }, self.kind)
    }

    /// The arguments (empty for PREMULT).
    pub fn args(&self) -> &[String] {
        match &self.body {
            PreBody::Args(a) => a,
            PreBody::Mult { .. } => &[],
        }
    }

    /// The leading count argument, for list-style kinds.
    pub fn count(&self) -> Option<i64> {
        match &self.body {
            PreBody::Mult { count, .. } => count.trim().parse().ok(),
            PreBody::Args(a) => a.first()?.trim().parse().ok(),
        }
    }
}

impl fmt::Display for Prereq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:", self.tag())?;
        match &self.body {
            PreBody::Args(a) => f.write_str(&a.join(",")),
            PreBody::Mult { count, subs } => {
                f.write_str(count)?;
                for s in subs {
                    write!(f, ",[{}]", s)?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple() {
        let p = Prereq::parse("PREABILITY:1,CATEGORY=FEAT,Dodge,Mobility").unwrap();
        assert_eq!(p.kind, "ABILITY");
        assert_eq!(p.count(), Some(1));
        assert_eq!(p.args().len(), 4);
        assert!(!p.negated);
        let p = Prereq::parse("!PRERACE:1,Elf%").unwrap();
        assert!(p.negated);
        assert_eq!(p.to_string(), "!PRERACE:1,Elf%");
    }

    #[test]
    fn mult() {
        let s = "PREMULT:1,[PRECLASS:1,Tinker=1],[!PRECLASS:1,TYPE=Base,ANY=2],[PREMULT:2,[PREA:1,x],[PREB:1,y]]";
        let p = Prereq::parse(s).unwrap();
        match &p.body {
            PreBody::Mult { count, subs } => {
                assert_eq!(count, "1");
                assert_eq!(subs.len(), 3);
                assert!(subs[1].negated);
                assert_eq!(subs[2].kind, "MULT");
            }
            _ => panic!(),
        }
        assert_eq!(p.to_string(), s);
    }

    #[test]
    fn bad() {
        assert!(Prereq::parse("PREMULT:1,PREA:1,x").is_err());
        assert!(Prereq::parse("NOPE:1").is_err());
        assert!(Prereq::is_pre_tag("!PREVARGT"));
        assert!(!Prereq::is_pre_tag("PRE"));
    }
}
