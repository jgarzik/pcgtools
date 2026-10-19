//
// rules/calc.rs -- derived character statistics
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! Computes a character's derived values (attack bonus, saves, hit
//! points, armor class, skill totals) from the data.
//!
//! The model follows the classic bonus system: every *active* object
//! (stats, saves, skills, race, size, templates, class levels gained,
//! abilities, equipped items, deity, domains) contributes `DEFINE`d
//! variables and `BONUS` tokens, each gated by prerequisites.  Values
//! depend on each other, so evaluation repeats until nothing changes.

use super::bonus::BonusTable;
use super::eval::{eval, eval_str, Env};
use crate::dataset::{Dataset, Object, Tok};
use crate::gamemode::GameMode;
use crate::lst::kind::FileKind;
use crate::pcg::character::Character;
use crate::token::bonus::{Bonus, BonusOpt};
use crate::token::formula::Expr;
use crate::token::pre::{PreBody, Prereq};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};

const MAX_PASSES: usize = 40;

/// Bonus values and variable values after a pass, for detecting
/// convergence.
type Snapshot = (Vec<f64>, Vec<(String, f64)>);

/// What kind of thing an active source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SrcKind {
    Stat,
    Save,
    Skill,
    Alignment,
    Race,
    Size,
    Template,
    Class,
    Ability,
    Equipment,
    EquipMod,
    Deity,
    Domain,
}

/// An active object and the tokens it currently contributes.
#[derive(Debug, Clone)]
pub struct Src<'a> {
    pub kind: SrcKind,
    pub obj: &'a Object,
    pub tokens: Vec<&'a Tok>,
    /// owning class key, for CL and classlevel()
    pub class: Option<String>,
    /// chosen values, for %LIST / %CHOICE
    pub choices: Vec<String>,
    /// index of the equipped item this source belongs to
    pub item: Option<usize>,
    /// times the object applies (abilities taken more than once)
    pub times: f64,
}

/// An equipped item.
#[derive(Debug, Clone)]
pub struct Item<'a> {
    pub name: String,
    pub obj: &'a Object,
    pub eqmods: Vec<&'a Object>,
    pub location: String,
}

/// Computed values.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Sheet {
    pub level: u32,
    pub stats: BTreeMap<String, (i64, i64)>,
    pub bab: i64,
    pub saves: BTreeMap<String, i64>,
    pub hp: i64,
    pub ac: BTreeMap<String, i64>,
    pub skills: BTreeMap<String, i64>,
    pub armor_check: i64,
    pub abilities: Vec<String>,
    pub passes: usize,
    /// names evaluated as zero because nothing defines them
    pub unknown: BTreeMap<String, usize>,
}

pub struct Calc<'a> {
    pub ds: &'a Dataset,
    pub gm: Option<&'a GameMode>,
    pub ch: &'a Character,
    /// (class object, level)
    classes: Vec<(&'a Object, u32)>,
    stat_base: HashMap<String, f64>,
    ranks: HashMap<String, f64>,
    pcg_class_skills: HashSet<String>,
    items: Vec<Item<'a>>,
    pub sources: Vec<Src<'a>>,
    pub bonus: BonusTable,
    vars: HashMap<String, f64>,
    defined: HashSet<String>,
    unknown: RefCell<BTreeMap<String, usize>>,
    class_skills: HashSet<String>,
    rules: HashMap<String, bool>,
}

fn upper(s: &str) -> String {
    s.to_ascii_uppercase()
}

fn lower(s: &str) -> String {
    s.to_lowercase()
}

/// Split "Key (Choice)" into key and choice.
fn split_choice(s: &str) -> (&str, Option<&str>) {
    match s.rfind(" (") {
        Some(i) if s.ends_with(')') => (&s[..i], Some(&s[i + 2..s.len() - 1])),
        _ => (s, None),
    }
}

