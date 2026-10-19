//
// token/choice.rs -- CHOOSE, ADD and AUTO values
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! Structured CHOOSE, ADD and AUTO values: a sub-token naming what is
//! chosen or granted, followed by `|`-separated selection items.
//!
//! Items select by name (`Dodge`), by type (`TYPE=Combat`,
//! `TYPE.Combat`), by keyword (`ALL`, `ANY`, `PC`, `QUALIFIED`), by
//! key and value (`CATEGORY=FEAT`, `TITLE=Pick one`), with negation
//! (`!TYPE=Exotic`), combined with `,` (all must hold), or narrowed
//! with brackets (`!PC[TYPE=Weapon Focus]`).  Prerequisites may follow.
//! Every value prints back exactly as written.

use super::pre::Prereq;
use super::split::split_top;
use std::fmt;

/// One selection item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// an object name
    Name(String),
    /// `TYPE=x` or `TYPE.x` (the separator is kept)
    Type { sep: char, types: String },
    /// an upper-case keyword: ALL, ANY, PC, QUALIFIED, NOCHOICE, ...
    Keyword(String),
    /// `KEY=value` with an upper-case key
    Pair(String, String),
    /// `!item`
    Not(Box<Item>),
    /// `item[selection]`
    Narrowed(Box<Item>, Vec<Item>),
    /// items joined with `,`: all must hold
    All(Vec<Item>),
    /// a prerequisite (`PRExxx:...` or `!PRExxx:...`), as written
    Pre(String),
}

impl Item {
    /// Parse one `|`-separated part.
    pub fn parse(s: &str) -> Item {
        if is_pre(s) {
            return Item::Pre(s.to_string());
        }
        // `a,b`: only when every part is a selector, since names may
        // contain commas
        let parts = split_top(s, ',');
        if parts.len() > 1 {
            let items: Vec<Item> = parts.iter().map(|p| Item::primitive(p)).collect();
            if items.iter().all(Item::is_selector) {
                return Item::All(items);
            }
            return Item::Name(s.to_string());
        }
        Item::primitive(s)
    }

    fn primitive(s: &str) -> Item {
        if s.ends_with(']') {
            if let Some(open) = bracket_start(s) {
                if open > 0 {
                    let head = Item::primitive(&s[..open]);
                    let inner = &s[open + 1..s.len() - 1];
                    let items = split_top(inner, '|').into_iter().map(Item::parse).collect();
                    return Item::Narrowed(Box::new(head), items);
                }
            }
        }
        if let Some(rest) = s.strip_prefix('!') {
            if !rest.is_empty() {
                return Item::Not(Box::new(Item::primitive(rest)));
            }
        }
        for sep in ['=', '.'] {
            if let Some(t) = s.strip_prefix("TYPE").and_then(|r| r.strip_prefix(sep)) {
                return Item::Type {
                    sep,
                    types: t.to_string(),
                };
            }
        }
        if let Some((k, v)) = s.split_once('=') {
            if is_keyword(k) {
                return Item::Pair(k.to_string(), v.to_string());
            }
        }
        if is_keyword(s) {
            return Item::Keyword(s.to_string());
        }
        Item::Name(s.to_string())
    }

    /// Does the item select by something other than a plain name?
    fn is_selector(&self) -> bool {
        !matches!(self, Item::Name(_))
    }

    /// The prerequisite, if this item is one.
    pub fn prereq(&self) -> Option<Prereq> {
        match self {
            Item::Pre(p) => Prereq::parse(p).ok(),
            _ => None,
        }
    }
}

impl fmt::Display for Item {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Item::Name(n) => f.write_str(n),
            Item::Type { sep, types } => write!(f, "TYPE{}{}", sep, types),
            Item::Keyword(k) => f.write_str(k),
            Item::Pair(k, v) => write!(f, "{}={}", k, v),
            Item::Not(i) => write!(f, "!{}", i),
            Item::Narrowed(h, items) => write!(f, "{}[{}]", h, join(items, "|")),
            Item::All(items) => f.write_str(&join(items, ",")),
            Item::Pre(p) => f.write_str(p),
        }
    }
}

