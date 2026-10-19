//
// rules/spells.rs -- the spells a character knows, prepares and can cast
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! Spells by caster: the race (spell-like abilities from SPELLS tokens)
//! and each spellcasting class (spells saved in the character file,
//! spells known automatically, domain spells), with per-level spells
//! known and cast per day, caster levels and save DCs.

use super::calc::Calc;
use crate::dataset::Object;
use crate::lst::kind::FileKind;
use crate::token::pre::Prereq;
use std::cmp::Ordering;
use std::collections::BTreeMap;

/// The default spell book and the book of innate abilities.
pub const KNOWN_BOOK: &str = "Known Spells";
pub const INNATE_BOOK: &str = "Innate";
/// The prepared list spells go in unless another is named.
pub const PREPARED_BOOK: &str = "Prepared Spells";

/// Who a spell belongs to.
#[derive(Debug, Clone, PartialEq)]
pub enum Owner<'a> {
    Class(&'a Object),
    /// a domain, with the class it was taken through
    Domain(&'a Object, Option<String>),
    Race,
}

impl Owner<'_> {
    fn order(&self) -> (u8, String) {
        match self {
            Owner::Domain(d, _) => (0, d.key.clone()),
            Owner::Class(c) => (1, c.key.clone()),
            Owner::Race => (2, String::new()),
        }
    }
}

/// One placement of a spell: its book, level and times.
#[derive(Debug, Clone, PartialEq)]
pub struct SpellInfo {
    pub book: String,
    pub level: i64,
    /// times prepared or usable; -1 is at will
    pub times: i64,
    pub unit: String,
    pub fixed_dc: Option<String>,
    /// keys of the metamagic feats it was prepared with
    pub feats: Vec<String>,
    /// pages it takes in a spellbook
    pub pages: i64,
}

/// A spell the character has, from one owner.
#[derive(Debug, Clone)]
pub struct CharSpell<'a> {
    pub spell: &'a Object,
    pub owner: Owner<'a>,
    pub fixed_cl: Option<String>,
    pub infos: Vec<SpellInfo>,
}

/// A caster: the race or a spellcasting class.
#[derive(Debug, Clone)]
pub enum Caster<'a> {
    Race,
    Class(&'a Object),
}

pub struct Spells<'a> {
    pub casters: Vec<Caster<'a>>,
    pub books: Vec<String>,
    /// spells per caster index
    pub by_caster: Vec<Vec<CharSpell<'a>>>,
    /// spell list membership: (list kind, list key lower) -> level -> spell keys
    lists: BTreeMap<(char, String), BTreeMap<i64, Vec<&'a Object>>>,
    /// spells SPELLKNOWN grants: (class list lower, level) -> spells
    granted_known: BTreeMap<(String, i64), Vec<&'a Object>>,
}

fn lower(s: &str) -> String {
    s.to_lowercase()
}

/// `Cleric,Paladin=1|Wizard=2` style list tokens: (names, level).
fn level_groups(v: &str) -> Vec<(Vec<String>, i64)> {
    let mut out = Vec::new();
    for g in v.split('|') {
        if Prereq::is_pre_tag(g.split(':').next().unwrap_or("")) || g.starts_with("!PRE") {
            continue;
        }
        // a level may carry prerequisites: `Psion=1[PREVARGTEQ:X,1]`
        let g = g.split('[').next().unwrap_or(g);
        let Some((names, lvl)) = g.rsplit_once('=') else {
            continue;
        };
        let Ok(lvl) = lvl.trim().parse::<i64>() else {
            continue;
        };
        out.push((
            names.split(',').map(|n| n.trim().to_string()).collect(),
            lvl,
        ));
    }
    out
}

impl<'a> Spells<'a> {
    pub fn new(calc: &Calc<'a>) -> Spells<'a> {
        let ds = calc.ds;
        let mut lists: BTreeMap<(char, String), BTreeMap<i64, Vec<&'a Object>>> = BTreeMap::new();
        let mut put = |kind: char, list: &str, lvl: i64, sp: &'a Object| {
            let e = lists
                .entry((kind, lower(list)))
                .or_default()
                .entry(lvl)
                .or_default();
            if !e.iter().any(|x| std::ptr::eq(*x, sp)) {
                e.push(sp);
            }
        };
        for sp in ds.objects(FileKind::Spell) {
            for v in sp.values("CLASSES") {
                for (names, lvl) in level_groups(v) {
                    for n in names {
                        put('C', &n, lvl, sp);
                    }
                }
            }
            for v in sp.values("DOMAINS") {
                for (names, lvl) in level_groups(v) {
                    for n in names {
                        put('D', &n, lvl, sp);
                    }
                }
            }
        }
        // SPELLLEVEL:CLASS|Wizard=1|Spell,Spell|Cleric=2|Spell on objects
        // the character has
        for src in &calc.sources {
            for t in src.tokens.iter().filter(|t| t.tag == "SPELLLEVEL") {
                let mut parts = t.value.split('|');
                let kind = match parts.next() {
                    Some(k) if k.eq_ignore_ascii_case("CLASS") => 'C',
                    Some(k) if k.eq_ignore_ascii_case("DOMAIN") => 'D',
                    _ => continue,
                };
                // trailing prerequisites gate the whole grant
                let is_pre = |p: &&str| {
                    Prereq::is_pre_tag(p.split(':').next().unwrap_or("")) || p.starts_with("!PRE")
                };
                let (pres, rest): (Vec<&str>, Vec<&str>) = parts.partition(is_pre);
                if !pres
                    .iter()
                    .filter_map(|p| Prereq::parse(p).ok())
                    .all(|p| calc.passes(&p, Some(src)))
                {
                    continue;
                }
                for pair in rest.chunks(2) {
                    let [lv, names] = pair else { continue };
                    let Some((list, lvl)) = lv.rsplit_once('=') else {
                        continue;
                    };
                    let Ok(lvl) = lvl.parse::<i64>() else {
                        continue;
                    };
                    for n in names.split(',') {
                        if let Some(sp) = ds.get(FileKind::Spell, n.trim()) {
                            put(kind, list, lvl, sp);
                        }
                    }
                }
            }
        }

        let mut s = Spells {
            casters: Vec::new(),
            books: vec![KNOWN_BOOK.to_string(), INNATE_BOOK.to_string()],
            by_caster: Vec::new(),
            lists,
            granted_known: BTreeMap::new(),
        };
        for l in calc.ch.pcg.lines_tagged("SPELLBOOK") {
            if let Some(n) = l.head().map(|h| h.value_text()) {
                s.add_book(&n);
            }
        }
        let slas = s.spell_likes(calc);
        if !slas.is_empty() {
            s.casters.push(Caster::Race);
            s.by_caster.push(slas);
        }
        for (cls, _) in calc.classes() {
            if spell_type(cls).is_some() {
                s.casters.push(Caster::Class(cls));
                s.by_caster.push(Vec::new());
            }
        }
        // spells known without choosing them are there before the
        // saved ones are read
        s.auto_known(calc);
        s.granted(calc);
        s.load_saved(calc);
        for v in &mut s.by_caster {
            v.sort_by(compare);
        }
        s
    }

