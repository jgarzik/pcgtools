//
// token/pre.rs -- PRExxx prerequisite tokens
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
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
    Mult {
        count: String,
        subs: Vec<Prereq>,
        /// whitespace before and after each bracketed item, kept so
        /// the value prints back exactly
        #[serde(skip)]
        pads: Vec<(String, String)>,
    },
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
            let mut pads = Vec::new();
            for p in &parts[1..] {
                let t = p.trim();
                let inner = unwrap_group(t, '[', ']')
                    .ok_or_else(|| format!("PREMULT item not in brackets: {}", p))?;
                subs.push(Prereq::parse(inner)?);
                let lead = &p[..p.len() - p.trim_start().len()];
                let trail = &p[p.trim_end().len()..];
                pads.push((lead.to_string(), trail.to_string()));
            }
            if subs.is_empty() {
                return Err("PREMULT with no sub-prerequisites".into());
            }
            PreBody::Mult { count, subs, pads }
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
            PreBody::Mult { count, subs, pads } => {
                f.write_str(count)?;
                for (i, s) in subs.iter().enumerate() {
                    let (lead, trail) = pads
                        .get(i)
                        .map(|(a, b)| (a.as_str(), b.as_str()))
                        .unwrap_or(("", ""));
                    write!(f, ",{}[{}]{}", lead, s, trail)?;
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
            PreBody::Mult { count, subs, .. } => {
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

    /// every one of these must parse and print back unchanged
    const ROUNDTRIP: &[&str] = &[
        "PREABILITY:1,CATEGORY=FEAT,Dodge",
        "PREABILITY:2,CATEGORY=Special Ability,TYPE=Rage,Evasion",
        "!PREABILITY:1,CATEGORY=FEAT,Toughness",
        "PREALIGN:LG,NG,CG",
        "PREARMORPROF:1,TYPE.Light",
        "PREBASESIZEGTEQ:M",
        "PRECAMPAIGN:1,INCLUDES=Some Book",
        "PRECHECK:1,Fortitude=4",
        "PRECLASS:1,Mage=5",
        "PRECLASS:2,ANY=3",
        "PRECSKILL:1,TYPE=Craft",
        "PREDEITY:1,Y",
        "PREDOMAIN:1,Fire,Water",
        "PREEQUIP:1,TYPE=Armor.Heavy",
        "PREFACT:1,DEITY,Alignment=CG",
        "PREGENDER:Female",
        "PREHANDSGTEQ:2",
        "PREHD:MIN=3,MAX=5",
        "PREITEM:1,Holy Symbol",
        "PRELANG:1,Common,Elven",
        "PRELEVEL:MIN=5",
        "PRELEVELMAX:10",
        "PREMOVE:1,Fly=30",
        "PREPCLEVEL:MAX=4",
        "PRERACE:1,TYPE=Humanoid,RACESUBTYPE=Elf",
        "PRERACE:1,Elf%",
        "PRERULE:1,SOMERULE",
        "PRESIZEEQ:S",
        "PRESKILL:2,Ride=5,Handle Animal=5",
        "PRESKILLTOT:Bluff,Diplomacy=10",
        "PRESPELL:1,Fireball",
        "PRESPELLCAST:MEMORIZE=Y,TYPE=Arcane",
        "PRESTAT:1,STR=13",
        "PRESTAT:2,DEX=13,WIS=13",
        "PRETEMPLATE:1,Half-Fiend%",
        "PRETEXT:Must swear an oath, before witnesses",
        "PRETYPE:1,Undead",
        "PREVARGT:SneakAttack,0",
        "PREVARGTEQ:a+b,c*2,d,e",
        "PREVARLT:if(x>1,1,0),1",
        "PREVISION:1,Darkvision=60",
        "PREWEAPONPROF:1,TYPE.Martial",
        "PRESKILL:1,Knowledge (Arcana),Knowledge (Nature)=2",
        "PREABILITY:1,CATEGORY=FEAT,Weapon Focus (Bow (Long))",
        "Q:PREFEAT:1,Alertness",
        "PREMULT:1,[PRERACE:1,Elf],[PRERACE:1,Gnome]",
        "PREMULT:2,[PRESTAT:1,STR=13],[!PREABILITY:1,CATEGORY=FEAT,Rage]",
        "!PREMULT:1,[PRECLASS:1,Mage=1],[PRECLASS:1,Seer=1]",
        "PREMULT:1,[PREMULT:2,[PREA:1,x],[PREB:1,y]],[PREC:1,z]",
        "PREMULT:1,[PRETEXT:a, b, c],[PREVARGT:x,(1,2)]",
        "PREMULT:1, [PRERACE:1,Elf]",
    ];

    #[test]
    fn roundtrip_table() {
        for s in ROUNDTRIP {
            let (tag, value) = s.split_once(':').unwrap();
            match Prereq::from_token(tag, value) {
                Ok(p) => assert_eq!(&p.to_string(), s),
                Err(e) if s.starts_with("Q:") => {
                    // Q: is a value prefix, not a tag; not a prereq tag
                    assert!(e.contains("not a prerequisite"), "{}", e)
                }
                Err(e) => panic!("{}: {}", s, e),
            }
        }
    }

    #[test]
    fn arg_structure() {
        let p = Prereq::parse("PRESKILL:1,Knowledge (Arcana),Craft (Bows, Arrows)=2").unwrap();
        assert_eq!(
            p.args(),
            ["1", "Knowledge (Arcana)", "Craft (Bows, Arrows)=2"]
        );
        assert_eq!(p.count(), Some(1));
        let p = Prereq::parse("PREALIGN:LG,NG").unwrap();
        assert_eq!(p.count(), None);
        assert_eq!(p.tag(), "PREALIGN");
        let p = Prereq::parse("!PREVARGT:x,1").unwrap();
        assert_eq!(p.tag(), "!PREVARGT");
        let p = Prereq::parse("PREMULT:3,[PREA:1,x],[PREB:1,y]").unwrap();
        assert_eq!(p.count(), Some(3));
        assert!(p.args().is_empty());
    }

    const BAD: &[&str] = &[
        "PREMULT:1",
        "PREMULT:1,",
        "PREMULT:1,[]",
        "PREMULT:1,[PREA:1,x]x",
        "PREMULT:1,{PREA:1,x}",
        "PREMULT:1,[NOTPRE:1]",
        "PREMULT:1,[PREA]",
        "PRE:1,x",
        "XPREA:1",
        "PREFEAT",
    ];

    #[test]
    fn rejects_table() {
        for s in BAD {
            assert!(Prereq::parse(s).is_err(), "accepted {:?}", s);
        }
    }

    #[test]
    fn tag_detection() {
        for t in ["PREA", "!PREA", "PREVARGTEQ", "!PREMULT"] {
            assert!(Prereq::is_pre_tag(t), "{}", t);
        }
        for t in ["PRE", "!PRE", "PR", "BONUS", "XPREA", ""] {
            assert!(!Prereq::is_pre_tag(t), "{}", t);
        }
    }
}
