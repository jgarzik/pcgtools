//
// rules/pools.rs -- ability pools and the choices abilities offer
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! How many abilities of each category a character may choose
//! (BONUS:ABILITYPOOL) against how many it has chosen, and what an
//! ability's CHOOSE lets the character pick.

use super::calc::{Calc, SrcKind};
use crate::dataset::Object;
use crate::lst::kind::FileKind;
use crate::token::choice::{Item, SubToken};

/// An ability pool: a category with points to spend.
#[derive(Debug, Clone, PartialEq)]
pub struct Pool {
    pub category: String,
    /// points granted
    pub total: f64,
    /// points spent on chosen abilities
    pub spent: f64,
}

impl Pool {
    pub fn left(&self) -> f64 {
        self.total - self.spent
    }
}

/// Ability categories the data and game mode define.
fn categories(c: &Calc) -> Vec<String> {
    let mut out: Vec<String> =
        c.ds.objects(FileKind::AbilityCategory)
            .iter()
            .map(|o| o.key.clone())
            .collect();
    if let Some(g) = c.gm {
        for l in g.lines("miscinfo.lst", "ABILITYCATEGORY") {
            if !out.iter().any(|x| x.eq_ignore_ascii_case(&l.value)) {
                out.push(l.value.clone());
            }
        }
    }
    if !out.iter().any(|x| x.eq_ignore_ascii_case("FEAT")) {
        out.insert(0, "FEAT".into());
    }
    out
}

/// The cost of choosing an ability (its COST, default 1).
pub fn ability_cost(o: &Object) -> f64 {
    o.get("COST")
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(1.0)
}

/// Feats a character gains from its levels and race: the race's
/// STARTFEATS, one per LEVELSPERFEAT levels of classes that set it, and
/// the game mode's BONUSFEATLEVELSTARTINTERVAL:start|interval schedule
/// over the remaining levels (interval 0 grants only at the start).
pub fn level_feats(c: &Calc) -> f64 {
    let mut total = 0.0;
    for s in c.sources.iter().filter(|s| s.kind == SrcKind::Race) {
        total += s
            .obj
            .get("STARTFEATS")
            .and_then(|v| v.trim().parse::<f64>().ok())
            .unwrap_or(0.0);
    }
    let mut levels = 0u32;
    for (cls, n) in c.classes() {
        match cls
            .get("LEVELSPERFEAT")
            .and_then(|v| v.trim().parse::<u32>().ok())
        {
            Some(0) => {}
            Some(per) => total += (n / per) as f64,
            None => levels += n,
        }
    }
    if let Some(g) = c.gm {
        for l in g.lines("miscinfo.lst", "BONUSFEATLEVELSTARTINTERVAL") {
            let mut it = l.value.split('|').map(|x| x.trim().parse::<u32>().ok());
            let (Some(Some(start)), interval) = (it.next(), it.next().flatten()) else {
                continue;
            };
            total += feats_on_schedule(levels, start, interval.unwrap_or(0)) as f64;
        }
    }
    total
}

/// How many of levels 1..=levels fall on start, start+interval, ...
fn feats_on_schedule(levels: u32, start: u32, interval: u32) -> u32 {
    if start == 0 || levels < start {
        return 0;
    }
    (levels - start).checked_div(interval).unwrap_or(0) + 1
}

/// Pools with points granted or spent.
pub fn pools(c: &Calc) -> Vec<Pool> {
    let mut out = Vec::new();
    for cat in categories(c) {
        // points granted, and points added by hand
        let mut total = c.bonus.total("ABILITYPOOL", &cat) + c.ch.user_pool(&cat);
        if cat.eq_ignore_ascii_case("FEAT") {
            total += level_feats(c);
        }
        let spent: f64 =
            c.ch.abilities()
                .iter()
                .filter(|a| {
                    a.nature.eq_ignore_ascii_case("NORMAL") && a.pool.eq_ignore_ascii_case(&cat)
                })
                .map(|a| {
                    let times = a.applied_to.len().max(1) as f64;
                    let cost =
                        c.ds.get_in(FileKind::Ability, &a.category, &a.key)
                            .map(ability_cost)
                            .unwrap_or(1.0);
                    cost * times
                })
                .sum();
        if total != 0.0 || spent != 0.0 {
            out.push(Pool {
                category: cat,
                total,
                spent,
            });
        }
    }
    out
}

