//
// pcg/character.rs -- typed view of a PCG character
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! [`Character`] wraps a [`PcgFile`] and offers typed accessors and
//! edits for the parts of a character that matter for rules: stats,
//! race, class levels, skills, abilities, equipment, and so on.  Lines
//! it does not understand are preserved untouched.

use super::{Elem, PcgFile, PcgLine};
use crate::error::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// The order PCGen writes sections in; new lines are inserted after
/// the last existing line of the same or an earlier tag.
const ORDER: &[&str] = &[
    "PCGVERSION",
    "CAMPAIGN",
    "VERSION",
    "ROLLMETHOD",
    "PURCHASEPOINTS",
    "CHARACTERTYPE",
    "PREVIEWSHEET",
    "POOLPOINTS",
    "POOLPOINTSAVAIL",
    "GAMEMODE",
    "TABLABEL",
    "AUTOSPELLS",
    "USEHIGHERKNOWN",
    "USEHIGHERPREPPED",
    "LOADCOMPANIONS",
    "USETEMPMODS",
    "SKILLSOUTPUTORDER",
    "IGNORECOST",
    "ALLOWDEBT",
    "AUTORESIZEGEAR",
    "CHARACTERNAME",
    "TABNAME",
    "PLAYERNAME",
    "HEIGHT",
    "WEIGHT",
    "AGE",
    "GENDER",
    "HANDED",
    "SKINCOLOR",
    "EYECOLOR",
    "HAIRCOLOR",
    "HAIRSTYLE",
    "LOCATION",
    "CITY",
    "BIRTHDAY",
    "BIRTHPLACE",
    "PERSONALITYTRAIT1",
    "PERSONALITYTRAIT2",
    "SPEECHPATTERN",
    "PHOBIAS",
    "INTERESTS",
    "CATCHPHRASE",
    "PORTRAIT",
    "STAT",
    "ALIGN",
    "RACE",
    "FAVOREDCLASS",
    "CLASS",
    "CLASSABILITIESLEVEL",
    "EXPERIENCE",
    "TEMPLATESAPPLIED",
    "REGION",
    "SKILL",
    "LANGUAGE",
    "FEATPOOL",
    "USERPOOL",
    "ABILITY",
    "WEAPONPROF",
    "MONEY",
    "EQUIPNAME",
    "EQUIPSET",
    "CALCEQUIPSET",
    "TEMPBONUS",
    "EQSETBONUS",
    "DEITY",
    "DOMAIN",
    "SPELLBOOK",
    "SPELLLIST",
    "SPELLNAME",
    "CHARACTERBIO",
    "CHARACTERDESC",
    "CHARACTERCOMP",
    "CHARACTERASSET",
    "CHARACTERMAGIC",
    "CHARACTERDMNOTES",
    "KIT",
    "NOTE",
    "AGESET",
];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClassEntry {
    pub name: String,
    pub level: u32,
}

/// One level gained, in the order gained.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LevelEntry {
    pub class: String,
    pub level: u32,
    pub hp: Option<i64>,
    pub skills_gained: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SkillEntry {
    pub name: String,
    /// ranks bought, per class
    pub ranks: Vec<(String, f64)>,
    /// sub-choices (e.g. languages for a linguistics skill), sorted
    /// and joined with ", "
    pub associated: Option<String>,
}

impl SkillEntry {
    pub fn total_ranks(&self) -> f64 {
        self.ranks.iter().map(|(_, r)| r).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AbilityEntry {
    /// ability category (pool) the ability fills
    pub pool: String,
    /// NORMAL (chosen), AUTOMATIC, or VIRTUAL
    pub nature: String,
    pub category: String,
    pub key: String,
    /// choices made; for abilities taken several times without a
    /// choice, one empty entry per time taken
    pub applied_to: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EquipEntry {
    pub name: String,
    pub quantity: f64,
    /// base item for customized equipment
    pub base: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EquipSetEntry {
    /// location name (e.g. "Armor", "Primary Hand", or a container)
    pub location: String,
    pub id: String,
    pub item: Option<String>,
    pub quantity: Option<f64>,
}

/// A temporary bonus applied, as a TEMPBONUS line records it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TempBonus {
    /// kind of object applying it: FEAT, SPELL, EQUIPMENT, ...
    pub kind: String,
    /// that object's key
    pub key: String,
    /// "PC", or the name of the item it is applied to
    pub target: String,
    /// false when switched off
    pub active: bool,
    /// the bonuses, as BONUS token values
    pub bonuses: Vec<String>,
}

impl TempBonus {
    /// Whether it is aimed at the character rather than an item.
    pub fn on_pc(&self) -> bool {
        self.target == "PC"
    }

    /// Whether PCGen keeps it when loading the character: the kind of
    /// source is one it reads back (class sources lose their bonuses),
    /// and an item aimed at is owned.
    pub fn loads(&self, ch: &Character) -> bool {
        matches!(
            self.kind.as_str(),
            "FEAT" | "EQUIPMENT" | "TEMPLATE" | "SKILL" | "SPELL" | "NAME"
        ) && (self.on_pc()
            || ch
                .equipment()
                .iter()
                .any(|e| e.name.eq_ignore_ascii_case(&self.target)))
    }
}

/// Copies of a spell placed in a book or list, as a SPELLNAME line
/// records them.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct SpellCopy {
    pub spell: String,
    pub class: String,
    pub book: String,
    /// the level of the slot it takes (raised by metamagic)
    pub level: i64,
    /// metamagic feat keys, in the order applied
    pub feats: Vec<String>,
    /// the domain whose list it is cast from, if not the class's
    pub domain: Option<String>,
    /// pages one copy takes in a spellbook
    pub pages: i64,
}

/// The equipment set PCGen gives a character that has none.
pub const DEFAULT_SET_ID: &str = "0.1";
pub const DEFAULT_SET_NAME: &str = "Default Set";

/// Whether an EQUIPSET id is a set (`0.N`) rather than an item in one.
pub fn is_set_root(id: &str) -> bool {
    id.strip_prefix("0.").is_some_and(|r| !r.contains('.'))
}

/// Feat keys of a SPELLNAME line's FEATLIST.
fn line_feats(l: &PcgLine) -> Vec<String> {
    l.get("FEATLIST")
        .map(|e| e.children().iter().map(Elem::value_text).collect())
        .unwrap_or_default()
}

/// The domain a SPELLNAME line's SOURCE names, if it is one.
fn line_domain(l: &PcgLine) -> Option<String> {
    let src = l.get("SOURCE")?;
    let kid = |t: &str| {
        src.children()
            .iter()
            .find(|c| c.tag == t)
            .map(Elem::value_text)
    };
    kid("TYPE")
        .is_some_and(|t| t.eq_ignore_ascii_case("DOMAIN"))
        .then(|| kid("NAME"))
        .flatten()
}

/// A character, backed by its PCG file.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Character {
    pub pcg: PcgFile,
    /// where the file was read from or last written to
    #[serde(skip)]
    pub path: Option<PathBuf>,
}

/// A number as saved: no trailing `.0` for whole values.
fn trim_float(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}

/// An amount of money as saved: a plain decimal without trailing zeros
/// (rounded to a ten-thousandth).
pub fn money_text(v: f64) -> String {
    let r = (v * 10000.0).round() / 10000.0;
    let s = format!("{:.4}", r);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".into()
    } else {
        s.to_string()
    }
}

fn num<T: std::str::FromStr>(e: Option<String>) -> Option<T> {
    e.and_then(|s| s.trim().parse().ok())
}

/// `ADD:[SET:what|CHOICE:...]`: choices made for an ADD token.
fn add_elem(set: &str, what: &str, choices: &[String]) -> Elem {
    let mut kids = vec![Elem::text(set, what)];
    kids.extend(choices.iter().map(|c| Elem::text("CHOICE", c)));
    Elem::list("ADD", kids)
}

impl Character {
    pub fn read(path: &Path) -> Result<Character> {
        Ok(Character {
            pcg: PcgFile::read(path)?,
            path: Some(path.to_path_buf()),
        })
    }

    pub fn from_pcg(pcg: PcgFile) -> Character {
        Character { pcg, path: None }
    }

    /// A new, empty character.
    pub fn new(gamemode: &str, campaigns: &[String], name: &str) -> Character {
        let mut pcg = PcgFile::default();
        let l = &mut pcg.lines;
        l.push(PcgLine::new(vec![Elem::raw("PCGVERSION", "2.0")]));
        l.push(PcgLine::blank());
        l.push(PcgLine::comment("System Information"));
        if !campaigns.is_empty() {
            l.push(PcgLine::new(
                campaigns
                    .iter()
                    .map(|c| Elem::text("CAMPAIGN", c))
                    .collect(),
            ));
        }
        l.push(PcgLine::new(vec![Elem::raw("VERSION", "6.09.00")]));
        // what PCGen writes for a character it has just made: no
        // point buy, no stat increases pending, no budget set
        l.push(PcgLine::new(vec![Elem::raw("PURCHASEPOINTS", "N")]));
        l.push(PcgLine::new(vec![Elem::raw("POOLPOINTS", "0")]));
        l.push(PcgLine::new(vec![Elem::raw("POOLPOINTSAVAIL", "-1")]));
        l.push(PcgLine::new(vec![Elem::text("GAMEMODE", gamemode)]));
        for (tag, v) in [
            ("IGNORECOST", "N"),
            ("ALLOWDEBT", "N"),
            ("AUTORESIZEGEAR", "Y"),
        ] {
            l.push(PcgLine::new(vec![Elem::raw(tag, v)]));
        }
        l.push(PcgLine::blank());
        l.push(PcgLine::comment("Character Bio"));
        l.push(PcgLine::new(vec![Elem::text("CHARACTERNAME", name)]));
        l.push(PcgLine::blank());
        l.push(PcgLine::comment("Character Attributes"));
        l.push(PcgLine::blank());
        l.push(PcgLine::comment("Character Equipment"));
        l.push(PcgLine::new(vec![Elem::raw("MONEY", "0")]));
        Character { pcg, path: None }
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        self.pcg.write(path)
    }

