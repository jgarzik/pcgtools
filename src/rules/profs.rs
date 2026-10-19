//
// rules/profs.rs -- weapon, armor and shield proficiencies
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! The proficiencies a character has, from what it has been granted:
//! AUTO:WEAPONPROF, AUTO:ARMORPROF and AUTO:SHIELDPROF tokens (by
//! name, by type, by choice), deity weapons, natural attacks and
//! proficiency types changed by CHANGEPROF.

use super::calc::Calc;
use crate::dataset::Object;
use crate::lst::kind::FileKind;
use crate::token::pre::Prereq;

/// Keys of the weapon proficiencies the character has.
pub fn weapon_prof_keys(c: &Calc) -> Vec<String> {
    let ds = c.ds;
    let mut keys: Vec<String> = Vec::new();
    let mut add = |k: &str| {
        if !keys.iter().any(|x| x.eq_ignore_ascii_case(k)) {
            keys.push(k.to_string());
        }
    };
    // the saved list is a record, except for proficiencies chosen
    // from an object's bonus choices (saved with their SOURCE)
    for l in c.ch.pcg.lines_tagged("WEAPONPROF") {
        if l.get("SOURCE").is_none() {
            continue;
        }
        if let Some(h) = l.head() {
            for e in h.children().iter().filter(|e| e.tag == "WEAPON") {
                add(&e.value_text());
            }
        }
    }
    // proficiency types, as changed by CHANGEPROF (`TYPE.x=Simple`,
    // `Key=Martial`)
    let mut changed: Vec<(String, String)> = Vec::new();
    for src in &c.sources {
        for t in src.tokens.iter().filter(|t| t.tag == "CHANGEPROF") {
            for part in t.value.split('|') {
                if let Some((what, to)) = part.split_once('=') {
                    for w in what.split(',') {
                        changed.push((w.to_string(), to.to_string()));
                    }
                }
            }
        }
    }
    let prof_types = |o: &Object| -> Vec<String> {
        let mut t: Vec<String> = Vec::new();
        let mut moved = false;
        for (what, to) in &changed {
            let hit = match what
                .strip_prefix("TYPE.")
                .or_else(|| what.strip_prefix("TYPE="))
            {
                Some(ty) => o.is_type(ty),
                None => o.key.eq_ignore_ascii_case(what),
            };
            if hit {
                t.push(to.clone());
                moved = true;
            }
        }
        if !moved {
            t.extend(o.types().iter().map(|x| x.to_string()));
        }
        t
    };
    for src in &c.sources {
        for t in src.tokens.iter().filter(|t| t.tag == "AUTO") {
            let Some(rest) = t.value.strip_prefix("WEAPONPROF|") else {
                continue;
            };
            let items: Vec<&str> = rest.split('|').collect();
            let pres_ok = items
                .iter()
                .filter(|i| {
                    Prereq::is_pre_tag(i.split(':').next().unwrap_or("")) || i.starts_with("!PRE")
                })
                .filter_map(|i| Prereq::parse(i).ok())
                .all(|p| c.passes(&p, Some(src)));
            if !pres_ok {
                continue;
            }
            for item in items {
                if Prereq::is_pre_tag(item.split(':').next().unwrap_or(""))
                    || item.starts_with("!PRE")
                {
                    continue;
                }
                if let Some(ty) = item
                    .strip_prefix("TYPE=")
                    .or_else(|| item.strip_prefix("TYPE."))
                {
                    for o in ds.objects(FileKind::WeaponProf) {
                        let types = prof_types(o);
                        if ty
                            .split('.')
                            .all(|t| types.iter().any(|x| x.eq_ignore_ascii_case(t)))
                        {
                            add(&o.key);
                        }
                    }
                    // the unarmed strike is a simple weapon even when the
                    // data does not define it
                    if ty.eq_ignore_ascii_case("Simple")
                        && ds.get(FileKind::WeaponProf, "Unarmed Strike").is_none()
                    {
                        add("Unarmed Strike");
                    }
                } else if item == "%LIST" {
                    for ch in &src.choices {
                        add(ch);
                    }
                } else if item.eq_ignore_ascii_case("DEITYWEAPONS") {
                    if let Some(d) = c.deity() {
                        for w in d.values("DEITYWEAP").flat_map(|v| v.split('|')) {
                            add(w);
                        }
                    }
                } else {
                    add(item);
                }
            }
        }
    }
    // natural attacks are proficiencies of their own
    for n in super::equip::natural_weapons(c) {
        add(&n.name);
    }
    keys
}

/// Armor (or shield) proficiency grants: (names, item types) from
/// AUTO:ARMORPROF / AUTO:SHIELDPROF whose prerequisites pass.
pub fn armor_grants(c: &Calc, shield: bool) -> (Vec<String>, Vec<String>) {
    let prefix = if shield { "SHIELDPROF|" } else { "ARMORPROF|" };
    let mut names = Vec::new();
    let mut types = Vec::new();
    for src in &c.sources {
        for t in src.tokens.iter().filter(|t| t.tag == "AUTO") {
            let Some(rest) = t.value.strip_prefix(prefix) else {
                continue;
            };
            let items: Vec<&str> = rest.split('|').collect();
            let is_pre = |i: &str| {
                Prereq::is_pre_tag(i.split(':').next().unwrap_or("")) || i.starts_with("!PRE")
            };
            let ok = items
                .iter()
                .filter(|i| is_pre(i))
                .filter_map(|i| Prereq::parse(i).ok())
                .all(|p| c.passes(&p, Some(src)));
            if !ok {
                continue;
            }
            for i in items.iter().filter(|i| !is_pre(i)) {
                if let Some(ty) = i
                    .strip_prefix("ARMORTYPE=")
                    .or_else(|| i.strip_prefix("SHIELDTYPE="))
                    .or_else(|| i.strip_prefix("TYPE="))
                    .or_else(|| i.strip_prefix("TYPE."))
                {
                    types.push(ty.to_string());
                } else if *i == "%LIST" {
                    names.extend(src.choices.iter().cloned());
                } else {
                    names.push(i.to_string());
                }
            }
        }
    }
    (names, types)
}

/// Is the character proficient with a worn armor or shield, given its
/// proficiency name and the item's types?
pub fn armor_proficient(c: &Calc, shield: bool, prof: &str, item_types: &[String]) -> bool {
    let (names, types) = armor_grants(c, shield);
    let kind = if shield {
        FileKind::ShieldProf
    } else {
        FileKind::ArmorProf
    };
    let obj: Option<&Object> = c.ds.get(kind, prof);
    names.iter().any(|n| n.eq_ignore_ascii_case(prof))
        || types.iter().any(|t| {
            item_types.iter().any(|x| x.eq_ignore_ascii_case(t))
                || obj.is_some_and(|o| o.is_type(t))
        })
}