fn join(items: &[Item], sep: &str) -> String {
    items
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(sep)
}

fn is_pre(s: &str) -> bool {
    let t = s.strip_prefix('!').unwrap_or(s);
    t.starts_with("PRE") && t.contains(':') && Prereq::is_pre_tag(t.split(':').next().unwrap_or(""))
}

/// Upper-case word (letters, digits, underscores), as keywords and
/// keys are written.
fn is_keyword(s: &str) -> bool {
    !s.is_empty()
        && s.chars().any(|c| c.is_ascii_uppercase())
        && s.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Index of the `[` matching a final `]`.
fn bracket_start(s: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in s.char_indices().rev() {
        match c {
            ']' => depth += 1,
            '[' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// A CHOOSE, ADD or AUTO value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubToken {
    /// `NUMCHOICES=n|` before the sub-token (CHOOSE)
    pub numchoices: Option<String>,
    /// what is chosen or granted: SKILL, FEAT, ABILITY, WEAPONPROF, ...
    pub kind: String,
    /// an old-style value written `KIND(selection)count` with no `|`
    pub legacy: Option<String>,
    pub items: Vec<Item>,
}

impl SubToken {
    pub fn parse(value: &str) -> SubToken {
        let (numchoices, rest) = match value.strip_prefix("NUMCHOICES=") {
            Some(r) => match r.split_once('|') {
                Some((n, r)) => (Some(n.to_string()), r),
                None => (Some(r.to_string()), ""),
            },
            None => (None, value),
        };
        let parts = split_top(rest, '|');
        let head = parts.first().copied().unwrap_or("");
        // ADD:FEAT(TYPE=Occupation)1
        if let Some(open) = head.find('(') {
            if parts.len() == 1 {
                return SubToken {
                    numchoices,
                    kind: head[..open].to_string(),
                    legacy: Some(head[open..].to_string()),
                    items: Vec::new(),
                };
            }
        }
        SubToken {
            numchoices,
            kind: head.to_string(),
            legacy: None,
            items: parts.iter().skip(1).map(|p| Item::parse(p)).collect(),
        }
    }

    /// A leading count (ADD:CLASSSKILLS|2|...), if the first item is a
    /// number or formula in parentheses.
    pub fn count(&self) -> Option<&str> {
        match self.items.first() {
            Some(Item::Name(n)) if n.parse::<f64>().is_ok() => Some(n),
            Some(Item::Keyword(n)) if n.chars().all(|c| c.is_ascii_digit()) => Some(n),
            _ => None,
        }
    }

    /// The prerequisites among the items.
    pub fn prereqs(&self) -> Vec<Prereq> {
        self.items.iter().filter_map(Item::prereq).collect()
    }

    /// Items other than prerequisites and options such as TITLE=.
    pub fn selection(&self) -> Vec<&Item> {
        self.items
            .iter()
            .filter(|i| {
                !matches!(i, Item::Pre(_))
                    && !matches!(i, Item::Pair(k, _) if k == "TITLE" || k == "NUMCHOICES")
            })
            .collect()
    }

    /// A TITLE= option.
    pub fn title(&self) -> Option<&str> {
        self.items.iter().find_map(|i| match i {
            Item::Pair(k, v) if k == "TITLE" => Some(v.as_str()),
            _ => None,
        })
    }
}

impl fmt::Display for SubToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(n) = &self.numchoices {
            write!(f, "NUMCHOICES={}|", n)?;
        }
        f.write_str(&self.kind)?;
        if let Some(l) = &self.legacy {
            return f.write_str(l);
        }
        for i in &self.items {
            write!(f, "|{}", i)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(v: &str) {
        assert_eq!(SubToken::parse(v).to_string(), v, "round trip of {}", v);
    }

    #[test]
    fn round_trips() {
        for v in [
            "NOCHOICE",
            "SKILL|ALL",
            "SKILL|Ability Skill 2|Ability Skill 3|TITLE=Skill",
            "NUMCHOICES=3|ABILITYSELECTION|FEAT|!PC,QUALIFIED[TYPE=Combat]",
            "WEAPONPROFICIENCY|!PC[TYPE=ExoticRanged]",
            "ABILITYSELECTION|Proficiency|!PC[TYPE=Weapon Focus]",
            "SKILL|ABILITY=FEAT[Feat Choice]",
            "SPELLS|CLASS=Wizard,LEVELMAX=3",
            "SPELLCASTER|Wizard",
            "ABILITY|FEAT|NORMAL|TYPE=General",
            "CLASSSKILLS|3|NONEXCLUSIVE",
            "FEAT(TYPE=Occupation)1",
            "WEAPONPROF|TYPE=Martial|PREVARGTEQ:X,1",
            "LANG|Common|!PRERACE:1,Human",
            "EQUIP|Gauntlet|PREARMORTYPE:1,TYPE.Heavy",
            "SKILL|TYPE.Craft|TYPE.Knowledge",
            "NUMBER|MIN=1|MAX=10|TITLE=Pick a number",
            "STRING|Charm, Psionic|Mind Thrust",
            "USERINPUT|TITLE=Name it",
            "WEAPONPROFICIENCY|DEITYWEAPON",
            "SKILL|",
            "",
        ] {
            round(v);
        }
    }

    #[test]
    fn items() {
        assert_eq!(Item::parse("Dodge"), Item::Name("Dodge".into()));
        assert_eq!(
            Item::parse("TYPE=Combat"),
            Item::Type {
                sep: '=',
                types: "Combat".into()
            }
        );
        assert_eq!(
            Item::parse("TYPE.Craft"),
            Item::Type {
                sep: '.',
                types: "Craft".into()
            }
        );
        assert_eq!(Item::parse("ALL"), Item::Keyword("ALL".into()));
        assert_eq!(
            Item::parse("CATEGORY=FEAT"),
            Item::Pair("CATEGORY".into(), "FEAT".into())
        );
        assert_eq!(
            Item::parse("!TYPE=Exotic"),
            Item::Not(Box::new(Item::Type {
                sep: '=',
                types: "Exotic".into()
            }))
        );
        assert_eq!(
            Item::parse("PRECLASS:1,Wizard=1"),
            Item::Pre("PRECLASS:1,Wizard=1".into())
        );
        assert!(Item::parse("PRECLASS:1,Wizard=1").prereq().is_some());
        // names with commas stay names
        assert_eq!(
            Item::parse("Charm, Psionic"),
            Item::Name("Charm, Psionic".into())
        );
        // a comma joining selectors means all must hold
        assert!(matches!(Item::parse("!PC,QUALIFIED"), Item::All(v) if v.len() == 2));
        // lower-case words with = are names, not pairs
        assert_eq!(Item::parse("a=b"), Item::Name("a=b".into()));
    }

    #[test]
    fn narrowed() {
        match Item::parse("!PC[TYPE=Weapon Focus|TYPE=Other]") {
            Item::Narrowed(h, items) => {
                assert_eq!(*h, Item::Not(Box::new(Item::Keyword("PC".into()))));
                assert_eq!(items.len(), 2);
            }
            x => panic!("{:?}", x),
        }
        assert_eq!(Item::parse("[odd]"), Item::Name("[odd]".into()));
    }

    #[test]
    fn subtoken_parts() {
        let t = SubToken::parse("NUMCHOICES=2|SKILL|Climb|Swim|TITLE=Pick");
        assert_eq!(t.numchoices.as_deref(), Some("2"));
        assert_eq!(t.kind, "SKILL");
        assert_eq!(t.title(), Some("Pick"));
        assert_eq!(t.selection().len(), 2);
        let a = SubToken::parse("CLASSSKILLS|3|NONEXCLUSIVE");
        assert_eq!(a.count(), Some("3"));
        let w = SubToken::parse("WEAPONPROF|Dagger|PREVARGTEQ:X,1");
        assert_eq!(w.prereqs().len(), 1);
        assert_eq!(w.selection(), vec![&Item::Name("Dagger".into())]);
        let l = SubToken::parse("FEAT(TYPE=Occupation)1");
        assert_eq!(l.kind, "FEAT");
        assert_eq!(l.legacy.as_deref(), Some("(TYPE=Occupation)1"));
        assert!(SubToken::parse("SKILL|Climb").count().is_none());
    }
}
