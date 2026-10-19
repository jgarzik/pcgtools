//
// sheet/lists.rs -- feats, ability objects, special abilities,
// templates, domains, proficiencies and other lists
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use super::skills::name_order;
use super::units::pattern;
use super::{describe, describe_with, output_name, Element, SheetCtx};
use crate::dataset::Object;
use crate::lst::kind::FileKind;
use crate::rules::calc::{Src, SrcKind};
use crate::token::pre::Prereq;
use std::collections::BTreeMap;

/// How an object is shown: VISIBLE:YES (default), NO, EXPORT, DISPLAY.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Vis {
    Default,
    Hidden,
    OutputOnly,
    DisplayOnly,
}

fn visibility(o: &Object) -> Vis {
    match o.get("VISIBLE").map(|v| v.to_ascii_uppercase()).as_deref() {
        Some("NO") => Vis::Hidden,
        Some("EXPORT") => Vis::OutputOnly,
        Some("DISPLAY") => Vis::DisplayOnly,
        _ => Vis::Default,
    }
}

fn shown(v: Vis, hidden: bool) -> bool {
    if hidden {
        matches!(v, Vis::Hidden | Vis::DisplayOnly)
    } else {
        matches!(v, Vis::Default | Vis::OutputOnly)
    }
}

/// An ability as listed: the object and every selection made for it
/// under one nature.
struct Listed<'a> {
    obj: &'a Object,
    selections: Vec<String>,
}

/// The parent category of an ability category.
fn parent_category(s: &SheetCtx, cat: &str) -> String {
    s.ctx
        .dataset
        .get(FileKind::AbilityCategory, cat)
        .and_then(|o| o.get("CATEGORY"))
        .unwrap_or(cat)
        .to_string()
}

/// Is a recorded ability entry (key possibly renamed since) this object?
fn recorded_as(s: &SheetCtx, a: &crate::pcg::character::AbilityEntry, o: &Object) -> bool {
    if a.key.eq_ignore_ascii_case(&o.key) {
        return true;
    }
    let m = s
        .calc
        .migrate("ABILITY", &format!("{}|{}", a.category, a.key));
    m.rsplit_once('|')
        .map(|(_, k)| k)
        .unwrap_or(&m)
        .eq_ignore_ascii_case(&o.key)
}

/// Was the ability recorded in the character file from a pool that
/// cannot be spent: one with EDITPOOL:NO, or one the loaded data does
/// not define?
fn fixed_pool(s: &SheetCtx, src: &Src) -> bool {
    s.ch.abilities().iter().any(|a| {
        if !recorded_as(s, a, src.obj) || a.applied_to != src.applied {
            return false;
        }
        pool_editpool(s, &a.pool).is_some_and(|v| v.eq_ignore_ascii_case("NO"))
    })
}

/// EDITPOOL of an ability category (dataset or game mode), Some("")
/// for a category without one, None for an undefined category.
fn pool_editpool(s: &SheetCtx, pool: &str) -> Option<String> {
    // the feat category always exists
    if pool.eq_ignore_ascii_case("FEAT") {
        return Some(String::new());
    }
    if let Some(o) = s.ctx.dataset.get(FileKind::AbilityCategory, pool) {
        return Some(o.get("EDITPOOL").unwrap_or("").to_string());
    }
    s.ctx.gamemode.as_ref().and_then(|g| {
        g.lines("miscinfo.lst", "ABILITYCATEGORY")
            .find(|l| l.value.eq_ignore_ascii_case(pool))
            .map(|l| l.get("EDITPOOL").unwrap_or("").to_string())
    })
}

