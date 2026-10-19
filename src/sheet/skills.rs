//
// sheet/skills.rs -- the skills section
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use super::units::{float_text, trim_zeros};
use super::{exported, output_name, Element, SheetCtx};
use crate::dataset::Object;
use crate::lst::kind::FileKind;
use std::cmp::Ordering;
use std::collections::HashMap;

/// One skill as shown on the sheet.
struct Row<'a> {
    obj: &'a Object,
    name: String,
    ranks: f64,
    total: i64,
    stat_mod: i64,
    untrained: bool,
    exclusive: bool,
    keystat: Option<&'a str>,
}

/// Sort key for names: spaces are ignored, punctuation sorts before
/// digits and digits before letters; case only breaks ties.
pub fn name_order(a: &str, b: &str) -> Ordering {
    fn key(s: &str) -> Vec<(u8, char)> {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| {
                let class = if c.is_alphabetic() {
                    2
                } else if c.is_ascii_digit() {
                    1
                } else {
                    0
                };
                (class, c.to_lowercase().next().unwrap_or(c))
            })
            .collect()
    }
    fn case(s: &str) -> Vec<bool> {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .map(char::is_uppercase)
            .collect()
    }
    key(a)
        .cmp(&key(b))
        .then_with(|| case(a).cmp(&case(b)))
        .then_with(|| a.cmp(b))
}

