//
// rules/adds.rs -- choices offered by ADD tokens
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! An ADD token offers a choice when the object carrying it is gained
//! (a class level, for one): spellcasting classes to advance
//! (ADD:SPELLCASTER), abilities (ADD:ABILITY), class skills
//! (ADD:CLASSSKILLS) or languages (ADD:LANGUAGE).  This module reads
//! such tokens, lists what they offer a character, and renders the
//! text a character file uses to name the token a choice was made for.

use super::calc::Calc;
use super::spells::spell_type;
use crate::dataset::Object;
use crate::lst::kind::FileKind;
use crate::token::pre::Prereq;
use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddKind {
    SpellCaster,
    /// abilities of a category, taken with a nature (NORMAL, VIRTUAL)
    Ability {
        category: String,
        nature: String,
    },
    ClassSkills,
    Language,
}

/// One ADD token of a kind this module handles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddToken {
    pub kind: AddKind,
    /// how many to choose (a formula)
    pub count: String,
    /// what may be chosen, as written
    pub items: Vec<String>,
    /// prerequisites gating the token
    pub prereqs: Vec<String>,
}

/// Split at `sep` outside parentheses and brackets.
fn split_top(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            c if c == sep && depth <= 0 => {
                out.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

fn is_pre(s: &str) -> bool {
    let s = s.strip_prefix('!').unwrap_or(s);
    s.starts_with("PRE") && s.contains(':')
}

fn type_item(s: &str) -> Option<&str> {
    s.strip_prefix("TYPE=").or_else(|| s.strip_prefix("TYPE."))
}

impl AddToken {
    /// Parse an ADD token's value (`SPELLCASTER|Arcane`, ...).  None
    /// for other kinds of ADD and malformed tokens.
    pub fn parse(value: &str) -> Option<AddToken> {
        let mut parts = split_top(value, '|');
        let mut prereqs = Vec::new();
        while parts.len() > 2 && parts.last().is_some_and(|p| is_pre(p)) {
            prereqs.insert(0, parts.pop()?.to_string());
        }
        let kind = parts.first()?.to_ascii_uppercase();
        let rest = &parts[1..];
        // the leading count is there when one part more than needed is
        let need = if kind == "ABILITY" { 3 } else { 1 };
        let (count, rest) = match rest.len() {
            n if n == need => ("1".to_string(), rest),
            n if n == need + 1 => (rest[0].to_string(), &rest[1..]),
            _ => return None,
        };
        let list = *rest.last()?;
        let items: Vec<String> = match kind.as_str() {
            "ABILITY" => split_top(list, ','),
            _ => list.split(',').collect(),
        }
        .into_iter()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
        let kind = match kind.as_str() {
            "SPELLCASTER" => AddKind::SpellCaster,
            "ABILITY" => AddKind::Ability {
                category: rest[0].to_string(),
                nature: rest[1].to_ascii_uppercase(),
            },
            "CLASSSKILLS" => AddKind::ClassSkills,
            "LANGUAGE" => AddKind::Language,
            _ => return None,
        };
        Some(AddToken {
            kind,
            count,
            items,
            prereqs,
        })
    }

    /// The name a character file gives this kind of choice.
    pub fn set_name(&self) -> &'static str {
        match self.kind {
            AddKind::SpellCaster => "SPELLCASTER",
            AddKind::Ability { .. } => "ABILITY",
            AddKind::ClassSkills => "CLASSSKILLS",
            AddKind::Language => "LANGUAGE",
        }
    }

    /// The text naming what the token offers, as a character file
    /// records it next to the choices made: the references sorted
    /// (single objects before groups, each by name) and joined with
    /// commas.  For spellcasting classes, named classes come first,
    /// then groups, then spell types in the order written.
    pub fn lst_format(&self) -> String {
        match &self.kind {
            AddKind::SpellCaster => {
                let mut singles = Vec::new();
                let mut groups = Vec::new();
                let mut spell_types = Vec::new();
                for it in &self.items {
                    if it.eq_ignore_ascii_case("ANY") {
                        groups.push(Ref::group("ALL", "ANY"));
                    } else if matches!(it.as_str(), "Arcane" | "Divine" | "Psionic") {
                        spell_types.push(it.clone());
                    } else if let Some(t) = type_item(it) {
                        groups.push(Ref::types(t));
                    } else {
                        singles.push(Ref::Single(it.clone()));
                    }
                }
                let mut out = Vec::new();
                for refs in [singles, groups] {
                    if !refs.is_empty() {
                        out.push(join_refs(refs, false));
                    }
                }
                out.extend(spell_types);
                out.join(",")
            }
            AddKind::Ability { .. } => {
                let refs = self
                    .items
                    .iter()
                    .filter(|it| *it != "STACKS" && !it.starts_with("STACKS="))
                    .map(|it| match it.as_str() {
                        "ALL" => Ref::group("ALL", "ALL"),
                        _ => match type_item(it) {
                            Some(t) => Ref::types(t),
                            None => Ref::Single(it.clone()),
                        },
                    })
                    .collect();
                join_refs(refs, true)
            }
            AddKind::ClassSkills => {
                let refs = self
                    .items
                    .iter()
                    .filter(|it| !it.starts_with("AUTORANK="))
                    .map(|it| match it.as_str() {
                        "ALL" | "ANY" => Ref::group("ALL", "ANY"),
                        "UNTRAINED" | "TRAINED" | "EXCLUSIVE" | "NONEXCLUSIVE"
                        | "CROSSCLASSSKILLS" => Ref::group(it, it),
                        _ => match type_item(it) {
                            Some(t) => Ref::types(t),
                            None => Ref::Single(it.clone()),
                        },
                    })
                    .collect();
                join_refs(refs, false)
            }
            AddKind::Language => {
                let refs = self
                    .items
                    .iter()
                    .map(|it| match it.as_str() {
                        "ALL" => Ref::group("ALL", "ALL"),
                        _ => match type_item(it) {
                            Some(t) => Ref::types(t),
                            None => Ref::Single(it.clone()),
                        },
                    })
                    .collect();
                join_refs(refs, false)
            }
        }
    }

    /// Does the character meet the token's prerequisites?
    pub fn passes(&self, calc: &Calc) -> bool {
        self.prereqs.iter().all(|p| match Prereq::parse(p) {
            Ok(p) => calc.passes(&p, None),
            Err(_) => true,
        })
    }

    /// Ranks bought in each skill chosen through ADD:CLASSSKILLS
    /// (AUTORANK=n), if any.
    pub fn auto_rank(&self) -> Option<u32> {
        if self.kind != AddKind::ClassSkills {
            return None;
        }
        self.items
            .iter()
            .find_map(|it| it.strip_prefix("AUTORANK="))
            .and_then(|n| n.trim().parse().ok())
            .filter(|n| *n > 0)
    }

    /// How many to choose.
    pub fn count(&self, calc: &Calc) -> usize {
        match self.count.trim().parse::<f64>() {
            Ok(n) => n.max(0.0) as usize,
            Err(_) => calc.eval_formula(&self.count).max(0.0) as usize,
        }
    }

    /// Keys of what the token offers the character, gaining a level of
    /// `class` (after [`Calc::run`]), sorted by name.
    pub fn options(&self, calc: &Calc, class: &str) -> Vec<String> {
        let mut out: Vec<String> = match &self.kind {
            AddKind::SpellCaster => calc
                .classes()
                .iter()
                .map(|(o, _)| *o)
                .filter(|o| self.items.iter().any(|it| caster_matches(o, it)))
                .map(|o| o.key.clone())
                .collect(),
            AddKind::Ability { category, nature } => {
                let parent = calc.pool_category(category);
                calc.ds
                    .objects(FileKind::Ability)
                    .iter()
                    .filter(|o| {
                        o.category
                            .as_deref()
                            .is_some_and(|c| c.eq_ignore_ascii_case(&parent))
                    })
                    .filter(|o| self.items.iter().any(|it| item_matches(o, it)))
                    .filter(|o| {
                        o.get("VISIBLE")
                            .is_none_or(|v| v.eq_ignore_ascii_case("YES"))
                    })
                    .filter(|o| nature == "VIRTUAL" || calc.failing_prereqs(o).is_empty())
                    .filter(|o| {
                        o.get("MULT").is_some_and(|v| v.eq_ignore_ascii_case("YES"))
                            || !calc.has_ability(Some(&parent), &o.key)
                    })
                    .map(|o| o.key.clone())
                    .collect()
            }
            AddKind::ClassSkills => calc
                .ds
                .objects(FileKind::Skill)
                .iter()
                .filter(|o| self.items.iter().any(|it| skill_matches(o, it)))
                .filter(|o| !calc.is_class_skill_for(&o.key, class))
                .map(|o| o.key.clone())
                .collect(),
            AddKind::Language => {
                let mut known = calc.ch.languages();
                known.extend(calc.auto_languages());
                calc.ds
                    .objects(FileKind::Language)
                    .iter()
                    .filter(|o| {
                        self.items
                            .iter()
                            .any(|it| it == "ALL" || item_matches(o, it))
                    })
                    .filter(|o| !known.iter().any(|k| k.eq_ignore_ascii_case(&o.key)))
                    .map(|o| o.key.clone())
                    .collect()
            }
        };
        out.sort_by(|a, b| collate(a, b));
        out.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        out
    }

    /// A choice as a character file records it.
    pub fn encode(&self, key: &str) -> String {
        match &self.kind {
            AddKind::Ability { category, nature } => {
                format!("CATEGORY={}|NATURE={}|{}", category, nature, key)
            }
            _ => key.to_string(),
        }
    }
}

/// A class offered by an ADD:SPELLCASTER item: a spell type, ANY
/// spellcasting class, a type of spellcasting class, or the class
/// itself.
fn caster_matches(o: &Object, item: &str) -> bool {
    let st = spell_type(o);
    if item.eq_ignore_ascii_case("ANY") {
        return st.is_some();
    }
    if matches!(item, "Arcane" | "Divine" | "Psionic") {
        return st.is_some_and(|t| t.eq_ignore_ascii_case(item));
    }
    if let Some(t) = type_item(item) {
        return st.is_some() && t.split('.').all(|t| o.is_type(t));
    }
    o.key.eq_ignore_ascii_case(item)
}

fn item_matches(o: &Object, item: &str) -> bool {
    match type_item(item) {
        Some(t) => t.split('.').all(|t| o.is_type(t)),
        None => item == "ALL" || o.key.eq_ignore_ascii_case(item),
    }
}

fn skill_matches(o: &Object, item: &str) -> bool {
    let yes = |tag: &str| o.get(tag).map(|v| v.eq_ignore_ascii_case("YES"));
    match item {
        "ALL" | "ANY" => true,
        "UNTRAINED" => yes("USEUNTRAINED") != Some(false),
        "TRAINED" => yes("USEUNTRAINED") == Some(false),
        "EXCLUSIVE" => yes("EXCLUSIVE") == Some(true),
        "NONEXCLUSIVE" | "CROSSCLASSSKILLS" => yes("EXCLUSIVE") != Some(true),
        _ if item.starts_with("AUTORANK=") => false,
        _ => item_matches(o, item),
    }
}

/// A reference in an ADD token: one object, or a group of them.
enum Ref {
    Single(String),
    /// a group, with the name it sorts by and its text
    Group(String, String),
}

impl Ref {
    fn group(name: &str, text: &str) -> Ref {
        Ref::Group(name.to_string(), text.to_string())
    }

    /// A TYPE= group; its types are kept sorted.
    fn types(t: &str) -> Ref {
        let mut types: Vec<&str> = t.split('.').collect();
        types.sort_unstable();
        Ref::Group(
            format!("TYPE [{}]", types.join(", ")),
            format!("TYPE={}", types.join(".")),
        )
    }
}

/// Sort references (single objects first, then groups, each by name)
/// and join their text with commas; `dedup` drops repeats.
fn join_refs(mut refs: Vec<Ref>, dedup: bool) -> String {
    refs.sort_by(|a, b| match (a, b) {
        (Ref::Single(x), Ref::Single(y)) => collate(x, y),
        (Ref::Single(_), Ref::Group(..)) => Ordering::Less,
        (Ref::Group(..), Ref::Single(_)) => Ordering::Greater,
        (Ref::Group(x, _), Ref::Group(y, _)) => collate(x, y),
    });
    let mut out: Vec<&str> = Vec::new();
    for r in &refs {
        let t = match r {
            Ref::Single(s) => s.as_str(),
            Ref::Group(_, t) => t.as_str(),
        };
        if !(dedup && out.last().is_some_and(|l| collate(l, t) == Ordering::Equal)) {
            out.push(t);
        }
    }
    out.join(",")
}

/// Order of punctuation, before digits and letters.
const PUNCT: &str = "_,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";

/// Primary weight of a character, or None if it is ignored at first
/// (spaces and hyphens).
fn weight(c: char) -> Option<(u8, u32)> {
    match c {
        ' ' | '-' => None,
        c if c.is_ascii_digit() => Some((1, c as u32)),
        c if c.is_alphabetic() => Some((2, c.to_lowercase().next().unwrap_or(c) as u32)),
        c => Some((0, PUNCT.find(c).map_or(c as u32 + 128, |i| i as u32))),
    }
}

/// Compare names the way an English-locale collator orders them:
/// letters regardless of case, punctuation before digits before
/// letters, spaces and hyphens only breaking ties; then lowercase
/// before uppercase.
pub fn collate(a: &str, b: &str) -> Ordering {
    let primary = |s: &str| -> Vec<(u8, u32)> { s.chars().filter_map(weight).collect() };
    primary(a)
        .cmp(&primary(b))
        .then_with(|| {
            // fewer and later spaces first, spaces before hyphens
            let ignored = |s: &str| -> Vec<(std::cmp::Reverse<usize>, char)> {
                s.chars()
                    .enumerate()
                    .filter(|(_, c)| weight(*c).is_none())
                    .map(|(i, c)| (std::cmp::Reverse(i), c))
                    .collect()
            };
            let (x, y) = (ignored(a), ignored(b));
            x.len().cmp(&y.len()).then_with(|| x.cmp(&y))
        })
        .then_with(|| {
            let case = |s: &str| -> Vec<bool> { s.chars().map(char::is_uppercase).collect() };
            case(a).cmp(&case(b))
        })
}

/// Does a class level line (`N` or `N:REPEATLEVEL:...`) apply at
/// exactly class level `level`?
pub fn level_line_at(value: &str, level: u32) -> bool {
    let mut parts = value.splitn(3, ':');
    let start: u32 = parts
        .next()
        .and_then(|x| x.trim().parse().ok())
        .unwrap_or(0);
    if start == 0 || level < start {
        return false;
    }
    let (Some("REPEATLEVEL"), Some(spec)) = (parts.next(), parts.next()) else {
        return start == level;
    };
    let mut it = spec.split('|');
    let step: u32 = it.next().and_then(|x| x.parse().ok()).unwrap_or(1).max(1);
    let mut skip = 0;
    let mut max = u32::MAX;
    for p in it {
        if let Some(x) = p.strip_prefix("SKIP=") {
            skip = x.parse().unwrap_or(0);
        } else if let Some(x) = p.strip_prefix("MAX=") {
            max = x.parse().unwrap_or(u32::MAX);
        } else if let Ok(x) = p.parse::<u32>() {
            // old form: inc|consecutive|max
            if skip == 0 {
                skip = x;
            } else {
                max = x;
            }
        }
    }
    (start..=level.min(max))
        .step_by(step as usize)
        .enumerate()
        .filter(|(i, _)| skip == 0 || !(*i as u32 + 1).is_multiple_of(skip + 1))
        .any(|(_, l)| l == level)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lst(v: &str) -> String {
        AddToken::parse(v).unwrap().lst_format()
    }

    #[test]
    fn parses_counts_and_prereqs() {
        let t = AddToken::parse("SPELLCASTER|Arcane").unwrap();
        assert_eq!(t.kind, AddKind::SpellCaster);
        assert_eq!(
            (t.count.as_str(), t.items.clone()),
            ("1", vec!["Arcane".into()])
        );
        let t = AddToken::parse("LANGUAGE|2|TYPE=Spoken|PRECLASS:1,Fighter=2").unwrap();
        assert_eq!(t.count, "2");
        assert_eq!(t.items, ["TYPE=Spoken"]);
        assert_eq!(t.prereqs, ["PRECLASS:1,Fighter=2"]);
        let t = AddToken::parse("ABILITY|2|Special Ability|NORMAL|A (x, y),B").unwrap();
        assert_eq!(
            t.kind,
            AddKind::Ability {
                category: "Special Ability".into(),
                nature: "NORMAL".into()
            }
        );
        assert_eq!(t.count, "2");
        assert_eq!(t.items, ["A (x, y)", "B"]);
        assert!(AddToken::parse("ABILITY|FEAT|NORMAL").is_none());
        assert!(AddToken::parse("FEAT|1").is_none());
        assert!(AddToken::parse("EQUIP|Rope").is_none());
    }

    #[test]
    fn list_formats() {
        assert_eq!(lst("SPELLCASTER|Arcane"), "Arcane");
        assert_eq!(lst("SPELLCASTER|Divine,Arcane"), "Divine,Arcane");
        assert_eq!(lst("SPELLCASTER|1|ANY"), "ANY");
        assert_eq!(lst("SPELLCASTER|Priest,Hermit"), "Hermit,Priest");
        assert_eq!(
            lst("SPELLCASTER|Arcane,Wizard,TYPE=Base"),
            "Wizard,TYPE=Base,Arcane"
        );
        assert_eq!(
            lst("ABILITY|FEAT|NORMAL|TYPE=Metamagic,TYPE=ItemCreation"),
            "TYPE=ItemCreation,TYPE=Metamagic"
        );
        assert_eq!(
            lst("ABILITY|FEAT|NORMAL|TYPE=Combat,Iron Fist"),
            "Iron Fist,TYPE=Combat"
        );
        assert_eq!(lst("ABILITY|FEAT|NORMAL|STACKS,ALL"), "ALL");
        assert_eq!(lst("ABILITY|FEAT|NORMAL|TYPE.B.A"), "TYPE=A.B");
        assert_eq!(lst("CLASSSKILLS|10|ANY"), "ANY");
        assert_eq!(lst("CLASSSKILLS|ALL"), "ANY");
        assert_eq!(
            lst("CLASSSKILLS|4|CROSSCLASSSKILLS,AUTORANK=2"),
            "CROSSCLASSSKILLS"
        );
        assert_eq!(
            lst("CLASSSKILLS|TYPE=Knowledge,Spellcraft,Appraise"),
            "Appraise,Spellcraft,TYPE=Knowledge"
        );
        assert_eq!(lst("LANGUAGE|3|ALL"), "ALL");
        assert_eq!(lst("LANGUAGE|TYPE.Spoken"), "TYPE=Spoken");
        assert_eq!(
            lst("LANGUAGE|Orc,Elven,Naru-Toma,Naru"),
            "Elven,Naru,Naru-Toma,Orc"
        );
    }

    #[test]
    fn auto_ranks() {
        let rank = |v: &str| AddToken::parse(v).unwrap().auto_rank();
        assert_eq!(rank("CLASSSKILLS|4|CROSSCLASSSKILLS,AUTORANK=2"), Some(2));
        assert_eq!(rank("CLASSSKILLS|AUTORANK=1,Climb"), Some(1));
        assert_eq!(rank("CLASSSKILLS|2|ANY"), None);
        assert_eq!(rank("CLASSSKILLS|ANY,AUTORANK=0"), None);
        assert_eq!(rank("CLASSSKILLS|ANY,AUTORANK=x"), None);
        assert_eq!(rank("LANGUAGE|AUTORANK=2"), None);
    }

    #[test]
    fn collation() {
        // as an English collator sorts them
        let mut v = vec![
            "a b",
            "Ab",
            "a-b",
            "ab",
            "aB",
            "a(b)",
            "a_b",
            "a10",
            "a2",
            "aa",
            "B",
            "b",
            "Zed",
            "zed",
            "[x]",
            "abc",
            "ab c",
            "Naru-Toma",
            "NaruA",
            "Naru",
        ];
        v.sort_by(|a, b| collate(a, b));
        assert_eq!(
            v,
            [
                "[x]",
                "a_b",
                "a(b)",
                "a10",
                "a2",
                "aa",
                "ab",
                "aB",
                "Ab",
                "a b",
                "a-b",
                "abc",
                "ab c",
                "b",
                "B",
                "Naru",
                "NaruA",
                "Naru-Toma",
                "zed",
                "Zed"
            ]
        );
    }

    #[test]
    fn level_lines() {
        assert!(level_line_at("3", 3));
        assert!(!level_line_at("3", 4));
        assert!(!level_line_at("0", 0));
        assert!(level_line_at("2:REPEATLEVEL:4", 6));
        assert!(!level_line_at("2:REPEATLEVEL:4", 4));
        assert!(!level_line_at("2:REPEATLEVEL:4|MAX=5", 6));
        assert!(level_line_at("1:REPEATLEVEL:1|SKIP=1", 3));
        assert!(!level_line_at("1:REPEATLEVEL:1|SKIP=1", 2));
    }
}