/// Abilities of a category (by parent category), nature and
/// visibility, in display order.
fn abilities<'a>(s: &SheetCtx<'a>, cat: &str, nature: &str, hidden: bool) -> Vec<Listed<'a>> {
    let mut by_key: BTreeMap<String, Listed<'a>> = BTreeMap::new();
    for src in s.calc.sources.iter().filter(|x| x.kind == SrcKind::Ability) {
        if src.nature.as_deref() != Some(nature) {
            continue;
        }
        let scat = src
            .category
            .clone()
            .or_else(|| src.obj.category.clone())
            .unwrap_or_default();
        if !parent_category(s, &scat).eq_ignore_ascii_case(cat) {
            continue;
        }
        if !shown(visibility(src.obj), hidden) {
            continue;
        }
        // a recorded choice from a pool that cannot be spent, which the
        // character is also granted outright, is listed only as granted
        if nature == "NORMAL"
            && fixed_pool(s, src)
            && s.calc.sources.iter().any(|x| {
                x.kind == SrcKind::Ability
                    && std::ptr::eq(x.obj, src.obj)
                    && x.nature.as_deref() == Some("AUTOMATIC")
                    && x.applied == src.applied
            })
        {
            continue;
        }
        let e = by_key
            .entry(src.obj.key.to_lowercase())
            .or_insert_with(|| Listed {
                obj: src.obj,
                selections: Vec::new(),
            });
        e.selections.extend(src.applied.iter().cloned());
    }
    let mut v: Vec<Listed> = by_key.into_values().collect();
    v.sort_by(|a, b| {
        let k = |o: &Object| {
            o.get("SORTKEY")
                .map(str::to_string)
                .unwrap_or(o.name.clone())
        };
        name_order(&k(a.obj), &k(b.obj))
            .then_with(|| name_order(&a.obj.name, &b.obj.name))
            .then_with(|| a.obj.key.cmp(&b.obj.key))
    });
    v
}

/// Selections as shown: chosen abilities appear as `Name (choice)`.
fn display_selections(s: &SheetCtx, l: &Listed) -> Vec<String> {
    let choose = l.obj.get("CHOOSE").unwrap_or("");
    let kind = choose.split('|').next().unwrap_or("").to_ascii_uppercase();
    let abilities = kind == "ABILITYSELECTION" || kind == "FEATSELECTION";
    l.selections
        .iter()
        .map(|x| {
            // abilities taken without a choice have empty selections
            if kind == "NOCHOICE" {
                return String::new();
            }
            // weapon proficiencies show their output names, alignments
            // their names
            if kind.starts_with("WEAPONPROF") {
                if let Some(o) = s.ctx.dataset.get(FileKind::WeaponProf, x) {
                    return output_name(o);
                }
            }
            let named = match kind.as_str() {
                "ALIGNMENT" => Some(FileKind::Alignment),
                "SIZE" => Some(FileKind::Size),
                _ => None,
            };
            if let Some(o) = named.and_then(|k| s.ctx.dataset.get(k, x)) {
                return o.name.clone();
            }
            // spell levels: CLASS.Wizard;LEVEL.1 shows as `Wizard 1`
            if kind == "SPELLLEVEL" {
                if let Some((c, lv)) = x
                    .strip_prefix("CLASS.")
                    .and_then(|r| r.split_once(";LEVEL."))
                {
                    return format!("{} {}", c, lv);
                }
            }
            if !abilities {
                return x.clone();
            }
            match x.find('(') {
                Some(i) if i > 0 && !x[..i].ends_with(' ') && x.ends_with(')') => {
                    format!("{} {}", &x[..i], &x[i..])
                }
                _ => x.clone(),
            }
        })
        .collect()
}

/// Name with its selections: `Weapon Focus (Longsword, Short Sword)`;
/// abilities chosen without a value show how many times: `(3x)`.
fn qualified_name(s: &SheetCtx, l: &Listed) -> String {
    let base = output_name(l.obj);
    let Some(choose) = l.obj.get("CHOOSE") else {
        return base;
    };
    let n = l.selections.len();
    let shown = if choose.starts_with("NOCHOICE") {
        if n > 1 {
            format!("{}x", n)
        } else {
            String::new()
        }
    } else {
        let mut sel = display_selections(s, l);
        sel.sort();
        sel.join(", ")
    };
    if shown.is_empty() {
        base
    } else {
        format!("{} ({})", base, shown)
    }
}

fn ability_fields(s: &SheetCtx, e: &mut Element, l: &Listed) {
    // choices only exist for abilities that ask for them
    let mut sel = if l.obj.has("CHOOSE") {
        display_selections(s, l)
    } else {
        Vec::new()
    };
    sel.sort();
    let choices: Vec<String> = l
        .selections
        .iter()
        .filter(|x| !x.is_empty())
        .cloned()
        .collect();
    e.add("name", qualified_name(s, l));
    e.push(super::paragraphs(
        "description",
        &describe_with(s, l.obj, &choices),
    ));
    let types: Vec<String> = l.obj.types().iter().map(|t| t.to_uppercase()).collect();
    e.add("type", types.join("."));
    e.add("associated", sel.join(","));
    let count = if l.obj.has("CHOOSE") {
        l.selections.len()
    } else {
        0
    };
    e.add("count", count);
}