    // ---- reading ----

    pub fn version(&self) -> Option<String> {
        self.pcg.value("PCGVERSION")
    }

    pub fn gamemode(&self) -> Option<String> {
        self.pcg.value("GAMEMODE")
    }

    pub fn name(&self) -> Option<String> {
        self.pcg.value("CHARACTERNAME")
    }

    /// Campaign keys, in order.
    pub fn campaigns(&self) -> Vec<String> {
        self.pcg
            .lines_tagged("CAMPAIGN")
            .flat_map(|l| l.elems.iter().flatten())
            .filter(|e| e.tag == "CAMPAIGN")
            .map(Elem::value_text)
            .collect()
    }

    /// (stat abbreviation, score)
    pub fn stats(&self) -> Vec<(String, i64)> {
        self.pcg
            .lines_tagged("STAT")
            .filter_map(|l| Some((l.head()?.value_text(), num(l.text("SCORE")).unwrap_or(0))))
            .collect()
    }

    pub fn stat(&self, abb: &str) -> Option<i64> {
        self.stats()
            .into_iter()
            .find(|(a, _)| a.eq_ignore_ascii_case(abb))
            .map(|(_, s)| s)
    }

    pub fn alignment(&self) -> Option<String> {
        self.pcg.value("ALIGN")
    }

    pub fn race(&self) -> Option<String> {
        self.pcg.value("RACE")
    }

    pub fn deity(&self) -> Option<String> {
        self.pcg.value("DEITY")
    }

    pub fn experience(&self) -> i64 {
        num(self.pcg.value("EXPERIENCE")).unwrap_or(0)
    }

    pub fn classes(&self) -> Vec<ClassEntry> {
        self.pcg
            .lines_tagged("CLASS")
            .filter_map(|l| {
                Some(ClassEntry {
                    name: l.head()?.value_text(),
                    level: num(l.text("LEVEL")).unwrap_or(0),
                })
            })
            .collect()
    }

    pub fn total_level(&self) -> u32 {
        self.classes().iter().map(|c| c.level).sum()
    }

    /// Levels in the order they were gained.
    pub fn levels(&self) -> Vec<LevelEntry> {
        self.pcg
            .lines_tagged("CLASSABILITIESLEVEL")
            .filter_map(|l| {
                let head = l.head()?.value_text();
                let (class, lvl) = head.rsplit_once('=')?;
                Some(LevelEntry {
                    class: class.to_string(),
                    level: lvl.trim().parse().ok()?,
                    hp: num(l.text("HITPOINTS")),
                    skills_gained: num(l.text("SKILLSGAINED")),
                })
            })
            .collect()
    }

    pub fn skills(&self) -> Vec<SkillEntry> {
        self.pcg
            .lines_tagged("SKILL")
            .filter_map(|l| {
                let elems = l.elems.as_ref()?;
                let ranks = elems
                    .iter()
                    .filter(|e| e.tag == "CLASSBOUGHT")
                    .map(|cb| {
                        let get = |t: &str| {
                            cb.children()
                                .iter()
                                .find(|c| c.tag == t)
                                .map(Elem::value_text)
                        };
                        (
                            get("CLASS").unwrap_or_default(),
                            num(get("RANKS")).unwrap_or(0.0),
                        )
                    })
                    .collect();
                Some(SkillEntry {
                    name: l.head()?.value_text(),
                    ranks,
                    associated: {
                        let mut all: Vec<String> = elems
                            .iter()
                            .filter(|e| e.tag == "ASSOCIATEDDATA")
                            .map(Elem::value_text)
                            .collect();
                        all.sort();
                        (!all.is_empty()).then(|| all.join(", "))
                    },
                })
            })
            .collect()
    }

    pub fn abilities(&self) -> Vec<AbilityEntry> {
        self.pcg
            .lines_tagged("ABILITY")
            .filter_map(|l| {
                let elems = l.elems.as_ref()?;
                // TYPE appears twice: nature first, then the ability's types
                let nature = elems
                    .iter()
                    .find(|e| e.tag == "TYPE")
                    .map(Elem::value_text)
                    .unwrap_or_default();
                Some(AbilityEntry {
                    pool: l.head()?.value_text(),
                    nature,
                    category: l.text("CATEGORY").unwrap_or_default(),
                    key: l.text("KEY")?,
                    applied_to: l
                        .text("APPLIEDTO")
                        .filter(|s| !s.is_empty())
                        .map(|s| s.split(',').map(str::to_string).collect())
                        .unwrap_or_default(),
                })
            })
            .collect()
    }

    /// Feats stored in the older one-line-per-feat form.
    /// (key, choices) of each.
    pub fn legacy_feats(&self) -> Vec<(String, Vec<String>)> {
        self.pcg
            .lines_tagged("FEAT")
            .filter_map(|l| {
                let applied = l
                    .text("APPLIEDTO")
                    .filter(|s| !s.is_empty())
                    .map(|s| s.split(',').map(str::to_string).collect())
                    .unwrap_or_default();
                Some((l.head()?.value_text(), applied))
            })
            .collect()
    }

    pub fn languages(&self) -> Vec<String> {
        self.pcg
            .lines_tagged("LANGUAGE")
            .flat_map(|l| l.elems.iter().flatten())
            .filter(|e| e.tag == "LANGUAGE")
            .map(Elem::value_text)
            .collect()
    }

    pub fn templates(&self) -> Vec<String> {
        self.pcg
            .lines_tagged("TEMPLATESAPPLIED")
            .filter_map(|l| {
                let h = l.head()?;
                Some(match h.children().iter().find(|c| c.tag == "NAME") {
                    Some(n) => n.value_text(),
                    None => h.value_text(),
                })
            })
            .collect()
    }

    pub fn domains(&self) -> Vec<String> {
        self.pcg
            .lines_tagged("DOMAIN")
            .filter_map(|l| l.head().map(Elem::value_text))
            .collect()
    }

    pub fn equipment(&self) -> Vec<EquipEntry> {
        self.pcg
            .lines_tagged("EQUIPNAME")
            .filter_map(|l| {
                let base = l.get("CUSTOMIZATION").and_then(|c| {
                    c.children()
                        .iter()
                        .find(|x| x.tag == "BASEITEM")
                        .map(Elem::value_text)
                });
                Some(EquipEntry {
                    name: l.head()?.value_text(),
                    quantity: num(l.text("QUANTITY")).unwrap_or(1.0),
                    base,
                })
            })
            .collect()
    }

    /// Whether temporary bonuses count (USETEMPMODS; on by default).
    pub fn use_temp_mods(&self) -> bool {
        self.flag("USETEMPMODS", true)
    }

    /// Temporary bonuses applied, in file order.  A line naming no
    /// source or no target is passed over, as PCGen does.
    pub fn temp_bonuses(&self) -> Vec<TempBonus> {
        self.pcg
            .lines_tagged("TEMPBONUS")
            .filter_map(|l| {
                let head = l.head()?.value_text();
                let mut parts = head.split('=').filter(|p| !p.is_empty());
                let (kind, key) = (parts.next()?, parts.next()?);
                Some(TempBonus {
                    kind: kind.to_string(),
                    key: key.to_string(),
                    target: l.text("TBTARGET")?,
                    active: l.text("TBACTIVE").is_none_or(|v| v.ends_with('Y')),
                    bonuses: l
                        .elems
                        .iter()
                        .flatten()
                        .filter(|e| e.tag == "TBBONUS")
                        .map(Elem::value_text)
                        .filter(|b| !b.is_empty())
                        .collect(),
                })
            })
            .collect()
    }

    /// Replace the temporary bonuses, written as PCGen writes them:
    /// every bonus sorted by category, targets and text (a bonus whose
    /// text another already has is dropped), and one line per source
    /// and target, written where its first bonus comes, holding all of
    /// its bonuses.
    pub fn set_temp_bonuses(&mut self, list: &[TempBonus]) {
        let mut all: Vec<(String, String, &str, usize)> = Vec::new();
        for (i, tb) in list.iter().enumerate() {
            for b in &tb.bonuses {
                let mut parts = b.split('|');
                let name = parts.next().unwrap_or("").to_string();
                let info = parts.next().unwrap_or("").to_uppercase();
                if !all
                    .iter()
                    .any(|(n, f, t, _)| *n == name && *f == info && t == b)
                {
                    all.push((name, info, b.as_str(), i));
                }
            }
        }
        all.sort_by(|a, b| (&a.0, &a.1, a.2).cmp(&(&b.0, &b.1, b.2)));
        let same = |a: &TempBonus, b: &TempBonus| {
            a.kind == b.kind && a.key == b.key && a.target == b.target
        };
        let mut lines: Vec<PcgLine> = Vec::new();
        let mut done: Vec<usize> = Vec::new();
        for (_, _, _, i) in &all {
            let tb = &list[*i];
            if done.iter().any(|d| same(&list[*d], tb)) {
                continue;
            }
            done.push(*i);
            let mut elems = vec![
                Elem::raw(
                    "TEMPBONUS",
                    &format!("{}={}", tb.kind, super::encode(&tb.key, false)),
                ),
                Elem::text("TBTARGET", &tb.target),
            ];
            if !tb.active {
                elems.push(Elem::raw("TBACTIVE", "N"));
            }
            for (_, _, b, j) in &all {
                if same(&list[*j], tb) {
                    elems.push(Elem::text("TBBONUS", b));
                }
            }
            lines.push(PcgLine::new(elems));
        }
        self.pcg.remove_all("TEMPBONUS");
        for l in lines {
            self.insert_ordered("TEMPBONUS", l);
        }
    }