    fn add_book(&mut self, name: &str) {
        if !self.books.iter().any(|b| b == name) {
            self.books.push(name.to_string());
        }
    }

    /// Spell-like abilities: SPELLS tokens on everything the character
    /// has, owned by the race.
    fn spell_likes(&mut self, calc: &Calc<'a>) -> Vec<CharSpell<'a>> {
        let mut out: Vec<CharSpell<'a>> = Vec::new();
        for src in &calc.sources {
            for t in src.tokens.iter().filter(|t| t.tag == "SPELLS") {
                let parts: Vec<&str> = t.value.split('|').collect();
                let Some(book) = parts.first() else { continue };
                let mut times = 1i64;
                let mut unit = "Day".to_string();
                let mut cl = None;
                let mut spells = Vec::new();
                let mut ok = true;
                for p in &parts[1..] {
                    if let Some(v) = p.strip_prefix("TIMES=") {
                        times = if v.eq_ignore_ascii_case("ATWILL") {
                            -1
                        } else {
                            calc.eval_formula(v) as i64
                        };
                    } else if let Some(v) = p.strip_prefix("TIMEUNIT=") {
                        unit = v.to_string();
                    } else if let Some(v) = p.strip_prefix("CASTERLEVEL=") {
                        cl = Some(v.to_string());
                    } else if Prereq::is_pre_tag(p.split(':').next().unwrap_or(""))
                        || p.starts_with("!PRE")
                    {
                        if let Ok(pr) = Prereq::parse(p) {
                            ok &= calc.passes(&pr, Some(src));
                        }
                    } else {
                        spells.push(*p);
                    }
                }
                if !ok {
                    continue;
                }
                self.add_book(book);
                for sp in spells {
                    let (name, dc) = match sp.split_once(',') {
                        Some((n, d)) => (n, Some(d.to_string())),
                        None => (sp, None),
                    };
                    let Some(obj) = calc.ds.get(FileKind::Spell, name) else {
                        continue;
                    };
                    let cs = CharSpell {
                        spell: obj,
                        owner: Owner::Race,
                        fixed_cl: cl.clone(),
                        infos: vec![SpellInfo {
                            book: book.to_string(),
                            level: 0,
                            times,
                            unit: unit.clone(),
                            fixed_dc: dc,
                            feats: Vec::new(),
                            pages: 0,
                        }],
                    };
                    // the same spell granted the same way is one entry
                    if !out.iter().any(|x| compare(x, &cs) == Ordering::Equal) {
                        out.push(cs);
                    }
                }
            }
        }
        out
    }

    pub fn caster_index(&self, cls: &Object) -> Option<usize> {
        self.casters.iter().position(|c| match c {
            Caster::Class(o) => std::ptr::eq(*o, cls),
            Caster::Race => false,
        })
    }

    /// Levels a spell has on a list ('C' class, 'D' domain).
    pub fn levels_on(&self, kind: char, list: &str, spell: &Object) -> Vec<i64> {
        self.lists
            .get(&(kind, lower(list)))
            .map(|m| {
                m.iter()
                    .filter(|(_, v)| {
                        v.iter()
                            .any(|x| std::ptr::eq(*x, spell) || x.key == spell.key)
                    })
                    .map(|(l, _)| *l)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Spells on a list at a level.
    pub fn on_list(&self, kind: char, list: &str, level: i64) -> Vec<&'a Object> {
        self.lists
            .get(&(kind, lower(list)))
            .and_then(|m| m.get(&level))
            .cloned()
            .unwrap_or_default()
    }

    /// Spells saved in the character file (SPELLNAME lines).
    fn load_saved(&mut self, calc: &Calc<'a>) {
        let auto = calc
            .ch
            .pcg
            .value("AUTOSPELLS")
            .is_none_or(|v| v.eq_ignore_ascii_case("Y"));
        let lines: Vec<_> = calc.ch.pcg.lines_tagged("SPELLNAME").cloned().collect();
        for l in lines {
            let Some(name) = l.head().map(|h| h.value_text()) else {
                continue;
            };
            let Some(spell) = calc.ds.get(FileKind::Spell, &name) else {
                continue;
            };
            let Some(cls_key) = l.text("CLASS") else {
                continue;
            };
            let Some((cls, _)) = calc
                .classes()
                .iter()
                .find(|(o, _)| o.key.eq_ignore_ascii_case(&cls_key))
            else {
                continue;
            };
            let Some(ci) = self.caster_index(cls) else {
                continue;
            };
            let book = l.text("BOOK").unwrap_or_else(|| KNOWN_BOOK.to_string());
            let times: i64 = l.text("TIMES").and_then(|v| v.parse().ok()).unwrap_or(1);
            let saved_level: i64 = l
                .text("SPELLLEVEL")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let source = l.get("SOURCE");
            let child = |tag: &str| {
                source
                    .and_then(|e| e.children().iter().find(|c| c.tag == tag))
                    .map(|c| c.value_text())
            };
            let owner = match child("TYPE") {
                Some(t) if t.eq_ignore_ascii_case("DOMAIN") => {
                    let dname = child("NAME").unwrap_or_default();
                    match calc.ds.get(FileKind::Domain, &dname) {
                        Some(d) => Owner::Domain(d, Some(cls.key.clone())),
                        None => continue,
                    }
                }
                _ => Owner::Class(cls),
            };
            let levels = match &owner {
                Owner::Domain(d, _) => self.levels_on('D', &d.key, spell),
                _ => self.class_levels(calc, cls, spell),
            };
            // metamagic feats it was prepared with (unknown ones are
            // dropped), which raise the level it is saved at
            let feats: Vec<String> = l
                .get("FEATLIST")
                .map(|e| {
                    e.children()
                        .iter()
                        .filter_map(|c| {
                            calc.ds
                                .get_in(FileKind::Ability, "FEAT", &c.value_text())
                                .map(|o| o.key.clone())
                        })
                        .collect()
                })
                .unwrap_or_default();
            let added = metamagic_levels(calc, &feats);
            let pages: i64 = l
                .text("SPELLNUMPAGES")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            let levels = if levels.is_empty() {
                vec![saved_level - added]
            } else {
                levels
            };
            self.add_book(&book);
            let info = |level: i64| SpellInfo {
                book: book.clone(),
                level,
                times,
                unit: "Day".into(),
                fixed_dc: None,
                feats: Vec::new(),
                pages: 0,
            };
            for lvl in levels {
                if saved_level > 0 && saved_level != lvl + added {
                    continue;
                }
                if book == KNOWN_BOOK && auto && self.auto_knows(calc, cls, spell, lvl) {
                    continue;
                }
                // a spell the class does not have yet starts with a
                // placement at its own level
                let has = |s: &Self| {
                    s.by_caster[ci].iter().any(|c| {
                        std::ptr::eq(c.spell, spell) && c.owner == owner && c.fixed_cl.is_none()
                    })
                };
                if !has(self) {
                    self.add_info(ci, spell, owner.clone(), info(lvl));
                }
                if !matches!(owner, Owner::Class(_)) && book == KNOWN_BOOK {
                    continue;
                }
                let Some(cs) = self.by_caster[ci].iter_mut().find(|c| {
                    std::ptr::eq(c.spell, spell) && c.owner == owner && c.fixed_cl.is_none()
                }) else {
                    continue;
                };
                // the same book and level again (ignoring feats) keeps
                // the first line's times, unless feats make it another
                match cs
                    .infos
                    .iter_mut()
                    .find(|i| i.book == book && i.level == saved_level)
                {
                    Some(i) if feats.is_empty() => i.pages = pages,
                    _ => cs.infos.push(SpellInfo {
                        feats: feats.clone(),
                        pages,
                        ..info(saved_level)
                    }),
                }
            }
        }
    }

    fn add_info(&mut self, ci: usize, spell: &'a Object, owner: Owner<'a>, info: SpellInfo) {
        let list = &mut self.by_caster[ci];
        match list
            .iter_mut()
            .find(|c| std::ptr::eq(c.spell, spell) && c.owner == owner && c.fixed_cl.is_none())
        {
            Some(c) => {
                if !c.infos.contains(&info) {
                    c.infos.push(info);
                }
            }
            None => list.push(CharSpell {
                spell,
                owner,
                fixed_cl: None,
                infos: vec![info],
            }),
        }
    }

    /// The spell lists a class uses (its own, or those saved for it).
    pub fn class_lists(&self, calc: &Calc<'a>, cls: &Object) -> Vec<String> {
        // SPELLLIST:Class|CLASS.Sorcerer|CLASS.Cleric (parts without a
        // colon, so read from the line's text)
        let saved: Vec<String> = calc
            .ch
            .pcg
            .lines_tagged("SPELLLIST")
            .filter_map(|l| l.raw.strip_prefix("SPELLLIST:").map(str::to_string))
            .filter_map(|rest| {
                let mut parts = rest.split('|');
                let head = crate::pcg::decode(parts.next()?);
                head.eq_ignore_ascii_case(&cls.key).then(|| {
                    parts
                        .filter_map(|p| p.strip_prefix("CLASS.").map(crate::pcg::decode))
                        .collect::<Vec<_>>()
                })
            })
            .flatten()
            .collect();
        if saved.is_empty() {
            vec![cls.key.clone()]
        } else {
            saved
        }
    }

    fn class_levels(&self, calc: &Calc<'a>, cls: &Object, spell: &Object) -> Vec<i64> {
        let mut out = Vec::new();
        for list in self.class_lists(calc, cls) {
            for l in self.levels_on('C', &list, spell) {
                if !out.contains(&l) {
                    out.push(l);
                }
            }
        }
        out
    }

    /// Does the class know the spell at the level without choosing it
    /// (KNOWNSPELLS)?
    fn auto_knows(&self, calc: &Calc<'a>, cls: &Object, spell: &Object, lvl: i64) -> bool {
        let Some(filters) = cls.get("KNOWNSPELLS") else {
            return false;
        };
        if cast_per_day(calc, cls, lvl, true) == 0 {
            return false;
        }
        filters.split('|').any(|f| {
            f.split(',').all(|part| {
                if let Some(l) = part.strip_prefix("LEVEL=") {
                    l.parse::<i64>().is_ok_and(|x| x == lvl)
                } else if let Some(t) = part.strip_prefix("TYPE=") {
                    spell.is_type(t)
                } else {
                    part.eq_ignore_ascii_case(&spell.key)
                }
            })
        })
    }

    /// Spells known automatically, and the domain spells of each
    /// class's domains, up to the highest level the class can cast.
    fn auto_known(&mut self, calc: &Calc<'a>) {
        let auto = calc
            .ch
            .pcg
            .value("AUTOSPELLS")
            .is_none_or(|v| v.eq_ignore_ascii_case("Y"));
        if !auto {
            return;
        }
        let classes: Vec<&'a Object> = self
            .casters
            .iter()
            .filter_map(|c| match c {
                Caster::Class(o) => Some(*o),
                _ => None,
            })
            .collect();
        for cls in classes {
            let ci = self.caster_index(cls).unwrap_or(0);
            let max = (0..=99)
                .filter(|l| cast_per_day(calc, cls, *l, true) > 0)
                .max();
            let Some(max) = max else { continue };
            if cls.has("KNOWNSPELLS") {
                for list in self.class_lists(calc, cls) {
                    for lvl in 0..=max {
                        for sp in self.on_list('C', &list, lvl) {
                            if prohibited(calc, cls, sp) {
                                continue;
                            }
                            if self.auto_knows(calc, cls, sp, lvl) {
                                self.add_info(
                                    ci,
                                    sp,
                                    Owner::Class(cls),
                                    SpellInfo {
                                        book: KNOWN_BOOK.into(),
                                        level: lvl,
                                        times: 1,
                                        unit: "Day".into(),
                                        fixed_dc: None,
                                        feats: Vec::new(),
                                        pages: 0,
                                    },
                                );
                            }
                        }
                    }
                }
            }
            for l in calc.ch.pcg.lines_tagged("DOMAIN") {
                let Some(d) = l.head().map(|h| h.value_text()) else {
                    continue;
                };
                let from = l
                    .get("SOURCE")
                    .and_then(|e| e.children().iter().find(|c| c.tag == "NAME"))
                    .map(|c| c.value_text());
                if !from
                    .as_deref()
                    .is_some_and(|f| f.eq_ignore_ascii_case(&cls.key))
                {
                    continue;
                }
                let Some(dom) = calc.ds.get(FileKind::Domain, &d) else {
                    continue;
                };
                for lvl in 0..=max {
                    if cast_per_day(calc, cls, lvl, true) <= 0 {
                        continue;
                    }
                    for sp in self.on_list('D', &dom.key, lvl) {
                        let owner = Owner::Domain(dom, Some(cls.key.clone()));
                        let exists = self.by_caster[ci]
                            .iter()
                            .any(|c| std::ptr::eq(c.spell, sp) && c.owner == owner);
                        if !exists {
                            self.add_info(
                                ci,
                                sp,
                                owner,
                                SpellInfo {
                                    book: KNOWN_BOOK.into(),
                                    level: lvl,
                                    times: 1,
                                    unit: "Day".into(),
                                    fixed_dc: None,
                                    feats: Vec::new(),
                                    pages: 0,
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    /// How many distinct spells SPELLKNOWN grants to a class's spell
    /// list at a level; they count toward the class's spells known.
    pub fn granted_known_count(&self, list: &str, level: i64) -> i64 {
        self.granted_known
            .get(&(lower(list), level))
            .map_or(0, |v| v.len() as i64)
    }

    /// Spells granted to class lists (SPELLKNOWN:CLASS|Class=1|Spell)
    /// and domains' spell-like abilities, which their class also
    /// holds at level 1.
    fn granted(&mut self, calc: &Calc<'a>) {
        let classes: Vec<&'a Object> = self
            .casters
            .iter()
            .filter_map(|c| match c {
                Caster::Class(o) => Some(*o),
                _ => None,
            })
            .collect();
        for src in &calc.sources {
            for t in src.tokens.iter().filter(|t| t.tag == "SPELLKNOWN") {
                let mut parts = t.value.split('|');
                if !parts
                    .next()
                    .is_some_and(|k| k.eq_ignore_ascii_case("CLASS"))
                {
                    continue;
                }
                // trailing prerequisites gate the whole grant
                let is_pre = |p: &&str| {
                    Prereq::is_pre_tag(p.split(':').next().unwrap_or("")) || p.starts_with("!PRE")
                };
                let (pres, rest): (Vec<&str>, Vec<&str>) = parts.partition(is_pre);
                if !pres
                    .iter()
                    .filter_map(|p| Prereq::parse(p).ok())
                    .all(|p| calc.passes(&p, Some(src)))
                {
                    continue;
                }
                for pair in rest.chunks(2) {
                    let [lv, names] = pair else { continue };
                    let Some((list, lvl)) = lv.rsplit_once('=') else {
                        continue;
                    };
                    let Ok(lvl) = lvl.parse::<i64>() else {
                        continue;
                    };
                    let Some(cls) = classes.iter().find(|c| {
                        self.class_lists(calc, c)
                            .iter()
                            .any(|l| l.eq_ignore_ascii_case(list))
                    }) else {
                        continue;
                    };
                    let ci = self.caster_index(cls).unwrap_or(0);
                    for n in names.split(',') {
                        if let Some(sp) = calc.ds.get(FileKind::Spell, n.trim()) {
                            let known = self.granted_known.entry((lower(list), lvl)).or_default();
                            if !known.iter().any(|x| std::ptr::eq(*x, sp)) {
                                known.push(sp);
                            }
                            // the spell is on the class's list as well
                            let on = self
                                .lists
                                .entry(('C', lower(list)))
                                .or_default()
                                .entry(lvl)
                                .or_default();
                            if !on.iter().any(|x| std::ptr::eq(*x, sp)) {
                                on.push(sp);
                            }
                            self.add_info(
                                ci,
                                sp,
                                Owner::Class(cls),
                                SpellInfo {
                                    book: KNOWN_BOOK.into(),
                                    level: lvl,
                                    times: 1,
                                    unit: "Day".into(),
                                    fixed_dc: None,
                                    feats: Vec::new(),
                                    pages: 0,
                                },
                            );
                        }
                    }
                }
            }
            if src.kind != super::calc::SrcKind::Domain {
                continue;
            }
            let Some(cls) = src.class.as_deref().and_then(|k| {
                classes
                    .iter()
                    .find(|c| c.key.eq_ignore_ascii_case(k))
                    .copied()
            }) else {
                continue;
            };
            let ci = self.caster_index(cls).unwrap_or(0);
            for t in src.tokens.iter().filter(|t| t.tag == "SPELLS") {
                let parts: Vec<&str> = t.value.split('|').collect();
                let Some(book) = parts.first() else { continue };
                let mut times = 1;
                let mut spells = Vec::new();
                for p in &parts[1..] {
                    if let Some(v) = p.strip_prefix("TIMES=") {
                        times = if v.eq_ignore_ascii_case("ATWILL") {
                            -1
                        } else {
                            calc.eval_formula(v) as i64
                        };
                    } else if !p.contains('=')
                        && !Prereq::is_pre_tag(p.split(':').next().unwrap_or(""))
                    {
                        spells.push(p.split(',').next().unwrap_or(p));
                    }
                }
                for n in spells {
                    if let Some(sp) = calc.ds.get(FileKind::Spell, n) {
                        self.add_info(
                            ci,
                            sp,
                            Owner::Domain(src.obj, Some(cls.key.clone())),
                            SpellInfo {
                                book: book.to_string(),
                                level: 1,
                                times,
                                unit: "Day".into(),
                                fixed_dc: None,
                                feats: Vec::new(),
                                pages: 0,
                            },
                        );
                    }
                }
            }
        }
    }

    /// Specialty spells known per day at a level: the subclass's or
    /// class's KNOWNSPELLSFROMSPECIALTY plus bonuses.
    pub fn specialty_known(calc: &Calc, cls: &Object, level: i64) -> i64 {
        let from = calc
            .subclass_of(cls)
            .and_then(|s| s.get("KNOWNSPELLSFROMSPECIALTY"))
            .or_else(|| cls.get("KNOWNSPELLSFROMSPECIALTY"))
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(0);
        let mut b = calc.bonus.total(
            "SPECIALTYSPELLKNOWN",
            &format!("CLASS.{};LEVEL.{}", cls.key, level),
        );
        if let Some(t) = spell_type(cls) {
            b += calc.bonus.total(
                "SPECIALTYSPELLKNOWN",
                &format!("TYPE.{};LEVEL.{}", t, level),
            );
        }
        from + b as i64
    }

    /// Spells of a caster in a book at a level (`None` for any level).
    pub fn spells_in(
        &self,
        caster: usize,
        book: &str,
        level: Option<i64>,
    ) -> Vec<(&CharSpell<'a>, &SpellInfo)> {
        let Some(list) = self.by_caster.get(caster) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for cs in list {
            if let Some(i) = cs
                .infos
                .iter()
                .find(|i| i.book == book && level.is_none_or(|l| i.level == l))
            {
                out.push((cs, i));
            }
        }
        out
    }
}

/// The class's FACT:SpellType (Arcane, Divine, Psionic, ...).
pub fn spell_type(cls: &Object) -> Option<&str> {
    cls.values("FACT").find_map(|v| {
        let (k, val) = v.split_once('|')?;
        k.eq_ignore_ascii_case("SpellType").then_some(val)
    })
}

/// Is a spell's school one the class may not use?
pub fn prohibited(calc: &Calc, cls: &Object, spell: &Object) -> bool {
    let mut schools: Vec<String> = cls
        .values("PROHIBITED")
        .flat_map(|v| v.split(','))
        .map(|x| x.to_string())
        .collect();
    for l in calc.ch.pcg.lines_tagged("CLASS") {
        if l.head()
            .is_some_and(|h| h.value_text().eq_ignore_ascii_case(&cls.key))
        {
            if let Some(p) = l.text("PROHIBITED") {
                schools.extend(p.split(',').map(str::to_string));
            }
        }
    }
    spell
        .values("SCHOOL")
        .any(|s| schools.iter().any(|x| x.eq_ignore_ascii_case(s)))
}

/// Order of spells within a caster: key, fixed caster level, owner.
fn compare(a: &CharSpell, b: &CharSpell) -> Ordering {
    lower(&a.spell.key)
        .cmp(&lower(&b.spell.key))
        .then_with(|| match (&a.fixed_cl, &b.fixed_cl) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(x), Some(y)) => x.cmp(y),
        })
        .then_with(|| a.owner.order().cmp(&b.owner.order()))
        .then_with(|| a.infos.len().cmp(&b.infos.len()))
        .then_with(|| {
            for (x, y) in a.infos.iter().zip(&b.infos) {
                let o = x
                    .book
                    .cmp(&y.book)
                    .then(x.level.cmp(&y.level))
                    .then(x.times.cmp(&y.times))
                    .then(x.unit.cmp(&y.unit))
                    .then(x.fixed_dc.cmp(&y.fixed_dc));
                if o != Ordering::Equal {
                    return o;
                }
            }
            Ordering::Equal
        })
}

/// Effective level of a class for spells: its level plus PCLEVEL bonuses.
pub fn caster_class_level(calc: &Calc, cls: &Object) -> i64 {
    let mut l = calc.class_level(&cls.key) as i64 + calc.bonus.total("PCLEVEL", &cls.key) as i64;
    if let Some(t) = spell_type(cls) {
        l += calc.bonus.total("PCLEVEL", &format!("TYPE.{}", t)) as i64;
    }
    l
}

/// The class's table row (CAST or KNOWN) for a level: the last level
/// line at or below it that has one.
fn table_row(cls: &Object, tag: &str, level: i64) -> Option<Vec<String>> {
    let mut best: Option<(i64, Vec<String>)> = None;
    for sl in cls.sublines.iter().filter(|sl| sl.tag == "LEVEL") {
        let n: i64 = sl
            .value
            .split(':')
            .next()
            .and_then(|x| x.trim().parse().ok())
            .unwrap_or(0);
        if n > level || n <= 0 {
            continue;
        }
        if let Some(t) = sl.tokens.iter().find(|t| t.tag == tag) {
            if best.as_ref().is_none_or(|(b, _)| n >= *b) {
                best = Some((n, t.value.split(',').map(str::to_string).collect()));
            }
        }
    }
    best.map(|(_, v)| v)
}

fn stat_key(cls: &Object, tag: &str) -> Option<String> {
    cls.get(tag).map(str::to_string)
}

/// Bonus spells for a stat score at a spell level (BONUSSPELLLEVEL).
pub fn stat_bonus_spells(calc: &Calc, score: f64, level: i64) -> i64 {
    let Some(gm) = calc.gm else { return 0 };
    let Some(row) = gm
        .lines("statsandchecks.lst", "BONUSSPELLLEVEL")
        .find(|l| l.value.trim().parse::<i64>().ok() == Some(level))
    else {
        return 0;
    };
    let base: f64 = row
        .get("BASESTATSCORE")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let range: f64 = row
        .get("STATRANGE")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0);
    if score < base || range <= 0.0 {
        return 0;
    }
    (((score - base + range) / range).floor() as i64).max(0)
}

fn spell_bonus_keys(cls: &Object, level: i64) -> Vec<String> {
    let mut v = vec![
        format!("CLASS.{};LEVEL.{}", cls.key, level),
        format!("CLASS.Any;LEVEL.{}", level),
        format!("CLASS.{};LEVEL.All", cls.key),
        "CLASS.Any;LEVEL.All".to_string(),
    ];
    if let Some(t) = spell_type(cls) {
        v.push(format!("TYPE.{};LEVEL.{}", t, level));
        v.push(format!("TYPE.{};LEVEL.All", t));
    }
    v
}

/// Spells per day at a level (without specialty or domain slots).
pub fn cast_per_day(calc: &Calc, cls: &Object, level: i64, limit_by_stat: bool) -> i64 {
    let stat = stat_key(cls, "BONUSSPELLSTAT").or_else(|| stat_key(cls, "SPELLSTAT"));
    let key = stat.clone().unwrap_or_else(|| "None".into());
    let mut score = stat.as_deref().map(|s| calc.stat_score(s)).unwrap_or(0.0);
    let bonus_stat = calc.bonus.total("STAT", &format!("CAST.{}", key))
        + calc.bonus.total("STAT", "BASESPELLSTAT")
        + calc
            .bonus
            .total("STAT", &format!("BASESPELLSTAT;CLASS={}", cls.key));
    let cl = caster_class_level(calc, cls);
    let row = (cl > 0).then(|| table_row(cls, "CAST", cl)).flatten();
    let from_table: Option<f64> = row
        .as_ref()
        .and_then(|r| r.get(level as usize))
        .map(|f| calc.eval_formula(f));
    let Some(n) = from_table else {
        let mut t = calc
            .bonus
            .total("SPELLCAST", &format!("CLASS.{};LEVEL.{}", cls.key, level))
            as i64;
        if t > 0 {
            t += stat_bonus_spells(calc, score + bonus_stat, level);
        }
        return t;
    };
    let mut total: f64 = spell_bonus_keys(cls, level)
        .iter()
        .map(|k| calc.bonus.total("SPELLCAST", k))
        .sum();
    if limit_by_stat {
        if let Some(st) = &stat {
            let max = calc.var(&format!("MAXLEVELSTAT={}", st)).unwrap_or(0.0);
            if max + bonus_stat < level as f64 {
                return total as i64;
            }
        }
    }
    // multipliers (at least 1) apply to the table's count only
    let mut mult = calc
        .bonus
        .total(
            "SPELLCASTMULT",
            &format!("CLASS.{};LEVEL.{}", cls.key, level),
        )
        .trunc();
    if let Some(t) = spell_type(cls) {
        mult += calc
            .bonus
            .total("SPELLCASTMULT", &format!("TYPE.{};LEVEL.{}", t, level))
            .trunc();
    }
    score += bonus_stat;
    total += n * mult.max(1.0) + stat_bonus_spells(calc, score, level) as f64;
    total as i64
}

/// Spells known at a level.
pub fn known_at(calc: &Calc, cls: &Object, level: i64, granted: i64) -> i64 {
    let cl = caster_class_level(calc, cls);
    let has_cast = cls
        .sublines
        .iter()
        .any(|sl| sl.tokens.iter().any(|t| t.tag == "CAST"));
    let cast_row = (cl > 0).then(|| table_row(cls, "CAST", cl)).flatten();
    let in_cast = cast_row
        .as_ref()
        .is_some_and(|r| (level as usize) < r.len());
    let first = calc
        .bonus
        .total("SPELLKNOWN", &format!("CLASS.{};LEVEL.{}", cls.key, level)) as i64;
    if has_cast && !in_cast {
        return first;
    }
    let mut total: f64 = spell_bonus_keys(cls, level)
        .iter()
        .map(|k| calc.bonus.total("SPELLKNOWN", k))
        .sum();
    let stat = stat_key(cls, "SPELLSTAT");
    let key = stat.clone().unwrap_or_else(|| "None".into());
    let bonus_stat = calc.bonus.total("STAT", &format!("KNOWN.{}", key))
        + calc.bonus.total("STAT", "BASESPELLKNOWNSTAT")
        + calc
            .bonus
            .total("STAT", &format!("BASESPELLKNOWNSTAT;CLASS={}", cls.key));
    let max = calc.var(&format!("MAXLEVELSTAT={}", key)).unwrap_or(0.0);
    if max + bonus_stat < level as f64 {
        return total as i64;
    }
    if let Some(row) = (cl > 0).then(|| table_row(cls, "KNOWN", cl)).flatten() {
        if let Some(f) = row.get(level as usize) {
            total += calc.eval_formula(f);
        }
    }
    total as i64 + granted
}

/// Does the spell's owner make it a bonus (specialty or domain) spell:
/// a domain spell, or a class spell of the class's specialty school?
pub fn is_specialty(calc: &Calc, cs: &CharSpell) -> bool {
    match &cs.owner {
        Owner::Domain(..) => true,
        Owner::Class(cls) => {
            let specs = specialties(calc, cls);
            let sp = cs.spell;
            specs.iter().any(|x| {
                sp.values("SCHOOL")
                    .chain(sp.values("SUBSCHOOL"))
                    .chain(sp.values("DESCRIPTOR"))
                    .flat_map(|v| v.split(['|', ',']))
                    .any(|v| v.trim().eq_ignore_ascii_case(x))
            })
        }
        Owner::Race => false,
    }
}

/// Specialties chosen for a class (SPECIALTIES on its level lines).
pub fn specialties(calc: &Calc, cls: &Object) -> Vec<String> {
    let mut out = Vec::new();
    for l in calc.ch.pcg.lines_tagged("CLASSABILITIESLEVEL") {
        let of_class = l
            .head()
            .map(|h| h.value_text())
            .and_then(|h| h.rsplit_once('=').map(|(c, _)| c.to_string()))
            .is_some_and(|c| c.eq_ignore_ascii_case(&cls.key));
        if !of_class {
            continue;
        }
        if let Some(e) = l.get("SPECIALTIES") {
            for c in e.children().iter().filter(|c| c.tag == "SPECIALTY") {
                out.push(c.value_text());
            }
        }
    }
    out
}

/// The class whose casting a spell uses (a domain's source class).
pub fn owner_class<'a>(calc: &Calc<'a>, cs: &CharSpell<'a>) -> Option<&'a Object> {
    match &cs.owner {
        Owner::Class(c) => Some(c),
        Owner::Domain(_, Some(k)) => calc
            .classes()
            .iter()
            .find(|(o, _)| o.key.eq_ignore_ascii_case(k))
            .map(|(o, _)| *o),
        _ => None,
    }
}

/// Caster level for a spell.
pub fn caster_level(calc: &Calc, sp: &Spells, cs: &CharSpell) -> i64 {
    if let Some(f) = &cs.fixed_cl {
        return calc.eval_formula(f) as i64;
    }
    let spell = cs.spell;
    let b = &calc.bonus;
    let positive = |v: f64| if v > 0.0 { v } else { 0.0 };
    let (mut base, key, stype) = match owner_class(calc, cs) {
        Some(cls) => {
            let pc = calc.bonus.total("PCLEVEL", &cls.key)
                + spell_type(cls)
                    .map(|t| calc.bonus.total("PCLEVEL", &format!("TYPE.{}", t)))
                    .unwrap_or(0.0);
            let declared = b.total("CASTERLEVEL", &cls.key);
            let lvl = if declared == 0.0 {
                calc.class_level(&cls.key) as f64
            } else {
                0.0
            };
            (
                lvl + pc,
                cls.key.clone(),
                spell_type(cls).map(str::to_string),
            )
        }
        None => (
            0.0,
            format!("RACE.{}", calc.ch.race().unwrap_or_default()),
            None,
        ),
    };
    let mut add = positive(b.total("CASTERLEVEL", &key));
    if owner_class(calc, cs).is_some() {
        add += positive(b.total("CASTERLEVEL", &format!("CLASS.{}", key)));
    }
    add += b.total("CASTERLEVEL", "ALLSPELLS");
    if let Some(t) = &stype {
        add += positive(b.total("CASTERLEVEL", &format!("TYPE.{}", t)));
    }
    add += positive(b.total("CASTERLEVEL", &format!("SPELL.{}", spell.key)));
    for s in spell.values("SCHOOL") {
        add += b.total("CASTERLEVEL", &format!("SCHOOL.{}", s));
    }
    for s in spell.values("SUBSCHOOL") {
        add += positive(b.total("CASTERLEVEL", &format!("SUBSCHOOL.{}", s)));
    }
    for d in spell.values("DESCRIPTOR").flat_map(|v| v.split(',')) {
        add += positive(b.total("CASTERLEVEL", &format!("DESCRIPTOR.{}", d.trim())));
    }
    for l in calc.ch.domains() {
        if !sp.levels_on('D', &l, spell).is_empty() {
            add += positive(b.total("CASTERLEVEL", &format!("DOMAIN.{}", l)));
        }
    }
    base += add;
    if base <= 0.0 {
        1
    } else {
        base as i64
    }
}

/// A metamagic feat by key.
pub fn metamagic_feat<'a>(calc: &Calc<'a>, key: &str) -> Option<&'a Object> {
    calc.ds.get_in(FileKind::Ability, "FEAT", key)
}

/// Spell levels metamagic feats add (their ADDSPELLLEVEL).
pub fn metamagic_levels(calc: &Calc, feats: &[String]) -> i64 {
    feats
        .iter()
        .filter_map(|k| metamagic_feat(calc, k))
        .filter_map(|f| f.get("ADDSPELLLEVEL"))
        .filter_map(|v| v.trim().parse::<i64>().ok())
        .sum()
}

/// A feat's own bonuses to a category and target (`BONUS:DC|FEATBONUS|1`).
fn feat_bonus(calc: &Calc, feat: &Object, category: &str, target: &str) -> f64 {
    feat.values("BONUS")
        .filter_map(|v| {
            let mut p = v.split('|');
            let (c, t, f) = (p.next()?, p.next()?, p.next()?);
            (c.eq_ignore_ascii_case(category)
                && t.split(',').any(|x| x.eq_ignore_ascii_case(target)))
            .then(|| calc.eval_formula(f))
        })
        .sum()
}

/// The name a prepared spell shows with its metamagic feats:
/// ` [Feat, Feat] ` after the spell's name.
pub fn metamagic_suffix(calc: &Calc, info: &SpellInfo) -> String {
    if info.feats.is_empty() {
        return String::new();
    }
    let names: Vec<String> = info
        .feats
        .iter()
        .map(|k| metamagic_feat(calc, k).map_or_else(|| k.clone(), |f| f.name.clone()))
        .collect();
    format!(" [{}] ", names.join(", "))
}

/// Save DC for a spell.
pub fn save_dc(calc: &Calc, cs: &CharSpell, info: &SpellInfo) -> i64 {
    let spell = cs.spell;
    if let Some(f) = &info.fixed_dc {
        if info.book.eq_ignore_ascii_case(INNATE_BOOK) || !matches!(cs.owner, Owner::Class(_)) {
            return calc.eval_formula(f) as i64;
        }
    }
    let b = &calc.bonus;
    let cls = owner_class(calc, cs);
    let stat_mod = cls
        .and_then(|c| c.get("SPELLSTAT"))
        .map(|st| {
            let score = calc.stat_score(st)
                + b.total("STAT", "BASESPELLSTAT")
                + cls
                    .map(|c| b.total("STAT", &format!("BASESPELLSTAT;CLASS={}", c.key)))
                    .unwrap_or(0.0)
                + b.total("STAT", &format!("CAST.{}", st));
            calc.stat_mod_for(st, score)
        })
        .unwrap_or(0.0);
    let formula = calc
        .gm
        .and_then(|g| g.misc("SPELLBASEDC"))
        .unwrap_or("10+SPELLLEVEL+BASESPELLSTAT")
        .to_string();
    // metamagic does not change the spell's own level, but a feat may
    // add to the DC
    let level = (info.level - metamagic_levels(calc, &info.feats)) as f64;
    let meta_dc: f64 = info
        .feats
        .iter()
        .filter_map(|k| metamagic_feat(calc, k))
        .map(|f| feat_bonus(calc, f, "DC", "FEATBONUS"))
        .sum();
    let mut dc = meta_dc
        + calc.eval_with(&formula, &|n| match n.to_ascii_uppercase().as_str() {
            "SPELLLEVEL" => Some(level),
            "BASESPELLSTAT" => Some(stat_mod),
            _ => None,
        });
    dc += b.total("DC", "ALLSPELLS");
    if cls.is_none() || cls.is_some_and(|c| c.has("USESPELLSPELLSTAT")) {
        if let Some(st) = spell.get("STAT") {
            dc += calc.stat_mod(st);
        }
    }
    dc += b.total("DC", &format!("SPELL.{}", spell.key));
    if let Owner::Domain(d, _) = &cs.owner {
        dc += b.total("DC", &format!("DOMAIN.{}", d.key));
    }
    if let Some(c) = cls {
        dc += b.total("DC", &format!("CLASS.{}", c.key));
        if let Some(t) = spell_type(c) {
            dc += b.total("DC", &format!("TYPE.{}", t));
        }
    }
    for s in spell.values("SCHOOL") {
        dc += b.total("DC", &format!("SCHOOL.{}", s));
    }
    for s in spell.values("SUBSCHOOL") {
        dc += b.total("DC", &format!("SUBSCHOOL.{}", s));
    }
    for d in spell.values("DESCRIPTOR").flat_map(|v| v.split(',')) {
        dc += b.total("DC", &format!("DESCRIPTOR.{}", d.trim()));
    }
    dc as i64
}

/// The highest spell level slots are counted to.
const MAX_SPELL_LEVEL: usize = 25;

/// Slots of one spell level of a prepared list: casts per day for any
/// spell and for specialty (or domain) spells only, and the copies of
/// each kind already prepared there.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SlotLevel {
    pub non: i64,
    pub spec: i64,
    pub mem_non: i64,
    pub mem_spec: i64,
}

/// Is there a slot for one more spell at `level`?  Lower levels'
/// shortfalls are made up from this level's spare slots first; spare
/// slots of higher levels count only with `use_higher`.  Arcane casters
/// may fill specialty slots' shortfalls from ordinary slots; divine
/// casters may not, and their specialty (domain) spells need a
/// specialty slot.
pub fn slot_available(
    levels: &[SlotLevel],
    level: usize,
    divine: bool,
    use_higher: bool,
    specialty: bool,
) -> bool {
    let at = |i: usize| levels.get(i).copied().unwrap_or_default();
    // make up a shortfall from spare slots
    let fill = |spare: &mut i64, short: &mut i64| {
        while *spare > 0 && *short < 0 {
            *spare -= 1;
            *short += 1;
        }
    };
    let (mut low_spec, mut low_non) = (0i64, 0i64);
    for i in 0..level {
        let l = at(i);
        let (mut exc_spec, mut exc_non) = (l.spec - l.mem_spec, l.non - l.mem_non);
        fill(&mut exc_non, &mut low_non);
        fill(&mut exc_spec, &mut low_spec);
        if !divine {
            fill(&mut exc_non, &mut low_spec);
            fill(&mut exc_non, &mut exc_spec);
        }
        low_spec += exc_spec.min(0);
        low_non += exc_non.min(0);
    }
    let (mut good_spec, mut good_non) = (0i64, 0i64);
    for i in level..=MAX_SPELL_LEVEL.max(level) {
        if !use_higher && i > level {
            break;
        }
        let l = at(i);
        let (mut exc_spec, mut exc_non) = (l.spec - l.mem_spec, l.non - l.mem_non);
        fill(&mut exc_non, &mut low_non);
        fill(&mut exc_non, &mut good_non);
        fill(&mut exc_spec, &mut low_spec);
        fill(&mut exc_spec, &mut good_spec);
        if !divine {
            fill(&mut exc_non, &mut low_spec);
            fill(&mut exc_non, &mut good_spec);
            fill(&mut exc_non, &mut exc_spec);
        }
        let free = if specialty {
            if divine {
                exc_spec > 0
            } else {
                exc_non + exc_spec > 0
            }
        } else {
            exc_non > 0 && exc_non + exc_spec > 0
        };
        if free {
            return true;
        }
        good_spec += exc_spec.min(0);
        good_non += exc_non.min(0);
    }
    false
}

/// Specialty (or domain) slots a class adds at a level of a prepared
/// list: those of its specialty, once it has a specialty spell (for a
/// divine caster, a domain spell above level 0) at or below the level.
fn specialty_slots(calc: &Calc, sp: &Spells, cls: &Object, level: i64) -> i64 {
    let domains: Vec<String> = calc
        .ch
        .pcg
        .lines_tagged("DOMAIN")
        .filter(|l| {
            l.get("SOURCE")
                .and_then(|e| e.children().iter().find(|c| c.tag == "NAME"))
                .is_some_and(|n| n.value_text().eq_ignore_ascii_case(&cls.key))
        })
        .filter_map(|l| l.head().map(|h| h.value_text()))
        .collect();
    if specialties(calc, cls).is_empty() && calc.ch.domains().is_empty() {
        return 0;
    }
    let divine = spell_type(cls).is_some_and(|t| t.eq_ignore_ascii_case("Divine"));
    let Some(ci) = sp.caster_index(cls) else {
        return 0;
    };
    for ix in 0..=level {
        let here: Vec<&CharSpell> = sp.by_caster[ci]
            .iter()
            .filter(|c| c.infos.iter().any(|i| i.level == ix))
            .collect();
        let found = if ix > 0 && divine && !domains.is_empty() {
            here.iter().any(|c| {
                domains
                    .iter()
                    .any(|d| sp.levels_on('D', d, c.spell).contains(&ix))
            })
        } else {
            here.iter().any(|c| is_specialty(calc, c))
        };
        if found {
            return specialty_slots_at(calc, sp, cls, &domains, level);
        }
    }
    0
}

/// Specialty slots of a level: bonuses to them, and (when the casting
/// stat allows the level) the specialty's own and one for a class
/// whose domains give it spells it can cast.
fn specialty_slots_at(
    calc: &Calc,
    sp: &Spells,
    cls: &Object,
    domains: &[String],
    level: i64,
) -> i64 {
    let mut total = calc.bonus.total(
        "SPECIALTYSPELLKNOWN",
        &format!("CLASS.{};LEVEL.{}", cls.key, level),
    );
    if let Some(t) = spell_type(cls) {
        total += calc.bonus.total(
            "SPECIALTYSPELLKNOWN",
            &format!("TYPE.{};LEVEL.{}", t, level),
        );
    }
    let total = total as i64;
    if let Some(st) = cls.get("SPELLSTAT") {
        let max = calc.var(&format!("MAXLEVELSTAT={}", st)).unwrap_or(0.0);
        if level as f64 > max {
            return total;
        }
    }
    let own = |o: &Object| {
        o.get("KNOWNSPELLSFROMSPECIALTY")
            .and_then(|v| v.trim().parse::<i64>().ok())
            .unwrap_or(0)
    };
    let from = calc.subclass_of(cls).map_or_else(|| own(cls), own);
    // a domain whose spells start below the highest level cast
    let max_cast = (0..10)
        .find(|l| cast_per_day(calc, cls, *l, true) == 0)
        .unwrap_or(10);
    let domain = own(cls) == 0
        && max_cast > 1
        && domains.iter().any(|d| {
            sp.lists
                .get(&('D', lower(d)))
                .is_some_and(|m| m.keys().any(|l| *l < max_cast))
        });
    total + from + i64::from(domain)
}

/// Can a class prepare one more spell at a level of a list?  `Err`
/// says why not.
pub fn check_prepared_slot(
    calc: &Calc,
    sp: &Spells,
    cls: &Object,
    book: &str,
    level: i64,
    specialty: bool,
) -> std::result::Result<(), String> {
    let Some(ci) = sp.caster_index(cls) else {
        return Err(format!("{} casts no spells", cls.key));
    };
    let levels: Vec<SlotLevel> = (0..=MAX_SPELL_LEVEL as i64)
        .map(|i| {
            let non = cast_per_day(calc, cls, i, true);
            let (mut mem_non, mut mem_spec) = (0, 0);
            for (cs, info) in sp.spells_in(ci, book, Some(i)) {
                if is_specialty(calc, cs) {
                    mem_spec += info.times;
                } else {
                    mem_non += info.times;
                }
            }
            SlotLevel {
                non,
                spec: specialty_slots(calc, sp, cls, i),
                mem_non,
                mem_spec,
            }
        })
        .collect();
    let divine = spell_type(cls).is_some_and(|t| t.eq_ignore_ascii_case("Divine"));
    let use_higher = calc
        .ch
        .pcg
        .value("USEHIGHERPREPPED")
        .is_some_and(|v| v.eq_ignore_ascii_case("Y"));
    let lvl = level.max(0) as usize;
    if slot_available(&levels, lvl, divine, use_higher, specialty) {
        return Ok(());
    }
    let cast = levels.get(lvl).map_or(0, |l| l.non + l.spec);
    if !specialty && slot_available(&levels, lvl, divine, use_higher, true) {
        Err("the remaining slots must be filled with specialty or domain spells".into())
    } else if specialty && slot_available(&levels, lvl, divine, use_higher, false) {
        Err("the remaining slots must be filled with spells not of the specialty or domain".into())
    } else {
        Err(format!(
            "{} can only prepare {} spells for level {}",
            cls.key, cast, level
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lv(non: i64, spec: i64, mem_non: i64, mem_spec: i64) -> SlotLevel {
        SlotLevel {
            non,
            spec,
            mem_non,
            mem_spec,
        }
    }

    #[test]
    fn prepared_slots() {
        // two slots at level 1, one used
        let l = [lv(3, 0, 0, 0), lv(2, 0, 1, 0)];
        assert!(slot_available(&l, 1, false, false, false));
        let l = [lv(3, 0, 0, 0), lv(2, 0, 2, 0)];
        assert!(!slot_available(&l, 1, false, false, false));
        // a level with no casts has no slot
        assert!(!slot_available(&l, 2, false, false, false));
        // a higher level's spare slot counts only when allowed
        let l = [lv(3, 0, 0, 0), lv(2, 0, 2, 0), lv(1, 0, 0, 0)];
        assert!(!slot_available(&l, 1, false, false, false));
        assert!(slot_available(&l, 1, false, true, false));
        // a lower level's overflow takes this level's spare slot
        let l = [lv(1, 0, 2, 0), lv(1, 0, 0, 0)];
        assert!(!slot_available(&l, 1, false, false, false));
    }

    #[test]
    fn specialty_slots_by_caster_type() {
        // one ordinary and one specialty slot, the ordinary one used
        let l = [lv(0, 0, 0, 0), lv(1, 1, 1, 0)];
        // only a specialty spell fits
        assert!(!slot_available(&l, 1, false, false, false));
        assert!(slot_available(&l, 1, false, false, true));
        assert!(slot_available(&l, 1, true, false, true));
        // the specialty slot used: an arcane caster's extra specialty
        // spell may take an ordinary slot, a divine caster's may not
        let l = [lv(0, 0, 0, 0), lv(2, 1, 0, 1)];
        assert!(slot_available(&l, 1, false, false, true));
        assert!(!slot_available(&l, 1, true, false, true));
        assert!(slot_available(&l, 1, true, false, false));
    }

    #[test]
    fn list_groups() {
        assert_eq!(
            level_groups("Cleric,Paladin=1|Wizard=2"),
            vec![
                (vec!["Cleric".to_string(), "Paladin".to_string()], 1),
                (vec!["Wizard".to_string()], 2)
            ]
        );
        assert_eq!(
            level_groups("Sun=3|PRECLASS:1,Cleric=1"),
            vec![(vec!["Sun".to_string()], 3)]
        );
        assert!(level_groups("bogus").is_empty());
        assert_eq!(
            level_groups("Psion=1[PREVARGTEQ:PsionicTelepath,1]"),
            vec![(vec!["Psion".to_string()], 1)]
        );
    }
}