fn cost(s: &SheetCtx, tag: &str, default: f64) -> f64 {
    s.ctx
        .gamemode
        .as_ref()
        .and_then(|g| g.misc(tag))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

pub fn build(s: &SheetCtx) -> Element {
    let c = &s.calc;
    let ds = &s.ctx.dataset;
    let entries = s.ch.skills();
    let assoc: HashMap<String, String> = entries
        .iter()
        .filter_map(|e| Some((e.name.to_lowercase(), e.associated.clone()?)))
        .collect();
    let has_class = !c.classes().is_empty();

    // the character's skills: every usable-untrained skill once the
    // character has a class, plus skills with ranks
    let mut rows: Vec<Row> = Vec::new();
    for o in ds.objects(FileKind::Skill) {
        let untrained = !o
            .get("USEUNTRAINED")
            .is_some_and(|v| v.eq_ignore_ascii_case("NO"));
        let exclusive = o
            .get("EXCLUSIVE")
            .is_some_and(|v| v.eq_ignore_ascii_case("YES"));
        let ranks = c.skill_ranks(&o.key);
        // an exclusive skill needs a cost for the character: class or
        // cross-class
        let class_skill = c.is_class_skill(&o.key) || c.is_cross_class_skill(&o.key);
        let member = ranks > 0.0 || has_class && untrained && (!exclusive || class_skill);
        if !member || !exported(o) {
            continue;
        }
        if exclusive && !class_skill && ranks <= 0.0 {
            continue;
        }
        if !c.failing_prereqs(o).is_empty() {
            continue;
        }
        let base = output_name(o);
        let name = match assoc.get(&o.name.to_lowercase()) {
            Some(a) => format!("{}({})", base, a),
            None => base,
        };
        let keystat = o.get("KEYSTAT");
        rows.push(Row {
            obj: o,
            name,
            ranks,
            total: s.sheet.skills.get(&o.key).copied().unwrap_or(0),
            stat_mod: keystat.map(|k| c.stat_mod(k) as i64).unwrap_or(0),
            untrained,
            exclusive,
            keystat,
        });
    }
    rows.sort_by(|a, b| name_order(&a.name, &b.name));

    let mut skills = Element::new("skills");

    // skill points: what is left from each class's levels plus what
    // the ranks bought cost, at each class's cost for the skill
    let mut pools: HashMap<String, f64> = HashMap::new();
    let retro = c.rule("RETROSKILL");
    let mut char_level = 0;
    for l in s.ch.pcg.lines_tagged("CLASSABILITIESLEVEL") {
        let Some(class) = l
            .head()
            .map(|h| h.value_text())
            .and_then(|h| h.rsplit_once('=').map(|(c, _)| c.to_string()))
        else {
            continue;
        };
        let mut left: f64 = l
            .text("SKILLSREMAINING")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0.0);
        char_level += 1;
        // where changes to skill points apply retroactively, a level
        // worth more (or less) now than when taken adjusts what is left
        if retro {
            let gained: Option<f64> = l.text("SKILLSGAINED").and_then(|v| v.trim().parse().ok());
            let cls = c
                .classes()
                .iter()
                .find(|(o, _)| o.key.eq_ignore_ascii_case(&class))
                .map(|(o, _)| *o);
            if let (Some(gained), Some(cls)) = (gained, cls) {
                let raw = c.skill_points_for_level(cls, char_level);
                if raw != gained {
                    left += raw - gained;
                }
            }
        }
        *pools.entry(class.to_lowercase()).or_default() += left;
    }
    let unused: f64 = pools.values().map(|v| v.max(0.0)).sum();
    let (cc, xc, ec) = (
        cost(s, "SKILLCOST_CLASS", 1.0),
        cost(s, "SKILLCOST_CROSSCLASS", 2.0),
        cost(s, "SKILLCOST_EXCLUSIVE", 0.0),
    );
    let mut used = 0.0;
    for e in &entries {
        let exclusive = ds
            .get(FileKind::Skill, &e.name)
            .and_then(|o| o.get("EXCLUSIVE"))
            .is_some_and(|v| v.eq_ignore_ascii_case("YES"));
        for (class, ranks) in &e.ranks {
            let per = if !class.eq_ignore_ascii_case("None") && c.is_class_skill_for(&e.name, class)
            {
                cc
            } else if exclusive && !c.is_cross_class_skill(&e.name) {
                ec
            } else {
                xc
            };
            used += ranks * per;
        }
    }
    let (unused, used) = (unused.trunc(), used.trunc());
    skills.push(
        Element::new("skillpoints")
            .with(Element::leaf("total", trim_zeros(unused + used)))
            .with(Element::leaf("used", trim_zeros(used)))
            .with(Element::leaf("unused", trim_zeros(unused))),
    );

    let mods: Vec<String> = rows
        .iter()
        .filter(|r| r.ranks > 0.0 || r.untrained)
        .filter(|r| {
            let m = r.total - r.ranks as i64;
            r.ranks as i64 > 0 || r.keystat.is_some() && m - r.stat_mod > 0
        })
        .map(|r| {
            let t = r.total;
            format!(
                "{} {}{}",
                output_name(r.obj),
                if t >= 0 { "+" } else { "" },
                t
            )
        })
        .collect();
    skills.add("list_mods", mods.join(", "));
    let level = c.total_level();
    skills.add(
        "max_class_skill_level",
        c.level_row_value(level, |r| r.class_skill_max.clone())
            .map(|v| v as i64)
            .unwrap_or(0),
    );
    skills.add(
        "max_cross_class_skill_level",
        c.level_row_value(level, |r| r.cross_class_skill_max.clone())
            .map(float_text)
            .unwrap_or_else(|| "0".into()),
    );

    for r in &rows {
        let int_ranks = r.ranks as i64;
        let m = r.total - int_ranks;
        let mut sk = Element::new("skill");
        sk.add("name", &r.name);
        sk.add("ranks", float_text(r.ranks));
        sk.add("mod", m);
        sk.add("skill_mod", r.total);
        sk.add("ability_mod", r.stat_mod);
        sk.add("misc_mod", m - r.stat_mod);
        sk.add("ability", r.keystat.unwrap_or(""));
        sk.add("synergy", &r.name);
        sk.add("untrained", if r.untrained { "Y" } else { "NO" });
        sk.add("exclusive", if r.exclusive { "Y" } else { "N" });
        let trained = if !r.untrained && int_ranks == 0 {
            0
        } else {
            r.total
        };
        let excl = if (r.exclusive || !r.untrained) && int_ranks == 0 {
            0
        } else {
            r.total
        };
        sk.add("trained_total", trained);
        sk.add("exclusive_total", excl);
        skills.push(sk);
    }
    skills
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_sort_ignoring_spaces() {
        let mut v = vec![
            "Test Crafting",
            "Test96h",
            "Test Coding Script",
            "Test96d",
            "Test Skill Chooser_1",
            "Test Skill (Class Level)",
            "Test96g",
        ];
        v.sort_by(|a, b| name_order(a, b));
        assert_eq!(
            v,
            vec![
                "Test96d",
                "Test96g",
                "Test96h",
                "Test Coding Script",
                "Test Crafting",
                "Test Skill (Class Level)",
                "Test Skill Chooser_1",
            ]
        );
    }

    #[test]
    fn case_breaks_ties_only() {
        assert_eq!(name_order("abc", "ABD"), Ordering::Less);
        assert_eq!(name_order("abc", "ABC"), Ordering::Less);
        assert_eq!(name_order("Bluff", "Bluff"), Ordering::Equal);
    }
}