fn flag(b: bool) -> &'static str {
    if b {
        "T"
    } else {
        "F"
    }
}

pub fn feats(s: &SheetCtx) -> Element {
    let mut out = Element::new("feats");
    let groups = [
        ("NORMAL", false),
        ("AUTOMATIC", false),
        ("VIRTUAL", false),
        ("NORMAL", true),
        ("VIRTUAL", true),
        ("AUTOMATIC", true),
    ];
    for (nature, hidden) in groups {
        for l in abilities(s, "FEAT", nature, hidden) {
            let mut e = Element::new("feat");
            ability_fields(s, &mut e, &l);
            e.add("auto", flag(nature == "AUTOMATIC"));
            e.add("hidden", flag(hidden));
            e.add("virtual", flag(nature == "VIRTUAL"));
            out.push(e);
        }
    }
    out
}

pub fn ability_objects(s: &SheetCtx) -> Element {
    let all = ["NORMAL", "AUTOMATIC", "VIRTUAL"];
    let blocks: [(&str, &[&str], bool); 8] = [
        ("Special Ability", &all, false),
        ("Special Ability", &all, true),
        ("Salient Divine Ability", &["NORMAL"], false),
        ("Mutation", &["NORMAL"], false),
        ("Talent", &all, false),
        ("Talent", &all, true),
        ("Internal", &all, false),
        ("Internal", &all, true),
    ];
    let mut out = Element::new("ability_objects");
    for (cat, natures, hidden) in blocks {
        for nature in natures {
            for l in abilities(s, cat, nature, hidden) {
                let mut e = Element::new("ability_object");
                ability_fields(s, &mut e, &l);
                e.add("auto", flag(*nature == "AUTOMATIC"));
                e.add("hidden", flag(hidden));
                e.add("virtual", flag(*nature == "VIRTUAL"));
                e.add("category", cat);
                out.push(e);
            }
        }
    }
    out
}

/// SAB text with its variables filled in: each `%` takes the next
/// variable's value.  None when every variable is zero.
fn sab_text(s: &SheetCtx, src: &Src, value: &str, drop_all_zero: bool) -> Option<String> {
    let mut parts = value.split('|');
    let text = parts.next().unwrap_or("");
    let mut vars = Vec::new();
    for p in parts {
        if Prereq::is_pre_tag(p.split(':').next().unwrap_or("")) || p.starts_with("!PRE") {
            match Prereq::parse(p) {
                Ok(pr) if !s.calc.passes(&pr, Some(src)) => return None,
                _ => {}
            }
        } else {
            vars.push(p);
        }
    }
    let mut text = text.to_string();
    if text.contains("%CHOICE") {
        let mut c = src.choices.clone();
        c.sort();
        text = text.replace("%CHOICE", &c.join(", "));
    }
    if vars.is_empty() {
        return Some(text);
    }
    let vals: Vec<i64> = vars.iter().map(|v| s.calc.eval_formula(v) as i64).collect();
    if drop_all_zero && vals.iter().all(|v| *v == 0) {
        return None;
    }
    let mut out = String::new();
    let mut it = vals.iter();
    for ch in text.chars() {
        if ch == '%' {
            match it.next() {
                Some(v) => out.push_str(&v.to_string()),
                None => out.push('%'),
            }
        } else {
            out.push(ch);
        }
    }
    Some(out)
}

fn sabs(s: &SheetCtx, src: &Src) -> Vec<String> {
    let mut out = Vec::new();
    for t in src.tokens.iter().filter(|t| t.tag == "SAB") {
        if t.value == ".CLEAR" {
            out.clear();
            continue;
        }
        if let Some(x) = sab_text(s, src, &t.value, true) {
            if !x.is_empty() {
                out.push(x);
            }
        }
    }
    out
}