/// What an object's CHOOSE offers, if it can be listed: None for free
/// text, numbers and choices this cannot enumerate.
pub fn candidates(c: &Calc, o: &Object) -> Option<Vec<String>> {
    let t = SubToken::parse(o.get("CHOOSE")?);
    let kind = t.kind.to_ascii_uppercase();
    let file_kind = match kind.as_str() {
        "SKILL" | "SKILLSNAMED" => FileKind::Skill,
        "LANG" => FileKind::Language,
        "WEAPONPROFICIENCY" | "WEAPONPROFS" => FileKind::WeaponProf,
        "ARMORPROFICIENCY" => FileKind::ArmorProf,
        "SHIELDPROFICIENCY" => FileKind::ShieldProf,
        "CLASS" => FileKind::Class,
        "PCSTAT" | "STAT" => FileKind::Stat,
        "DOMAIN" => FileKind::Domain,
        "DEITY" => FileKind::Deity,
        "RACE" => FileKind::Race,
        "TEMPLATE" => FileKind::Template,
        "SPELLS" => FileKind::Spell,
        "EQUIPMENT" => FileKind::Equipment,
        "ALIGNMENT" => FileKind::Alignment,
        "SIZE" => FileKind::Size,
        "CHECK" => FileKind::Save,
        _ => return None,
    };
    let objs = c.ds.objects(file_kind);
    let mut out: Vec<String> = Vec::new();
    let mut add = |k: &str| {
        if !out.iter().any(|x| x.eq_ignore_ascii_case(k)) {
            out.push(k.to_string());
        }
    };
    for item in t.selection() {
        match item {
            Item::Name(n) => {
                if objs.iter().any(|o| o.key.eq_ignore_ascii_case(n)) {
                    add(n);
                }
            }
            Item::Keyword(k) if k == "ALL" || k == "ANY" => {
                for o in objs {
                    add(&o.key);
                }
            }
            Item::Type { types, .. } => {
                for o in objs
                    .iter()
                    .filter(|o| types.split('.').all(|t| o.is_type(t)))
                {
                    add(&o.key);
                }
            }
            // keywords and narrowed or combined selections need the
            // character's state; offer anything
            _ => return None,
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dataset::{Origin, Tok};

    #[test]
    fn costs() {
        let mut o = Object {
            kind: FileKind::Ability,
            head_tag: None,
            name: "A".into(),
            key: "A".into(),
            category: None,
            tokens: Vec::new(),
            sublines: Vec::new(),
            children: Vec::new(),
            source: Vec::new(),
            origin: Origin { file: 0, line: 0 },
            history: Vec::new(),
        };
        assert_eq!(ability_cost(&o), 1.0);
        o.tokens.push(Tok {
            tag: "COST".into(),
            value: "2".into(),
            origin: Origin { file: 0, line: 0 },
        });
        assert_eq!(ability_cost(&o), 2.0);
        let p = Pool {
            category: "FEAT".into(),
            total: 3.0,
            spent: 2.0,
        };
        assert_eq!(p.left(), 1.0);
    }

    #[test]
    fn feat_schedule() {
        // a feat at 3 and every 2 levels after
        assert_eq!(feats_on_schedule(1, 3, 2), 0);
        assert_eq!(feats_on_schedule(3, 3, 2), 1);
        assert_eq!(feats_on_schedule(4, 3, 2), 1);
        assert_eq!(feats_on_schedule(5, 3, 2), 2);
        assert_eq!(feats_on_schedule(20, 3, 2), 9);
        // only at the start
        assert_eq!(feats_on_schedule(1, 1, 0), 1);
        assert_eq!(feats_on_schedule(9, 1, 0), 1);
        assert_eq!(feats_on_schedule(0, 1, 0), 0);
        assert_eq!(feats_on_schedule(5, 0, 2), 0);
    }
}
