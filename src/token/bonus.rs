//
// token/bonus.rs -- BONUS tokens
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! ```text
//! bonus  = category "|" target { "," target } "|" formula { "|" option } ;
//! option = "TYPE=" name [ ".STACK" | ".REPLACE" ] | prereq | other ;
//! ```

use super::formula::{Expr, FormulaError};
use super::pre::Prereq;
use super::split::split_top;
use serde::Serialize;
use std::fmt;

/// A trailing BONUS option.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum BonusOpt {
    /// `TYPE=name[.STACK|.REPLACE]`
    Type(String),
    Pre(Prereq),
    Other(String),
}

/// A parsed BONUS value (the part after `BONUS:`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Bonus {
    pub category: String,
    pub targets: Vec<String>,
    pub formula: String,
    pub opts: Vec<BonusOpt>,
}

impl Bonus {
    pub fn parse(value: &str) -> Result<Bonus, String> {
        let parts = split_top(value, '|');
        if parts.len() < 3 {
            return Err(format!("BONUS needs category|target|value: {}", value));
        }
        let opts = parts[3..]
            .iter()
            .map(|p| {
                if Prereq::is_pre_tag(p.split(':').next().unwrap_or("")) {
                    Prereq::parse(p)
                        .map(BonusOpt::Pre)
                        .unwrap_or_else(|_| BonusOpt::Other(p.to_string()))
                } else if let Some(t) = p.strip_prefix("TYPE=") {
                    BonusOpt::Type(t.to_string())
                } else {
                    BonusOpt::Other(p.to_string())
                }
            })
            .collect();
        Ok(Bonus {
            category: parts[0].to_string(),
            targets: split_top(parts[1], ',')
                .into_iter()
                .map(str::to_string)
                .collect(),
            formula: parts[2].to_string(),
            opts,
        })
    }

    pub fn expr(&self) -> Result<Expr, FormulaError> {
        Expr::parse(&self.formula)
    }

    /// The bonus type name and stacking flag (`TYPE=Morale.STACK` gives
    /// `("Morale", Some("STACK"))`).
    pub fn bonus_type(&self) -> Option<(&str, Option<&str>)> {
        self.opts.iter().find_map(|o| match o {
            BonusOpt::Type(t) => Some(match t.rsplit_once('.') {
                Some((n, m)) if m == "STACK" || m == "REPLACE" => (n, Some(m)),
                _ => (t.as_str(), None),
            }),
            _ => None,
        })
    }

    pub fn prereqs(&self) -> impl Iterator<Item = &Prereq> {
        self.opts.iter().filter_map(|o| match o {
            BonusOpt::Pre(p) => Some(p),
            _ => None,
        })
    }
}

impl fmt::Display for Bonus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}|{}|{}",
            self.category,
            self.targets.join(","),
            self.formula
        )?;
        for o in &self.opts {
            match o {
                BonusOpt::Type(t) => write!(f, "|TYPE={}", t)?,
                BonusOpt::Pre(p) => write!(f, "|{}", p)?,
                BonusOpt::Other(s) => write!(f, "|{}", s)?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse() {
        let s = "COMBAT|AC,TOHIT|1+floor(TL/4)|TYPE=Dodge.STACK|!PRERACE:1,Elf|PREVARGT:x,1";
        let b = Bonus::parse(s).unwrap();
        assert_eq!(b.category, "COMBAT");
        assert_eq!(b.targets, vec!["AC", "TOHIT"]);
        assert_eq!(b.bonus_type(), Some(("Dodge", Some("STACK"))));
        assert_eq!(b.prereqs().count(), 2);
        assert!(b.expr().is_ok());
        assert_eq!(b.to_string(), s);
    }

    #[test]
    fn short() {
        assert!(Bonus::parse("SKILL|Ride").is_err());
        let b = Bonus::parse("SKILL|Climb,Swim|DEX-STR").unwrap();
        assert_eq!(b.bonus_type(), None);
    }

    const ROUNDTRIP: &[&str] = &[
        "COMBAT|AC|2|TYPE=Armor",
        "COMBAT|TOHIT|1",
        "COMBAT|TOHIT,DAMAGE|1|TYPE=Morale",
        "COMBAT|BASEAB|CL*3/4|TYPE=Base.REPLACE",
        "SAVE|Fortitude,Reflex,Will|1|TYPE=Resistance",
        "SAVE|BASE.Fortitude|CL/2+2",
        "SKILL|Climb,Swim|DEX-STR",
        "SKILL|TYPE=Craft|2|TYPE=Competence",
        "SKILL|%LIST|2",
        "STAT|STR,CON|2|TYPE=Enhancement",
        "VAR|SneakAttack|(CL+1)/2",
        "VAR|A,B,C|1|PRECLASS:1,Rogue=3",
        "HP|CURRENTMAX|8*CL",
        "MOVEADD|TYPE.Walk|10|PREVARLT:Encumbrance,1",
        "WEAPONPROF=Club|TOHIT|1",
        "ABILITYPOOL|Feat|1",
        "CASTERLEVEL|Mage|1",
        "SPELLCAST|CLASS=Mage;LEVEL=1|1",
        "SIZEMOD|NUMBER|1",
        "DC|ALLSPELLS|1|TYPE=Insight",
        "COMBAT|AC|1|TYPE=Dodge.STACK|!PREABILITY:1,CATEGORY=FEAT,Dodge",
        "COMBAT|AC|1|PREMULT:1,[PRERACE:1,Elf],[PRERACE:1,Gnome]|TYPE=Racial",
        "VAR|X|if(Y>=5,2,1)",
        "VAR|X|max(0,min(4,Y))",
        "SKILL|Knowledge (Arcana),Knowledge (Planes)|2",
        "VAR|Y|1|SOMETHINGELSE",
    ];

    #[test]
    fn roundtrip_table() {
        for s in ROUNDTRIP {
            let b = Bonus::parse(s).unwrap_or_else(|e| panic!("{}: {}", s, e));
            assert_eq!(&b.to_string(), s);
        }
    }

    #[test]
    fn fields() {
        let b = Bonus::parse("SKILL|Knowledge (Arcana),Knowledge (Planes)|2").unwrap();
        assert_eq!(b.targets, vec!["Knowledge (Arcana)", "Knowledge (Planes)"]);
        let b = Bonus::parse("COMBAT|BASEAB|CL|TYPE=Base.REPLACE").unwrap();
        assert_eq!(b.bonus_type(), Some(("Base", Some("REPLACE"))));
        let b = Bonus::parse("COMBAT|AC|1|TYPE=Natural.Armor").unwrap();
        assert_eq!(b.bonus_type(), Some(("Natural.Armor", None)));
        let b = Bonus::parse("VAR|Y|1|SOMETHINGELSE").unwrap();
        assert_eq!(b.opts, vec![BonusOpt::Other("SOMETHINGELSE".into())]);
        let b = Bonus::parse("VAR|Y|1|PREBAD").unwrap();
        assert_eq!(b.opts, vec![BonusOpt::Other("PREBAD".into())]);
    }

    #[test]
    fn rejects() {
        for s in ["", "VAR", "VAR|X", "|X"] {
            assert!(Bonus::parse(s).is_err(), "accepted {:?}", s);
        }
    }
}
