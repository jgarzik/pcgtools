//
// rules/calc.rs -- derived character statistics
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
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

use super::bonus::{BonusTable, Source};
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
    /// a companion modifier granting bonuses to the master
    MasterBonus,
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
    /// for abilities: how the character has it (NORMAL, AUTOMATIC or
    /// VIRTUAL), the category it was taken in, and the choices as
    /// recorded (empty entries included)
    pub nature: Option<String>,
    pub category: Option<String>,
    pub applied: Vec<String>,
}

impl Src<'_> {
    /// Description of this source for bonus bookkeeping.
    pub fn source(&self) -> Source {
        Source {
            origin: format!("{:?} {}", self.kind, self.obj.key),
            kind: format!("{:?}", self.kind),
            key: self.obj.key.clone(),
            category: self.obj.category.clone(),
            temp: false,
        }
    }
}

/// An equipped item.
#[derive(Debug, Clone)]
pub struct Item<'a> {
    pub name: String,
    pub obj: &'a Object,
    pub eqmods: Vec<&'a Object>,
    /// choices made for each modifier (parallel to `eqmods`)
    pub eqmod_choices: Vec<Vec<String>>,
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
    /// equipped items: `location: item [eqmods]`
    pub equipped: Vec<String>,
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
    /// set while proficiencies are worked out, so prerequisites on
    /// proficiency grants do not ask for proficiencies again
    in_profs: std::cell::Cell<bool>,
    class_skills: HashSet<String>,
    cross_class_skills: HashSet<String>,
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
    if !s.ends_with(')') {
        return (s, None);
    }
    // the parenthesis matching the final one, so choices may nest:
    // `Giving(Feat(Ride))` is Giving with choice `Feat(Ride)`
    let mut depth = 0;
    for (i, c) in s.char_indices().rev() {
        match c {
            ')' => depth += 1,
            '(' => {
                depth -= 1;
                if depth == 0 {
                    if i == 0 {
                        return (s, None);
                    }
                    return (s[..i].trim_end(), Some(&s[i + 1..s.len() - 1]));
                }
            }
            _ => {}
        }
    }
    (s, None)
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
            in_profs: std::cell::Cell::new(false),
            class_skills: HashSet::new(),
            cross_class_skills: HashSet::new(),
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
                if let Some(v) = l.get("VAR").or_else(|| l.get("PARM")) {
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
        let mut out = Vec::new();
        for e in self.ch.equipped() {
            let loc = lower(&e.location);
            // items directly in the set are equipped unless merely
            // carried; items inside containers are deeper in the tree
            let depth = e.id.matches('.').count();
            let worn = depth == 2 && loc != "carried" && loc != "not carried";
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
            let mut eqmod_choices: Vec<Vec<String>> = Vec::new();
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
                                    // KEY|choice|choice...
                                    let mut parts = k.split('|');
                                    let key = parts.next().unwrap_or(k);
                                    if let Some(m) = self.find_eqmod(key) {
                                        eqmods.push(m);
                                        eqmod_choices.push(parts.map(str::to_string).collect());
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
                            eqmod_choices.push(Vec::new());
                        }
                    }
                }
            }
            out.push(Item {
                name,
                obj,
                eqmods,
                eqmod_choices,
                location: e.location.clone(),
            });
        }
        out
    }

    /// Map an old key through the game mode's migration rules.
    pub fn migrate(&self, kind: &str, key: &str) -> String {
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
    pub fn max_load(&self) -> f64 {
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
        // quadrupeds and the like carry more for their size
        let size_mult = size_mult + self.bonus.total("LOADMULT", "TYPE=SIZE");
        lookup(s) * factor * size_mult
    }

    /// The game mode's multiplier of the maximum load for a load
    /// category (Light, Medium, Heavy, OverHead, OffGround, PushDrag).
    pub fn load_fraction(&self, name: &str) -> Option<f64> {
        let g = self.gm?;
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
    }

    /// Load category: 0 light, 1 medium, 2 heavy, 3 overloaded.
    pub fn encumbrance(&self) -> u32 {
        if !self.rules.get("SYS_LDPACSK").copied().unwrap_or(true) {
            return 0;
        }
        if self.gm.is_none() {
            return 0;
        }
        let max = self.max_load();
        if max <= 0.0 {
            return 0;
        }
        let ratio = self.carried_weight() / max;
        for (i, n) in ["Light", "Medium", "Heavy"].iter().enumerate() {
            if self.load_fraction(n).is_some_and(|f| ratio <= f) {
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

    /// Speed for a movement type, with MOVEADD bonuses.
    /// Final speed for a movement type (0 if the character has none).
    pub fn move_speed(&self, mtype: &str) -> f64 {
        self.movement_named()
            .into_iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(mtype))
            .map(|(_, v)| v)
            .unwrap_or(0.0)
    }

    /// Equipped items.
    pub fn items(&self) -> &[Item<'a>] {
        &self.items
    }

    /// Number of legs or hands, from the race (default `dflt`).
    fn limbs(&self, tag: &str, dflt: f64) -> f64 {
        let mut v = self
            .race()
            .and_then(|r| r.get(tag))
            .and_then(|x| x.parse().ok())
            .unwrap_or(dflt);
        for s in self.sources.iter().filter(|s| s.kind == SrcKind::Template) {
            if let Some(x) = s.obj.get(tag).and_then(|x| x.parse().ok()) {
                v = x;
            }
        }
        v
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

    /// Movement types with their final speeds, named as first written:
    /// the best MOVE per type (none at all unless the race has MOVE),
    /// MOVECLONE copies, types created by MOVEADD bonuses, then
    /// MOVEADD, MOVEMULT and POSTMOVEADD bonuses and the slowing of
    /// medium or heavy armor and load.
    pub fn movement_named(&self) -> Vec<(String, f64)> {
        let Some(race) = self.race() else {
            return Vec::new();
        };
        if !race.has("MOVE") {
            return Vec::new();
        }
        let mut table: Vec<(String, f64)> = Vec::new();
        let find =
            |t: &Vec<(String, f64)>, n: &str| t.iter().position(|(k, _)| k.eq_ignore_ascii_case(n));
        for s in &self.sources {
            for t in s.tokens.iter().filter(|t| t.tag == "MOVE") {
                let parts: Vec<&str> = t.value.split(',').collect();
                for pair in parts.chunks(2) {
                    if let [ty, sp] = pair {
                        let ty = ty.trim();
                        let sp: f64 = sp.trim().parse().unwrap_or(0.0);
                        match find(&table, ty) {
                            Some(i) => table[i].1 = table[i].1.max(sp),
                            None => table.push((ty.to_string(), sp)),
                        }
                    }
                }
            }
        }
        let raw = table.clone();
        for s in &self.sources {
            for t in s.tokens.iter().filter(|t| t.tag == "MOVECLONE") {
                let parts: Vec<&str> = t.value.split(',').map(str::trim).collect();
                let [src, dst, f] = parts[..] else { continue };
                let Some(base) = find(&raw, src).map(|i| raw[i].1) else {
                    continue;
                };
                let num = |x: &str| -> f64 {
                    match x.split_once('/') {
                        Some((a, b)) => {
                            a.parse::<f64>().unwrap_or(0.0) / b.parse::<f64>().unwrap_or(1.0)
                        }
                        None => x.parse().unwrap_or(0.0),
                    }
                };
                let v = if let Some(n) = f.strip_prefix('/') {
                    base / num(n)
                } else if let Some(n) = f.strip_prefix('*') {
                    base * num(n)
                } else {
                    base + num(f.trim_start_matches('+'))
                };
                match find(&table, dst) {
                    Some(i) => table[i].1 = v,
                    None => table.push((dst.to_string(), v)),
                }
            }
        }
        // types named only by a MOVEADD bonus start at 0
        for c in &self.bonus.log {
            let Some(t) = c
                .key
                .strip_prefix("MOVEADD.TYPE.")
                .or_else(|| c.key.strip_prefix("MOVEADD.TYPE="))
            else {
                continue;
            };
            if t == "ALL" || find(&table, t).is_some() {
                continue;
            }
            let mut name: String = t.to_lowercase();
            if let Some(f) = name.get(0..1) {
                name = f.to_uppercase() + &name[1..];
            }
            table.push((name, 0.0));
        }
        let bonus = |cat: &str, t: &str| {
            self.bonus.total(cat, &format!("TYPE.{}", t))
                + self.bonus.total(cat, &format!("TYPE={}", t))
                + self.bonus.total(cat, "TYPE.ALL")
                + self.bonus.total(cat, "TYPE=ALL")
                + self.bonus.total(cat, "ALL")
        };
        // slowest of armor worn and load carried, unless ignored
        let rank = |v: &str| match v.to_ascii_uppercase().as_str() {
            "LIGHTLOAD" => 0,
            "MEDIUMLOAD" | "MEDIUMARMOR" => 1,
            "HEAVYLOAD" => 2,
            "OVERLOAD" | "HEAVYARMOR" => 3,
            _ => -1,
        };
        let mut unencumbered = -1;
        for s in &self.sources {
            for t in s.tokens.iter().filter(|t| t.tag == "UNENCUMBEREDMOVE") {
                for v in t.value.split('|') {
                    unencumbered = unencumbered.max(rank(v));
                }
            }
        }
        let checks = self.rule("SYS_LDPACSK") || !self.rules.contains_key("SYS_LDPACSK");
        let mut armor = 0;
        if checks {
            for it in &self.items {
                if !it.obj.is_type("Armor") || it.obj.is_type("Shield") {
                    continue;
                }
                if it.obj.is_type("Heavy") && unencumbered < 3 {
                    armor = armor.max(2);
                } else if it.obj.is_type("Medium") && unencumbered < 1 {
                    armor = armor.max(1);
                }
            }
        }
        let load = self.encumbrance() as i32;
        let load = if load as i64 <= unencumbered as i64 {
            0
        } else {
            load
        };
        let slow = |load: i32, m: f64| -> f64 {
            match load {
                0 => m,
                1 | 2 => {
                    if m == 5.0 || m == 10.0 {
                        5.0
                    } else {
                        (m / 15.0).floor() * 10.0 + (m as i64 % 15) as f64
                    }
                }
                _ => 0.0,
            }
        };
        table
            .into_iter()
            .map(|(n, base)| {
                let m = base + bonus("MOVEADD", &n);
                let mult = bonus("MOVEMULT", &n);
                let calc = if mult > 0.0 { (m * mult).trunc() } else { m };
                let m = calc.max(calc + bonus("POSTMOVEADD", &n));
                let v = slow(armor, m).min(slow(load, m));
                (n, v)
            })
            .collect()
    }

    /// Experience needed for a level, from the level table.
    pub fn min_xp(&self, level: u32) -> Option<f64> {
        self.level_value(level, |r| r.min_xp.clone())
    }

    /// A level-table value for a level: the level's own row, else the
    /// `LEVEL:LEVEL` formula row.
    pub fn level_value(
        &self,
        level: u32,
        field: impl Fn(&crate::gamemode::LevelRow) -> String,
    ) -> Option<f64> {
        let gm = self.gm?;
        let table = self.ch.pcg.value("EXPERIENCETABLE");
        let mut rows = gm.level_table(table.as_deref());
        if rows.is_empty() {
            rows = gm.level_table(None);
        }
        let f = rows
            .iter()
            .find(|r| r.level.trim().parse::<u32>().ok() == Some(level))
            .map(&field)
            .filter(|f| !f.is_empty())
            .or_else(|| {
                rows.iter()
                    .find(|r| r.level.trim().eq_ignore_ascii_case("LEVEL"))
                    .map(&field)
                    .filter(|f| !f.is_empty())
            })?;
        let env = LevelEnv {
            calc: self,
            level: level as f64,
        };
        Some(eval_str(&f, &env))
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

    pub fn find_eqmod(&self, key: &str) -> Option<&'a Object> {
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

    /// Classes taken, with levels, in the order first taken.
    pub fn classes(&self) -> &[(&'a Object, u32)] {
        &self.classes
    }

    /// The game-mode class type row of a class (FACT:ClassType, else
    /// its TYPE values matched against CLASSTYPE rows).
    fn class_type(&self, cls: &Object) -> Option<&'a crate::gamemode::GmLine> {
        let g = self.gm?;
        let rows = || g.lines("miscinfo.lst", "CLASSTYPE");
        if let Some(t) = cls.values("FACT").find_map(|v| {
            let (k, val) = v.split_once('|')?;
            k.eq_ignore_ascii_case("ClassType").then_some(val)
        }) {
            return rows().find(|l| l.value.eq_ignore_ascii_case(t));
        }
        cls.types()
            .iter()
            .find_map(|t| rows().find(|l| l.value.eq_ignore_ascii_case(t)))
    }

    /// Is the class a monster (racial hit dice) class?
    pub fn is_monster_class(&self, cls: &Object) -> bool {
        cls.get("ISMONSTER")
            .map(|v| v.eq_ignore_ascii_case("YES"))
            .unwrap_or_else(|| {
                self.class_type(cls)
                    .and_then(|l| l.get("ISMONSTER"))
                    .is_some_and(|v| v.eq_ignore_ascii_case("YES"))
            })
    }

    /// Challenge rating, or None when it is undefined (no levels).
    /// Without racial hit dice: the classes' CR formulas plus the
    /// modifier of the highest-priority class type.  With them: the
    /// race's CR plus key-role classes and halved other levels.  Then
    /// templates (including level- and hit-dice-based parts) and
    /// MISC|CR bonuses.
    pub fn challenge_rating(&self) -> Option<f64> {
        let steps: Vec<String> = self
            .gm
            .and_then(|g| g.misc("CRSTEPS"))
            .unwrap_or("")
            .split('|')
            .map(|x| x.trim().to_string())
            .collect();
        let monster_levels: u32 = self
            .classes
            .iter()
            .filter(|(c, _)| self.is_monster_class(c))
            .map(|(_, l)| l)
            .sum();
        let other_levels = self.total_level() - monster_levels;
        let class_cr = |c: &Object, level: u32| -> Option<f64> {
            let line = self.class_type(c);
            let formula = c
                .get("CRFORMULA")
                .or_else(|| line.and_then(|l| l.get("CRFORMULA")));
            let Some(formula) = formula else {
                return Some(0.0);
            };
            if formula.eq_ignore_ascii_case("NONE") {
                return None;
            }
            if formula == "0" {
                return Some(0.0);
            }
            if formula.eq_ignore_ascii_case("CL") {
                return Some(level as f64);
            }
            let env = LevelEnv {
                calc: self,
                level: level as f64,
            };
            Some(eval_str(formula, &env))
        };
        let race = self.race();
        let mut cr;
        if monster_levels == 0 {
            if other_levels == 0 {
                return None;
            }
            cr = 0.0;
            let mut best: Option<(i64, f64)> = None;
            for (c, l) in &self.classes {
                cr += class_cr(c, *l).unwrap_or(0.0);
                if let Some(line) = self.class_type(c) {
                    let prio: i64 = line
                        .get("CRMODPRIORITY")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    if prio != 0 && best.is_none_or(|(p, _)| prio < p) {
                        let m: f64 = line
                            .get("CRMOD")
                            .map(|f| {
                                eval_str(
                                    f,
                                    &Ctx {
                                        calc: self,
                                        src: None,
                                    },
                                )
                            })
                            .unwrap_or(0.0);
                        best = Some((prio, m.min(0.0)));
                    }
                }
            }
            cr += best.map(|(_, m)| m).unwrap_or(0.0);
        } else {
            let race_roles: Vec<String> = race
                .and_then(|r| r.get("ROLE"))
                .or_else(|| self.gm.and_then(|g| g.misc("MONSTERROLEDEFAULT")))
                .unwrap_or("")
                .split('.')
                .map(|x| x.to_lowercase())
                .collect();
            let (mut key, mut non_key) = (0.0, 0.0);
            for (c, l) in &self.classes {
                let v = class_cr(c, *l)?;
                let shares = c
                    .get("ROLE")
                    .is_some_and(|r| r.split('.').any(|x| race_roles.contains(&x.to_lowercase())));
                if shares {
                    key += v;
                } else {
                    non_key += v;
                }
            }
            let threshold = self
                .gm
                .and_then(|g| g.misc("CRTHRESHOLD"))
                .map(|f| {
                    eval_str(
                        f,
                        &Ctx {
                            calc: self,
                            src: None,
                        },
                    )
                })
                .unwrap_or(0.0);
            let mut converted = 0.0;
            let mut c = 0.0;
            while non_key > 1.0 {
                c += 1.0;
                non_key -= 2.0;
                converted += 2.0;
                if converted >= threshold {
                    break;
                }
            }
            if converted > 0.0 {
                c += non_key;
            }
            let race_cr = race
                .and_then(|r| r.get("CR"))
                .map(|v| match v.split_once('/') {
                    Some(_) => steps
                        .iter()
                        .position(|x| x == v.trim())
                        .map(|i| -(i as f64))
                        .unwrap_or(0.0),
                    None => v.trim().parse().unwrap_or(0.0),
                })
                .or_else(|| {
                    race.and_then(|r| r.get("LEVELADJUSTMENT"))
                        .and_then(|v| v.trim().parse::<f64>().ok())
                        .filter(|v| *v != 0.0)
                })
                .unwrap_or(0.0);
            cr = race_cr + c + key;
        }
        let total = self.total_level();
        for s in self.sources.iter().filter(|s| s.kind == SrcKind::Template) {
            for t in &s.tokens {
                let v = match t.tag.as_str() {
                    "CR" => t.value.trim().parse::<f64>().ok(),
                    "LEVEL" | "REPEATLEVEL" => {
                        let mut p = t.value.splitn(3, ':');
                        let n: u32 = p.next().and_then(|x| x.parse().ok()).unwrap_or(u32::MAX);
                        match (p.next(), p.next()) {
                            (Some("CR"), Some(x)) if n <= total => x.trim().parse().ok(),
                            _ => None,
                        }
                    }
                    "HD" => {
                        let mut p = t.value.splitn(3, ':');
                        let range = p.next().unwrap_or("");
                        let (lo, hi) = range.split_once('-').unwrap_or((range, range));
                        let lo: u32 = lo.parse().unwrap_or(0);
                        let hi: u32 = if hi.ends_with('+') {
                            u32::MAX
                        } else {
                            hi.parse().unwrap_or(u32::MAX)
                        };
                        match (p.next(), p.next()) {
                            (Some("CR"), Some(x))
                                if lo <= monster_levels && monster_levels <= hi =>
                            {
                                x.trim().parse().ok()
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                };
                if let Some(v) = v {
                    cr += (v as i64) as f64;
                }
            }
        }
        Some(cr + self.bonus.total("MISC", "CR").trunc())
    }

    /// Damage reduction, e.g. `10/magic; 5/-`: the best value per
    /// bypass type from DR tokens, plus BONUS:DR|type bonuses.
    pub fn damage_reduction(&self) -> String {
        let mut best: BTreeMap<String, (String, f64)> = BTreeMap::new();
        for s in &self.sources {
            for t in s.tokens.iter().filter(|t| t.tag == "DR") {
                let mut parts = t.value.split('|');
                let Some((v, kind)) = parts.next().and_then(|d| d.split_once('/')) else {
                    continue;
                };
                let pres: Vec<Prereq> = parts.filter_map(|p| Prereq::parse(p).ok()).collect();
                if !pres.iter().all(|p| self.passes(p, Some(s))) {
                    continue;
                }
                let env = Ctx {
                    calc: self,
                    src: Some(s),
                };
                let v = eval_str(v, &env);
                let e = best
                    .entry(lower(kind))
                    .or_insert_with(|| (kind.to_string(), 0.0));
                e.1 = e.1.max(v);
            }
        }
        let mut out = Vec::new();
        for (k, (kind, v)) in &best {
            let v = v + self.bonus.total("DR", k);
            if v > 0.0 {
                out.push(format!("{}/{}", v as i64, kind));
            }
        }
        out.join("; ")
    }

    /// Hit die size of a class, after HITDIE adjustments from the
    /// race and templates.
    pub fn hit_die(&self, cls: &Object) -> i64 {
        let mut die: i64 = cls.get("HD").and_then(|v| v.parse().ok()).unwrap_or(0);
        let sizes: Vec<(i64, Option<&str>)> = self
            .gm
            .and_then(|g| g.misc("DIESIZES"))
            .unwrap_or("1,2,3,MIN=4,6,8,10,MAX=12,20,100,1000")
            .split(',')
            .filter_map(|p| match p.split_once('=') {
                Some((m, v)) => Some((v.trim().parse().ok()?, Some(m))),
                None => Some((p.trim().parse().ok()?, None)),
            })
            .collect();
        let lo = sizes
            .iter()
            .position(|(_, m)| *m == Some("MIN"))
            .unwrap_or(0);
        let hi = sizes
            .iter()
            .position(|(_, m)| *m == Some("MAX"))
            .unwrap_or(sizes.len().saturating_sub(1));
        for s in self
            .sources
            .iter()
            .filter(|s| matches!(s.kind, SrcKind::Race | SrcKind::Template))
        {
            for t in s.tokens.iter().filter(|t| t.tag == "HITDIE") {
                let mut parts = t.value.split('|');
                let spec = parts.next().unwrap_or("");
                let applies = parts.all(|f| {
                    if let Some(ty) = f.strip_prefix("CLASS.TYPE=") {
                        cls.is_type(ty)
                    } else if let Some(c) = f.strip_prefix("CLASS=") {
                        c.eq_ignore_ascii_case(&cls.key) || c.eq_ignore_ascii_case(&cls.name)
                    } else {
                        true
                    }
                });
                if applies {
                    die = adjust_die(die, spec, &sizes, lo, hi);
                }
            }
        }
        die
    }

    /// Effective character level: levels plus level adjustments.
    pub fn ecl(&self) -> u32 {
        let mut la = 0.0;
        for s in self
            .sources
            .iter()
            .filter(|s| matches!(s.kind, SrcKind::Race | SrcKind::Template))
        {
            for t in s.tokens.iter().filter(|t| t.tag == "LEVELADJUSTMENT") {
                let env = Ctx {
                    calc: self,
                    src: Some(s),
                };
                la += eval_str(&t.value, &env);
            }
        }
        (self.total_level() as f64 + la).max(0.0) as u32
    }

    /// Is the stat a non-ability for this character (DEFINESTAT:NONSTAT
    /// from its race or a template)?
    pub fn non_stat(&self, abb: &str) -> bool {
        let mut non = false;
        for s in self
            .sources
            .iter()
            .filter(|s| matches!(s.kind, SrcKind::Race | SrcKind::Template))
        {
            for t in s.tokens.iter().filter(|t| t.tag == "DEFINESTAT") {
                match t.value.split_once('|') {
                    Some((k, a)) if a.eq_ignore_ascii_case(abb) => {
                        if k.eq_ignore_ascii_case("NONSTAT") {
                            non = true;
                        } else if k.eq_ignore_ascii_case("STAT") {
                            non = false;
                        }
                    }
                    _ => {}
                }
            }
        }
        non
    }

    /// The stat's score before any bonuses.
    pub fn stat_base(&self, abb: &str) -> f64 {
        self.stat_base.get(&upper(abb)).copied().unwrap_or(0.0)
    }

    /// The stat's modifier for a given score.
    pub fn stat_mod_for(&self, abb: &str, score: f64) -> f64 {
        let formula = self
            .ds
            .get(FileKind::Stat, &upper(abb))
            .and_then(|s| s.get("STATMOD"))
            .unwrap_or("floor(SCORE/2)-5");
        let env = ScoreEnv { calc: self, score };
        eval_str(formula, &env).floor()
    }

    /// A variable's value (after [`Calc::run`]).
    pub fn var(&self, name: &str) -> Option<f64> {
        self.var_value(name, None)
    }

    /// Evaluate a formula for the character.
    pub fn eval_formula(&self, formula: &str) -> f64 {
        let env = Ctx {
            calc: self,
            src: None,
        };
        eval_str(formula, &env)
    }

    /// Evaluate a formula with extra terms looked up first (for
    /// equipment formulas: BASECOST, PLUSTOTAL, ...).
    pub fn eval_with(&self, formula: &str, extra: &dyn Fn(&str) -> Option<f64>) -> f64 {
        let env = ExtraEnv { calc: self, extra };
        eval_str(formula, &env)
    }

    /// Hit points gained per level from stats (BONUS:HP|BONUS on stats).
    pub fn hp_stat_bonus(&self) -> i64 {
        self.bonus.total_where("HP", "BONUS", |s| s.kind == "Stat") as i64
    }

    /// Armor check penalty as shown on the sheet: the equipped
    /// armor's penalties, or the load penalty if worse, plus
    /// MISC|ACCHECK bonuses.
    pub fn check_penalty(&self) -> i64 {
        let mut acp = 0.0;
        for (i, it) in self.items.iter().enumerate() {
            let base: f64 = it
                .obj
                .get("ACCHECK")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0);
            let adj = self.bonus.total(&format!("ITEM{}.EQMARMOR", i), "ACCHECK");
            if base != 0.0 || adj != 0.0 {
                acp += (base + adj).min(0.0);
            }
        }
        let load = match self.encumbrance() {
            1 => -3.0,
            2 | 3 => -6.0,
            _ => 0.0,
        };
        (acp.min(load) + self.bonus.total("MISC", "ACCHECK")) as i64
    }

    /// Arcane spell failure chance from equipped items.
    pub fn spell_failure(&self) -> i64 {
        let mut total = 0.0;
        for (i, it) in self.items.iter().enumerate() {
            total += it
                .obj
                .get("SPELLFAILURE")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            total += self
                .bonus
                .total(&format!("ITEM{}.EQMARMOR", i), "SPELLFAILURE");
        }
        total as i64 + self.bonus.total("MISC", "SPELLFAILURE") as i64
    }

    /// Spell resistance: the best SR of any source, plus MISC|SR.
    pub fn spell_resistance(&self) -> i64 {
        let mut best = 0.0f64;
        for s in &self.sources {
            for t in s.tokens.iter().filter(|t| t.tag == "SR") {
                let env = Ctx {
                    calc: self,
                    src: Some(s),
                };
                best = best.max(eval_str(&t.value, &env));
            }
        }
        best as i64 + self.bonus.total("MISC", "SR") as i64
    }

    pub fn stat_score(&self, abb: &str) -> f64 {
        let a = upper(abb);
        self.stat_base.get(&a).copied().unwrap_or(0.0) + self.bonus.total("STAT", &a)
    }

    pub fn stat_mod(&self, abb: &str) -> f64 {
        self.stat_mod_for(abb, self.stat_score(abb))
    }

    /// Ranks bought in a skill.
    pub fn base_ranks(&self, skill: &str) -> f64 {
        self.ranks.get(&lower(skill)).copied().unwrap_or(0.0)
    }

    /// Total ranks: bought plus SKILLRANK bonuses, capped at the
    /// maximum for the character's level (but never below the ranks
    /// actually bought).
    pub fn skill_ranks(&self, skill: &str) -> f64 {
        let base = self.base_ranks(skill);
        let ranks = base + self.bonus.total("SKILLRANK", skill);
        if self.rules.get("SKILLMAX").copied().unwrap_or(false) || self.classes.is_empty() {
            return ranks;
        }
        match self.max_ranks(skill) {
            Some(max) => ranks.min(max.max(base)),
            None => ranks,
        }
    }

    /// Maximum ranks for a skill at the character's level, from the
    /// game mode's experience table.
    pub fn max_ranks(&self, skill: &str) -> Option<f64> {
        let class = self.is_class_skill(skill);
        self.level_value(self.total_level(), |r| {
            if class {
                r.class_skill_max.clone()
            } else {
                r.cross_class_skill_max.clone()
            }
        })
    }

    /// The character's race type: the race's RACETYPE, replaced by
    /// that of the last template that sets one.
    pub fn race_type(&self) -> Option<String> {
        let mut t = self
            .race()
            .and_then(|r| r.get("RACETYPE"))
            .map(str::to_string);
        for s in self.sources.iter().filter(|s| s.kind == SrcKind::Template) {
            if let Some(v) = s.obj.get("RACETYPE") {
                t = Some(v.to_string());
            }
        }
        t
    }

    /// Does a choice made for an ability match a prerequisite's
    /// pattern: the same value, a `prefix%`, or `TYPE=x` for a chosen
    /// object (weapon proficiency, skill, equipment, ...) of that type?
    pub fn choice_matches(&self, pattern: &str, choice: &str) -> bool {
        if let Some(p) = pattern.strip_suffix('%') {
            return lower(choice).starts_with(&lower(p));
        }
        if let Some(t) = pattern
            .strip_prefix("TYPE=")
            .or_else(|| pattern.strip_prefix("TYPE."))
        {
            return [
                FileKind::WeaponProf,
                FileKind::Skill,
                FileKind::Equipment,
                FileKind::Language,
                FileKind::Class,
                FileKind::Domain,
                FileKind::Deity,
                FileKind::Template,
                FileKind::Race,
                FileKind::Spell,
                FileKind::Ability,
            ]
            .iter()
            .filter_map(|k| self.ds.get(*k, choice))
            .any(|o| t.split('.').all(|x| o.is_type(x)));
        }
        pattern.eq_ignore_ascii_case(choice)
    }

    /// Count items by type: `ARMOR.IS.HEAVY_ARMOR`, `ARMOR.EQUIPPED`,
    /// `WEAPON.NOT.MELEE`.  The first part (and each part after IS) is a
    /// type the item must have, each part after NOT one it must not;
    /// EQUIPPED counts only equipped items.
    fn count_eqtype(&self, spec: &str) -> usize {
        let parts: Vec<&str> = spec.split('.').collect();
        let mut need: Vec<&str> = Vec::new();
        let mut exclude: Vec<&str> = Vec::new();
        let mut equipped = false;
        let mut i = 0;
        while i < parts.len() {
            match parts[i] {
                "EQUIPPED" => equipped = true,
                "IS" if i + 1 < parts.len() => {
                    need.push(parts[i + 1]);
                    i += 1;
                }
                "NOT" if i + 1 < parts.len() => {
                    exclude.push(parts[i + 1]);
                    i += 1;
                }
                "NOTEQUIPPED" | "CARRIED" | "CONTAINER" => {}
                t => need.push(t),
            }
            i += 1;
        }
        let ok =
            |o: &Object| need.iter().all(|t| o.is_type(t)) && !exclude.iter().any(|t| o.is_type(t));
        if equipped {
            self.items.iter().filter(|it| ok(it.obj)).count()
        } else {
            self.ch
                .equipped()
                .iter()
                .filter_map(|e| e.item.as_deref())
                .filter_map(|n| {
                    let base = self
                        .ch
                        .equipment()
                        .into_iter()
                        .find(|x| x.name == n)
                        .and_then(|x| x.base);
                    self.ds
                        .get(FileKind::Equipment, base.as_deref().unwrap_or(n))
                        .or_else(|| self.ds.get(FileKind::Equipment, n))
                })
                .filter(|o| ok(o))
                .count()
        }
    }

    /// Stats in display (SORTKEY) order.
    pub fn stats_in_order(&self) -> Vec<&'a Object> {
        let mut v: Vec<&Object> = self.ds.objects(FileKind::Stat).iter().collect();
        v.sort_by_key(|o| {
            o.get("SORTKEY")
                .and_then(|k| k.parse::<i64>().ok())
                .unwrap_or(i64::MAX)
        });
        v
    }

    /// A stat score with only some bonuses: without those from
    /// equipment, and/or without temporary ones.
    pub fn stat_score_without(&self, abb: &str, equipment: bool, temporary: bool) -> f64 {
        let k = upper(abb);
        self.stat_base(&k)
            + self.bonus.total_where("STAT", &k, |src| {
                let eq = matches!(src.kind.as_str(), "Equipment" | "EquipMod")
                    || src.kind == "Ability"
                        && self
                            .ds
                            .get_in(
                                FileKind::Ability,
                                src.category.as_deref().unwrap_or(""),
                                &src.key,
                            )
                            .is_some_and(|o| o.is_type("Equipment"));
                !(equipment && eq) && !(temporary && src.temp)
            })
    }

    /// `STAT.n[.MOD][.BASE][.NOEQUIP][.NOTEMP]` terms.
    fn stat_term(&self, u: &str) -> Option<f64> {
        let rest = u.strip_prefix("STAT.")?;
        let mut parts = rest.split('.');
        let idx: usize = parts.next()?.parse().ok()?;
        let stat = self.stats_in_order().get(idx).copied()?;
        let (mut m, mut base, mut noequip, mut notemp) = (false, false, false, false);
        for p in parts {
            match p {
                "MOD" => m = true,
                "BASE" => base = true,
                "BASEMOD" => {
                    m = true;
                    base = true
                }
                "NOEQUIP" => noequip = true,
                "NOTEMP" => notemp = true,
                "NOTEMPMOD" => {
                    m = true;
                    notemp = true
                }
                _ => return None,
            }
        }
        let score = if base {
            self.stat_base(&stat.key)
        } else {
            self.stat_score_without(&stat.key, noequip, notemp)
        };
        Some(if m {
            self.stat_mod_for(&stat.key, score)
        } else {
            score
        })
    }

    /// Does the game mode define a rule?
    pub fn rules_known(&self, name: &str) -> bool {
        self.rules.contains_key(&upper(name))
    }

    /// Is a game-mode rule (rules.lst PARM/VAR) on?
    pub fn rule(&self, name: &str) -> bool {
        self.rules.get(&upper(name)).copied().unwrap_or(false)
    }

    /// Skill points a level of a class is worth now, at a given
    /// character level: the class's base points, bonus points, the
    /// stat's points (at least 1), race extras and the level's
    /// multiplier.
    pub fn skill_points_for_level(&self, cls: &'a Object, char_level: u32) -> f64 {
        let env = Ctx {
            calc: self,
            src: self.sources.iter().find(|x| std::ptr::eq(x.obj, cls)),
        };
        let class_sp = cls
            .get("STARTSKILLPTS")
            .map(|f| eval_str(f, &env))
            .unwrap_or(0.0);
        let mut sp = class_sp + self.bonus.total("SKILLPOINTS", "NUMBER");
        let race_sp = sp - class_sp;
        let skill_min = if sp > 0.0 { 1.0 } else { 0.0 };
        let mod_to_skills = !cls
            .get("MODTOSKILLS")
            .is_some_and(|v| v.eq_ignore_ascii_case("NO"));
        if mod_to_skills {
            sp += self
                .bonus
                .total_where("MODSKILLPOINTS", "NUMBER", |s| s.kind == "Stat");
            sp = sp.max(1.0);
        }
        if let Some(r) = self.race() {
            if let Some(f) = r.get("XTRASKILLPTSPERLVL") {
                sp += eval_str(f, &env);
            }
        }
        sp = sp.max(skill_min);
        let min = self.bonus.total("MINCLASSSKILLPOINTS", "NUMBER");
        let other = sp - class_sp - race_sp;
        if min > 0.0 && class_sp + other < min {
            sp = min.max(class_sp + other) + race_sp;
        }
        let mults: Vec<f64> = self
            .gm
            .and_then(|g| g.misc("SKILLMULTIPLIER"))
            .unwrap_or("1")
            .split('|')
            .filter_map(|v| v.trim().parse().ok())
            .collect();
        let mult = if char_level == 1 {
            self.race()
                .and_then(|r| r.get("SKILLMULT"))
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| mults.first().copied().unwrap_or(1.0))
        } else {
            mults.get(char_level as usize - 1).copied().unwrap_or(1.0)
        };
        sp * mult
    }

    /// The character's deity: the one named in the character file, else
    /// the data's default deity (DEFAULTVARIABLEVALUE:DEITY|key).
    pub fn deity(&self) -> Option<&'a Object> {
        if let Some(d) = self.ch.deity() {
            if let Some(o) = self.ds.get(FileKind::Deity, &d) {
                return Some(o);
            }
        }
        let def = crate::rules::vars::system::data_defaults(self.ds, self.gm);
        let key = def.get("DEITY")?;
        self.ds.get(FileKind::Deity, key)
    }

    /// Is the skill a class skill for the given class?  Class skills
    /// from a class (and its levels, subclass and domains) count for
    /// that class only; those from races, templates, abilities and the
    /// like count for every class.
    pub fn is_class_skill_for(&self, skill: &str, class: &str) -> bool {
        let Some(sk) = self.ds.get(FileKind::Skill, skill) else {
            return false;
        };
        let cls = self
            .classes
            .iter()
            .find(|(o, _)| o.key.eq_ignore_ascii_case(class) || o.name.eq_ignore_ascii_case(class))
            .map(|(o, _)| *o);
        let monster = cls.is_some_and(|c| c.is_type("Monster"));
        for src in &self.sources {
            let for_class = match src.kind {
                SrcKind::Class => src.obj.key.eq_ignore_ascii_case(class),
                SrcKind::Domain => src
                    .class
                    .as_deref()
                    .is_none_or(|c| c.eq_ignore_ascii_case(class)),
                _ => true,
            };
            if !for_class {
                continue;
            }
            for t in &src.tokens {
                let listed = match t.tag.as_str() {
                    "CSKILL" => true,
                    "MONCSKILL" => monster && src.kind == SrcKind::Race,
                    _ => false,
                };
                if listed && skill_list_has(self.ds, &t.value, sk, &src.choices) {
                    return true;
                }
            }
        }
        // the skill may name the classes it is a class skill for
        if sk.values("CLASSES").flat_map(|v| v.split('|')).any(|c| {
            let c = c.split('=').next().unwrap_or(c).trim();
            c.eq_ignore_ascii_case("ALL")
                || cls.is_some_and(|o| {
                    o.key.eq_ignore_ascii_case(c) || o.name.eq_ignore_ascii_case(c)
                })
        }) {
            return true;
        }
        // class skills chosen when a level was taken
        self.ch.pcg.lines_tagged("CLASSABILITIESLEVEL").any(|l| {
            let of_class = l
                .head()
                .map(|h| h.value_text())
                .and_then(|h| h.rsplit_once('=').map(|(c, _)| c.to_string()))
                .is_some_and(|c| c.eq_ignore_ascii_case(class));
            of_class
                && l.elems.iter().flatten().any(|e| {
                    e.tag == "ADD"
                        && e.children().iter().any(|c| c.tag == "CLASSSKILLS")
                        && e.children().iter().any(|c| {
                            c.tag == "CHOICE" && c.value_text().eq_ignore_ascii_case(&sk.key)
                        })
                })
        })
    }

    /// Is the skill made a cross-class skill (CCSKILL) by some object?
    pub fn is_cross_class_skill(&self, skill: &str) -> bool {
        self.cross_class_skills.contains(&lower(skill))
    }

    pub fn is_class_skill(&self, skill: &str) -> bool {
        let s = lower(skill);
        if self.class_skills.contains(&s) || self.pcg_class_skills.contains(&s) {
            return true;
        }
        // the skill may name the classes it is a class skill for
        let Some(o) = self.ds.get(FileKind::Skill, skill) else {
            return false;
        };
        o.values("CLASSES").flat_map(|v| v.split('|')).any(|c| {
            let c = c.split('=').next().unwrap_or(c).trim();
            c.eq_ignore_ascii_case("ALL")
                || self.classes.iter().any(|(cl, _)| {
                    cl.key.eq_ignore_ascii_case(c) || cl.name.eq_ignore_ascii_case(c)
                })
        })
    }

    fn race(&self) -> Option<&'a Object> {
        let r = self.ch.race()?;
        self.ds
            .get(FileKind::Race, &r)
            .or_else(|| self.ds.get(FileKind::Race, &self.migrate("RACE", &r)))
    }

    /// Index of a size key among the sizes, smallest first.
    pub fn size_index_of(&self, key: &str) -> Option<i64> {
        self.size_list()
            .iter()
            .position(|o| o.key.eq_ignore_ascii_case(key))
            .map(|i| i as i64)
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
        if let Some(v) = self.stat_term(&u) {
            return Some(v);
        }
        match u.as_str() {
            "TL" => return Some(self.total_level() as f64),
            // hit dice from monster (racial) classes
            "HD" => {
                return Some(
                    self.classes
                        .iter()
                        .filter(|(c, _)| self.is_monster_class(c))
                        .map(|(_, l)| *l as f64)
                        .sum(),
                )
            }
            "ECL" => return Some(self.ecl() as f64),
            // the race's walking speed before any bonus
            "MOVEBASE" => {
                return Some(
                    self.race()
                        .and_then(|r| r.get("MOVE"))
                        .and_then(|v| {
                            let p: Vec<&str> = v.split(',').collect();
                            p.chunks(2).find_map(|c| match c {
                                [t, n] if t.trim().eq_ignore_ascii_case("Walk") => {
                                    n.trim().parse().ok()
                                }
                                _ => None,
                            })
                        })
                        .unwrap_or(0.0),
                )
            }
            // racial hit dice the race starts with (MONSTERCLASS:Class:n)
            "BASEHD" => {
                return Some(
                    self.race()
                        .and_then(|r| r.get("MONSTERCLASS"))
                        .and_then(|v| v.rsplit(':').next())
                        .and_then(|n| n.trim().parse().ok())
                        .unwrap_or(0.0),
                )
            }
            "CR" => return Some(self.challenge_rating().unwrap_or(0.0)),
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
            _ if u.starts_with("COUNT[EQTYPE.") && u.ends_with(']') => {
                return Some(self.count_eqtype(&u["COUNT[EQTYPE.".len()..u.len() - 1]) as f64)
            }
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
        if let Some(t) = u.strip_prefix("MOVE[").and_then(|x| x.strip_suffix(']')) {
            return Some(self.move_speed(t));
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
                                    s.choices.iter().any(|x| self.choice_matches(ch, x))
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
                // TYPE=t or TYPE.t, optionally with =level
                if let Some(rest) = a.strip_prefix("TYPE.").or_else(|| a.strip_prefix("TYPE=")) {
                    let (t, lv) = rest.split_once('=').unwrap_or((rest, "1"));
                    let need = num(lv);
                    return self
                        .classes
                        .iter()
                        .any(|(o, l)| o.is_type(t) && *l as f64 >= need);
                }
                let (c, lv) = a.split_once('=').unwrap_or((a, "1"));
                let need = num(lv);
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
                // TYPE.x or TYPE=x: any skill of that type with the ranks
                if let Some(rest) = a.strip_prefix("TYPE.").or_else(|| a.strip_prefix("TYPE=")) {
                    let (t, v) = rest.split_once('=').unwrap_or((rest, "1"));
                    return self
                        .ds
                        .objects(FileKind::Skill)
                        .iter()
                        .filter(|o| t.split('.').all(|x| o.is_type(x)))
                        .any(|o| self.skill_ranks(&o.key) >= num(v));
                }
                let (s, v) = a.split_once('=').unwrap_or((a, "1"));
                self.skill_ranks(s) >= num(v)
            }),
            "CSKILL" => self.count_items(args, |a| self.is_class_skill(a)),
            "RACE" => self.count_items(args, |a| {
                let Some(r) = self.race() else { return false };
                let rt = self.race_type();
                if let Some(t) = a.strip_prefix("TYPE=").or_else(|| a.strip_prefix("TYPE.")) {
                    return r.is_type(t)
                        || rt.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(t));
                }
                if let Some(t) = a.strip_prefix("RACETYPE=") {
                    return rt.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(t));
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
                    self.items
                        .iter()
                        .filter(|i| hand_matches(&kind, &i.location))
                        .map(|i| i.obj)
                        .collect()
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
            "TOTALAB" | "ATT" => args
                .first()
                .is_none_or(|a| self.bonus.total("COMBAT", "BASEAB") >= num(a)),
            "CHECKBASE" => self.count_items(args, |a| {
                let (k, v) = a.split_once('=').unwrap_or((a, "0"));
                self.bonus.total("SAVE", &format!("BASE.{}", k)) >= num(v)
            }),
            "CHECK" => self.count_items(args, |a| {
                let (k, v) = a.split_once('=').unwrap_or((a, "0"));
                self.bonus.total("SAVE", &format!("BASE.{}", k))
                    + self.bonus.total("SAVE", k)
                    + self.bonus.total("SAVE", "ALL")
                    >= num(v)
            }),
            "REGION" => {
                let r = self.ch.pcg.value("REGION").unwrap_or_default();
                args.iter().any(|a| a.trim().eq_ignore_ascii_case(&r))
            }
            "RACETYPE" => {
                let rt = self.race_type();
                self.count_items(args, |a| {
                    rt.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(a))
                })
            }
            "SKILLTOT" => {
                // PRESKILLTOT:skill,skill=total
                let Some(last) = args.last() else { return true };
                let (lastskill, total) = last.split_once('=').unwrap_or((last, "0"));
                let mut sum = 0.0;
                for a in &args[..args.len() - 1] {
                    sum += self.skill_ranks(a);
                }
                sum += self.skill_ranks(lastskill);
                sum >= num(total)
            }
            "LEGSEQ" | "LEGSGT" | "LEGSGTEQ" | "LEGSLT" | "LEGSLTEQ" | "LEGSNEQ" => {
                let legs = self.limbs("LEGS", 2.0);
                args.first().is_none_or(|a| cmp(&kind[4..], legs, num(a)))
            }
            "HANDSEQ" | "HANDSGT" | "HANDSGTEQ" | "HANDSLT" | "HANDSLTEQ" | "HANDSNEQ" => {
                let hands = self.limbs("HANDS", 2.0);
                args.first().is_none_or(|a| cmp(&kind[5..], hands, num(a)))
            }
            "VISION" => self.count_items(args, |a| {
                let (t, _) = a.split_once('=').unwrap_or((a, ""));
                self.sources.iter().any(|s| {
                    s.tokens
                        .iter()
                        .any(|tk| tk.tag == "VISION" && tk.value.to_lowercase().contains(&lower(t)))
                })
            }),
            "SA" => self.count_items(args, |a| {
                let pat = lower(a.trim_end_matches('%'));
                self.ability_srcs().any(|s| {
                    lower(&s.obj.key).starts_with(&pat) || lower(&s.obj.name).starts_with(&pat)
                })
            }),
            "DEITYDOMAIN" => {
                let d = self
                    .ch
                    .deity()
                    .and_then(|d| self.ds.get(FileKind::Deity, &d));
                self.count_items(args, |a| {
                    d.is_some_and(|o| {
                        o.values("DOMAINS")
                            .flat_map(|v| v.split([',', '|']))
                            .any(|x| x.eq_ignore_ascii_case(a))
                    })
                })
            }
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
            "CHARACTERTYPE" => {
                let t = self.character_type();
                self.count_items(args, |a| a.eq_ignore_ascii_case(&t))
            }
            "WEAPONPROF" | "PROFWITHARMOR" | "PROFWITHSHIELD" => {
                if self.in_profs.get() {
                    return true;
                }
                self.in_profs.set(true);
                let r = match kind.as_str() {
                    "WEAPONPROF" => {
                        let keys = super::profs::weapon_prof_keys(self);
                        self.count_items(args, |a| {
                            if let Some(t) =
                                a.strip_prefix("TYPE.").or_else(|| a.strip_prefix("TYPE="))
                            {
                                keys.iter().any(|k| {
                                    self.ds
                                        .get(FileKind::WeaponProf, k)
                                        .is_some_and(|o| o.is_type(t))
                                })
                            } else if a.eq_ignore_ascii_case("DEITYWEAPON") {
                                self.deity().is_some_and(|d| {
                                    d.values("DEITYWEAP")
                                        .flat_map(|v| v.split('|'))
                                        .any(|w| keys.iter().any(|k| k.eq_ignore_ascii_case(w)))
                                })
                            } else {
                                keys.iter().any(|k| k.eq_ignore_ascii_case(a))
                            }
                        })
                    }
                    other => {
                        let shield = other == "PROFWITHSHIELD";
                        let (names, types) = super::profs::armor_grants(self, shield);
                        self.count_items(args, |a| {
                            match a.strip_prefix("TYPE.").or_else(|| a.strip_prefix("TYPE=")) {
                                Some(t) => types.iter().any(|x| x.eq_ignore_ascii_case(t)),
                                None => names.iter().any(|x| x.eq_ignore_ascii_case(a)),
                            }
                        })
                    }
                };
                self.in_profs.set(false);
                r
            }
            // PREARMORTYPE: wearing armor of a type
            "ARMORTYPE" => self.count_items(args, |a| {
                let t = a
                    .strip_prefix("TYPE.")
                    .or_else(|| a.strip_prefix("TYPE="))
                    .unwrap_or(a);
                self.items.iter().any(|it| {
                    it.obj.is_type("Armor") && (t.eq_ignore_ascii_case("ANY") || it.obj.is_type(t))
                })
            }),
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
            nature: None,
            category: None,
            applied: Vec::new(),
        }
    }

    /// Tokens of a class active at its current level.
    fn class_tokens(&self, o: &'a Object, level: u32) -> Vec<&'a Tok> {
        let mut v: Vec<&Tok> = o.tokens.iter().collect();
        let subs = self.substitutions(o);
        for (n, sub) in &subs {
            if *n > level {
                continue;
            }
            for sl in sub
                .sublines
                .iter()
                .filter(|sl| sl.tag == "SUBSTITUTIONLEVEL")
            {
                if level_number(&sl.value) == *n {
                    v.extend(sl.tokens.iter());
                }
            }
        }
        for sl in &o.sublines {
            if sl.tag != "LEVEL" {
                continue;
            }
            let mut parts = sl.value.splitn(3, ':');
            let start: u32 = parts.next().and_then(|x| x.parse().ok()).unwrap_or(0);
            // a substitution level replaces the class's own line
            if subs.iter().any(|(n, _)| *n == start) && !sl.value.contains("REPEATLEVEL") {
                continue;
            }
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
        // the chosen subclass adds its own tokens and level lines
        if let Some(sub) = self.subclass_of(o) {
            v.extend(sub.tokens.iter());
            for sl in sub.sublines.iter().filter(|sl| sl.tag == "SUBCLASSLEVEL") {
                let n = level_number(&sl.value);
                if n > 0 && n <= level {
                    v.extend(sl.tokens.iter());
                }
            }
        }
        v
    }

    /// Substitution levels taken in a class: (level, substitution class),
    /// from SUBSTITUTIONLEVEL on the saved level lines.
    fn substitutions(&self, cls: &'a Object) -> Vec<(u32, &'a Object)> {
        let mut out = Vec::new();
        for l in self.ch.pcg.lines_tagged("CLASSABILITIESLEVEL") {
            let Some((c, n)) = l.head().and_then(|h| {
                h.value_text()
                    .rsplit_once('=')
                    .map(|(c, n)| (c.to_string(), n.to_string()))
            }) else {
                continue;
            };
            if !(c.eq_ignore_ascii_case(&cls.key) || c.eq_ignore_ascii_case(&cls.name)) {
                continue;
            }
            let Some(name) = l.text("SUBSTITUTIONLEVEL") else {
                continue;
            };
            let sub = cls.children.iter().find(|ch| {
                ch.head_tag.as_deref() == Some("SUBSTITUTIONCLASS")
                    && (ch.key.eq_ignore_ascii_case(&name) || ch.name.eq_ignore_ascii_case(&name))
            });
            if let (Some(sub), Ok(n)) = (sub, n.trim().parse()) {
                out.push((n, sub));
            }
        }
        out
    }

    /// Languages granted by AUTO:LANG on active objects whose trailing
    /// prerequisites pass: names, TYPE=t (every language of the type)
    /// and %LIST (the object's choices).
    pub fn auto_languages(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut add = |k: &str| {
            if !k.is_empty() && !out.iter().any(|x| x.eq_ignore_ascii_case(k)) {
                out.push(k.to_string());
            }
        };
        let is_pre = |i: &str| {
            Prereq::is_pre_tag(i.split(':').next().unwrap_or("")) || i.starts_with("!PRE")
        };
        for src in &self.sources {
            for t in src.tokens.iter().filter(|t| t.tag == "AUTO") {
                let Some(rest) = t.value.strip_prefix("LANG|") else {
                    continue;
                };
                let items: Vec<&str> = rest.split('|').collect();
                let ok = items
                    .iter()
                    .filter(|i| is_pre(i))
                    .filter_map(|i| Prereq::parse(i).ok())
                    .all(|p| self.passes(&p, Some(src)));
                if !ok {
                    continue;
                }
                for item in items.iter().filter(|i| !is_pre(i)) {
                    if let Some(ty) = item
                        .strip_prefix("TYPE=")
                        .or_else(|| item.strip_prefix("TYPE."))
                    {
                        for o in self.ds.objects(FileKind::Language) {
                            if ty.split('.').all(|x| o.is_type(x)) {
                                add(&o.key);
                            }
                        }
                    } else if *item == "%LIST" {
                        for c in &src.choices {
                            add(c);
                        }
                    } else {
                        add(item);
                    }
                }
            }
        }
        out
    }

    /// The character's type: the saved one if the game mode lists it,
    /// else the first type the game mode lists ("Default" if none).
    pub fn character_type(&self) -> String {
        let listed: Vec<String> = self
            .gm
            .and_then(|g| g.misc("CHARACTERTYPE"))
            .map(|v| {
                v.split('|')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let default = listed.first().cloned().unwrap_or_else(|| "Default".into());
        match self.ch.pcg.value("CHARACTERTYPE") {
            Some(t) if listed.iter().any(|x| x.eq_ignore_ascii_case(&t)) => t,
            _ => default,
        }
    }

    /// The subclass chosen for a class, if any.
    pub fn subclass_of(&self, cls: &'a Object) -> Option<&'a Object> {
        let name = self
            .ch
            .pcg
            .lines_tagged("CLASS")
            .find(|l| {
                l.head()
                    .is_some_and(|h| h.value_text().eq_ignore_ascii_case(&cls.key))
            })
            .and_then(|l| l.text("SUBCLASS"))
            .filter(|v| !v.is_empty() && v != "None")?;
        cls.children.iter().find(|c| {
            c.head_tag.as_deref() == Some("SUBCLASS")
                && (c.key.eq_ignore_ascii_case(&name) || c.name.eq_ignore_ascii_case(&name))
        })
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
        // skills the character has: those with ranks (bought, or
        // granted as of the last pass), and once the character has a
        // class, every skill usable untrained
        let has_class = !self.classes.is_empty();
        for o in ds.objects(FileKind::Skill) {
            let untrained = !o
                .get("USEUNTRAINED")
                .is_some_and(|v| v.eq_ignore_ascii_case("NO"));
            let ranked =
                self.base_ranks(&o.key) > 0.0 || self.bonus.total("SKILLRANK", &o.key) > 0.0;
            if ranked || has_class && untrained {
                srcs.push(self.src(SrcKind::Skill, o));
            }
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
        // bonuses a follower's race grants its master
        for f in self.ch.pcg.lines_tagged("FOLLOWER") {
            let Some(race) = f.text("RACE") else { continue };
            for o in ds.objects(FileKind::CompanionMod) {
                if o.head_tag.as_deref() == Some("MASTERBONUSRACE")
                    && o.name.eq_ignore_ascii_case(&race)
                {
                    srcs.push(self.src(SrcKind::MasterBonus, o));
                }
            }
        }
        if let Some(d) = self.deity() {
            srcs.push(self.src(SrcKind::Deity, d));
        }
        for l in self.ch.pcg.lines_tagged("DOMAIN") {
            let Some(d) = l.head().map(|h| h.value_text()) else {
                continue;
            };
            if let Some(o) = ds.get(FileKind::Domain, &d) {
                let mut src = self.src(SrcKind::Domain, o);
                // the class the domain was taken through
                src.class = l
                    .get("SOURCE")
                    .and_then(|e| e.children().iter().find(|c| c.tag == "NAME"))
                    .map(|c| c.value_text());
                if let Some(a) = l.text("ASSOCIATEDDATA") {
                    src.choices = a.split(',').map(|x| x.trim().to_string()).collect();
                }
                srcs.push(src);
            }
        }
        for (i, it) in self.items.iter().enumerate() {
            let mut s = self.src(SrcKind::Equipment, it.obj);
            s.item = Some(i);
            srcs.push(s);
            for (m, ch) in it.eqmods.iter().zip(&it.eqmod_choices) {
                let mut s = self.src(SrcKind::EquipMod, m);
                s.item = Some(i);
                s.choices = ch.clone();
                srcs.push(s);
            }
        }

        // abilities stored in the character
        let mut seen: HashSet<(String, String, Vec<String>, &str)> = HashSet::new();
        let mut applied: HashSet<(String, String, Vec<String>)> = HashSet::new();
        let mut pending: Vec<Grant> = Vec::new();
        for a in self.ch.abilities() {
            if a.key.starts_with('*') {
                continue;
            }
            let migrated = self.migrate("ABILITY", &format!("{}|{}", a.category, a.key));
            let key = migrated
                .rsplit_once('|')
                .map(|(_, k)| k.to_string())
                .unwrap_or(migrated);
            pending.push((
                a.category.clone(),
                key,
                a.applied_to.clone(),
                None,
                nature_of(&a.nature),
                false,
            ));
        }
        for (f, applied) in self.ch.legacy_feats() {
            pending.push(("FEAT".into(), f, applied, None, "NORMAL", false));
        }

        // grants from active objects, repeated to closure
        let mut i = 0;
        loop {
            while i < srcs.len() {
                // an object's own PRExxx tokens decide whether it may be
                // taken, not whether it works once the character has it;
                // only prerequisites on grants and bonuses gate here
                let s = srcs[i].clone();
                i += 1;
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
                                // %LIST grants each choice made for the source
                                let keys: Vec<String> = if *k == "%LIST" {
                                    s.choices.clone()
                                } else if k.contains("%LIST") {
                                    // `Weapon Focus(%LIST)`: one per choice
                                    s.choices.iter().map(|c| k.replace("%LIST", c)).collect()
                                } else {
                                    vec![k.to_string()]
                                };
                                for k in keys {
                                    pending.push((
                                        self.pool_category(parts[0]),
                                        k,
                                        Vec::new(),
                                        s.class.clone(),
                                        nature_of(parts[1]),
                                        true,
                                    ));
                                }
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
                                        "AUTOMATIC",
                                        true,
                                    ));
                                }
                            }
                        }
                        "VFEAT" | "FEAT" if s.kind != SrcKind::Ability => {
                            let nature = if t.tag == "VFEAT" {
                                "VIRTUAL"
                            } else {
                                "NORMAL"
                            };
                            for k in t.value.split('|') {
                                if !Prereq::is_pre_tag(k.split(':').next().unwrap_or("")) {
                                    pending.push((
                                        "FEAT".into(),
                                        k.to_string(),
                                        Vec::new(),
                                        s.class.clone(),
                                        nature,
                                        true,
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
            // TYPE=x grants every ability of the category with that type
            let mut batch = Vec::new();
            for (cat, key, choices, class, nature, granted) in std::mem::take(&mut pending) {
                match key
                    .strip_prefix("TYPE=")
                    .or_else(|| key.strip_prefix("TYPE."))
                {
                    Some(types) => {
                        for o in ds.objects(FileKind::Ability) {
                            let in_cat = o
                                .category
                                .as_deref()
                                .is_some_and(|c| c.eq_ignore_ascii_case(&cat));
                            if in_cat && types.split('.').all(|t| o.is_type(t)) {
                                batch.push((
                                    cat.clone(),
                                    o.key.clone(),
                                    choices.clone(),
                                    class.clone(),
                                    nature,
                                    granted,
                                ));
                            }
                        }
                    }
                    None => batch.push((cat, key, choices, class, nature, granted)),
                }
            }
            for (cat, key, choices, class, nature, granted) in batch {
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
                // an ability chosen without a value is held once per time
                // taken, recorded or granted
                let _ = granted;
                let choices = if choices.is_empty()
                    && obj.get("CHOOSE").is_some_and(|c| c.starts_with("NOCHOICE"))
                {
                    vec![String::new()]
                } else {
                    choices
                };
                let id = (lower(&cat), lower(&obj.key), choices.clone(), nature);
                let stacks = obj
                    .get("STACK")
                    .is_some_and(|v| v.eq_ignore_ascii_case("YES"));
                // a stacking ability granted again is held again
                if !seen.insert(id) && !stacks {
                    continue;
                }
                // held a second way (say, chosen and also granted): listed
                // under both natures, but its effects apply once
                let again = !applied.insert((lower(&cat), lower(&obj.key), choices.clone()));
                // recorded choices an explicit CHOOSE list does not offer
                // are dropped
                let choices: Vec<String> = choices
                    .into_iter()
                    .filter(|c| c.is_empty() || choose_allows(obj, c))
                    .collect();
                let mut s = self.src(SrcKind::Ability, obj);
                if again && !stacks {
                    s.tokens.clear();
                }
                s.nature = Some(nature.to_string());
                s.category = Some(cat.clone());
                s.applied = choices.clone();
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

        // class skills and cross-class skills granted to exclusive skills
        let mut cs = HashSet::new();
        let mut ccs = HashSet::new();
        for s in &srcs {
            for t in &s.tokens {
                let set = match t.tag.as_str() {
                    "CSKILL" => &mut cs,
                    "CCSKILL" => &mut ccs,
                    _ => continue,
                };
                for item in t.value.split('|') {
                    if let Some(ty) = item
                        .strip_prefix("TYPE=")
                        .or_else(|| item.strip_prefix("TYPE."))
                    {
                        for sk in ds.objects(FileKind::Skill) {
                            if sk.is_type(ty) {
                                set.insert(lower(&sk.key));
                            }
                        }
                    } else if item == "%LIST" || item == "LIST" {
                        for c in &s.choices {
                            set.insert(lower(c));
                        }
                    } else if item.eq_ignore_ascii_case("ALL") {
                        for sk in ds.objects(FileKind::Skill) {
                            set.insert(lower(&sk.key));
                        }
                    } else {
                        set.insert(lower(item));
                    }
                }
            }
        }
        self.cross_class_skills = ccs;
        self.class_skills = cs;
        self.sources = srcs;
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
                            let origin = s.source();
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
                            let origin = s.source();
                            // a category naming the choice
                            // (WEAPONPROF=%LIST) applies once per choice
                            let cats: Vec<String> =
                                if cat.contains("%LIST") || cat.contains("%CHOICE") {
                                    s.choices
                                        .iter()
                                        .map(|c| cat.replace("%LIST", c).replace("%CHOICE", c))
                                        .collect()
                                } else {
                                    vec![cat.clone()]
                                };
                            for cat in &cats {
                                for tg in &tgs {
                                    table.add_from(cat, tg, btype, v, &origin);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            let _ = si;
        }
        // temporary bonuses the user switched on, aimed at the character
        for line in self.ch.pcg.lines_tagged("TEMPBONUS") {
            if line.text("TBTARGET").as_deref() != Some("PC") {
                continue;
            }
            let name = line.head().map(|h| h.value_text()).unwrap_or_default();
            let origin = Source {
                origin: format!("TempBonus {}", name),
                kind: "TempBonus".into(),
                key: name,
                category: None,
                temp: true,
            };
            let env = Ctx {
                calc: self,
                src: None,
            };
            for e in line.elems.iter().flatten().filter(|e| e.tag == "TBBONUS") {
                let Ok(b) = Bonus::parse(&e.value_text()) else {
                    continue;
                };
                if !b.prereqs().all(|p| self.passes(p, None)) {
                    continue;
                }
                let Ok(x) = b.expr() else { continue };
                let v = eval(&x, &env);
                let btype = b.opts.iter().find_map(|o| match o {
                    BonusOpt::Type(t) => Some(t.as_str()),
                    _ => None,
                });
                for tg in &b.targets {
                    table.add_from(&b.category, tg, btype, v, &origin);
                }
            }
        }
        // bonuses saved with choices made (SAVE:BONUS|... on ability
        // lines and level lines)
        let mut saved: Vec<String> = Vec::new();
        for l in self.ch.pcg.lines_tagged("ABILITY") {
            for e in l.elems.iter().flatten().filter(|e| e.tag == "SAVE") {
                saved.push(e.value_text());
            }
        }
        for l in self.ch.pcg.lines_tagged("CLASSABILITIESLEVEL") {
            if let Some(e) = l.get("SAVES") {
                for c in e.children().iter().filter(|c| c.tag == "SAVE") {
                    saved.push(c.value_text());
                }
            }
        }
        // a spellcaster level chosen through the race or a template
        // raises the chosen class's level for spells (level lines save
        // that bonus themselves)
        for tag in ["RACE", "TEMPLATE"] {
            for l in self.ch.pcg.lines_tagged(tag) {
                for e in l.elems.iter().flatten().filter(|e| e.tag == "ADD") {
                    let kids = e.children();
                    if !kids.iter().any(|c| c.tag == "SPELLCASTER") {
                        continue;
                    }
                    for c in kids.iter().filter(|c| c.tag == "CHOICE") {
                        saved.push(format!("BONUS|PCLEVEL|{}|1", c.value_text()));
                    }
                }
            }
        }
        let origin = Source {
            origin: "Saved bonus".into(),
            kind: "Saved".into(),
            key: String::new(),
            category: None,
            temp: false,
        };
        let env = Ctx {
            calc: self,
            src: None,
        };
        for v in saved {
            let Some(rest) = v.strip_prefix("BONUS|") else {
                continue;
            };
            let Ok(b) = Bonus::parse(rest) else { continue };
            if !b.prereqs().all(|p| self.passes(p, None)) {
                continue;
            }
            let Ok(e) = b.expr() else { continue };
            let val = eval(&e, &env);
            let btype = b.opts.iter().find_map(|o| match o {
                BonusOpt::Type(t) => Some(t.as_str()),
                _ => None,
            });
            for tg in &b.targets {
                table.add_from(&b.category, tg, btype, val, &origin);
            }
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
            // one point per five pounds carried
            Some("WEIGHT") => -(self.carried_weight() / 5.0).floor(),
            _ => 0.0,
        };
        (self.skill_ranks(&sk.key) + b).floor() as i64
    }

    /// Armor class of a game-mode AC type (Total, Flatfooted, ...).
    pub fn ac_of_type(&self, name: &str) -> i64 {
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

    /// The object's prerequisites that the character fails (after
    /// [`Calc::run`]), as written.
    pub fn failing_prereqs(&self, obj: &'a Object) -> Vec<String> {
        let src = self.src(SrcKind::Ability, obj);
        obj.tokens
            .iter()
            .filter(|t| Prereq::is_pre_tag(&t.tag))
            .filter(|t| match Prereq::from_token(&t.tag, &t.value) {
                Ok(p) => !self.passes(&p, Some(&src)),
                Err(_) => false,
            })
            .map(|t| t.text())
            .collect()
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
        sheet.equipped = self
            .items
            .iter()
            .map(|it| {
                let mods: Vec<&str> = it.eqmods.iter().map(|m| m.key.as_str()).collect();
                format!(
                    "{}: {} ({}) [{}]",
                    it.location,
                    it.name,
                    it.obj.key,
                    mods.join(", ")
                )
            })
            .collect();
        sheet.unknown = self.unknown.borrow().clone();
        sheet
    }
}

/// An ability grant waiting to be resolved: category, key, choices,
/// owning class, nature, and whether an object granted it (rather than
/// the character file).
type Grant = (
    String,
    String,
    Vec<String>,
    Option<String>,
    &'static str,
    bool,
);

/// Does a CSKILL-style list (`a|TYPE=b|ALL|LIST|%LIST`) include the
/// skill?
fn skill_list_has(ds: &Dataset, list: &str, sk: &Object, choices: &[String]) -> bool {
    let _ = ds;
    list.split('|').any(|item| {
        if let Some(ty) = item
            .strip_prefix("TYPE=")
            .or_else(|| item.strip_prefix("TYPE."))
        {
            sk.is_type(ty)
        } else if item == "%LIST" || item == "LIST" {
            choices.iter().any(|c| c.eq_ignore_ascii_case(&sk.key))
        } else {
            item.eq_ignore_ascii_case("ALL") || item.eq_ignore_ascii_case(&sk.key)
        }
    })
}

/// Does an object's CHOOSE offer this choice?  Only lists of explicit
/// names are checked; lists with types, wildcards or formulas offer
/// anything.
pub fn choose_allows(obj: &Object, choice: &str) -> bool {
    use crate::token::choice::{Item, SubToken};
    let Some(v) = obj.get("CHOOSE") else {
        return true;
    };
    let t = SubToken::parse(v);
    if !t.kind.eq_ignore_ascii_case("SKILL") {
        return true;
    }
    let items = t.selection();
    // types, keywords and formulas offer anything
    let open = items.is_empty() || items.iter().any(|i| !matches!(i, Item::Name(_)));
    open || items
        .iter()
        .any(|i| matches!(i, Item::Name(n) if n.eq_ignore_ascii_case(choice)))
}

/// The nature an ability is held with: NORMAL, AUTOMATIC or VIRTUAL.
fn nature_of(s: &str) -> &'static str {
    match s.to_ascii_uppercase().as_str() {
        "AUTOMATIC" => "AUTOMATIC",
        "VIRTUAL" => "VIRTUAL",
        _ => "NORMAL",
    }
}

/// Apply one HITDIE adjustment (`12`, `%/2`, `%*2`, `%+1`, `%-1`,
/// `%up1`, `%down1`) to a die size.
fn adjust_die(die: i64, spec: &str, sizes: &[(i64, Option<&str>)], lo: usize, hi: usize) -> i64 {
    let Some(rest) = spec.strip_prefix('%') else {
        return spec.parse().unwrap_or(die);
    };
    let num = |s: &str| s.parse::<i64>().ok();
    let lower_rest = rest.to_ascii_lowercase();
    let step = |n: i64| -> i64 {
        let Some(i) = sizes.iter().position(|(v, _)| *v == die) else {
            return die;
        };
        let j = (i as i64 + n).clamp(lo as i64, hi as i64) as usize;
        sizes[j].0
    };
    if let Some(n) = lower_rest.strip_prefix("up").and_then(num) {
        step(n)
    } else if let Some(n) = lower_rest.strip_prefix("down").and_then(num) {
        step(-n)
    } else if let Some(n) = rest.strip_prefix('/').and_then(num) {
        if n == 0 {
            die
        } else {
            die / n
        }
    } else if let Some(n) = rest.strip_prefix('*').and_then(num) {
        die * n
    } else if let Some(n) = rest.strip_prefix('+').and_then(num) {
        die + n
    } else if let Some(n) = rest.strip_prefix('-').and_then(num) {
        die - n
    } else {
        die
    }
}

/// Does an equipment location satisfy a PREEQUIP-family test?
fn hand_matches(kind: &str, location: &str) -> bool {
    let l = lower(location);
    match kind {
        "EQUIPPRIMARY" => l == "primary hand" || l == "natural-primary",
        "EQUIPSECONDARY" => l.starts_with("secondary hand") || l == "natural-secondary",
        "EQUIPBOTH" => l == "both hands",
        "EQUIPTWOWEAPON" => l == "double weapon" || l == "two weapons",
        _ => true,
    }
}

fn si_item(s: &Src<'_>) -> usize {
    s.item.unwrap_or(usize::MAX)
}

/// Evaluation context with extra terms ahead of the character's.
struct ExtraEnv<'c, 'a> {
    calc: &'c Calc<'a>,
    extra: &'c dyn Fn(&str) -> Option<f64>,
}

impl Env for ExtraEnv<'_, '_> {
    fn var(&self, name: &str) -> Option<f64> {
        (self.extra)(name)
            .or_else(|| (self.extra)(&upper(name)))
            .or_else(|| self.calc.var_value(name, None))
    }

    fn call(&self, name: &str, args: &[Expr]) -> Option<f64> {
        self.calc.call(name, args, None)
    }
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

/// Context for experience table formulas, where LEVEL is the level.
struct LevelEnv<'c, 'a> {
    calc: &'c Calc<'a>,
    level: f64,
}

impl Env for LevelEnv<'_, '_> {
    fn var(&self, name: &str) -> Option<f64> {
        if name.eq_ignore_ascii_case("LEVEL") {
            Some(self.level)
        } else {
            self.calc.var_value(name, None)
        }
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

/// The level a level line names ("3" or "3:REPEATLEVEL:..."), 0 if none.
fn level_number(v: &str) -> u32 {
    v.split(':')
        .next()
        .and_then(|x| x.trim().parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_die_adjustments() {
        let sizes: Vec<(i64, Option<&str>)> = vec![
            (1, None),
            (2, None),
            (4, Some("MIN")),
            (6, None),
            (8, None),
            (10, None),
            (12, Some("MAX")),
            (20, None),
        ];
        let a = |d, s| adjust_die(d, s, &sizes, 2, 6);
        assert_eq!(a(8, "%up1"), 10);
        assert_eq!(a(8, "%UP2"), 12);
        assert_eq!(a(12, "%up1"), 12);
        assert_eq!(a(6, "%down2"), 4);
        assert_eq!(a(8, "%/2"), 4);
        assert_eq!(a(8, "%/0"), 8);
        assert_eq!(a(8, "%*2"), 16);
        assert_eq!(a(8, "%+2"), 10);
        assert_eq!(a(8, "%-2"), 6);
        assert_eq!(a(8, "12"), 12);
        assert_eq!(a(7, "%up1"), 7);
        assert_eq!(a(8, "%bogus"), 8);
    }

    #[test]
    fn choices_split_from_names() {
        assert_eq!(
            split_choice("Weapon Focus (Longsword)"),
            ("Weapon Focus", Some("Longsword"))
        );
        assert_eq!(split_choice("Feat(Ride)"), ("Feat", Some("Ride")));
        assert_eq!(
            split_choice("Giving(Feat(Ride))"),
            ("Giving", Some("Feat(Ride)"))
        );
        assert_eq!(split_choice("Foo (a) (b)"), ("Foo (a)", Some("b")));
        assert_eq!(split_choice("Plain"), ("Plain", None));
        assert_eq!(split_choice("(odd)"), ("(odd)", None));
        assert_eq!(split_choice("Bad)"), ("Bad)", None));
    }

    #[test]
    fn choose_lists() {
        let mk = |choose: &str| Object {
            kind: FileKind::Ability,
            head_tag: None,
            name: "A".into(),
            key: "A".into(),
            category: None,
            tokens: vec![Tok {
                tag: "CHOOSE".into(),
                value: choose.into(),
                origin: crate::dataset::Origin { file: 0, line: 0 },
            }],
            sublines: Vec::new(),
            children: Vec::new(),
            source: Vec::new(),
            origin: crate::dataset::Origin { file: 0, line: 0 },
            history: Vec::new(),
        };
        let o = mk("SKILL|Ability Skill 2|Ability Skill 3|TITLE=Skill");
        assert!(choose_allows(&o, "Ability Skill 2"));
        assert!(!choose_allows(&o, "PLEASE MAKE APPROPRIATE SELECTION"));
        assert!(choose_allows(&mk("SKILL|TYPE=Craft"), "Anything"));
        assert!(choose_allows(&mk("SKILL|ALL"), "Anything"));
        assert!(choose_allows(&mk("NOCHOICE"), ""));
        assert!(choose_allows(&mk("FEAT|Alertness"), "Other"));
    }

    #[test]
    fn hand_locations() {
        assert!(hand_matches("EQUIPPRIMARY", "Primary Hand"));
        assert!(hand_matches("EQUIPSECONDARY", "Secondary Hand 2"));
        assert!(hand_matches("EQUIPBOTH", "Both Hands"));
        assert!(!hand_matches("EQUIPBOTH", "Shield"));
        assert!(hand_matches("EQUIP", "Shield"));
        assert!(hand_matches("EQUIPTWOWEAPON", "Double Weapon"));
    }
}