    /// Turn temporary modifiers on or off: for the character, and for
    /// the equipment set used and what is placed in it.
    pub fn set_temp_mods(&mut self, on: bool) {
        self.set_flag("USETEMPMODS", on);
        let calc = self
            .pcg
            .value("CALCEQUIPSET")
            .unwrap_or_else(|| "0.1".into());
        let inside = format!("{}.", calc);
        for i in self.pcg.indices("EQUIPSET") {
            let l = &self.pcg.lines[i];
            let Some(id) = l.text("ID") else { continue };
            if id != calc && !id.starts_with(&inside) {
                continue;
            }
            let mut elems = l.elems.clone().unwrap_or_default();
            elems.retain(|e| e.tag != "USETEMPMODS");
            if on {
                elems.push(Elem::raw("USETEMPMODS", "Y"));
            }
            self.pcg.lines[i].set_elems(elems);
        }
    }

    pub fn equipsets(&self) -> Vec<EquipSetEntry> {
        self.pcg
            .lines_tagged("EQUIPSET")
            .filter_map(|l| {
                Some(EquipSetEntry {
                    location: l.head()?.value_text(),
                    id: l.text("ID").unwrap_or_default(),
                    item: l.text("VALUE"),
                    quantity: num(l.text("QUANTITY")),
                })
            })
            .collect()
    }

    /// Equipment sets (the top-level EQUIPSET lines, `0.N`) as (id,
    /// name), in file order.
    pub fn equip_set_roots(&self) -> Vec<(String, String)> {
        self.equipsets()
            .into_iter()
            .filter(|e| is_set_root(&e.id))
            .map(|e| (e.id, e.location))
            .collect()
    }

    /// The id of the equipment set used for calculations: the one
    /// CALCEQUIPSET names if there is such a set, else (as PCGen does)
    /// the first set in the file.
    pub fn calc_equip_set(&self) -> String {
        let stored = self.pcg.value("CALCEQUIPSET");
        let sets = self.equipsets();
        if sets.iter().any(|e| Some(&e.id) == stored.as_ref()) {
            return stored.unwrap_or_default();
        }
        sets.into_iter()
            .find(|e| is_set_root(&e.id))
            .map(|e| e.id)
            .or(stored)
            .unwrap_or_else(|| DEFAULT_SET_ID.into())
    }

    /// Items placed in an equipment set (and what they hold).
    pub fn set_items(&self, set: &str) -> Vec<EquipSetEntry> {
        let prefix = format!("{}.", set);
        self.equipsets()
            .into_iter()
            .filter(|e| e.item.is_some() && e.id.starts_with(&prefix))
            .collect()
    }

    /// Items placed in the equipment set used for calculations.
    pub fn equipped(&self) -> Vec<EquipSetEntry> {
        self.set_items(&self.calc_equip_set())
    }

    // ---- editing ----

    /// Insert a line in PCGen's section order.
    fn insert_ordered(&mut self, tag: &str, line: PcgLine) -> usize {
        let pos = ORDER.iter().position(|t| *t == tag).unwrap_or(ORDER.len());
        let earlier: Vec<&str> = ORDER[..pos.min(ORDER.len())]
            .iter()
            .rev()
            .copied()
            .collect();
        // after the last line of this tag, else after the nearest
        // earlier section present
        if !self.pcg.indices(tag).is_empty() {
            return self.pcg.insert_near(tag, &[], line);
        }
        for t in earlier {
            if !self.pcg.indices(t).is_empty() {
                return self.pcg.insert_near(t, &[], line);
            }
        }
        self.pcg.insert_near(tag, &[], line)
    }

    /// Replace the first line with `tag` (or insert one).
    fn put(&mut self, tag: &str, elems: Vec<Elem>) {
        match self.pcg.indices(tag).first() {
            Some(&i) => self.pcg.lines[i].set_elems(elems),
            None => {
                self.insert_ordered(tag, PcgLine::new(elems));
            }
        }
    }

    /// Set a single-valued text field (CHARACTERNAME, AGE, CITY, ...).
    pub fn set_field(&mut self, tag: &str, value: &str) {
        self.put(tag, vec![Elem::text(tag, value)]);
    }

    pub fn set_stat(&mut self, abb: &str, score: i64) {
        let line = PcgLine::new(vec![
            Elem::text("STAT", abb),
            Elem::raw("SCORE", &score.to_string()),
        ]);
        let found = self.pcg.indices("STAT").into_iter().find(|&i| {
            self.pcg.lines[i]
                .head()
                .is_some_and(|h| h.value_text().eq_ignore_ascii_case(abb))
        });
        match found {
            Some(i) => {
                let mut elems = self.pcg.lines[i].elems.clone().unwrap_or_default();
                match elems.iter_mut().find(|e| e.tag == "SCORE") {
                    Some(e) => *e = Elem::raw("SCORE", &score.to_string()),
                    None => elems.push(Elem::raw("SCORE", &score.to_string())),
                }
                self.pcg.lines[i].set_elems(elems);
            }
            None => {
                self.insert_ordered("STAT", line);
            }
        }
    }

    /// Record how the stats were generated (ROLLMETHOD:code with its
    /// EXPRESSION) and, for point buy, the method (PURCHASEPOINTS).
    pub fn set_roll_method(&mut self, code: u32, expression: &str) {
        self.put(
            "ROLLMETHOD",
            vec![
                Elem::raw("ROLLMETHOD", &code.to_string()),
                Elem::raw("EXPRESSION", expression),
            ],
        );
        let purchase = if code == 2 {
            vec![
                Elem::raw("PURCHASEPOINTS", "Y"),
                Elem::raw("TYPE", expression),
            ]
        } else {
            vec![Elem::raw("PURCHASEPOINTS", "N")]
        };
        self.put("PURCHASEPOINTS", purchase);
    }

    /// How the stats were generated: (ROLLMETHOD code, EXPRESSION).
    pub fn roll_method(&self) -> Option<(u32, String)> {
        let l = self.pcg.lines_tagged("ROLLMETHOD").next()?;
        Some((
            num(l.head().map(Elem::value_text))?,
            l.text("EXPRESSION").unwrap_or_default(),
        ))
    }

    /// POOLPOINTS: points spent buying stats, or stat increases not
    /// yet taken.
    pub fn pool_points(&self) -> i64 {
        num(self.pcg.value("POOLPOINTS")).unwrap_or(0)
    }

    pub fn set_pool_points(&mut self, n: i64) {
        self.put("POOLPOINTS", vec![Elem::raw("POOLPOINTS", &n.to_string())]);
    }

    /// POOLPOINTSAVAIL: the point-buy budget, -1 when none is set.
    pub fn pool_budget(&self) -> i64 {
        num(self.pcg.value("POOLPOINTSAVAIL")).unwrap_or(-1)
    }

    pub fn set_pool_budget(&mut self, n: i64) {
        self.put(
            "POOLPOINTSAVAIL",
            vec![Elem::raw("POOLPOINTSAVAIL", &n.to_string())],
        );
    }

    /// The character's funds (MONEY).
    pub fn gold(&self) -> f64 {
        num(self.pcg.value("MONEY")).unwrap_or(0.0)
    }

    /// Set the character's funds.
    pub fn set_gold(&mut self, gold: f64) {
        self.put("MONEY", vec![Elem::raw("MONEY", &money_text(gold))]);
    }

    /// A Y/N setting line (ALLOWDEBT, IGNORECOST, AUTORESIZEGEAR): on
    /// when it ends in Y, else `default` when there is none.
    pub fn flag(&self, tag: &str, default: bool) -> bool {
        match self.pcg.value(tag) {
            Some(v) => v.trim_end().ends_with('Y'),
            None => default,
        }
    }

    pub fn set_flag(&mut self, tag: &str, on: bool) {
        self.put(tag, vec![Elem::raw(tag, if on { "Y" } else { "N" })]);
    }

    /// Record a change to a stat's base score with the latest level
    /// gained (`PRESTAT:ABB=delta`, before its skill points).  False
    /// if the character has no levels.
    pub fn add_stat_change(&mut self, abb: &str, delta: i64) -> bool {
        let Some(&i) = self.pcg.indices("CLASSABILITIESLEVEL").last() else {
            return false;
        };
        let mut elems = self.pcg.lines[i].elems.clone().unwrap_or_default();
        let at = elems
            .iter()
            .position(|e| matches!(e.tag.as_str(), "SKILLSGAINED" | "SKILLSREMAINING"))
            .unwrap_or(elems.len());
        elems.insert(at, Elem::text("PRESTAT", &format!("{}={}", abb, delta)));
        self.pcg.lines[i].set_elems(elems);
        true
    }

    /// Stat changes recorded with the latest level gained.
    pub fn last_level_stat_changes(&self) -> Vec<(String, i64)> {
        let Some(l) = self.pcg.lines_tagged("CLASSABILITIESLEVEL").last() else {
            return Vec::new();
        };
        l.elems
            .iter()
            .flatten()
            .filter(|e| e.tag == "PRESTAT" || e.tag == "POSTSTAT")
            .filter_map(|e| {
                let v = e.value_text();
                let (a, d) = v.split_once('=')?;
                Some((a.to_string(), d.trim().parse().ok()?))
            })
            .collect()
    }

    pub fn set_race(&mut self, race: &str) {
        self.put("RACE", vec![Elem::text("RACE", race)]);
    }

    pub fn set_alignment(&mut self, abb: &str) {
        self.put("ALIGN", vec![Elem::text("ALIGN", abb)]);
    }

    pub fn set_deity(&mut self, deity: Option<&str>) {
        match deity {
            Some(d) => self.put("DEITY", vec![Elem::text("DEITY", d)]),
            None => self.pcg.remove_all("DEITY"),
        }
    }

    pub fn set_experience(&mut self, xp: i64) {
        self.put("EXPERIENCE", vec![Elem::raw("EXPERIENCE", &xp.to_string())]);
    }