pub fn special_abilities(s: &SheetCtx) -> Element {
    let c = &s.calc;
    let mut all: Vec<String> = Vec::new();
    for src in &c.sources {
        if matches!(src.kind, SrcKind::Stat | SrcKind::Save | SrcKind::Skill) {
            continue;
        }
        all.extend(sabs(s, src));
    }
    all.sort();
    let mut out = Element::new("special_abilities");
    let mut i = 0;
    while i < all.len() {
        let mut j = i + 1;
        while j < all.len() && all[j] == all[i] {
            j += 1;
        }
        let name = if j - i > 1 {
            format!("{} ({})", all[i], j - i)
        } else {
            all[i].clone()
        };
        out.push(
            Element::new("ability")
                .with(Element::leaf("name", name))
                .with(Element::leaf("description", "")),
        );
        i = j;
    }
    let race: Vec<String> = c
        .sources
        .iter()
        .filter(|x| x.kind == SrcKind::Race)
        .flat_map(|x| x.obj.values("SAB"))
        .map(|v| v.split('|').next().unwrap_or("").to_string())
        .collect();
    out.add("race", race.join(", "));
    for entry in s.ch.classes() {
        let obj = s.ctx.dataset.get(FileKind::Class, &entry.name);
        let sub = obj.and_then(|o| c.subclass_of(o));
        let name = match (sub, obj) {
            (Some(sc), _) => sc.name.clone(),
            (None, Some(o)) => output_name(o),
            (None, None) => entry.name.clone(),
        };
        let mut list: Vec<(String, String)> = Vec::new();
        if let Some(src) = c
            .sources
            .iter()
            .find(|x| x.kind == SrcKind::Class && obj.is_some_and(|o| std::ptr::eq(o, x.obj)))
        {
            for t in src.tokens.iter().filter(|t| t.tag == "SAB") {
                let has_pct = t.value.split('|').next().unwrap_or("").contains('%');
                if let Some(x) = sab_text(s, src, &t.value, has_pct) {
                    let key = t.value.split('|').next().unwrap_or("").to_lowercase();
                    list.push((key, x));
                }
            }
        }
        list.sort_by(|a, b| a.0.cmp(&b.0));
        let texts: Vec<String> = list.into_iter().map(|(_, x)| x).collect();
        out.push(
            Element::new("class")
                .with(Element::leaf("class", name))
                .with(Element::leaf("ability", texts.join(", "))),
        );
    }
    out
}

pub fn templates(s: &SheetCtx) -> Option<Element> {
    let c = &s.calc;
    let mut temps: Vec<&Src> = c
        .sources
        .iter()
        .filter(|x| x.kind == SrcKind::Template)
        .filter(|x| shown(visibility(x.obj), false) && !x.obj.name.is_empty())
        .collect();
    if temps.is_empty() {
        return None;
    }
    temps.sort_by(|a, b| a.obj.key.cmp(&b.obj.key));
    let mut out = Element::new("templates");
    let names: Vec<String> = temps.iter().map(|t| output_name(t.obj)).collect();
    out.add("list", names.join(", "));
    let dr = c.damage_reduction();
    let stats = super::combat::stats(s);
    for t in &temps {
        let mut e = Element::new("template");
        e.add("name", output_name(t.obj));
        for abb in ["STR", "DEX", "CON", "INT", "WIS", "CHA"] {
            let tag = format!("{}mod", abb.to_lowercase());
            let Some(st) = stats.iter().find(|o| o.key.eq_ignore_ascii_case(abb)) else {
                e.add(&tag, 0);
                continue;
            };
            let v = c.bonus.raw_sum("STAT", &st.key, |src| {
                src.kind == "Template" && src.key == t.obj.key
            }) as i64;
            e.add(&tag, v);
        }
        let cr: f64 = t
            .obj
            .get("CR")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0.0);
        e.add("cr", pattern(cr, "#.######"));
        e.add("dr", &dr);
        e.add("feat", "");
        e.add("sa", sabs(s, t).join(", "));
        let sr: i64 = t
            .obj
            .get("SR")
            .map(|v| c.eval_formula(v) as i64)
            .unwrap_or(0);
        e.add("sr", sr);
        e.add("bonuslist", "");
        out.push(e);
    }
    Some(out)
}

