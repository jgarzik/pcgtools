//
// pcg/character.rs -- typed view of a PCG character
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
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
use std::path::Path;

/// The order PCGen writes sections in; new lines are inserted after
/// the last existing line of the same or an earlier tag.
const ORDER: &[&str] = &[
    "PCGVERSION",
    "CAMPAIGN",
    "VERSION",
    "ROLLMETHOD",
    "PURCHASEPOINTS",
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
    /// sub-choice (e.g. a language for a linguistics skill)
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

/// A character, backed by its PCG file.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Character {
    pub pcg: PcgFile,
}

fn num<T: std::str::FromStr>(e: Option<String>) -> Option<T> {
    e.and_then(|s| s.trim().parse().ok())
}

impl Character {
    pub fn read(path: &Path) -> Result<Character> {
        Ok(Character {
            pcg: PcgFile::read(path)?,
        })
    }

    pub fn from_pcg(pcg: PcgFile) -> Character {
        Character { pcg }
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
        l.push(PcgLine::new(vec![Elem::text("GAMEMODE", gamemode)]));
        l.push(PcgLine::blank());
        l.push(PcgLine::comment("Character Bio"));
        l.push(PcgLine::new(vec![Elem::text("CHARACTERNAME", name)]));
        l.push(PcgLine::blank());
        l.push(PcgLine::comment("Character Attributes"));
        Character { pcg }
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
                    associated: l.text("ASSOCIATEDDATA"),
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
                        .map(|s| {
                            s.split(',')
                                .filter(|x| !x.is_empty())
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default(),
                })
            })
            .collect()
    }

    /// Feats stored in the older one-line-per-feat form.
    pub fn legacy_feats(&self) -> Vec<String> {
        self.pcg
            .lines_tagged("FEAT")
            .filter_map(|l| l.head().map(Elem::value_text))
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

    /// The equipment set used for calculations (CALCEQUIPSET), as
    /// (location, item, quantity) for items placed in it.
    pub fn equipped(&self) -> Vec<EquipSetEntry> {
        let calc = self
            .pcg
            .value("CALCEQUIPSET")
            .unwrap_or_else(|| "0.1".into());
        let prefix = format!("{}.", calc);
        self.equipsets()
            .into_iter()
            .filter(|e| e.item.is_some() && e.id.starts_with(&prefix))
            .collect()
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
                Elem::raw("SKILLSREMAINING", "0"),
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

    /// Add a chosen ability.
    pub fn add_ability(&mut self, pool: &str, category: &str, key: &str, applied_to: &[String]) {
        let mut elems = vec![
            Elem::text("ABILITY", pool),
            Elem::raw("TYPE", "NORMAL"),
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
                && !(l.tag() == Some("EQUIPSET") && l.text("VALUE").as_deref() == Some(name))
        });
        before != self.pcg.lines.len()
    }

    /// Place an item at a location in the calculation equipment set.
    pub fn equip(&mut self, name: &str, location: &str, qty: f64) {
        let calc = self
            .pcg
            .value("CALCEQUIPSET")
            .unwrap_or_else(|| "0.1".into());
        if self.equipsets().iter().all(|e| e.id != calc) {
            self.insert_ordered(
                "EQUIPSET",
                PcgLine::new(vec![
                    Elem::text("EQUIPSET", "Default Set"),
                    Elem::raw("ID", &calc),
                ]),
            );
            self.put("CALCEQUIPSET", vec![Elem::raw("CALCEQUIPSET", &calc)]);
        }
        let prefix = format!("{}.", calc);
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
            + 1;
        self.insert_ordered(
            "EQUIPSET",
            PcgLine::new(vec![
                Elem::text("EQUIPSET", location),
                Elem::raw("ID", &format!("{}{}", prefix, next)),
                Elem::text("VALUE", name),
                Elem::raw("QUANTITY", &format!("{:.1}", qty)),
            ]),
        );
    }

    /// Remove an item from the calculation equipment set.
    pub fn unequip(&mut self, name: &str) -> bool {
        let before = self.pcg.lines.len();
        self.pcg
            .lines
            .retain(|l| !(l.tag() == Some("EQUIPSET") && l.text("VALUE").as_deref() == Some(name)));
        before != self.pcg.lines.len()
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