    /// Gain one level in a class.  Returns the new class level.
    pub fn add_level(&mut self, class: &str, hp: i64, skills_gained: f64) -> u32 {
        let idx = self.pcg.indices("CLASS").into_iter().find(|&i| {
            self.pcg.lines[i]
                .head()
                .is_some_and(|h| h.value_text().eq_ignore_ascii_case(class))
        });
        let level = match idx {
            Some(i) => {
                let line = &self.pcg.lines[i];
                let lvl = num::<u32>(line.text("LEVEL")).unwrap_or(0) + 1;
                let mut elems = line.elems.clone().unwrap_or_default();
                match elems.iter_mut().find(|e| e.tag == "LEVEL") {
                    Some(e) => *e = Elem::raw("LEVEL", &lvl.to_string()),
                    None => elems.push(Elem::raw("LEVEL", &lvl.to_string())),
                }
                self.pcg.lines[i].set_elems(elems);
                lvl
            }
            None => {
                self.insert_ordered(
                    "CLASS",
                    PcgLine::new(vec![
                        Elem::text("CLASS", class),
                        Elem::raw("LEVEL", "1"),
                        Elem::raw("SKILLPOOL", "0"),
                    ]),
                );
                1
            }
        };
        self.insert_ordered(
            "CLASSABILITIESLEVEL",
            PcgLine::new(vec![
                Elem::text("CLASSABILITIESLEVEL", &format!("{}={}", class, level)),
                Elem::raw("HITPOINTS", &hp.to_string()),
                Elem::raw("SKILLSGAINED", &skills_gained.to_string()),
                Elem::raw("SKILLSREMAINING", &skills_gained.to_string()),
            ]),
        );
        level
    }

    /// Remove the most recently gained level.  Returns its class.
    pub fn remove_last_level(&mut self) -> Option<String> {
        let last = *self.pcg.indices("CLASSABILITIESLEVEL").last()?;
        let head = self.pcg.lines[last].head()?.value_text();
        let (class, _) = head.rsplit_once('=')?;
        let class = class.to_string();
        self.pcg.lines.remove(last);
        if let Some(i) = self.pcg.indices("CLASS").into_iter().find(|&i| {
            self.pcg.lines[i]
                .head()
                .is_some_and(|h| h.value_text() == class)
        }) {
            let lvl = num::<u32>(self.pcg.lines[i].text("LEVEL")).unwrap_or(1);
            if lvl <= 1 {
                self.pcg.lines.remove(i);
            } else {
                let mut elems = self.pcg.lines[i].elems.clone().unwrap_or_default();
                if let Some(e) = elems.iter_mut().find(|e| e.tag == "LEVEL") {
                    *e = Elem::raw("LEVEL", &(lvl - 1).to_string());
                }
                self.pcg.lines[i].set_elems(elems);
            }
        }
        Some(class)
    }

    /// Set the ranks a class has bought in a skill.
    pub fn set_skill_ranks(&mut self, skill: &str, class: &str, ranks: f64, class_skill: bool) {
        let bought = Elem::list(
            "CLASSBOUGHT",
            vec![
                Elem::text("CLASS", class),
                Elem::raw("RANKS", &format!("{:.1}", ranks)),
                Elem::raw("COST", if class_skill { "1" } else { "2" }),
                Elem::raw("CLASSSKILL", if class_skill { "Y" } else { "N" }),
            ],
        );
        let idx = self.pcg.indices("SKILL").into_iter().find(|&i| {
            self.pcg.lines[i]
                .head()
                .is_some_and(|h| h.value_text().eq_ignore_ascii_case(skill))
        });
        match idx {
            Some(i) => {
                let mut elems = self.pcg.lines[i].elems.clone().unwrap_or_default();
                let pos = elems.iter().position(|e| {
                    e.tag == "CLASSBOUGHT"
                        && e.children()
                            .iter()
                            .any(|c| c.tag == "CLASS" && c.value_text() == class)
                });
                match (pos, ranks > 0.0) {
                    (Some(p), true) => elems[p] = bought,
                    (Some(p), false) => {
                        elems.remove(p);
                    }
                    (None, true) => {
                        let at = elems
                            .iter()
                            .position(|e| e.tag == "ASSOCIATEDDATA")
                            .unwrap_or(elems.len());
                        elems.insert(at, bought);
                    }
                    (None, false) => {}
                }
                self.pcg.lines[i].set_elems(elems);
            }
            None if ranks > 0.0 => {
                let order = self.pcg.indices("SKILL").len() + 1;
                self.insert_ordered(
                    "SKILL",
                    PcgLine::new(vec![
                        Elem::text("SKILL", skill),
                        Elem::raw("OUTPUTORDER", &order.to_string()),
                        bought,
                    ]),
                );
            }
            None => {}
        }
    }

    /// Record choices made for an ADD token of a class level:
    /// `ADD:[SET:what|CHOICE:...]`, placed before the level's stat
    /// changes and skill points.
    pub fn add_level_choice(
        &mut self,
        class: &str,
        level: u32,
        set: &str,
        what: &str,
        choices: &[String],
    ) {
        let Some(i) = self.level_line(class, level) else {
            return;
        };
        let mut elems = self.pcg.lines[i].elems.clone().unwrap_or_default();
        let at = elems
            .iter()
            .position(|e| {
                matches!(
                    e.tag.as_str(),
                    "PRESTAT" | "POSTSTAT" | "SKILLSGAINED" | "SKILLSREMAINING"
                )
            })
            .unwrap_or(elems.len());
        elems.insert(at, add_elem(set, what, choices));
        self.pcg.lines[i].set_elems(elems);
    }

    /// Record choices made for an ADD token of a class's own line.
    pub fn add_class_choice(&mut self, class: &str, set: &str, what: &str, choices: &[String]) {
        let Some(i) = self.pcg.indices("CLASS").into_iter().find(|&i| {
            self.pcg.lines[i]
                .head()
                .is_some_and(|h| h.value_text().eq_ignore_ascii_case(class))
        }) else {
            return;
        };
        let mut elems = self.pcg.lines[i].elems.clone().unwrap_or_default();
        elems.push(add_elem(set, what, choices));
        self.pcg.lines[i].set_elems(elems);
    }

    /// Save a bonus with a class level (`SAVES:[SAVE:BONUS|...]`, after
    /// its hit points).
    pub fn add_level_save(&mut self, class: &str, level: u32, bonus: &str) {
        let Some(i) = self.level_line(class, level) else {
            return;
        };
        let mut elems = self.pcg.lines[i].elems.clone().unwrap_or_default();
        let save = Elem::text("SAVE", bonus);
        match elems.iter_mut().find(|e| e.tag == "SAVES") {
            Some(e) => {
                let mut kids = e.children().to_vec();
                kids.push(save);
                *e = Elem::list("SAVES", kids);
            }
            None => {
                let at = elems
                    .iter()
                    .position(|e| e.tag == "HITPOINTS")
                    .map_or(elems.len(), |p| p + 1);
                elems.insert(at, Elem::list("SAVES", vec![save]));
            }
        }
        self.pcg.lines[i].set_elems(elems);
    }

    /// Points added to an ability pool by hand (USERPOOL).
    pub fn user_pool(&self, category: &str) -> f64 {
        self.pcg
            .lines_tagged("USERPOOL")
            .filter(|l| {
                l.head()
                    .is_some_and(|h| h.value_text().eq_ignore_ascii_case(category))
            })
            .filter_map(|l| num::<f64>(l.text("POOLPOINTS")))
            .sum()
    }

    /// Add points to an ability pool by hand.
    pub fn adjust_user_pool(&mut self, category: &str, delta: f64) {
        let total = self.user_pool(category) + delta;
        let value = format!("{:?}", total);
        let idx = self.pcg.indices("USERPOOL").into_iter().find(|&i| {
            self.pcg.lines[i]
                .head()
                .is_some_and(|h| h.value_text().eq_ignore_ascii_case(category))
        });
        match idx {
            Some(i) => self.set_elem(i, "POOLPOINTS", &value),
            None => {
                self.insert_ordered(
                    "USERPOOL",
                    PcgLine::new(vec![
                        Elem::text("USERPOOL", category),
                        Elem::raw("POOLPOINTS", &value),
                    ]),
                );
            }
        }
    }

    /// The level line of a class level.
    fn level_line(&self, class: &str, level: u32) -> Option<usize> {
        self.class_level_lines(class).into_iter().find(|&i| {
            self.pcg.lines[i]
                .head()
                .map(|h| h.value_text())
                .and_then(|h| h.rsplit_once('=').map(|(_, n)| n.trim().to_string()))
                .is_some_and(|n| n == level.to_string())
        })
    }

    /// Add a chosen ability.
    pub fn add_ability(&mut self, pool: &str, category: &str, key: &str, applied_to: &[String]) {
        self.add_ability_as(pool, category, key, "NORMAL", applied_to);
    }

    /// Add an ability the character has by a nature (NORMAL, VIRTUAL).
    pub fn add_ability_as(
        &mut self,
        pool: &str,
        category: &str,
        key: &str,
        nature: &str,
        applied_to: &[String],
    ) {
        let mut elems = vec![
            Elem::text("ABILITY", pool),
            Elem::raw("TYPE", nature),
            Elem::text("CATEGORY", category),
            Elem::text("KEY", key),
        ];
        if !applied_to.is_empty() {
            elems.push(Elem::text("APPLIEDTO", &applied_to.join(",")));
        }
        self.insert_ordered("ABILITY", PcgLine::new(elems));
    }

    /// Remove abilities with this key (and category, if given).
    /// Returns how many were removed.
    pub fn remove_ability(&mut self, category: Option<&str>, key: &str) -> usize {
        let before = self.pcg.lines.len();
        self.pcg.lines.retain(|l| {
            !(l.tag() == Some("ABILITY")
                && l.text("KEY").is_some_and(|k| k.eq_ignore_ascii_case(key))
                && category.is_none_or(|c| {
                    l.text("CATEGORY")
                        .is_some_and(|x| x.eq_ignore_ascii_case(c))
                }))
        });
        before - self.pcg.lines.len()
    }