impl<'a> Calc<'a> {
    pub fn new(ds: &'a Dataset, gm: Option<&'a GameMode>, ch: &'a Character) -> Calc<'a> {
        let stacking = gm.map(|g| g.stacking_bonus_types()).unwrap_or_default();
        let mut c = Calc {
            ds,
            gm,
            ch,
            classes: Vec::new(),
            stat_base: HashMap::new(),
            ranks: HashMap::new(),
            pcg_class_skills: HashSet::new(),
            items: Vec::new(),
            sources: Vec::new(),
            bonus: BonusTable::new(&stacking),
            vars: HashMap::new(),
            defined: HashSet::new(),
            unknown: RefCell::new(BTreeMap::new()),
            class_skills: HashSet::new(),
            rules: HashMap::new(),
        };
        for cl in ch.classes() {
            if let Some(o) = ds.get(FileKind::Class, &cl.name) {
                c.classes.push((o, cl.level));
            }
        }
        for (abb, score) in ch.stats() {
            c.stat_base.insert(upper(&abb), score as f64);
        }
        for s in ch.skills() {
            let ranks = s.total_ranks();
            *c.ranks.entry(lower(&s.name)).or_default() += ranks;
            if let Some(line) = ch
                .pcg
                .lines_tagged("SKILL")
                .find(|l| l.head().is_some_and(|h| h.value_text() == s.name))
            {
                let y = line.elems.iter().flatten().any(|e| {
                    e.tag == "CLASSBOUGHT"
                        && e.children()
                            .iter()
                            .any(|x| x.tag == "CLASSSKILL" && x.value_text() == "Y")
                });
                if y {
                    c.pcg_class_skills.insert(lower(&s.name));
                }
            }
        }
        if let Some(g) = gm {
            for l in g.lines("rules.lst", "NAME") {
                if let Some(v) = l.get("VAR") {
                    let on = l
                        .get("DEFAULT")
                        .is_some_and(|d| d.eq_ignore_ascii_case("yes"));
                    c.rules.insert(upper(v), on);
                }
            }
        }
        c.items = c.equipped_items();
        c
    }

    fn equipped_items(&self) -> Vec<Item<'a>> {
        let slots: HashSet<String> = self
            .gm
            .map(|g| {
                g.lines("equipmentslots.lst", "EQSLOT")
                    .map(|l| lower(&l.value))
                    .collect()
            })
            .unwrap_or_default();
        let mut out = Vec::new();
        for e in self.ch.equipped() {
            let loc = lower(&e.location);
            // items directly in the set, at a body slot
            let depth = e.id.matches('.').count();
            let worn = depth == 2 && (slots.contains(&loc) || loc == "equipped");
            if !worn {
                continue;
            }
            let Some(name) = e.item else { continue };
            let entry = self.ch.equipment().into_iter().find(|x| x.name == name);
            let base = entry.as_ref().and_then(|x| x.base.clone());
            let key = base.clone().unwrap_or_else(|| name.clone());
            let obj = self
                .ds
                .get(FileKind::Equipment, &key)
                .or_else(|| {
                    self.ds
                        .get(FileKind::Equipment, &self.migrate("EQUIPMENT", &key))
                })
                .or_else(|| self.ds.get(FileKind::Equipment, &name));
            let Some(obj) = obj else { continue };
            let mut eqmods = Vec::new();
            // eqmods named on the customization line
            if let Some(line) = self
                .ch
                .pcg
                .lines_tagged("EQUIPNAME")
                .find(|l| l.head().is_some_and(|h| h.value_text() == name))
            {
                if let Some(cust) = line.get("CUSTOMIZATION") {
                    for d in cust.children().iter().filter(|x| x.tag == "DATA") {
                        let data = d.value_text();
                        for part in data.split('$') {
                            if let Some(list) = part.strip_prefix("EQMOD=") {
                                for k in list.split('.') {
                                    let k = k.split('|').next().unwrap_or(k);
                                    if let Some(m) = self.find_eqmod(k) {
                                        eqmods.push(m);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // eqmods built into the item
            for v in obj.values("EQMOD") {
                for k in v.split('.') {
                    if let Some(m) = self.find_eqmod(k.split('|').next().unwrap_or(k)) {
                        if !eqmods.iter().any(|x| std::ptr::eq(*x, m)) {
                            eqmods.push(m);
                        }
                    }
                }
            }
            out.push(Item {
                name,
                obj,
                eqmods,
                location: e.location.clone(),
            });
        }
        out
    }

    /// Map an old key through the game mode's migration rules.
    fn migrate(&self, kind: &str, key: &str) -> String {
        let ver = self.ch.pcg.value("VERSION").unwrap_or_default();
        self.gm
            .and_then(|g| g.migrate(kind, key, &ver))
            .unwrap_or_else(|| key.to_string())
    }

    /// Item types include the ITYPE values its modifiers add.
    fn item_is_type(&self, i: usize, t: &str) -> bool {
        let it = &self.items[i];
        it.obj.is_type(t)
            || it.eqmods.iter().any(|m| {
                m.values("ITYPE")
                    .flat_map(|v| v.split('.'))
                    .any(|x| x.eq_ignore_ascii_case(t))
            })
    }

    /// Weight carried: items placed in the calculation equipment set,
    /// except those under "Not Carried".
    pub fn carried_weight(&self) -> f64 {
        let sets = self.ch.equipped();
        let not_carried: Vec<String> = sets
            .iter()
            .filter(|e| e.location.eq_ignore_ascii_case("Not Carried"))
            .map(|e| format!("{}.", e.id))
            .collect();
        let mut total = 0.0;
        for e in &sets {
            if e.location.eq_ignore_ascii_case("Not Carried")
                || not_carried.iter().any(|p| e.id.starts_with(p.as_str()))
            {
                continue;
            }
            let Some(item) = &e.item else { continue };
            let wt: f64 = self
                .ch
                .pcg
                .lines_tagged("EQUIPNAME")
                .find(|l| l.head().is_some_and(|h| h.value_text() == *item))
                .and_then(|l| l.text("WT"))
                .and_then(|w| w.parse().ok())
                .unwrap_or(0.0);
            total += wt * e.quantity.unwrap_or(1.0);
        }
        total
    }

    /// Maximum load before the heaviest category.
    fn max_load(&self) -> f64 {
        let Some(g) = self.gm else { return 0.0 };
        let score = self
            .var_value("LOADSCORE", None)
            .unwrap_or_else(|| self.stat_score("STR"));
        let table: Vec<(f64, f64)> = g
            .lines("load.lst", "LOAD")
            .filter_map(|l| {
                let (a, b) = l.value.split_once('|')?;
                Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
            })
            .collect();
        if table.is_empty() {
            return 0.0;
        }
        let mult: f64 = g
            .lines("load.lst", "LOADMULT")
            .next()
            .and_then(|l| l.value.parse().ok())
            .unwrap_or(4.0);
        let (min, max) = (table[0].0, table[table.len() - 1].0);
        let lookup = |s: f64| -> f64 {
            table
                .iter()
                .rev()
                .find(|(k, _)| *k <= s)
                .map(|(_, v)| *v)
                .unwrap_or(0.0)
        };
        let mut s = score.floor();
        let mut factor = 1.0;
        if s < min {
            return 0.0;
        }
        while s > max {
            s -= 10.0;
            factor *= mult;
        }
        let size = self.size_key();
        let size_mult: f64 = g
            .lines("load.lst", "SIZEMULT")
            .find_map(|l| {
                let (a, b) = l.value.split_once('|')?;
                a.eq_ignore_ascii_case(&size)
                    .then(|| b.parse().ok())
                    .flatten()
            })
            .unwrap_or(1.0);
        lookup(s) * factor * size_mult
    }

    /// Load category: 0 light, 1 medium, 2 heavy, 3 overloaded.
    pub fn encumbrance(&self) -> u32 {
        if !self.rules.get("SYS_LDPACSK").copied().unwrap_or(true) {
            return 0;
        }
        let Some(g) = self.gm else { return 0 };
        let max = self.max_load();
        if max <= 0.0 {
            return 0;
        }
        let ratio = self.carried_weight() / max;
        let frac = |name: &str| -> Option<f64> {
            let l = g.lines("load.lst", "ENCUMBRANCE").find(|l| {
                l.value
                    .split('|')
                    .next()
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
            })?;
            let f = l.value.split('|').nth(1)?;
            match f.split_once('/') {
                Some((a, b)) => Some(a.parse::<f64>().ok()? / b.parse::<f64>().ok()?),
                None => f.parse().ok(),
            }
        };
        for (i, n) in ["Light", "Medium", "Heavy"].iter().enumerate() {
            if frac(n).is_some_and(|f| ratio <= f) {
                return i as u32;
            }
        }
        3
    }

    /// Maximum dexterity bonus allowed by equipped armor and load.
    pub fn max_dex(&self) -> f64 {
        const MAX: f64 = 100.0;
        let stat_bonus = self
            .sources
            .iter()
            .filter(|s| s.kind == SrcKind::Stat)
            .count() as f64
            * 0.0;
        let load = self.encumbrance();
        let mut bonus = match load {
            1 => 3.0,
            2 => 1.0,
            3 => 0.0,
            _ => stat_bonus,
        };
        let mut use_max = load == 0;
        for (i, it) in self.items.iter().enumerate() {
            let Some(md) = it.obj.get("MAXDEX").and_then(|v| v.parse::<f64>().ok()) else {
                continue;
            };
            let md = md + self.bonus.total(&format!("ITEM{}.EQMARMOR", i), "MAXDEX");
            if use_max || bonus > md {
                bonus = md;
            }
            use_max = false;
        }
        if use_max {
            bonus = MAX;
        }
        bonus += self.bonus.total("MISC", "MAXDEX") - stat_bonus;
        bonus.clamp(0.0, MAX)
    }

    /// Movement types and speeds from active objects.
    pub fn movement(&self) -> BTreeMap<String, f64> {
        let mut m = BTreeMap::new();
        for s in &self.sources {
            for t in s.tokens.iter().filter(|t| t.tag == "MOVE") {
                let parts: Vec<&str> = t.value.split(',').collect();
                for pair in parts.chunks(2) {
                    if let [ty, sp] = pair {
                        let sp: f64 = sp.trim().parse().unwrap_or(0.0);
                        let e = m.entry(lower(ty.trim())).or_insert(0.0);
                        *e = f64::max(*e, sp);
                    }
                }
            }
        }
        m
    }

    /// The ability category an ability pool draws from (pools are
    /// declared by ABILITYCATEGORY objects; a category is its own pool).
    fn pool_category(&self, pool: &str) -> String {
        self.ds
            .get(FileKind::AbilityCategory, pool)
            .and_then(|o| o.get("CATEGORY"))
            .unwrap_or(pool)
            .to_string()
    }

    /// The character's age set (index, name), from the race's bio
    /// settings: the first age set whose MAXAGE covers the age.
    pub fn age_set(&self) -> Option<(u32, String)> {
        let age: f64 = self.ch.pcg.value("AGE")?.trim().parse().ok()?;
        let race = self.race()?;
        let rkey = lower(&race.key);
        let mut best: Option<(u32, String)> = None;
        for o in self.ds.objects(FileKind::BioSet) {
            let (idx, name) = o.name.split_once('|').unwrap_or(("0", &o.name));
            let Ok(idx) = idx.trim().parse::<u32>() else {
                continue;
            };
            for sl in o.sublines.iter().filter(|s| s.tag == "RACENAME") {
                let pat = lower(sl.value.trim_end_matches('%'));
                let hit = if sl.value.ends_with('%') {
                    rkey.starts_with(&pat)
                } else {
                    rkey == pat
                };
                let max = sl
                    .tokens
                    .iter()
                    .find(|t| t.tag == "MAXAGE")
                    .and_then(|t| t.value.parse::<f64>().ok());
                if let (true, Some(m)) = (hit, max) {
                    if age <= m && best.as_ref().is_none_or(|(b, _)| idx < *b) {
                        best = Some((idx, name.to_string()));
                    }
                }
            }
        }
        best
    }

    /// Index of a named age set.
    fn age_set_index(&self, name: &str) -> Option<u32> {
        self.ds.objects(FileKind::BioSet).iter().find_map(|o| {
            let (idx, n) = o.name.split_once('|')?;
            n.eq_ignore_ascii_case(name)
                .then(|| idx.trim().parse().ok())
                .flatten()
        })
    }

    fn find_eqmod(&self, key: &str) -> Option<&'a Object> {
        self.ds.get(FileKind::EquipMod, key).or_else(|| {
            let lk = lower(key);
            self.ds
                .objects(FileKind::EquipMod)
                .iter()
                .find(|o| o.get("KEY").is_some_and(|k| lower(k) == lk))
        })
    }

    // ---- facts about the character ----

    pub fn total_level(&self) -> u32 {
        self.classes.iter().map(|(_, l)| l).sum()
    }

    pub fn class_level(&self, name: &str) -> u32 {
        let n = lower(name);
        self.classes
            .iter()
            .filter(|(o, _)| lower(&o.key) == n || lower(&o.name) == n)
            .map(|(_, l)| *l)
            .sum()
    }

    pub fn stat_score(&self, abb: &str) -> f64 {
        let a = upper(abb);
        self.stat_base.get(&a).copied().unwrap_or(0.0) + self.bonus.total("STAT", &a)
    }

    pub fn stat_mod(&self, abb: &str) -> f64 {
        let a = upper(abb);
        let score = self.stat_score(&a);
        let formula = self
            .ds
            .get(FileKind::Stat, &a)
            .and_then(|s| s.get("STATMOD"))
            .unwrap_or("floor(SCORE/2)-5");
        let env = ScoreEnv { calc: self, score };
        eval_str(formula, &env).floor()
    }

    pub fn skill_ranks(&self, skill: &str) -> f64 {
        self.ranks.get(&lower(skill)).copied().unwrap_or(0.0) + self.bonus.total("SKILLRANK", skill)
    }

    pub fn is_class_skill(&self, skill: &str) -> bool {
        let s = lower(skill);
        self.class_skills.contains(&s) || self.pcg_class_skills.contains(&s)
    }

    fn race(&self) -> Option<&'a Object> {
        let r = self.ch.race()?;
        self.ds
            .get(FileKind::Race, &r)
            .or_else(|| self.ds.get(FileKind::Race, &self.migrate("RACE", &r)))
    }

    /// Size categories in order, smallest first.
    fn size_list(&self) -> Vec<&'a Object> {
        let mut v: Vec<&Object> = self.ds.objects(FileKind::Size).iter().collect();
        if v.iter().all(|o| o.get("SIZENUM").is_some()) {
            v.sort_by_key(|o| {
                o.get("SIZENUM")
                    .and_then(|n| n.parse::<i64>().ok())
                    .unwrap_or(0)
            });
        }
        v
    }

    /// Resolve a SIZE value: a size key, or a formula giving an index.
    fn size_value_index(&self, v: &str, src: Option<&Src<'a>>) -> Option<i64> {
        let sizes = self.size_list();
        if let Some(i) = sizes.iter().position(|o| o.key.eq_ignore_ascii_case(v)) {
            return Some(i as i64);
        }
        if v.eq_ignore_ascii_case("PC") {
            return None;
        }
        let env = Ctx { calc: self, src };
        Some(eval_str(v, &env) as i64)
    }

    /// Index of a size named by key, name or display name.
    fn size_named(&self, name: &str) -> Option<i64> {
        self.size_list()
            .iter()
            .position(|o| {
                o.key.eq_ignore_ascii_case(name)
                    || o.name.eq_ignore_ascii_case(name)
                    || o.get("DISPLAYNAME")
                        .is_some_and(|d| d.eq_ignore_ascii_case(name))
            })
            .map(|i| i as i64)
    }

    fn default_size_index(&self) -> i64 {
        let sizes = self.size_list();
        sizes
            .iter()
            .position(|o| {
                o.get("ISDEFAULTSIZE")
                    .is_some_and(|v| v.eq_ignore_ascii_case("Y") || v.eq_ignore_ascii_case("YES"))
            })
            .unwrap_or(sizes.len().min(5).saturating_sub(1)) as i64
    }

    /// Racial size: the race's size, or the last template that sets
    /// one (before SIZEMOD adjustments).
    pub fn base_size_index(&self) -> i64 {
        let mut idx = self
            .race()
            .and_then(|r| r.get("SIZE"))
            .and_then(|v| self.size_value_index(v, None))
            .unwrap_or_else(|| self.default_size_index());
        for s in &self.sources {
            if s.kind == SrcKind::Template {
                if let Some(v) = s.obj.get("SIZE") {
                    if let Some(i) = self.size_value_index(v, Some(s)) {
                        idx = i;
                    }
                }
            }
        }
        idx
    }

    /// Index of the character's size category.
    pub fn size_index(&self) -> i64 {
        let n = self.size_list().len();
        let idx = self.base_size_index() + self.bonus.total("SIZEMOD", "NUMBER") as i64;
        idx.clamp(0, n.saturating_sub(1) as i64)
    }

    pub fn size_key(&self) -> String {
        let sizes = self.size_list();
        sizes
            .get(self.size_index() as usize)
            .map(|o| o.key.clone())
            .unwrap_or_else(|| "M".into())
    }

    pub fn size_num(&self) -> f64 {
        self.size_index() as f64
    }

    /// Abilities active now: (category, key, choices)
    fn has_ability(&self, category: Option<&str>, key: &str) -> bool {
        let k = lower(key);
        self.sources.iter().any(|s| {
            s.kind == SrcKind::Ability
                && (lower(&s.obj.key) == k || lower(&s.obj.name) == k)
                && category.is_none_or(|c| {
                    s.obj
                        .category
                        .as_deref()
                        .is_some_and(|x| x.eq_ignore_ascii_case(c))
                })
        })
    }

    fn ability_srcs(&self) -> impl Iterator<Item = &Src<'a>> {
        self.sources.iter().filter(|s| s.kind == SrcKind::Ability)
    }

    // ---- variables ----

    fn var_value(&self, name: &str, src: Option<&Src<'a>>) -> Option<f64> {
        let u = upper(name);
        if self.defined.contains(&u) {
            return Some(self.vars.get(&u).copied().unwrap_or(0.0) + self.bonus.total("VAR", &u));
        }
        self.builtin(name, src)
    }

    /// Built-in terms.
    fn builtin(&self, name: &str, src: Option<&Src<'a>>) -> Option<f64> {
        let u = upper(name);
        if let Some(r) = self.rules.get(&u) {
            return Some(if *r { 1.0 } else { 0.0 });
        }
        if self.stat_base.contains_key(&u) {
            return Some(self.stat_mod(&u));
        }
        if let Some(a) = u.strip_suffix("SCORE") {
            if self.stat_base.contains_key(a) {
                return Some(self.stat_score(a));
            }
        }
        match u.as_str() {
            "TL" | "HD" => return Some(self.total_level() as f64),
            "CL" => {
                return Some(
                    src.and_then(|s| s.class.as_deref())
                        .map(|c| self.class_level(c) as f64)
                        .unwrap_or(0.0),
                )
            }
            "BAB" => return Some(self.bonus.total("COMBAT", "BASEAB")),
            "SIZE" | "SIZEMOD" => return Some(self.size_num()),
            "ACCHECK" | "ACHECK" | "MODEQUIPACCHECK" => return Some(self.armor_check() as f64),
            "MODEQUIPMAXDEX" => return Some(self.max_dex()),
            "ENCUMBERANCE" => return Some(self.encumbrance() as f64),
            "SCORE" => return Some(0.0),
            "COUNT[CLASSES]" => return Some(self.classes.len() as f64),
            "COUNT[FEATS]" | "COUNT[FEATS.ALL]" | "COUNT[FEATSALL]" => {
                return Some(
                    self.ability_srcs()
                        .filter(|s| {
                            s.obj
                                .category
                                .as_deref()
                                .is_some_and(|c| c.eq_ignore_ascii_case("FEAT"))
                        })
                        .count() as f64,
                )
            }
            _ => {}
        }
        for p in [
            "CL=",
            "CL.",
            "CLASSLEVEL=",
            "CLASSLEVEL.",
            "CLASS=",
            "CLASS.",
        ] {
            if let Some(c) = name.get(p.len()..).filter(|_| u.starts_with(p)) {
                let lvl = self.class_level(c) as f64;
                return Some(if p.starts_with("CLASS=") || p.starts_with("CLASS.") {
                    if lvl > 0.0 {
                        1.0
                    } else {
                        0.0
                    }
                } else {
                    lvl
                });
            }
        }
        for p in ["SKILLRANK=", "SKILLRANK."] {
            if u.starts_with(p) {
                return Some(self.skill_ranks(&name[p.len()..]));
            }
        }
        for p in ["SKILLTOTAL=", "SKILLTOTAL."] {
            if u.starts_with(p) {
                return Some(self.skill_total(&name[p.len()..]) as f64);
            }
        }
        if let Some(rest) = u.strip_prefix("SKILL.") {
            // SKILL.name.RANK / .TOTAL / .MISC
            let orig = &name[6..];
            if let Some((sk, what)) = orig.rsplit_once('.') {
                let _ = rest;
                return Some(match upper(what).as_str() {
                    "RANK" => self.skill_ranks(sk),
                    "TOTAL" => self.skill_total(sk) as f64,
                    _ => 0.0,
                });
            }
        }
        if let Some(rest) = u.strip_prefix("HASFEAT:") {
            let _ = rest;
            return Some(if self.has_ability(Some("FEAT"), &name[8..]) {
                1.0
            } else {
                0.0
            });
        }
        if let Some(inner) = u.strip_prefix("COUNT[").and_then(|x| x.strip_suffix(']')) {
            let orig = &name[6..name.len() - 1];
            for p in ["FEATTYPE=", "FEATTYPE.", "VFEATTYPE=", "FEATAUTOTYPE="] {
                if inner.starts_with(p) {
                    let t = &orig[p.len()..];
                    return Some(
                        self.ability_srcs()
                            .filter(|s| {
                                s.obj
                                    .category
                                    .as_deref()
                                    .is_some_and(|c| c.eq_ignore_ascii_case("FEAT"))
                                    && s.obj.is_type(t)
                            })
                            .count() as f64,
                    );
                }
            }
            if inner.starts_with("FEATNAME=") {
                let n = &orig[9..];
                return Some(if self.has_ability(Some("FEAT"), n) {
                    1.0
                } else {
                    0.0
                });
            }
            if inner == "SKILLS" {
                return Some(self.ranks.values().filter(|r| **r > 0.0).count() as f64);
            }
            if inner == "TEMPLATES" {
                return Some(
                    self.sources
                        .iter()
                        .filter(|s| s.kind == SrcKind::Template)
                        .count() as f64,
                );
            }
            if inner == "LANGUAGES" {
                return Some(self.ch.languages().len() as f64);
            }
            return Some(0.0);
        }
        None
    }

    fn call(&self, name: &str, args: &[Expr], src: Option<&Src<'a>>) -> Option<f64> {
        let s = |i: usize| -> Option<String> {
            match args.get(i)? {
                Expr::Str(s) => Some(s.clone()),
                Expr::Var(v) => Some(v.clone()),
                _ => None,
            }
        };
        match lower(name).as_str() {
            "var" => self.var_value(&s(0)?, src),
            "classlevel" => {
                let a = s(0).unwrap_or_default();
                if a.is_empty() {
                    let c = src.and_then(|x| x.class.as_deref())?;
                    return Some(self.class_level(c) as f64);
                }
                if a.eq_ignore_ascii_case("APPLIEDAS=NONEPIC") {
                    let c = src.and_then(|x| x.class.as_deref())?;
                    let lvl = self.class_level(c);
                    let max_non_epic: u32 = self
                        .gm
                        .and_then(|g| g.misc("MAXNONEPICLEVEL"))
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(20);
                    let before = self
                        .classes
                        .iter()
                        .take_while(|(o, _)| lower(&o.key) != lower(c))
                        .map(|(_, l)| *l)
                        .sum::<u32>();
                    let allowed = max_non_epic.saturating_sub(before);
                    Some(lvl.min(allowed) as f64)
                } else {
                    Some(self.class_level(&a) as f64)
                }
            }
            "skillinfo" => {
                let what = upper(&s(0)?);
                let sk = s(1)?;
                Some(match what.as_str() {
                    "RANK" | "TOTALRANK" => self.skill_ranks(&sk),
                    "TOTAL" => self.skill_total(&sk) as f64,
                    _ => 0.0,
                })
            }
            "charbonusto" => {
                let cat = s(0)?;
                let tgt = s(1)?;
                Some(self.bonus.total(&cat, &tgt))
            }
            "count" => {
                let what = upper(&s(0)?);
                if what != "ABILITIES" {
                    return Some(0.0);
                }
                let mut n = 0;
                'srcs: for a in self.ability_srcs() {
                    for i in 1..args.len() {
                        let Some(f) = s(i) else { continue };
                        let (k, v) = f.split_once('=').unwrap_or((&f, ""));
                        let ok = match upper(k).as_str() {
                            "CATEGORY" => a
                                .obj
                                .category
                                .as_deref()
                                .is_some_and(|c| c.eq_ignore_ascii_case(v)),
                            "KEY" => a.obj.key.eq_ignore_ascii_case(v),
                            "NAME" => a.obj.name.eq_ignore_ascii_case(v),
                            "TYPE" => v.split('.').all(|t| a.obj.is_type(t)),
                            _ => true,
                        };
                        if !ok {
                            continue 'srcs;
                        }
                    }
                    n += 1;
                }
                Some(n as f64)
            }
            _ => None,
        }
    }

    // ---- prerequisites ----

    pub fn passes(&self, p: &Prereq, src: Option<&Src<'a>>) -> bool {
        let r = self.passes_inner(p, src);
        r != p.negated
    }

    fn count_items(&self, args: &[String], f: impl Fn(&str) -> bool) -> bool {
        let need: usize = args
            .first()
            .and_then(|a| a.trim().parse().ok())
            .unwrap_or(1);
        let n = args.iter().skip(1).filter(|a| f(a.trim())).count();
        n >= need
    }

    fn passes_inner(&self, p: &Prereq, src: Option<&Src<'a>>) -> bool {
        let env = Ctx { calc: self, src };
        let num = |s: &str| eval_str(s, &env);
        let args = p.args();
        let kind = upper(&p.kind);
        let cmp = |op: &str, a: f64, b: f64| match op {
            "EQ" => (a - b).abs() < 1e-9,
            "NEQ" => (a - b).abs() >= 1e-9,
            "GT" => a > b,
            "GTEQ" => a >= b,
            "LT" => a < b,
            "LTEQ" => a <= b,
            _ => true,
        };
        if let Some(op) = kind.strip_prefix("VAR") {
            // pairs of formula,value
            return args
                .chunks(2)
                .all(|c| c.len() < 2 || cmp(op, num(&c[0]), num(&c[1])));
        }
        match kind.as_str() {
            "MULT" => {
                let PreBody::Mult { count, subs, .. } = &p.body else {
                    return true;
                };
                let need: usize = count.trim().parse().unwrap_or(1);
                subs.iter().filter(|s| self.passes(s, src)).count() >= need
            }
            "ABILITY" | "FEAT" => {
                let mut cat: Option<String> = if kind == "FEAT" {
                    Some("FEAT".into())
                } else {
                    None
                };
                let need: usize = args
                    .first()
                    .and_then(|a| a.trim().parse().ok())
                    .unwrap_or(1);
                let mut n = 0;
                for a in args.iter().skip(1) {
                    let a = a.trim();
                    if let Some(c) = a.strip_prefix("CATEGORY=") {
                        cat = Some(c.to_string());
                        continue;
                    }
                    let hit = if let Some(t) =
                        a.strip_prefix("TYPE=").or_else(|| a.strip_prefix("TYPE."))
                    {
                        self.ability_srcs()
                            .filter(|s| {
                                cat.as_deref().is_none_or(|c| {
                                    s.obj
                                        .category
                                        .as_deref()
                                        .is_some_and(|x| x.eq_ignore_ascii_case(c))
                                })
                            })
                            .filter(|s| t.split('.').all(|tt| s.obj.is_type(tt)))
                            .count()
                    } else if a.eq_ignore_ascii_case("ANY") {
                        self.ability_srcs().count()
                    } else {
                        let (k, choice) = split_choice(a);
                        let k = k.trim_end_matches('%');
                        usize::from(self.ability_srcs().any(|s| {
                            (s.obj.key.eq_ignore_ascii_case(k)
                                || s.obj.name.eq_ignore_ascii_case(k)
                                || s.obj.key.eq_ignore_ascii_case(a))
                                && cat.as_deref().is_none_or(|c| {
                                    s.obj
                                        .category
                                        .as_deref()
                                        .is_some_and(|x| x.eq_ignore_ascii_case(c))
                                })
                                && choice.is_none_or(|ch| {
                                    s.choices.iter().any(|x| x.eq_ignore_ascii_case(ch))
                                        || s.obj.key.eq_ignore_ascii_case(a)
                                })
                        }))
                    };
                    n += hit;
                }
                n >= need
            }
            "CLASS" | "LEVEL" | "PCLEVEL"
                if args
                    .iter()
                    .any(|a| a.starts_with("MIN=") || a.starts_with("MAX=")) =>
            {
                let lvl = self.total_level() as f64;
                args.iter().all(|a| {
                    if let Some(v) = a.strip_prefix("MIN=") {
                        lvl >= num(v)
                    } else if let Some(v) = a.strip_prefix("MAX=") {
                        lvl <= num(v)
                    } else {
                        true
                    }
                })
            }
            "CLASS" => self.count_items(args, |a| {
                let (c, lv) = a.split_once('=').unwrap_or((a, "1"));
                let need = num(lv);
                if let Some(t) = c.strip_prefix("TYPE.").or_else(|| c.strip_prefix("TYPE=")) {
                    return self
                        .classes
                        .iter()
                        .any(|(o, l)| o.is_type(t) && *l as f64 >= need);
                }
                if c.eq_ignore_ascii_case("ANY") {
                    return self.classes.iter().any(|(_, l)| *l as f64 >= need);
                }
                self.class_level(c) as f64 >= need
            }),
            "LEVELMAX" => args
                .first()
                .is_none_or(|a| self.total_level() as f64 <= num(a)),
            "STAT" | "STATEQ" | "STATGT" | "STATGTEQ" | "STATLT" | "STATLTEQ" | "STATNEQ" => {
                let op = match kind.as_str() {
                    "STAT" => "GTEQ",
                    k => &k[4..],
                };
                self.count_items(args, |a| {
                    let (s, v) = a.split_once('=').unwrap_or((a, "0"));
                    cmp(op, self.stat_score(s), num(v))
                })
            }
            "SKILL" => self.count_items(args, |a| {
                let (s, v) = a.split_once('=').unwrap_or((a, "1"));
                self.skill_ranks(s) >= num(v)
            }),
            "CSKILL" => self.count_items(args, |a| self.is_class_skill(a)),
            "RACE" => self.count_items(args, |a| {
                let Some(r) = self.race() else { return false };
                if let Some(t) = a.strip_prefix("TYPE=").or_else(|| a.strip_prefix("TYPE.")) {
                    return r.is_type(t) || r.values("RACETYPE").any(|x| x.eq_ignore_ascii_case(t));
                }
                if let Some(t) = a.strip_prefix("RACETYPE=") {
                    return r.values("RACETYPE").any(|x| x.eq_ignore_ascii_case(t));
                }
                if let Some(t) = a.strip_prefix("RACESUBTYPE=") {
                    return r
                        .values("RACESUBTYPE")
                        .any(|x| x.split('|').any(|y| y.eq_ignore_ascii_case(t)));
                }
                let pat = a.trim_end_matches('%');
                if a.ends_with('%') {
                    lower(&r.key).starts_with(&lower(pat))
                } else {
                    r.key.eq_ignore_ascii_case(a) || r.name.eq_ignore_ascii_case(a)
                }
            }),
            "TEMPLATE" => self.count_items(args, |a| {
                let pat = lower(a.trim_end_matches('%'));
                self.sources.iter().any(|s| {
                    s.kind == SrcKind::Template
                        && if a.ends_with('%') {
                            lower(&s.obj.key).starts_with(&pat)
                        } else {
                            lower(&s.obj.key) == pat
                        }
                })
            }),
            "ALIGN" => {
                let Some(al) = self.ch.alignment() else {
                    return false;
                };
                args.iter().any(|a| a.trim().eq_ignore_ascii_case(&al))
            }
            "SIZE" | "SIZEEQ" | "SIZEGT" | "SIZEGTEQ" | "SIZELT" | "SIZELTEQ" | "SIZENEQ"
            | "BASESIZEEQ" | "BASESIZEGT" | "BASESIZEGTEQ" | "BASESIZELT" | "BASESIZELTEQ"
            | "BASESIZENEQ" => {
                let op = kind.trim_start_matches("BASE").trim_start_matches("SIZE");
                let op = if op.is_empty() { "EQ" } else { op };
                let want = args.first().map(|a| a.trim()).unwrap_or("M");
                let Some(wn) = self.size_named(want) else {
                    return false;
                };
                let cur = if kind.starts_with("BASE") {
                    self.base_size_index()
                } else {
                    self.size_index()
                };
                cmp(op, cur as f64, wn as f64)
            }
            "RULE" => self.count_items(args, |a| {
                self.rules.get(&upper(a)).copied().unwrap_or(false)
            }),
            "TYPE" => {
                // the type of the source object (equipment: also eqmods)
                let Some(s) = src else { return false };
                self.count_items(args, |a| {
                    if let Some(m) = a.strip_prefix("EQMOD=") {
                        return s.item.is_some_and(|i| {
                            self.items[i].eqmods.iter().any(|e| {
                                e.key.eq_ignore_ascii_case(m) || e.name.eq_ignore_ascii_case(m)
                            })
                        });
                    }
                    match s.item {
                        Some(i) => a.split('.').all(|t| self.item_is_type(i, t)),
                        None => a.split('.').all(|t| s.obj.is_type(t)),
                    }
                })
            }
            "EQUIP" | "EQUIPPRIMARY" | "EQUIPSECONDARY" | "EQUIPBOTH" | "EQUIPTWOWEAPON"
            | "ITEM" => self.count_items(args, |a| {
                let items: Vec<&Object> = if kind == "ITEM" {
                    self.ch
                        .equipment()
                        .iter()
                        .filter_map(|e| {
                            self.ds
                                .get(FileKind::Equipment, e.base.as_deref().unwrap_or(&e.name))
                        })
                        .collect()
                } else {
                    self.items.iter().map(|i| i.obj).collect()
                };
                if let Some(t) = a.strip_prefix("TYPE=").or_else(|| a.strip_prefix("TYPE.")) {
                    items.iter().any(|o| t.split('.').all(|x| o.is_type(x)))
                } else {
                    let pat = lower(a.trim_end_matches('%'));
                    items.iter().any(|o| {
                        if a.ends_with('%') {
                            lower(&o.key).starts_with(&pat)
                        } else {
                            lower(&o.key) == pat || lower(&o.name) == pat
                        }
                    })
                }
            }),
            "DEITY" => {
                let d = self.ch.deity();
                self.count_items(args, |a| match a.to_ascii_uppercase().as_str() {
                    "Y" | "YES" => d.is_some(),
                    "N" | "NO" => d.is_none(),
                    _ => d.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(a)),
                })
            }
            "DOMAIN" => {
                let doms = self.ch.domains();
                self.count_items(args, |a| {
                    a.eq_ignore_ascii_case("ANY") && !doms.is_empty()
                        || doms.iter().any(|d| d.eq_ignore_ascii_case(a))
                })
            }
            "LANG" => {
                let langs = self.ch.languages();
                self.count_items(args, |a| {
                    a.eq_ignore_ascii_case("ANY") || langs.iter().any(|l| l.eq_ignore_ascii_case(a))
                })
            }
            "HD" => {
                let lvl = self.total_level() as f64;
                args.iter().all(|a| {
                    if let Some(v) = a.strip_prefix("MIN=") {
                        lvl >= num(v)
                    } else if let Some(v) = a.strip_prefix("MAX=") {
                        lvl <= num(v)
                    } else {
                        true
                    }
                })
            }
            "GENDER" => {
                let g = self.ch.pcg.value("GENDER").unwrap_or_default();
                args.iter().any(|a| lower(a) == lower(&g))
            }
            "MOVE" => {
                let mv = self.movement();
                self.count_items(args, |a| {
                    let (t, v) = a.split_once('=').unwrap_or((a, "1"));
                    mv.get(&lower(t)).is_some_and(|sp| *sp >= num(v))
                })
            }
            "AGESET" => {
                // at least the named age set
                let cur = self.age_set().map(|(i, _)| i);
                self.count_items(args, |a| match (cur, self.age_set_index(a)) {
                    (Some(c), Some(w)) => c >= w,
                    _ => false,
                })
            }
            "SUBCLASS" => {
                let subs: Vec<String> = self
                    .ch
                    .pcg
                    .lines_tagged("CLASS")
                    .filter_map(|l| l.text("SUBCLASS"))
                    .collect();
                self.count_items(args, |a| subs.iter().any(|s| s.eq_ignore_ascii_case(a)))
            }
            "TEXT" => true,
            "FACT" => {
                // PREFACT:n,KIND,Fact=Value[,Fact=Value]
                let need: usize = args
                    .first()
                    .and_then(|a| a.trim().parse().ok())
                    .unwrap_or(1);
                let kind = args.get(1).map(|k| upper(k.trim())).unwrap_or_default();
                let objs: Vec<&Object> = match kind.as_str() {
                    "ABILITIES" | "ABILITY" => self.ability_srcs().map(|s| s.obj).collect(),
                    "RACE" => self.race().into_iter().collect(),
                    "CLASS" | "CLASSES" => self.classes.iter().map(|(o, _)| *o).collect(),
                    "DEITY" => self
                        .ch
                        .deity()
                        .and_then(|d| self.ds.get(FileKind::Deity, &d))
                        .into_iter()
                        .collect(),
                    _ => Vec::new(),
                };
                let tests: Vec<(String, String)> = args
                    .iter()
                    .skip(2)
                    .filter_map(|a| a.split_once('='))
                    .map(|(f, v)| (lower(f.trim()), lower(v.trim())))
                    .collect();
                let n = objs
                    .iter()
                    .filter(|o| {
                        tests.iter().any(|(f, v)| {
                            o.values("FACT").any(|fv| {
                                fv.split_once('|')
                                    .is_some_and(|(a, b)| lower(a) == *f && lower(b) == *v)
                            })
                        })
                    })
                    .count();
                n >= need
            }
            "DEITYALIGN" => {
                let al = self
                    .ch
                    .deity()
                    .and_then(|d| self.ds.get(FileKind::Deity, &d))
                    .and_then(|d| d.get("ALIGN").map(str::to_string));
                al.is_some_and(|a| args.iter().any(|x| x.trim().eq_ignore_ascii_case(&a)))
            }
            "CHARACTERTYPE" => self.count_items(args, |a| {
                let t = self
                    .ch
                    .pcg
                    .value("CHARACTERTYPE")
                    .unwrap_or_else(|| "PC".into());
                a.eq_ignore_ascii_case(&t)
            }),
            "ARMORTYPE" | "PROFWITHARMOR" | "PROFWITHSHIELD" | "WEAPONPROF" => true,
            // things that cannot be judged here pass
            other => {
                *self
                    .unknown
                    .borrow_mut()
                    .entry(format!("PRE{}", other))
                    .or_default() += 1;
                true
            }
        }
    }

    fn prereqs_pass(&self, toks: &[&Tok], src: &Src<'a>) -> bool {
        toks.iter().filter(|t| Prereq::is_pre_tag(&t.tag)).all(|t| {
            match Prereq::from_token(&t.tag, &t.value) {
                Ok(p) => self.passes(&p, Some(src)),
                Err(_) => true,
            }
        })
    }

    // ---- gathering active objects ----

    fn src(&self, kind: SrcKind, obj: &'a Object) -> Src<'a> {
        Src {
            kind,
            obj,
            tokens: obj.tokens.iter().collect(),
            class: None,
            choices: Vec::new(),
            item: None,
            times: 1.0,
        }
    }

    /// Tokens of a class active at its current level.
    fn class_tokens(&self, o: &'a Object, level: u32) -> Vec<&'a Tok> {
        let mut v: Vec<&Tok> = o.tokens.iter().collect();
        for sl in &o.sublines {
            if sl.tag != "LEVEL" {
                continue;
            }
            let mut parts = sl.value.splitn(3, ':');
            let start: u32 = parts.next().and_then(|x| x.parse().ok()).unwrap_or(0);
            let applies = match (parts.next(), parts.next()) {
                (Some("REPEATLEVEL"), Some(spec)) => {
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
                        .count() as u32
                }
                _ => u32::from(start <= level && start > 0),
            };
            for _ in 0..applies {
                v.extend(sl.tokens.iter());
            }
        }
        v
    }

    fn gather(&mut self) {
        let ds = self.ds;
        let mut srcs: Vec<Src<'a>> = Vec::new();
        for o in ds.objects(FileKind::Stat) {
            srcs.push(self.src(SrcKind::Stat, o));
        }
        for o in ds.objects(FileKind::Save) {
            srcs.push(self.src(SrcKind::Save, o));
        }
        for o in ds.objects(FileKind::Skill) {
            srcs.push(self.src(SrcKind::Skill, o));
        }
        if let Some(a) = self
            .ch
            .alignment()
            .and_then(|a| ds.get(FileKind::Alignment, &a))
        {
            srcs.push(self.src(SrcKind::Alignment, a));
        }
        if let Some(r) = self.race() {
            srcs.push(self.src(SrcKind::Race, r));
        }
        // size: the race's size, overridden by templates gathered last pass
        let size = self.size_key();
        if let Some(o) = ds.get(FileKind::Size, &size) {
            srcs.push(self.src(SrcKind::Size, o));
        }
        let mut templates: Vec<String> = self.ch.templates();
        for (o, level) in self.classes.clone() {
            let mut s = self.src(SrcKind::Class, o);
            s.tokens = self.class_tokens(o, level);
            s.class = Some(o.key.clone());
            srcs.push(s);
        }
        if let Some(d) = self.ch.deity().and_then(|d| ds.get(FileKind::Deity, &d)) {
            srcs.push(self.src(SrcKind::Deity, d));
        }
        for d in self.ch.domains() {
            if let Some(o) = ds.get(FileKind::Domain, &d) {
                srcs.push(self.src(SrcKind::Domain, o));
            }
        }
        for (i, it) in self.items.iter().enumerate() {
            let mut s = self.src(SrcKind::Equipment, it.obj);
            s.item = Some(i);
            srcs.push(s);
            for m in &it.eqmods {
                let mut s = self.src(SrcKind::EquipMod, m);
                s.item = Some(i);
                srcs.push(s);
            }
        }

        // abilities stored in the character
        let mut seen: HashSet<(String, String, Vec<String>)> = HashSet::new();
        let mut pending: Vec<(String, String, Vec<String>, Option<String>)> = Vec::new();
        for a in self.ch.abilities() {
            if a.key.starts_with('*') {
                continue;
            }
            let migrated = self.migrate("ABILITY", &format!("{}|{}", a.category, a.key));
            let key = migrated
                .rsplit_once('|')
                .map(|(_, k)| k.to_string())
                .unwrap_or(migrated);
            pending.push((a.category.clone(), key, a.applied_to.clone(), None));
        }
        for f in self.ch.legacy_feats() {
            pending.push(("FEAT".into(), f, Vec::new(), None));
        }

        // grants from active objects, repeated to closure
        let mut i = 0;
        loop {
            while i < srcs.len() {
                let s = srcs[i].clone();
                i += 1;
                if !self.prereqs_pass_obj(&s) {
                    continue;
                }
                for t in &s.tokens {
                    match t.tag.as_str() {
                        "ABILITY" => {
                            let parts: Vec<&str> = t.value.split('|').collect();
                            if parts.len() < 3 {
                                continue;
                            }
                            let pres: Vec<Prereq> = parts[2..]
                                .iter()
                                .filter(|p| Prereq::is_pre_tag(p.split(':').next().unwrap_or("")))
                                .filter_map(|p| Prereq::parse(p).ok())
                                .collect();
                            if !pres.iter().all(|p| self.passes(p, Some(&s))) {
                                continue;
                            }
                            for k in parts[2..]
                                .iter()
                                .filter(|p| !Prereq::is_pre_tag(p.split(':').next().unwrap_or("")))
                            {
                                let k = if *k == "%LIST" {
                                    match s.choices.first() {
                                        Some(c) => c.as_str(),
                                        None => continue,
                                    }
                                } else {
                                    k
                                };
                                pending.push((
                                    self.pool_category(parts[0]),
                                    k.to_string(),
                                    Vec::new(),
                                    s.class.clone(),
                                ));
                            }
                        }
                        "AUTO" if t.value.starts_with("FEAT|") => {
                            for k in t.value[5..].split('|') {
                                if !Prereq::is_pre_tag(k.split(':').next().unwrap_or("")) {
                                    pending.push((
                                        "FEAT".into(),
                                        k.to_string(),
                                        Vec::new(),
                                        s.class.clone(),
                                    ));
                                }
                            }
                        }
                        "VFEAT" | "FEAT" if s.kind != SrcKind::Ability => {
                            for k in t.value.split('|') {
                                if !Prereq::is_pre_tag(k.split(':').next().unwrap_or("")) {
                                    pending.push((
                                        "FEAT".into(),
                                        k.to_string(),
                                        Vec::new(),
                                        s.class.clone(),
                                    ));
                                }
                            }
                        }
                        "TEMPLATE" if !t.value.starts_with("CHOOSE:") => {
                            for k in t.value.split('|') {
                                if !templates.iter().any(|x| x.eq_ignore_ascii_case(k)) {
                                    templates.push(k.to_string());
                                }
                            }
                        }
                        "CSKILL" => {}
                        _ => {}
                    }
                }
                // templates discovered so far
                for tname in templates.clone() {
                    if srcs.iter().any(|x| {
                        x.kind == SrcKind::Template && x.obj.key.eq_ignore_ascii_case(&tname)
                    }) {
                        continue;
                    }
                    if let Some(o) = ds.get(FileKind::Template, &tname) {
                        srcs.push(self.src(SrcKind::Template, o));
                    }
                }
            }
            if pending.is_empty() {
                break;
            }
            for (cat, key, choices, class) in std::mem::take(&mut pending) {
                let (obj, choices) = match ds.get_in(FileKind::Ability, &cat, &key) {
                    Some(o) => (Some(o), choices),
                    None => {
                        let (k, ch) = split_choice(&key);
                        let mut c = choices;
                        if let Some(ch) = ch {
                            c.push(ch.to_string());
                        }
                        (ds.get_in(FileKind::Ability, &cat, k), c)
                    }
                };
                let Some(obj) = obj.or_else(|| ds.get(FileKind::Ability, &key)) else {
                    continue;
                };
                let id = (lower(&cat), lower(&obj.key), choices.clone());
                if !seen.insert(id) {
                    continue;
                }
                let mut s = self.src(SrcKind::Ability, obj);
                // stacking abilities apply once per time taken
                if obj
                    .get("STACK")
                    .is_some_and(|v| v.eq_ignore_ascii_case("YES"))
                    && choices.len() > 1
                {
                    s.times = choices.len() as f64;
                }
                s.choices = choices.into_iter().filter(|c| !c.is_empty()).collect();
                s.class = class;
                srcs.push(s);
            }
        }

        // class skills
        let mut cs = HashSet::new();
        for s in &srcs {
            for t in &s.tokens {
                if t.tag != "CSKILL" {
                    continue;
                }
                for item in t.value.split('|') {
                    if let Some(ty) = item
                        .strip_prefix("TYPE=")
                        .or_else(|| item.strip_prefix("TYPE."))
                    {
                        for sk in ds.objects(FileKind::Skill) {
                            if sk.is_type(ty) {
                                cs.insert(lower(&sk.key));
                            }
                        }
                    } else if item == "%LIST" {
                        for c in &s.choices {
                            cs.insert(lower(c));
                        }
                    } else if item.eq_ignore_ascii_case("ALL") {
                        for sk in ds.objects(FileKind::Skill) {
                            cs.insert(lower(&sk.key));
                        }
                    } else {
                        cs.insert(lower(item));
                    }
                }
            }
        }
        self.class_skills = cs;
        self.sources = srcs;
    }

    /// Object-level prerequisites (PRExxx tokens directly on the object).
    fn prereqs_pass_obj(&self, s: &Src<'a>) -> bool {
        matches!(s.kind, SrcKind::Stat | SrcKind::Save | SrcKind::Class)
            || self.prereqs_pass(&s.obj.tokens.iter().collect::<Vec<_>>(), s)
    }

    // ---- evaluation ----

    fn pass(&mut self) {
        self.unknown.borrow_mut().clear();
        self.gather();
        let mut table = BonusTable::new(
            &self
                .gm
                .map(|g| g.stacking_bonus_types())
                .unwrap_or_default(),
        );
        let mut vars: HashMap<String, f64> = HashMap::new();
        let mut defined = HashSet::new();
        for (si, s) in self.sources.iter().enumerate() {
            if !self.prereqs_pass_obj(s) {
                continue;
            }
            let env = Ctx {
                calc: self,
                src: Some(s),
            };
            for t in &s.tokens {
                match t.tag.as_str() {
                    "DEFINE" => {
                        let (name, f) = t.value.split_once('|').unwrap_or((&t.value, "0"));
                        // DEFINE:LOCK. and similar forms are not variables
                        if name.contains('.') && name.starts_with("LOCK") {
                            continue;
                        }
                        let u = upper(name);
                        let v = eval_str(f, &env);
                        defined.insert(u.clone());
                        let e = vars.entry(u).or_insert(f64::NEG_INFINITY);
                        *e = e.max(v);
                    }
                    "BONUS" => {
                        let Ok(b) = Bonus::parse(&t.value) else {
                            continue;
                        };
                        if !b.prereqs().all(|p| self.passes(p, Some(s))) {
                            continue;
                        }
                        let v = match b.expr() {
                            Ok(e) => eval(&e, &env),
                            Err(_) => continue,
                        };
                        let btype = b.opts.iter().find_map(|o| match o {
                            BonusOpt::Type(t) => Some(t.as_str()),
                            _ => None,
                        });
                        let cat = if b.category.eq_ignore_ascii_case("CHECKS") {
                            "SAVE".to_string()
                        } else {
                            b.category.clone()
                        };
                        // item-local bonus categories apply to the item
                        if cat.starts_with("EQM") && s.item.is_some() {
                            let origin = format!("{:?} {}", s.kind, s.obj.key);
                            for tg in &b.targets {
                                table.add_from(
                                    &format!("ITEM{}.{}", si_item(s), cat),
                                    tg,
                                    btype,
                                    v,
                                    &origin,
                                );
                            }
                            continue;
                        }
                        for tg in &b.targets {
                            // a stacking ability taken several times applies
                            // its plain bonuses once per time taken
                            let per_choice =
                                tg.contains("%LIST") || tg.contains("%CHOICE") || tg == "LIST";
                            let v = if per_choice { v } else { v * s.times };
                            let tgs: Vec<String> = if tg == "LIST" {
                                if s.choices.is_empty() {
                                    vec![tg.clone()]
                                } else {
                                    s.choices.clone()
                                }
                            } else if per_choice {
                                s.choices
                                    .iter()
                                    .map(|c| tg.replace("%LIST", c).replace("%CHOICE", c))
                                    .collect()
                            } else {
                                vec![tg.clone()]
                            };
                            let origin = format!("{:?} {}", s.kind, s.obj.key);
                            for tg in tgs {
                                table.add_from(&cat, &tg, btype, v, &origin);
                            }
                        }
                    }
                    _ => {}
                }
            }
            let _ = si;
        }
        self.bonus = table;
        self.vars = vars;
        self.defined = defined;
    }

    /// Armor check penalty of equipped armor and shields (zero or less).
    pub fn armor_check(&self) -> i64 {
        let mut total = 0.0;
        for (i, it) in self.items.iter().enumerate() {
            if !(it.obj.is_type("Armor") || it.obj.is_type("Shield")) {
                continue;
            }
            let base: f64 = it
                .obj
                .get("ACCHECK")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0);
            let adj = self.bonus.total(&format!("ITEM{}.EQMARMOR", i), "ACCHECK");
            total += (base + adj).min(0.0);
        }
        total as i64
    }

    pub fn skill_total(&self, skill: &str) -> i64 {
        let Some(sk) = self.ds.get(FileKind::Skill, skill) else {
            return self.skill_ranks(skill) as i64;
        };
        let mut b = 0.0;
        if let Some(stat) = sk.get("KEYSTAT") {
            b += self.stat_mod(stat);
            b += self.bonus.total("SKILL", &format!("STAT.{}", stat));
        }
        b += self.bonus.total("SKILL", &sk.key);
        for t in sk.types() {
            b += self.bonus.total("SKILL", &format!("TYPE.{}", t));
        }
        b += self.bonus.total("SKILL", "LIST");
        b += self.bonus.total("SKILL", "ALL");
        let cs = self.is_class_skill(&sk.key);
        if cs {
            b += self.bonus.total("CSKILL", &sk.key);
        } else if sk.get("EXCLUSIVE") != Some("YES") {
            b += self.bonus.total("CCSKILL", &sk.key);
        }
        let ac = self.armor_check() as f64;
        b += match sk.get("ACHECK").map(upper).as_deref() {
            Some("YES") => ac,
            Some("DOUBLE") => 2.0 * ac,
            _ => 0.0,
        };
        (self.skill_ranks(&sk.key) + b).floor() as i64
    }

    fn ac_of_type(&self, name: &str) -> i64 {
        let Some(gm) = self.gm else {
            return 10 + self.bonus.total("COMBAT", "AC") as i64;
        };
        let Some(line) = gm
            .lines("miscinfo.lst", "ACTYPE")
            .find(|l| l.value.eq_ignore_ascii_case(name))
        else {
            return 0;
        };
        let mut total = 0.0;
        for (tag, val) in &line.tokens {
            let sign = match tag.as_str() {
                "ADD" => 1.0,
                "REMOVE" => -1.0,
                _ => continue,
            };
            let parts: Vec<&str> = val.split('|').collect();
            let pres: Vec<Prereq> = parts
                .iter()
                .filter(|p| Prereq::is_pre_tag(p.split(':').next().unwrap_or("")))
                .filter_map(|p| Prereq::parse(p).ok())
                .collect();
            if !pres.iter().all(|p| self.passes(p, None)) {
                continue;
            }
            for t in parts
                .iter()
                .filter(|p| !Prereq::is_pre_tag(p.split(':').next().unwrap_or("")))
            {
                let v = if t.eq_ignore_ascii_case("TOTAL") {
                    self.bonus.total("COMBAT", "AC")
                } else {
                    self.bonus.by_type("COMBAT", "AC", t)
                };
                total += sign * v;
            }
        }
        total as i64
    }

    fn hit_points(&self) -> i64 {
        // CON-style bonus: BONUS:HP|BONUS from stats only
        let mut con = 0.0;
        for s in self.sources.iter().filter(|s| s.kind == SrcKind::Stat) {
            let env = Ctx {
                calc: self,
                src: Some(s),
            };
            for t in &s.tokens {
                if t.tag != "BONUS" {
                    continue;
                }
                if let Ok(b) = Bonus::parse(&t.value) {
                    if b.category.eq_ignore_ascii_case("HP")
                        && b.targets.iter().any(|x| x.eq_ignore_ascii_case("BONUS"))
                        && b.prereqs().all(|p| self.passes(p, Some(s)))
                    {
                        if let Ok(e) = b.expr() {
                            con += eval(&e, &env).trunc();
                        }
                    }
                }
            }
        }
        let mut total = 0i64;
        for l in self.ch.levels() {
            if let Some(hp) = l.hp {
                if hp > 0 {
                    total += (hp + con as i64).max(1);
                }
            }
        }
        total + self.bonus.total("HP", "CURRENTMAX") as i64
    }

    /// Contributions to a bonus key, for explaining a value.
    pub fn explain(&self, category: &str, target: &str) -> Vec<String> {
        self.bonus
            .explain(category, target)
            .into_iter()
            .map(|(t, v, o)| {
                if t.is_empty() {
                    format!("{:+} from {}", v, o)
                } else {
                    format!("{:+} [{}] from {}", v, t, o)
                }
            })
            .collect()
    }

    /// Run the calculation.
    pub fn run(&mut self) -> Sheet {
        let mut passes = 0;
        let mut last: Option<Snapshot> = None;
        while passes < MAX_PASSES {
            passes += 1;
            self.pass();
            let mut vs: Vec<(String, f64)> =
                self.vars.iter().map(|(k, v)| (k.clone(), *v)).collect();
            vs.sort_by(|a, b| a.0.cmp(&b.0));
            let snap = (self.bonus.snapshot(), vs);
            if last.as_ref() == Some(&snap) {
                break;
            }
            last = Some(snap);
        }

        let mut sheet = Sheet {
            level: self.total_level(),
            passes,
            ..Default::default()
        };
        for abb in self.stat_base.keys() {
            sheet.stats.insert(
                abb.clone(),
                (self.stat_score(abb) as i64, self.stat_mod(abb) as i64),
            );
        }
        sheet.bab = self.bonus.total("COMBAT", "BASEAB") as i64;
        for s in self.ds.objects(FileKind::Save) {
            let v = self.bonus.total("SAVE", &format!("BASE.{}", s.key))
                + self.bonus.total("SAVE", &s.key)
                + self.bonus.total("SAVE", "ALL");
            sheet.saves.insert(s.key.clone(), v as i64);
        }
        sheet.hp = self.hit_points();
        for t in ["Total", "Flatfooted", "Touch"] {
            sheet.ac.insert(t.to_string(), self.ac_of_type(t));
        }
        for sk in self.ds.objects(FileKind::Skill) {
            sheet
                .skills
                .insert(sk.key.clone(), self.skill_total(&sk.key));
        }
        sheet.armor_check = self.armor_check();
        sheet.abilities = self
            .ability_srcs()
            .map(|s| {
                format!(
                    "{}|{}",
                    s.obj.category.clone().unwrap_or_default(),
                    s.obj.key
                )
            })
            .collect();
        sheet.unknown = self.unknown.borrow().clone();
        sheet
    }
}

fn si_item(s: &Src<'_>) -> usize {
    s.item.unwrap_or(usize::MAX)
}

/// Evaluation context: the calculator plus the source being evaluated.
struct Ctx<'c, 'a> {
    calc: &'c Calc<'a>,
    src: Option<&'c Src<'a>>,
}

impl Env for Ctx<'_, '_> {
    fn var(&self, name: &str) -> Option<f64> {
        if name == "%LIST" || name == "%CHOICE" {
            return self
                .src
                .and_then(|s| s.choices.first())
                .and_then(|c| c.parse().ok());
        }
        self.calc.var_value(name, self.src)
    }

    fn call(&self, name: &str, args: &[Expr]) -> Option<f64> {
        self.calc.call(name, args, self.src)
    }

    fn unknown(&self, name: &str) {
        *self
            .calc
            .unknown
            .borrow_mut()
            .entry(name.to_string())
            .or_default() += 1;
    }
}

/// Context for a stat's STATMOD formula, where SCORE is that stat.
struct ScoreEnv<'c, 'a> {
    calc: &'c Calc<'a>,
    score: f64,
}

impl Env for ScoreEnv<'_, '_> {
    fn var(&self, name: &str) -> Option<f64> {
        if name.eq_ignore_ascii_case("SCORE") {
            Some(self.score)
        } else {
            self.calc.var_value(name, None)
        }
    }
}