pub fn domains(s: &SheetCtx) -> Option<Element> {
    let ds = &s.ctx.dataset;
    let mut doms: Vec<&Object> =
        s.ch.domains()
            .iter()
            .filter_map(|d| ds.get(FileKind::Domain, d))
            .collect();
    if doms.is_empty() {
        return None;
    }
    doms.sort_by(|a, b| a.key.cmp(&b.key));
    let mut out = Element::new("domains");
    for d in doms {
        let mut power = describe(s, d);
        if !power.is_empty()
            && d.get("DESCISPI")
                .is_some_and(|v| v.eq_ignore_ascii_case("YES"))
        {
            power = format!("<html><b><i>{}</i></b></html>", power);
        }
        out.push(
            Element::new("domain")
                .with(Element::leaf("name", output_name(d)))
                .with(Element::leaf("power", power)),
        );
    }
    Some(out)
}

pub fn weapon_proficiencies(s: &SheetCtx) -> Element {
    let ds = &s.ctx.dataset;
    let keys = weapon_prof_keys(s);
    // proficiencies the data does not define are dropped; the rest
    // show their output names
    let natural: Vec<String> = crate::rules::equip::natural_weapons(&s.calc)
        .into_iter()
        .map(|n| n.name)
        .collect();
    let mut items: Vec<(String, String)> = keys
        .iter()
        .filter_map(|k| match ds.get(FileKind::WeaponProf, k) {
            Some(o) => Some((o.key.to_lowercase(), output_name(o))),
            // made up for the character: the unarmed strike and natural
            // attacks
            None if k == "Unarmed Strike" || natural.contains(k) => {
                Some((k.to_lowercase(), k.clone()))
            }
            None => None,
        })
        .filter(|(_, n)| !n.is_empty())
        .collect();
    items.dedup();
    items.sort();
    let names: Vec<&str> = items.iter().map(|(_, n)| n.as_str()).collect();
    Element::leaf("weapon_proficiencies", names.join(", "))
}