    pub fn add_language(&mut self, lang: &str) {
        if self
            .languages()
            .iter()
            .any(|l| l.eq_ignore_ascii_case(lang))
        {
            return;
        }
        match self.pcg.indices("LANGUAGE").last() {
            Some(&i) => {
                let mut elems = self.pcg.lines[i].elems.clone().unwrap_or_default();
                elems.push(Elem::text("LANGUAGE", lang));
                self.pcg.lines[i].set_elems(elems);
            }
            None => {
                self.insert_ordered("LANGUAGE", PcgLine::new(vec![Elem::text("LANGUAGE", lang)]));
            }
        }
    }

    pub fn add_template(&mut self, name: &str) {
        self.insert_ordered(
            "TEMPLATESAPPLIED",
            PcgLine::new(vec![Elem::list(
                "TEMPLATESAPPLIED",
                vec![Elem::text("NAME", name)],
            )]),
        );
    }

    pub fn add_domain(&mut self, name: &str) {
        self.insert_ordered("DOMAIN", PcgLine::new(vec![Elem::text("DOMAIN", name)]));
    }

    /// Record a kit as applied.
    pub fn add_kit(&mut self, key: &str) {
        self.insert_ordered("KIT", PcgLine::new(vec![Elem::text("KIT", key)]));
    }

    /// Add a domain granted by a class at one of its levels.
    pub fn add_class_domain(&mut self, name: &str, class: &str, level: u32) {
        self.insert_ordered(
            "DOMAIN",
            PcgLine::new(vec![
                Elem::text("DOMAIN", name),
                Elem::list(
                    "SOURCE",
                    vec![
                        Elem::raw("TYPE", "PCClass"),
                        Elem::text("NAME", class),
                        Elem::raw("LEVEL", &level.to_string()),
                    ],
                ),
            ]),
        );
    }

    /// Domains a given class granted.
    pub fn class_domains_of(&self, class: &str) -> Vec<String> {
        self.pcg
            .lines_tagged("DOMAIN")
            .filter(|l| {
                l.get("SOURCE").is_some_and(|src| {
                    src.children()
                        .iter()
                        .any(|c| c.tag == "NAME" && c.value_text().eq_ignore_ascii_case(class))
                })
            })
            .filter_map(|l| l.head().map(Elem::value_text))
            .collect()
    }

    /// Domains a class granted (their SOURCE names a class).
    pub fn class_domains(&self) -> Vec<String> {
        self.pcg
            .lines_tagged("DOMAIN")
            .filter(|l| {
                l.get("SOURCE").is_some_and(|src| {
                    src.children()
                        .iter()
                        .any(|c| c.tag == "TYPE" && c.value_text().eq_ignore_ascii_case("PCClass"))
                })
            })
            .filter_map(|l| l.head().map(Elem::value_text))
            .collect()
    }

    /// Add equipment (or increase its quantity).
    pub fn add_equipment(&mut self, name: &str, qty: f64) {
        let idx = self.pcg.indices("EQUIPNAME").into_iter().find(|&i| {
            self.pcg.lines[i]
                .head()
                .is_some_and(|h| h.value_text() == name)
        });
        match idx {
            Some(i) => {
                let line = &self.pcg.lines[i];
                let q = num::<f64>(line.text("QUANTITY")).unwrap_or(0.0) + qty;
                let mut elems = line.elems.clone().unwrap_or_default();
                match elems.iter_mut().find(|e| e.tag == "QUANTITY") {
                    Some(e) => *e = Elem::raw("QUANTITY", &format!("{:.1}", q)),
                    None => elems.push(Elem::raw("QUANTITY", &format!("{:.1}", q))),
                }
                self.pcg.lines[i].set_elems(elems);
            }
            None => {
                let order = self.pcg.indices("EQUIPNAME").len() + 1;
                self.insert_ordered(
                    "EQUIPNAME",
                    PcgLine::new(vec![
                        Elem::text("EQUIPNAME", name),
                        Elem::raw("OUTPUTORDER", &order.to_string()),
                        Elem::raw("QUANTITY", &format!("{:.1}", qty)),
                    ]),
                );
            }
        }
    }

    /// Remove equipment and anything placed from it in equipment sets.
    pub fn remove_equipment(&mut self, name: &str) -> bool {
        let before = self.pcg.lines.len();
        self.pcg.lines.retain(|l| {
            !(l.tag() == Some("EQUIPNAME") && l.head().is_some_and(|h| h.value_text() == name))
        });
        self.remove_set_lines(|_, value| value == Some(name));
        before != self.pcg.lines.len()
    }

    /// Remove the EQUIPSET lines `pick` chooses by (id, item), with
    /// everything under them.
    fn remove_set_lines(&mut self, pick: impl Fn(&str, Option<&str>) -> bool) -> bool {
        let gone: Vec<String> = self
            .equipsets()
            .into_iter()
            .filter(|e| pick(&e.id, e.item.as_deref()))
            .map(|e| e.id)
            .collect();
        if gone.is_empty() {
            return false;
        }
        self.pcg.lines.retain(|l| {
            if l.tag() != Some("EQUIPSET") {
                return true;
            }
            let id = l.text("ID").unwrap_or_default();
            !gone.iter().any(|g| {
                id == *g
                    || id
                        .strip_prefix(g.as_str())
                        .is_some_and(|r| r.starts_with('.'))
            })
        });
        true
    }

    /// Keep the EQUIPSET lines in the order PCGen writes them (by id,
    /// as text) and CALCEQUIPSET naming a set that exists.
    fn sync_equip_sets(&mut self) {
        let idx = self.pcg.indices("EQUIPSET");
        let mut lines: Vec<PcgLine> = idx.iter().map(|&i| self.pcg.lines[i].clone()).collect();
        lines.sort_by_key(|l| l.text("ID").unwrap_or_default().to_lowercase());
        for (i, l) in idx.into_iter().zip(lines) {
            self.pcg.lines[i] = l;
        }
        if !self.equip_set_roots().is_empty() {
            let calc = self.calc_equip_set();
            self.put("CALCEQUIPSET", vec![Elem::raw("CALCEQUIPSET", &calc)]);
        }
    }

    /// A free top-level set id, after the highest in use.
    fn next_set_id(&self) -> String {
        let n = self
            .equip_set_roots()
            .iter()
            .filter_map(|(id, _)| id.strip_prefix("0.")?.parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("0.{}", n.saturating_add(1))
    }

    /// The set used for calculations, first making PCGen's default
    /// set if there is none.
    pub fn ensure_equip_set(&mut self) -> String {
        if self.equip_set_roots().is_empty() {
            self.insert_ordered(
                "EQUIPSET",
                PcgLine::new(vec![
                    Elem::text("EQUIPSET", DEFAULT_SET_NAME),
                    Elem::raw("ID", DEFAULT_SET_ID),
                    Elem::raw("USETEMPMODS", "Y"),
                ]),
            );
            self.put(
                "CALCEQUIPSET",
                vec![Elem::raw("CALCEQUIPSET", DEFAULT_SET_ID)],
            );
            self.sync_equip_sets();
        }
        self.calc_equip_set()
    }

    /// Add an empty equipment set; returns its id.
    pub fn add_equip_set(&mut self, name: &str) -> String {
        self.ensure_equip_set();
        let id = self.next_set_id();
        self.insert_ordered(
            "EQUIPSET",
            PcgLine::new(vec![
                Elem::text("EQUIPSET", name),
                Elem::raw("ID", &id),
                Elem::raw("USETEMPMODS", "Y"),
            ]),
        );
        self.sync_equip_sets();
        id
    }

    /// Add a set holding what set `from` holds, in the same places;
    /// returns its id.
    pub fn copy_equip_set(&mut self, from: &str, name: &str) -> String {
        let id = self.next_set_id();
        let prefix = format!("{}.", from);
        let copies: Vec<PcgLine> = self
            .pcg
            .lines_tagged("EQUIPSET")
            .filter_map(|l| {
                let old = l.text("ID")?;
                let new = if old == from {
                    id.clone()
                } else {
                    format!("{}.{}", id, old.strip_prefix(&prefix)?)
                };
                let mut elems = l.elems.clone().unwrap_or_default();
                for e in &mut elems {
                    if e.tag == "ID" {
                        *e = Elem::raw("ID", &new);
                    }
                }
                if old == from {
                    elems[0] = Elem::text("EQUIPSET", name);
                }
                Some(PcgLine::new(elems))
            })
            .collect();
        for l in copies {
            self.insert_ordered("EQUIPSET", l);
        }
        self.sync_equip_sets();
        id
    }

    /// Rename an equipment set.
    pub fn rename_equip_set(&mut self, set: &str, name: &str) -> bool {
        let Some(i) = self
            .pcg
            .indices("EQUIPSET")
            .into_iter()
            .find(|&i| self.pcg.lines[i].text("ID").as_deref() == Some(set))
        else {
            return false;
        };
        let mut elems = self.pcg.lines[i].elems.clone().unwrap_or_default();
        elems[0] = Elem::text("EQUIPSET", name);
        self.pcg.lines[i].set_elems(elems);
        true
    }

    /// Remove an equipment set and everything in it.  If it was the
    /// set used for calculations, the first remaining set is.
    pub fn remove_equip_set(&mut self, set: &str) -> bool {
        let done = self.remove_set_lines(|id, _| id == set);
        self.sync_equip_sets();
        done
    }

    /// Use an equipment set for calculations.  As in PCGen, the set's
    /// own temporary-bonus setting becomes the character's.
    pub fn use_equip_set(&mut self, set: &str) {
        let temp = self
            .pcg
            .lines_tagged("EQUIPSET")
            .find(|l| l.text("ID").as_deref() == Some(set))
            .and_then(|l| l.text("USETEMPMODS"))
            .is_some_and(|v| v.eq_ignore_ascii_case("Y"));
        self.put("CALCEQUIPSET", vec![Elem::raw("CALCEQUIPSET", set)]);
        self.put(
            "USETEMPMODS",
            vec![Elem::raw("USETEMPMODS", if temp { "Y" } else { "N" })],
        );
    }

    /// Give up some of an item: all of it (with its places in
    /// equipment sets) when no more than `qty` is had.  Returns how
    /// many were given up.
    pub fn reduce_equipment(&mut self, name: &str, qty: f64) -> f64 {
        let Some(i) = self.pcg.indices("EQUIPNAME").into_iter().find(|&i| {
            self.pcg.lines[i]
                .head()
                .is_some_and(|h| h.value_text() == name)
        }) else {
            return 0.0;
        };
        let have = num::<f64>(self.pcg.lines[i].text("QUANTITY")).unwrap_or(1.0);
        if qty >= have {
            self.remove_equipment(name);
            return have;
        }
        self.set_elem(i, "QUANTITY", &format!("{:.1}", have - qty));
        qty
    }

    /// Place an item at a location in the calculation equipment set.
    pub fn equip(&mut self, name: &str, location: &str, qty: f64) {
        let set = self.ensure_equip_set();
        self.equip_in(&set, name, location, qty);
    }

    /// Place an item at a location in an equipment set.
    pub fn equip_in(&mut self, set: &str, name: &str, location: &str, qty: f64) {
        let prefix = format!("{}.", set);
        let next = self
            .equipsets()
            .iter()
            .filter_map(|e| {
                e.id.strip_prefix(&prefix)?
                    .split('.')
                    .next()?
                    .parse::<u32>()
                    .ok()
            })
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        self.insert_ordered(
            "EQUIPSET",
            PcgLine::new(vec![
                Elem::text("EQUIPSET", location),
                // numbered as PCGen numbers them: 01, 02, ...
                Elem::raw("ID", &format!("{}{:02}", prefix, next)),
                Elem::text("VALUE", name),
                Elem::raw("QUANTITY", &format!("{:.1}", qty)),
                Elem::raw("USETEMPMODS", "Y"),
            ]),
        );
        self.sync_equip_sets();
    }

    /// Skill points left unspent from a class's levels.
    pub fn skill_points_left(&self, class: &str) -> f64 {
        self.class_level_lines(class)
            .iter()
            .map(|&i| num::<f64>(self.pcg.lines[i].text("SKILLSREMAINING")).unwrap_or(0.0))
            .sum()
    }

    /// Spend (or, if negative, refund) skill points of a class: taken
    /// from its latest levels first, refunded to its latest level.
    pub fn spend_skill_points(&mut self, class: &str, amount: f64) {
        let lines = self.class_level_lines(class);
        let mut left = amount;
        for &i in lines.iter().rev() {
            if left == 0.0 {
                break;
            }
            let have = num::<f64>(self.pcg.lines[i].text("SKILLSREMAINING")).unwrap_or(0.0);
            let take = if left < 0.0 { left } else { left.min(have) };
            if take == 0.0 {
                continue;
            }
            self.set_elem(i, "SKILLSREMAINING", &trim_float(have - take));
            left -= take;
        }
        // anything still owed comes from the latest level
        if left != 0.0 {
            if let Some(&i) = lines.last() {
                let have = num::<f64>(self.pcg.lines[i].text("SKILLSREMAINING")).unwrap_or(0.0);
                self.set_elem(i, "SKILLSREMAINING", &trim_float(have - left));
            }
        }
    }

    fn class_level_lines(&self, class: &str) -> Vec<usize> {
        self.pcg
            .indices("CLASSABILITIESLEVEL")
            .into_iter()
            .filter(|&i| {
                self.pcg.lines[i]
                    .head()
                    .map(|h| h.value_text())
                    .and_then(|h| h.rsplit_once('=').map(|(c, _)| c.to_string()))
                    .is_some_and(|c| c.eq_ignore_ascii_case(class))
            })
            .collect()
    }

    /// Set (replace) one element of a line.
    fn set_elem(&mut self, i: usize, tag: &str, value: &str) {
        let mut elems = self.pcg.lines[i].elems.clone().unwrap_or_default();
        match elems.iter_mut().find(|e| e.tag == tag) {
            Some(e) => *e = Elem::raw(tag, value),
            None => elems.push(Elem::raw(tag, value)),
        }
        self.pcg.lines[i].set_elems(elems);
    }

    /// Choose a class's subclass.
    pub fn set_subclass(&mut self, class: &str, subclass: &str) -> bool {
        let Some(i) = self.pcg.indices("CLASS").into_iter().find(|&i| {
            self.pcg.lines[i]
                .head()
                .is_some_and(|h| h.value_text().eq_ignore_ascii_case(class))
        }) else {
            return false;
        };
        let mut elems = self.pcg.lines[i].elems.clone().unwrap_or_default();
        let sub = Elem::text("SUBCLASS", subclass);
        match elems.iter().position(|e| e.tag == "SUBCLASS") {
            Some(p) => elems[p] = sub,
            None => elems.insert(1.min(elems.len()), sub),
        }
        self.pcg.lines[i].set_elems(elems);
        true
    }

    /// Spells saved for a class: (spell, class, book, level, times).
    pub fn spells(&self) -> Vec<(String, String, String, i64, i64)> {
        self.pcg
            .lines_tagged("SPELLNAME")
            .filter_map(|l| {
                Some((
                    l.head()?.value_text(),
                    l.text("CLASS")?,
                    l.text("BOOK").unwrap_or_else(|| "Known Spells".into()),
                    num(l.text("SPELLLEVEL")).unwrap_or(0),
                    num(l.text("TIMES")).unwrap_or(1),
                ))
            })
            .collect()
    }

    /// Spell books and lists recorded with SPELLBOOK lines: (name,
    /// type).  Types: 1 known spells, 2 prepared list, 3 spellbook
    /// (an item), 4 innate.
    pub fn spell_books(&self) -> Vec<(String, i64)> {
        self.pcg
            .lines_tagged("SPELLBOOK")
            .filter_map(|l| Some((l.head()?.value_text(), num(l.text("TYPE")).unwrap_or(2))))
            .collect()
    }

    /// Pages the spells written in a book take (their SPELLNUMPAGES).
    pub fn spell_book_pages(&self, book: &str) -> i64 {
        self.pcg
            .lines_tagged("SPELLNAME")
            .filter(|l| l.text("BOOK").as_deref() == Some(book))
            .filter_map(|l| num::<i64>(l.text("SPELLNUMPAGES")))
            .sum()
    }

    /// Record a spell book or list of a type (see [`Self::spell_books`]).
    pub fn add_spell_book(&mut self, name: &str, kind: i64) {
        self.insert_ordered(
            "SPELLBOOK",
            PcgLine::new(vec![
                Elem::text("SPELLBOOK", name),
                Elem::raw("TYPE", &kind.to_string()),
            ]),
        );
    }

    /// Remove a spell book or list and every spell in it.  Returns
    /// whether there was one.
    pub fn remove_spell_book(&mut self, name: &str) -> bool {
        let before = self.pcg.lines.len();
        self.pcg.lines.retain(|l| match l.tag() {
            Some("SPELLBOOK") => l.head().is_none_or(|h| h.value_text() != name),
            Some("SPELLNAME") => l.text("BOOK").as_deref() != Some(name),
            _ => true,
        });
        before != self.pcg.lines.len()
    }

    /// Add a spell to a class's book at a level.  A book not yet
    /// recorded is made a prepared list.  Another copy of a spell
    /// already in a list adds to the times on its line.
    pub fn add_spell(&mut self, spell: &str, class: &str, book: &str, level: i64, times: i64) {
        let copy = SpellCopy {
            spell: spell.into(),
            class: class.into(),
            book: book.into(),
            level,
            ..Default::default()
        };
        self.add_spell_copy(&copy, times);
    }

    /// Add copies of a spell to a book or list (made a prepared list if
    /// not yet recorded).  Copies already in a list (at the same level,
    /// with the same feats) add to the times on their line, and their
    /// pages to its pages.  Returns the times now on the line.
    pub fn add_spell_copy(&mut self, c: &SpellCopy, times: i64) -> i64 {
        if c.book != "Known Spells" && self.spell_books().iter().all(|(b, _)| *b != c.book) {
            self.add_spell_book(&c.book, 2);
        }
        if c.book != "Known Spells" {
            let found = self
                .spell_lines(&c.spell, &c.class, Some(&c.book), Some(c.level))
                .into_iter()
                .find(|&i| {
                    let l = &self.pcg.lines[i];
                    line_feats(l) == c.feats && line_domain(l) == c.domain
                });
            if let Some(i) = found {
                let l = &self.pcg.lines[i];
                let had = num::<i64>(l.text("TIMES")).unwrap_or(1);
                let pages = num::<i64>(l.text("SPELLNUMPAGES")).unwrap_or(0);
                self.set_elem(i, "TIMES", &(had + times).to_string());
                if c.pages != 0 {
                    self.set_elem(i, "SPELLNUMPAGES", &(pages + c.pages * times).to_string());
                }
                return had + times;
            }
        }
        let mut elems = vec![
            Elem::text("SPELLNAME", &c.spell),
            Elem::raw("TIMES", &times.to_string()),
            Elem::text("CLASS", &c.class),
            Elem::text("BOOK", &c.book),
            Elem::raw("SPELLLEVEL", &c.level.to_string()),
        ];
        if c.pages > 0 {
            elems.push(Elem::raw("SPELLNUMPAGES", &(c.pages * times).to_string()));
        }
        if !c.feats.is_empty() {
            elems.push(Elem::list(
                "FEATLIST",
                c.feats.iter().map(|f| Elem::text("FEAT", f)).collect(),
            ));
        }
        elems.push(Elem::list(
            "SOURCE",
            match &c.domain {
                Some(d) => vec![Elem::raw("TYPE", "DOMAIN"), Elem::text("NAME", d)],
                None => vec![Elem::raw("TYPE", "CLASS"), Elem::text("NAME", &c.class)],
            },
        ));
        self.insert_ordered("SPELLNAME", PcgLine::new(elems));
        times
    }

    /// SPELLNAME lines of a class's spell, optionally only in a book
    /// and at a level.
    fn spell_lines(
        &self,
        spell: &str,
        class: &str,
        book: Option<&str>,
        level: Option<i64>,
    ) -> Vec<usize> {
        self.pcg
            .indices("SPELLNAME")
            .into_iter()
            .filter(|&i| {
                let l = &self.pcg.lines[i];
                l.head()
                    .is_some_and(|h| h.value_text().eq_ignore_ascii_case(spell))
                    && l.text("CLASS")
                        .is_some_and(|c| c.eq_ignore_ascii_case(class))
                    && book.is_none_or(|b| {
                        l.text("BOOK")
                            .unwrap_or_else(|| "Known Spells".into())
                            .eq_ignore_ascii_case(b)
                    })
                    && level.is_none_or(|v| num::<i64>(l.text("SPELLLEVEL")).unwrap_or(0) == v)
            })
            .collect()
    }

    /// Copies of a class's spell saved in books and lists.
    pub fn spell_copies(
        &self,
        spell: &str,
        class: &str,
        book: Option<&str>,
    ) -> Vec<(SpellCopy, i64)> {
        self.spell_lines(spell, class, book, None)
            .into_iter()
            .map(|i| {
                let l = &self.pcg.lines[i];
                let times = num::<i64>(l.text("TIMES")).unwrap_or(1);
                let pages = num::<i64>(l.text("SPELLNUMPAGES")).unwrap_or(0);
                (
                    SpellCopy {
                        spell: l.head().map(Elem::value_text).unwrap_or_default(),
                        class: l.text("CLASS").unwrap_or_default(),
                        book: l.text("BOOK").unwrap_or_else(|| "Known Spells".into()),
                        level: num(l.text("SPELLLEVEL")).unwrap_or(0),
                        feats: line_feats(l),
                        domain: line_domain(l),
                        pages: if times > 0 { pages / times } else { pages },
                    },
                    times,
                )
            })
            .collect()
    }

    /// Remove one copy of a spell from a class's book: the times on
    /// its latest line go down by one, and the line goes at none.
    /// Returns whether there was one.
    pub fn remove_spell(&mut self, spell: &str, class: &str, book: Option<&str>) -> bool {
        self.remove_spell_copy(spell, class, book, None)
    }

    /// Remove one copy of a spell, optionally only one prepared with
    /// the given feats (none for a plain copy): the times on its latest
    /// line go down by one (and its pages by one copy's), and the line
    /// goes at none.  Returns whether there was one.
    pub fn remove_spell_copy(
        &mut self,
        spell: &str,
        class: &str,
        book: Option<&str>,
        feats: Option<&[String]>,
    ) -> bool {
        let Some(i) = self
            .spell_lines(spell, class, book, None)
            .into_iter()
            .rev()
            .find(|&i| feats.is_none_or(|f| line_feats(&self.pcg.lines[i]) == f))
        else {
            return false;
        };
        let l = &self.pcg.lines[i];
        let had = num::<i64>(l.text("TIMES")).unwrap_or(1);
        let pages = num::<i64>(l.text("SPELLNUMPAGES")).unwrap_or(0);
        if had > 1 {
            self.set_elem(i, "TIMES", &(had - 1).to_string());
            if pages > 0 {
                self.set_elem(i, "SPELLNUMPAGES", &(pages - pages / had).to_string());
            }
        } else {
            self.pcg.lines.remove(i);
        }
        true
    }

    /// Followers: (name, type, race, file).
    pub fn followers(&self) -> Vec<(String, String, String, String)> {
        self.pcg
            .lines_tagged("FOLLOWER")
            .filter_map(|l| {
                Some((
                    l.head()?.value_text(),
                    l.text("TYPE").unwrap_or_default(),
                    l.text("RACE").unwrap_or_default(),
                    l.text("FILE").unwrap_or_default(),
                ))
            })
            .collect()
    }

    /// The FOLLOWER lines PCGen keeps when it loads the character:
    /// those naming the follower, its companion list and a file it
    /// finds, beside the character's own file or as written.  (PCGen
    /// also looks in its character directory, which is not known here.)
    /// A character not read from a file keeps them all.
    pub fn loaded_followers(&self) -> impl Iterator<Item = &PcgLine> {
        self.pcg.lines_tagged("FOLLOWER").filter(|l| {
            let named = l.head().is_some_and(|h| !h.value_text().is_empty());
            let typed = l.text("TYPE").is_some_and(|t| !t.is_empty());
            named && typed && l.text("FILE").is_some_and(|f| self.follower_file_found(&f))
        })
    }

    /// Does PCGen find a follower file the character names?
    pub fn follower_file_found(&self, file: &str) -> bool {
        let Some(path) = &self.path else {
            return true;
        };
        if file.is_empty() {
            return false;
        }
        let beside = path.parent().unwrap_or(Path::new(".")).join(file);
        beside.exists() || Path::new(file).exists()
    }

    pub fn add_follower(&mut self, name: &str, kind: &str, race: &str, file: &str) {
        self.insert_ordered(
            "FOLLOWER",
            PcgLine::new(vec![
                Elem::text("FOLLOWER", name),
                Elem::text("TYPE", kind),
                Elem::text("RACE", race),
                Elem::raw("HD", "0"),
                Elem::text("FILE", file),
            ]),
        );
    }

    pub fn remove_follower(&mut self, name: &str) -> bool {
        let before = self.pcg.lines.len();
        self.pcg.lines.retain(|l| {
            !(l.tag() == Some("FOLLOWER")
                && l.head()
                    .is_some_and(|h| h.value_text().eq_ignore_ascii_case(name)))
        });
        before != self.pcg.lines.len()
    }

    /// An item's saved customization: (base item, DATA text).
    pub fn customization(&self, name: &str) -> Option<(String, String)> {
        let l = self
            .pcg
            .lines_tagged("EQUIPNAME")
            .find(|l| l.head().is_some_and(|h| h.value_text() == name))?;
        let c = l.get("CUSTOMIZATION")?;
        let base = c
            .children()
            .iter()
            .find(|e| e.tag == "BASEITEM")?
            .value_text();
        let data = c
            .children()
            .iter()
            .find(|e| e.tag == "DATA")
            .map(|e| e.value_text())
            .unwrap_or_default();
        Some((base, data))
    }

    /// Set an item's customization (base item and DATA text).
    pub fn set_customization(&mut self, name: &str, base: &str, data: &str) -> bool {
        let Some(i) = self.pcg.indices("EQUIPNAME").into_iter().find(|&i| {
            self.pcg.lines[i]
                .head()
                .is_some_and(|h| h.value_text() == name)
        }) else {
            return false;
        };
        let mut elems = self.pcg.lines[i].elems.clone().unwrap_or_default();
        let cust = Elem::list(
            "CUSTOMIZATION",
            vec![Elem::text("BASEITEM", base), Elem::text("DATA", data)],
        );
        match elems.iter().position(|e| e.tag == "CUSTOMIZATION") {
            Some(p) => elems[p] = cust,
            None => elems.push(cust),
        }
        self.pcg.lines[i].set_elems(elems);
        true
    }

    /// Remove an item (and what it holds) from the calculation
    /// equipment set.
    pub fn unequip(&mut self, name: &str) -> bool {
        let set = self.calc_equip_set();
        self.unequip_in(&set, name)
    }

    /// Remove an item (and what it holds) from an equipment set.
    pub fn unequip_in(&mut self, set: &str, name: &str) -> bool {
        let prefix = format!("{}.", set);
        self.remove_set_lines(|id, value| id.starts_with(&prefix) && value == Some(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PCG: &str = "PCGVERSION:2.0\n\
CAMPAIGN:Book A|CAMPAIGN:Book B\n\
GAMEMODE:Testmode\n\
CHARACTERNAME:Tess\n\
STAT:MGT|SCORE:14\n\
STAT:GRC|SCORE:10\n\
ALIGN:LG\n\
RACE:Human\n\
CLASS:Tinker|LEVEL:2|SKILLPOOL:0\n\
CLASSABILITIESLEVEL:Tinker=1|HITPOINTS:6|SKILLSGAINED:4|SKILLSREMAINING:0\n\
CLASSABILITIESLEVEL:Tinker=2|HITPOINTS:4|SKILLSGAINED:4|SKILLSREMAINING:0\n\
EXPERIENCE:1500\n\
TEMPLATESAPPLIED:[NAME:Clockwork]\n\
SKILL:Ride|OUTPUTORDER:1|CLASSBOUGHT:[CLASS:Tinker|RANKS:2.0|COST:1|CLASSSKILL:Y]\n\
SKILL:Swim|OUTPUTORDER:2|\n\
LANGUAGE:Common|LANGUAGE:Gnomish\n\
ABILITY:FEAT|TYPE:NORMAL|CATEGORY:FEAT|KEY:Steady Hands|TYPE:General\n\
ABILITY:FEAT|TYPE:NORMAL|CATEGORY:FEAT|KEY:Keen Eye|APPLIEDTO:Ride,Swim\n\
EQUIPNAME:Rope|OUTPUTORDER:1|QUANTITY:2.0\n\
EQUIPNAME:Shiny Sword|OUTPUTORDER:2|QUANTITY:1.0|CUSTOMIZATION:[BASEITEM:Sword|DATA:EQMOD=SHINY]\n\
EQUIPSET:Default Set|ID:0.1\n\
EQUIPSET:Primary Hand|ID:0.1.1|VALUE:Shiny Sword|QUANTITY:1.0\n\
CALCEQUIPSET:0.1\n\
DEITY:Sun God|DEITYDOMAINS:[DOMAIN:Light]\n\
DOMAIN:Light|SOURCE:[TYPE:PCClass|NAME:Tinker|LEVEL:1]\n";

    fn ch() -> Character {
        Character::from_pcg(PcgFile::parse(PCG))
    }

    #[test]
    fn skill_point_bookkeeping() {
        let mut c = ch();
        assert_eq!(c.skill_points_left("Tinker"), 0.0);
        let lvl = c.add_level("Tinker", 5, 4.0);
        assert_eq!(lvl, 3);
        assert_eq!(c.skill_points_left("Tinker"), 4.0);
        c.spend_skill_points("Tinker", 3.0);
        assert_eq!(c.skill_points_left("Tinker"), 1.0);
        c.spend_skill_points("Tinker", -2.0);
        assert_eq!(c.skill_points_left("Tinker"), 3.0);
        // overspending leaves a debt on the latest level
        c.spend_skill_points("Tinker", 5.0);
        assert_eq!(c.skill_points_left("Tinker"), -2.0);
        assert_eq!(c.skill_points_left("Nobody"), 0.0);
    }

    #[test]
    fn subclasses() {
        let mut c = ch();
        assert!(c.set_subclass("Tinker", "Gearwright"));
        let text = |c: &Character| String::from_utf8_lossy(&c.pcg.encode()).into_owned();
        assert!(text(&c).contains("CLASS:Tinker|SUBCLASS:Gearwright|LEVEL:2"));
        assert!(c.set_subclass("tinker", "Clocksmith"));
        assert!(text(&c).contains("SUBCLASS:Clocksmith"));
        assert!(!c.set_subclass("Nobody", "X"));
    }

    #[test]
    fn spells_and_books() {
        let mut c = ch();
        c.add_spell("Spark", "Tinker", "Gadgets", 1, 2);
        c.add_spell("Glow", "Tinker", "Known Spells", 0, 1);
        let sp = c.spells();
        assert!(sp.contains(&("Spark".into(), "Tinker".into(), "Gadgets".into(), 1, 2)));
        assert!(c
            .pcg
            .lines_tagged("SPELLBOOK")
            .any(|l| l.head().unwrap().value_text() == "Gadgets"));
        // two copies, removed one at a time
        assert!(c.remove_spell("spark", "Tinker", Some("Gadgets")));
        assert!(c
            .spells()
            .contains(&("Spark".into(), "Tinker".into(), "Gadgets".into(), 1, 1)));
        assert!(c.remove_spell("Spark", "Tinker", None));
        assert!(!c.remove_spell("Spark", "Tinker", None));
        assert_eq!(c.spells().len(), 1);
    }

    #[test]
    fn money_amounts() {
        assert_eq!(money_text(0.0), "0");
        assert_eq!(money_text(-0.0), "0");
        assert_eq!(money_text(473.0), "473");
        assert_eq!(money_text(480.5), "480.5");
        assert_eq!(money_text(-202.0), "-202");
        assert_eq!(money_text(0.1 + 0.2), "0.3");
        assert_eq!(money_text(12.345678), "12.3457");
        assert_eq!(money_text(1000000.0), "1000000");
    }

    #[test]
    fn gold_flags_and_partial_removal() {
        let mut c = ch();
        assert_eq!(c.gold(), 0.0);
        c.set_gold(12.5);
        assert_eq!(c.gold(), 12.5);
        assert!(c.pcg.value("MONEY").is_some_and(|m| m == "12.5"));
        assert!(!c.flag("ALLOWDEBT", false));
        assert!(c.flag("AUTORESIZEGEAR", true));
        c.set_flag("ALLOWDEBT", true);
        assert!(c.flag("ALLOWDEBT", false));
        assert_eq!(c.reduce_equipment("Rope", 1.0), 1.0);
        assert_eq!(c.equipment()[0].quantity, 1.0);
        assert_eq!(c.reduce_equipment("Shiny Sword", 3.0), 1.0);
        assert!(c.equipped().is_empty());
        assert_eq!(c.reduce_equipment("Nothing", 1.0), 0.0);
    }

    #[test]
    fn followers_list() {
        let mut c = ch();
        c.add_follower("Whiskers", "Familiar", "Cat", "whiskers.pcg");
        assert_eq!(
            c.followers(),
            vec![(
                "Whiskers".into(),
                "Familiar".into(),
                "Cat".into(),
                "whiskers.pcg".into()
            )]
        );
        assert!(c.remove_follower("whiskers"));
        assert!(c.followers().is_empty());
    }

    #[test]
    fn equipment_customization() {
        let mut c = ch();
        assert_eq!(
            c.customization("Shiny Sword"),
            Some(("Sword".into(), "EQMOD=SHINY".into()))
        );
        assert!(c.set_customization("Rope", "Rope", "NAME=Rope$SIZE=S"));
        assert_eq!(
            c.customization("Rope"),
            Some(("Rope".into(), "NAME=Rope$SIZE=S".into()))
        );
        assert!(!c.set_customization("Nothing", "x", "y"));
    }

    #[test]
    fn read_fields() {
        let c = ch();
        assert!(c.pcg.diags.is_empty());
        assert_eq!(c.version().as_deref(), Some("2.0"));
        assert_eq!(c.campaigns(), vec!["Book A", "Book B"]);
        assert_eq!(c.gamemode().as_deref(), Some("Testmode"));
        assert_eq!(c.name().as_deref(), Some("Tess"));
        assert_eq!(c.stat("mgt"), Some(14));
        assert_eq!(c.alignment().as_deref(), Some("LG"));
        assert_eq!(c.race().as_deref(), Some("Human"));
        assert_eq!(c.total_level(), 2);
        let lv = c.levels();
        assert_eq!(lv.len(), 2);
        assert_eq!(lv[1].hp, Some(4));
        assert_eq!(c.experience(), 1500);
        assert_eq!(c.templates(), vec!["Clockwork"]);
        let sk = c.skills();
        assert_eq!(sk[0].total_ranks(), 2.0);
        assert_eq!(sk[1].total_ranks(), 0.0);
        assert_eq!(c.languages(), vec!["Common", "Gnomish"]);
        let ab = c.abilities();
        assert_eq!(ab[0].nature, "NORMAL");
        assert_eq!(ab[1].applied_to, vec!["Ride", "Swim"]);
        let eq = c.equipment();
        assert_eq!(eq[1].base.as_deref(), Some("Sword"));
        assert_eq!(c.equipped().len(), 1);
        assert_eq!(c.deity().as_deref(), Some("Sun God"));
        assert_eq!(c.domains(), vec!["Light"]);
    }

    #[test]
    fn unchanged_roundtrip() {
        assert_eq!(ch().pcg.encode(), PCG.as_bytes());
    }

    #[test]
    fn edits() {
        let mut c = ch();
        c.set_stat("GRC", 16);
        c.set_stat("NEW", 9);
        assert_eq!(c.stat("GRC"), Some(16));
        assert_eq!(c.stat("NEW"), Some(9));

        assert_eq!(c.add_level("Tinker", 5, 4.0), 3);
        assert_eq!(c.add_level("Scout", 8, 6.0), 1);
        assert_eq!(c.total_level(), 4);
        assert_eq!(c.levels().last().unwrap().class, "Scout");
        assert_eq!(c.remove_last_level().as_deref(), Some("Scout"));
        assert_eq!(c.classes().len(), 1);

        c.set_skill_ranks("Ride", "Tinker", 3.0, true);
        c.set_skill_ranks("Climb", "Tinker", 1.0, false);
        c.set_skill_ranks("Swim", "Tinker", 0.0, true);
        let sk = c.skills();
        assert_eq!(
            sk.iter().find(|s| s.name == "Ride").unwrap().total_ranks(),
            3.0
        );
        assert_eq!(
            sk.iter().find(|s| s.name == "Climb").unwrap().total_ranks(),
            1.0
        );

        c.add_ability("FEAT", "FEAT", "Quick Step", &[]);
        assert_eq!(c.abilities().len(), 3);
        assert_eq!(c.remove_ability(Some("FEAT"), "steady hands"), 1);
        assert_eq!(c.abilities().len(), 2);

        c.add_language("Elvish");
        c.add_language("common");
        assert_eq!(c.languages().len(), 3);

        c.add_equipment("Rope", 1.0);
        c.add_equipment("Lantern", 1.0);
        assert_eq!(c.equipment()[0].quantity, 3.0);
        c.equip("Lantern", "Carried", 1.0);
        assert_eq!(c.equipped().len(), 2);
        assert!(c.unequip("Lantern"));
        assert!(c.remove_equipment("Shiny Sword"));
        assert!(c.equipped().is_empty());

        c.set_field("CITY", "Gearton|North");
        c.set_alignment("CG");
        c.set_race("Gnome");
        c.set_experience(9000);
        c.set_deity(None);

        // the result re-parses cleanly and keeps the edits
        let re = Character::from_pcg(PcgFile::parse(&String::from_utf8(c.pcg.encode()).unwrap()));
        assert!(re.pcg.diags.is_empty());
        assert_eq!(re.pcg.value("CITY").as_deref(), Some("Gearton|North"));
        assert_eq!(re.race().as_deref(), Some("Gnome"));
        assert_eq!(re.alignment().as_deref(), Some("CG"));
        assert_eq!(re.experience(), 9000);
        assert_eq!(re.deity(), None);
        assert_eq!(re.total_level(), 3);
        // new STAT line follows the existing ones
        let tags: Vec<_> = re.pcg.lines.iter().filter_map(|l| l.tag()).collect();
        let first_stat = tags.iter().position(|t| *t == "STAT").unwrap();
        assert_eq!(&tags[first_stat..first_stat + 3], &["STAT", "STAT", "STAT"]);
    }

    #[test]
    fn new_character() {
        let mut c = Character::new("Testmode", &["Book A".into()], "Nova");
        c.set_stat("MGT", 12);
        c.set_race("Human");
        c.add_level("Tinker", 6, 4.0);
        let s = String::from_utf8(c.pcg.encode()).unwrap();
        let re = Character::from_pcg(PcgFile::parse(&s));
        assert!(re.pcg.diags.is_empty(), "{:?}", re.pcg.diags);
        assert_eq!(re.name().as_deref(), Some("Nova"));
        assert_eq!(re.campaigns(), vec!["Book A"]);
        assert_eq!(re.total_level(), 1);
        assert!(s.starts_with("PCGVERSION:2.0\n"));
        // stat comes after the bio section
        assert!(s.find("STAT:MGT").unwrap() > s.find("CHARACTERNAME").unwrap());
    }
}