/// Keys of the weapon proficiencies the character has.
pub fn weapon_prof_keys(s: &SheetCtx) -> Vec<String> {
    let ds = &s.ctx.dataset;
    let c = &s.calc;
    let mut keys: Vec<String> = Vec::new();
    let mut add = |k: &str| {
        if !keys.iter().any(|x| x.eq_ignore_ascii_case(k)) {
            keys.push(k.to_string());
        }
    };
    // the saved list is a record, except for proficiencies chosen
    // from an object's bonus choices (saved with their SOURCE)
    for l in s.ch.pcg.lines_tagged("WEAPONPROF") {
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
    for n in crate::rules::equip::natural_weapons(c) {
        add(&n.name);
    }
    keys
}

pub fn languages(s: &SheetCtx) -> Element {
    let ds = &s.ctx.dataset;
    let mut langs: Vec<(String, String)> =
        s.ch.languages()
            .into_iter()
            .map(|k| {
                let name = ds
                    .get(FileKind::Language, &k)
                    .map(|o| o.name.clone())
                    .unwrap_or_else(|| k.clone());
                (k.to_lowercase(), name)
            })
            .collect();
    langs.sort();
    langs.dedup_by(|a, b| a.0 == b.0);
    let names: Vec<&str> = langs.iter().map(|(_, n)| n.as_str()).collect();
    Element::leaf("languages", names.join(", "))
}

pub fn misc(s: &SheetCtx) -> Element {
    let mut out = Element::new("misc");
    for (tag, outer, inner) in [
        ("CHARACTERASSET", "funds", "fund"),
        ("CHARACTERCOMP", "companions", "companion"),
        ("CHARACTERMAGIC", "magics", "magic"),
    ] {
        let v = s.ch.pcg.value(tag).unwrap_or_default();
        let lines: Vec<&str> = v.split('\n').map(|l| l.trim_end_matches('\r')).collect();
        let text = lines.join("\n");
        let text = text.trim();
        if !text.is_empty() {
            out.push(Element::new(outer).with(Element::leaf(inner, text)));
        }
    }
    out
}

pub fn companions(s: &SheetCtx) -> Element {
    let mut out = Element::new("companions");
    for (node, ty) in [
        ("familiar", "FAMILIAR"),
        ("psicrystal", "Psicrystal"),
        ("mount", "SPECIAL MOUNT"),
        ("companion", "ANIMAL COMPANION"),
        ("follower", "FOLLOWER"),
    ] {
        for l in s.ch.pcg.lines_tagged("FOLLOWER") {
            if !l.text("TYPE").is_some_and(|t| t.eq_ignore_ascii_case(ty)) {
                continue;
            }
            let mut e = Element::new(node);
            for f in [
                "name",
                "race",
                "hp",
                "ac",
                "fortitude",
                "reflex",
                "willpower",
                "initiative_mod",
                "special_properties",
            ] {
                e.add(f, "");
            }
            e.add("attacks", "");
            out.push(e);
        }
    }
    out
}

pub fn class_features(s: &SheetCtx) -> Element {
    let c = &s.calc;
    let v = |n: &str| c.var(n).unwrap_or(0.0);
    let i = |n: &str| (v(n) as i64).to_string();
    let mut out = Element::new("class_features");
    if v("BardicMusicLevel") >= 1.0 {
        out.push(
            Element::new("bardic_music")
                .with(Element::leaf("uses_per_day", i("BardicMusicLevel")))
                .with(Element::leaf("effects", ""))
                .with(Element::leaf("text", "")),
        );
    }
    if v("BardicMusic") >= 1.0 {
        out.push(
            Element::new("bardic_music")
                .with(Element::leaf("uses_per_day", i("BardicMusic")))
                .with(Element::leaf("effects", ""))
                .with(Element::leaf("text", "")),
        );
    }
    if v("RageLVL") >= 1.0 {
        out.push(
            Element::new("rage")
                .with(Element::leaf("uses_per_day", i("RageDuration")))
                .with(Element::leaf("uses_per_day.title", "Rounds/day")),
        );
    }
    if v("RageTimes") >= 1.0 {
        let desc = format!(
            "The Barbarian gains +{} to Strength, +{} to Constitution, and a +{} morale bonus on Will saves, but suffers a {} penalty to AC for {} rounds. At the end of the rage, the barbarian is fatigued (-2 to Strength, -2 to Dexterity, can't charge or run) for the duration of that encounter. The barbarian can only rage once per encounter. Entering a rage takes no time itself, but the barbarian can only do it during his action.",
            i("RageStrBonus"),
            i("RageConBonus"),
            i("RageMorale"),
            i("RageACPenalty"),
            i("RageConBonus"),
        );
        out.push(
            Element::new("rage")
                .with(Element::leaf("uses_per_day", i("RageTimes")))
                .with(Element::leaf("uses_per_day.title", "Uses per day"))
                .with(Element::leaf("description", desc)),
        );
    }
    for t in turning(s) {
        out.push(t);
    }
    if v("StunningAttack") >= 1.0 {
        out.push(
            Element::new("stunning_fist")
                .with(Element::leaf("save_dc", i("StunDC")))
                .with(Element::leaf("uses_per_day", i("StunningAttack"))),
        );
    }
    if v("StunningFistAttack") >= 1.0 {
        out.push(
            Element::new("stunning_fist")
                .with(Element::leaf("save_dc", i("StunningFistDC")))
                .with(Element::leaf("uses_per_day", i("StunningFistAttack"))),
        );
    }
    if v("WholenessHpLVL") >= 1.0 {
        out.push(Element::new("wholeness_of_body").with(Element::leaf(
            "hp_per_day",
            (v("WholenessHpLVL") as i64) * 2,
        )));
    }
    if v("WholenessBody") >= 1.0 {
        out.push(
            Element::new("wholeness_of_body").with(Element::leaf("hp_per_day", i("WholenessBody"))),
        );
    }
    if v("TOTALPOWERPOINTS") >= 1.0 {
        let old = c.var("Manifester").is_some() || c.var("PsychicWarriorManifester").is_some();
        out.push(
            Element::new("psionics")
                .with(Element::leaf("type", if old { "3.0" } else { "3.5" }))
                .with(Element::leaf("base_pp", i("BASEPOWERPOINTS")))
                .with(Element::leaf("bonus_pp", i("BONUSPOWERPOINTS")))
                .with(Element::leaf("total_pp", i("TOTALPOWERPOINTS"))),
        );
    }
    if v("LayOnHands") >= 1.0 {
        out.push(Element::new("layonhands").with(Element::leaf("hp_per_day", i("LayOnHands"))));
    }
    if v("WildshapeTimes") >= 1.0 {
        out.push(
            Element::new("wildshape")
                .with(Element::leaf("uses_per_day", i("WildShapeTimes")))
                .with(Element::leaf(
                    "elemental_uses_per_day",
                    i("WildShapeElementalTimes"),
                ))
                .with(Element::leaf("duration", i("WildShapeDuration"))),
        );
    }
    if v("LeadershipScore") >= 1.0 {
        out.push(
            Element::new("leadership")
                .with(Element::leaf("score", i("LeadershipScore")))
                .with(Element::leaf(
                    "max_cohort_level",
                    super::units::float_text(v("LeadershipMaxCohortLvl")),
                )),
        );
    }
    out
}

/// Turning abilities: special abilities with a TurnType aspect.
fn turning(s: &SheetCtx) -> Vec<Element> {
    let mut out = Vec::new();
    for l in abilities_with_aspect(s, "TurnType") {
        let aspect = |name: &str| aspect_value(s, l, name).unwrap_or_default();
        let int = |name: &str| aspect(name).trim().parse::<f64>().unwrap_or(0.0) as i64;
        let check = int("TurnCheck");
        let mut e = Element::new("turning")
            .attr("kind", aspect("TurnKind").to_uppercase())
            .attr("type", aspect("TurnType").to_uppercase());
        e.add("level", int("TurnLevel"));
        e.add(
            "turn_check",
            format!("1d20{}{}", if check >= 0 { "+" } else { "" }, check),
        );
        e.add("damage", aspect("TurnDamage"));
        e.add("uses_per_day", int("TurnTimes"));
        e.add("notes", aspect("TurnNotes"));
        out.push(e);
    }
    out
}

fn abilities_with_aspect<'a>(s: &SheetCtx<'a>, aspect: &str) -> Vec<&'a Object> {
    let mut v: Vec<&Object> = Vec::new();
    for src in s.calc.sources.iter().filter(|x| x.kind == SrcKind::Ability) {
        let has = src.obj.values("ASPECT").any(|a| {
            a.split('|')
                .next()
                .is_some_and(|n| n.eq_ignore_ascii_case(aspect))
        });
        if has && !v.iter().any(|o| std::ptr::eq(*o, src.obj)) {
            v.push(src.obj);
        }
    }
    v
}

/// An aspect's text with `%1`.. filled in; the last aspect of the name
/// whose prerequisites pass wins.
fn aspect_value(s: &SheetCtx, o: &Object, name: &str) -> Option<String> {
    let mut found = None;
    for a in o.values("ASPECT") {
        let mut parts = a.split('|');
        if !parts.next().is_some_and(|n| n.eq_ignore_ascii_case(name)) {
            continue;
        }
        let text = parts.next().unwrap_or("");
        let mut vars = Vec::new();
        let mut ok = true;
        for p in parts {
            if Prereq::is_pre_tag(p.split(':').next().unwrap_or("")) || p.starts_with("!PRE") {
                if let Ok(pr) = Prereq::parse(p) {
                    ok &= s.calc.passes(&pr, None);
                }
            } else {
                vars.push(p);
            }
        }
        if !ok {
            continue;
        }
        let value = |n: usize| -> String {
            vars.get(n)
                .map(|f| (s.calc.eval_formula(f) as i64).to_string())
                .unwrap_or_default()
        };
        found = Some(super::substitute(&crate::pcg::decode(text), value));
    }
    found
}

pub fn prohibited_schools(s: &SheetCtx) -> Option<Element> {
    let mut set: Vec<String> = Vec::new();
    for (cls, lvl) in s.calc.classes() {
        if *lvl == 0 {
            continue;
        }
        for v in cls.values("PROHIBITED") {
            for x in v.split(',') {
                set.push(x.to_string());
            }
        }
        for l in s.ch.pcg.lines_tagged("CLASS") {
            if !l
                .head()
                .is_some_and(|h| h.value_text().eq_ignore_ascii_case(&cls.key))
            {
                continue;
            }
            if let Some(p) = l.text("PROHIBITED") {
                for x in p.split(',') {
                    if !x.is_empty() {
                        set.push(x.to_string());
                    }
                }
            }
        }
    }
    set.sort();
    set.dedup();
    (!set.is_empty()).then(|| Element::leaf("prohibited_schools", set.join(",")))
}
