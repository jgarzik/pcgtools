//
// sheet/basics.rs -- the basics section: identity, biography, classes
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use super::units::{pattern, Units};
use super::{output_name, paragraphs, signed, Element, SheetCtx};
use crate::dataset::Object;
use crate::lst::kind::FileKind;

pub fn build(s: &SheetCtx) -> Element {
    let ch = s.ch;
    let c = &s.calc;
    let ds = &s.ctx.dataset;
    let units = Units::from_gamemode(s.ctx.gamemode.as_ref());
    let pcg = |tag: &str| ch.pcg.value(tag).unwrap_or_default();
    let var = |name: &str| c.var(name).unwrap_or(0.0) as i64;

    let mut b = Element::new("basics");
    for _ in 0..4 {
        b.add("bonuses", "");
    }
    b.add("name", ch.name().unwrap_or_default());
    b.add("followerof", "");
    b.add("playername", pcg("PLAYERNAME"));
    b.add("action_points", var("ACTION"));
    b.add("age", pcg("AGE").trim().parse::<i64>().unwrap_or(0));
    b.push(alignment(s));
    b.add("bab", signed(s.sheet.bab));
    b.push(paragraphs("bio", &pcg("CHARACTERBIO")));
    b.add("birthplace", pcg("BIRTHPLACE"));
    b.add("catchphrase", pcg("CATCHPHRASE"));
    b.push(classes(s));
    if let Some(d) = c.deity() {
        b.push(deity(s, d));
    }
    b.push(paragraphs("description", &pcg("CHARACTERDESC")));

    let mut xp = Element::new("experience");
    xp.add("current", ch.experience());
    xp.add(
        "next_level",
        c.min_xp(c.ecl() + 1).map(|v| v as i64).unwrap_or(0),
    );
    xp.add("factor", "100%");
    xp.add("penalty", "0%");
    b.push(xp);
    b.push(Element::new("eyes").with(Element::leaf("color", pcg("EYECOLOR"))));
    b.push(
        Element::new("hair")
            .with(Element::leaf("color", pcg("HAIRCOLOR")))
            .with(Element::leaf("length", pcg("HAIRSTYLE"))),
    );
    b.push(Element::new("skin").with(Element::leaf("color", pcg("SKINCOLOR"))));
    b.add("cr", cr_text(s, c.challenge_rating()));
    b.push(face(s, &units));
    b.add("favoredlist", favored(s));
    b.add("followerlist", "");
    let gender = match pcg("GENDER").as_str() {
        "M" => "Male".to_string(),
        "F" => "Female".to_string(),
        "" => "Unknown".to_string(),
        g => g.to_string(),
    };
    b.push(
        Element::new("gender")
            .with(Element::leaf("long", &gender))
            .with(Element::leaf("short", &gender)),
    );
    b.add("handed", pcg("HANDED"));
    let h: i64 = pcg("HEIGHT").trim().parse().unwrap_or(0);
    b.push(
        Element::new("height")
            .with(Element::leaf("total", units.height(h)))
            .with(Element::leaf("feet", h / 12))
            .with(Element::leaf("inches", h % 12)),
    );
    b.add("hitdice", super::combat::hit_dice(s));
    b.add("interests", pcg("INTERESTS"));
    b.push(languages(s));
    b.add("location", pcg("LOCATION"));
    b.push(movement(s, &units));
    b.push(
        Element::new("personality")
            .with(Element::leaf("trait", pcg("PERSONALITYTRAIT1")))
            .with(Element::leaf("trait", pcg("PERSONALITYTRAIT2"))),
    );
    b.add("portrait", pcg("PORTRAIT"));
    b.add("phobias", pcg("PHOBIAS"));
    b.add("race", race_name(s));
    b.push(reach(s, &units));
    let region = pcg("REGION");
    b.add(
        "region",
        if region.is_empty() {
            "None".into()
        } else {
            region
        },
    );
    b.add("reputation", var("REPUTATION"));
    b.add("residence", pcg("RESIDENCE"));
    let size = c.size_key();
    let size_long = ds
        .get(FileKind::Size, &size)
        .map(|o| o.name.clone())
        .unwrap_or_default();
    b.push(
        Element::new("size")
            .with(Element::leaf("long", size_long))
            .with(Element::leaf("short", size)),
    );
    b.add("speechtendency", pcg("SPEECHPATTERN"));
    b.add("type", race_type(s));
    b.push(vision(s, &units));
    b.add("wealth", var("WEALTH"));
    let w: f64 = pcg("WEIGHT").trim().parse().unwrap_or(0.0);
    b.push(
        Element::new("weight")
            .with(Element::leaf("weight_unit", units.weight(w)))
            .with(Element::leaf("weight_nounit", units.weight_number(w))),
    );
    b.push(
        Element::new("poolpoints")
            .with(Element::leaf(
                "cost",
                pcg("POOLPOINTS").trim().parse::<i64>().unwrap_or(0),
            ))
            .with(Element::leaf("current", 0)),
    );
    b.push(notes(s));
    b
}

fn alignment(s: &SheetCtx) -> Element {
    let mut e = Element::new("alignment");
    let enabled = s
        .vars
        .controls
        .get("ALIGNMENTFEATURE")
        .is_none_or(|v| !v.eq_ignore_ascii_case("NO"));
    let (long, short) = if !enabled {
        (String::new(), String::new())
    } else {
        match s.ch.alignment() {
            Some(a) => {
                let name = s
                    .ctx
                    .dataset
                    .get(FileKind::Alignment, &a)
                    .map(|o| o.name.clone())
                    .unwrap_or_else(|| a.clone());
                (name, a)
            }
            None => ("None".into(), "None".into()),
        }
    };
    e.add("long", long);
    e.add("short", short);
    e
}

/// A FACT value of an object.
fn fact<'a>(o: &'a Object, name: &str) -> Option<&'a str> {
    o.values("FACT").find_map(|v| {
        let (k, val) = v.split_once('|')?;
        k.eq_ignore_ascii_case(name).then_some(val)
    })
}

fn classes(s: &SheetCtx) -> Element {
    let c = &s.calc;
    let ds = &s.ctx.dataset;
    let mut out = Element::new("classes");
    let mut short = Vec::new();
    for (i, entry) in s.ch.classes().iter().enumerate() {
        let obj = ds.get(FileKind::Class, &entry.name);
        let sub = obj.and_then(|o| c.subclass_of(o));
        let name = match (sub, obj) {
            (Some(sc), _) => sc.name.clone(),
            (None, Some(o)) => output_name(o),
            (None, None) => entry.name.clone(),
        };
        let abb = sub
            .or(obj)
            .and_then(|o| fact(o, "Abb"))
            .map(str::to_string)
            .unwrap_or_else(|| name.chars().take(3).collect());
        let level = if entry.level > 0 {
            entry.level.to_string()
        } else {
            String::new()
        };
        short.push(format!("{}{}", abb, level));
        let mut e = Element::new("class");
        e.add("name", &name);
        e.add("abbreviation", &abb);
        e.add("level", &level);
        e.add("bonuslist", &name);
        e.add("sequence", i);
        out.push(e);
    }
    out.add("levels_total", c.total_level());
    out.add("levels_ecl", c.ecl());
    out.add("shortform", short.join(","));
    out
}

fn deity(s: &SheetCtx, d: &Object) -> Element {
    let ds = &s.ctx.dataset;
    let mut e = Element::new("deity");
    e.add("name", output_name(d));
    e.add("alignment", d.get("ALIGN").unwrap_or(""));
    e.add("description", super::describe(s, d));
    let mut doms: Vec<String> = Vec::new();
    for v in d.values("DOMAINS") {
        if v == ".CLEAR" {
            doms.clear();
            continue;
        }
        let list = v.split('|').next().unwrap_or("");
        for k in list.split(',') {
            if k.eq_ignore_ascii_case("ALL") || k.eq_ignore_ascii_case("ANY") {
                doms.extend(ds.objects(FileKind::Domain).iter().map(|o| o.name.clone()));
                continue;
            }
            let name = ds
                .get(FileKind::Domain, k)
                .map(|o| o.name.clone())
                .unwrap_or_else(|| k.to_string());
            doms.push(name);
        }
    }
    doms.sort();
    doms.dedup();
    e.add("domainlist", doms.join(", "));
    let weapons: Vec<String> = d
        .values("DEITYWEAP")
        .flat_map(|v| v.split('|'))
        .map(|w| {
            if w.eq_ignore_ascii_case("ALL") {
                "ANY".to_string()
            } else {
                w.to_string()
            }
        })
        .collect();
    e.add("favoredweapon", weapons.join("|"));
    e.add("holyitem", fact(d, "Symbol").unwrap_or(""));
    let mut pantheons: Vec<&str> = d
        .values("FACTSET")
        .filter_map(|v| {
            let (k, rest) = v.split_once('|')?;
            k.eq_ignore_ascii_case("Pantheon").then_some(rest)
        })
        .flat_map(|r| r.split('|'))
        .collect();
    pantheons.sort();
    pantheons.dedup();
    e.add("pantheonlist", pantheons.join(", "));
    e.add("source", super::source_text(s, d));
    e.add("special_abilities", "");
    e.add("appearance", fact(d, "Appearance").unwrap_or(""));
    e.add("title", fact(d, "Title").unwrap_or(""));
    e.add("worshippers", fact(d, "Worshippers").unwrap_or(""));
    e
}

fn cr_text(s: &SheetCtx, cr: Option<f64>) -> String {
    let Some(cr) = cr else {
        return "0".into();
    };
    if cr >= 1.0 {
        return (cr as i64).to_string();
    }
    // zero and below are listed as CRSTEPS (1/2|1/3|...), from 0 down
    let steps = s
        .ctx
        .gamemode
        .as_ref()
        .and_then(|g| g.misc("CRSTEPS"))
        .unwrap_or("");
    steps
        .split('|')
        .map(str::trim)
        .nth((-cr) as usize)
        .unwrap_or("")
        .to_string()
}

/// Face of the character's space, as (width, depth) in feet.
fn face_size(s: &SheetCtx) -> (f64, f64) {
    let c = &s.calc;
    let mut face = (5.0, 0.0);
    for src in c.sources.iter().filter(|x| {
        matches!(
            x.kind,
            crate::rules::calc::SrcKind::Race | crate::rules::calc::SrcKind::Template
        )
    }) {
        if let Some(v) = src.obj.get("FACE") {
            let mut it = v.split(',').map(|x| x.trim().parse::<f64>().unwrap_or(0.0));
            face = (it.next().unwrap_or(5.0), it.next().unwrap_or(0.0));
        }
    }
    if let Some(var) = s.vars.controls.get("FACE") {
        if let Some(v) = s
            .vars
            .get(&crate::rules::vars::system::Owner::global(), var)
        {
            let t = v.to_string();
            if let Some((a, b)) = t.split_once(',') {
                face = (
                    a.trim().parse().unwrap_or(face.0),
                    b.trim().parse().unwrap_or(face.1),
                );
            }
        }
    }
    face
}

fn square_size(s: &SheetCtx) -> f64 {
    s.ctx
        .gamemode
        .as_ref()
        .and_then(|g| g.misc("SQUARESIZE"))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(5.0)
}

fn face(s: &SheetCtx, units: &Units) -> Element {
    let (x, y) = face_size(s);
    let sq = square_size(s);
    let has_control = s.vars.controls.get("FACE").is_some();
    let long = if y == 0.0 {
        units.distance(x)
    } else {
        format!("{} by {}", units.distance(x), units.distance(y))
    };
    let short = if y == 0.0 || has_control && x == y {
        units.distance(x)
    } else {
        format!("{} x {}", units.distance(x), units.distance(y))
    };
    let squares = if y == 0.0 {
        pattern(x / sq, "#.#")
    } else {
        format!("{} x {}", pattern(x / sq, "#.#"), pattern(y / sq, "#.#"))
    };
    Element::new("face")
        .with(Element::leaf("face", long))
        .with(Element::leaf("short", short))
        .with(Element::leaf("squares", squares))
}

/// Favored classes from the race (FAVCLASS) and templates
/// (FAVOREDCLASS): `Any` for HIGHESTLEVELCLASS, else the class keys
/// in order, run together.
fn favored(s: &SheetCtx) -> String {
    use crate::rules::calc::SrcKind;
    let pcg = &s.ch.pcg;
    let mut keys: Vec<String> = Vec::new();
    for src in &s.calc.sources {
        let tag = match src.kind {
            SrcKind::Race => "FAVCLASS",
            SrcKind::Template => "FAVOREDCLASS",
            _ => continue,
        };
        // choices recorded for %LIST
        let applied = || -> Vec<String> {
            let found = match src.kind {
                SrcKind::Race => pcg
                    .lines_tagged("RACE")
                    .next()
                    .and_then(|l| l.text("APPLIEDTO")),
                _ => pcg.lines_tagged("TEMPLATESAPPLIED").find_map(|l| {
                    let h = l.head()?;
                    let name = h.children().iter().find(|c| c.tag == "NAME")?.value_text();
                    if !name.eq_ignore_ascii_case(&src.obj.key) {
                        return None;
                    }
                    h.children()
                        .iter()
                        .find(|c| c.tag == "APPLIEDTO")
                        .map(|c| c.value_text())
                }),
            };
            found
                .map(|v| v.split(',').map(str::to_string).collect())
                .unwrap_or_default()
        };
        for v in src.obj.values(tag) {
            for k in v.split('|') {
                if k.eq_ignore_ascii_case("HIGHESTLEVELCLASS") || k.eq_ignore_ascii_case("ANY") {
                    return "Any".into();
                }
                if k == "%LIST" {
                    keys.extend(applied());
                } else {
                    keys.push(k.to_string());
                }
            }
        }
    }
    let sort_key = |k: &String| k.rsplit('.').next().unwrap_or(k).to_string();
    keys.sort_by_key(sort_key);
    keys.dedup();
    keys.concat()
}

fn languages(s: &SheetCtx) -> Element {
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
    let mut e = Element::new("languages");
    for (_, n) in &langs {
        e.add("language", n);
    }
    let all: Vec<&str> = langs.iter().map(|(_, n)| n.as_str()).collect();
    e.add("all", all.join(", "));
    e
}

fn movement(s: &SheetCtx, units: &Units) -> Element {
    let c = &s.calc;
    let sq = square_size(s);
    let mut moves = c.movement_named();
    moves.sort_by(|a, b| {
        let walk = |n: &str| !n.eq_ignore_ascii_case("Walk");
        walk(&a.0).cmp(&walk(&b.0)).then(a.0.cmp(&b.0))
    });
    let mut e = Element::new("move");
    let mut all = Vec::new();
    for (name, feet) in &moves {
        let rate = units.distance(*feet);
        all.push(format!("{} {}", name, rate));
        e.push(
            Element::new("move")
                .with(Element::leaf("name", name))
                .with(Element::leaf("rate", &rate))
                .with(Element::leaf("squares", (feet / sq) as i64)),
        );
    }
    e.add("all", all.join(", "));
    e
}

fn race_name(s: &SheetCtx) -> String {
    let Some(r) = s.ch.race() else {
        return String::new();
    };
    let mut name = s
        .ctx
        .dataset
        .get(FileKind::Race, &r)
        .map(output_name)
        .unwrap_or(r);
    for src in s
        .calc
        .sources
        .iter()
        .filter(|x| x.kind == crate::rules::calc::SrcKind::Template)
    {
        if let Some(sub) = src.obj.get("SUBRACE") {
            let sub = if sub.eq_ignore_ascii_case("YES") {
                src.obj.name.as_str()
            } else {
                sub
            };
            name = format!("{} ({})", name, sub);
        }
    }
    name
}

fn race_type(s: &SheetCtx) -> String {
    s.calc.race_type().unwrap_or_else(|| "None".into())
}

fn reach(s: &SheetCtx, units: &Units) -> Element {
    let c = &s.calc;
    let mut r = 5.0;
    for src in c.sources.iter().filter(|x| {
        matches!(
            x.kind,
            crate::rules::calc::SrcKind::Race | crate::rules::calc::SrcKind::Template
        )
    }) {
        if let Some(v) = src.obj.get("REACH") {
            r = v.trim().parse().unwrap_or(r);
        }
    }
    r += c.bonus.total("COMBAT", "REACH");
    if let Some(v) = s.vars.controls.get("PCREACH") {
        if let Some(x) = s.vars.get(&crate::rules::vars::system::Owner::global(), v) {
            r = x.as_f64();
        }
    }
    Element::new("reach")
        .with(Element::leaf("reach", units.distance(r)))
        .with(Element::leaf("squares", pattern(r / square_size(s), "#.#")))
}

fn vision(s: &SheetCtx, units: &Units) -> Element {
    let c = &s.calc;
    // type -> (display name, distance)
    let mut best: std::collections::BTreeMap<String, (String, f64)> = Default::default();
    for src in &c.sources {
        for v in src.obj.values("VISION") {
            for part in v.split('|') {
                if part.starts_with("PRE") || part.starts_with('!') || part == ".CLEAR" {
                    continue;
                }
                let (name, dist) = match part.split_once('(') {
                    Some((n, d)) => (
                        n.trim(),
                        c.eval_formula(d.trim_end_matches(')').trim_end_matches('\'')),
                    ),
                    None => (part.trim(), 0.0),
                };
                if name.is_empty() {
                    continue;
                }
                let e = best
                    .entry(name.to_lowercase())
                    .or_insert_with(|| (name.to_string(), 0.0));
                e.1 = e.1.max(dist);
            }
        }
    }
    // a VISION bonus creates the type or adds to it
    for con in &c.bonus.log {
        if let Some(t) = con.key.strip_prefix("VISION.") {
            if !best.contains_key(&t.to_lowercase()) && c.bonus.total("VISION", t) > 0.0 {
                let name =
                    c.ds.objects(FileKind::Ability)
                        .iter()
                        .flat_map(|o| o.values("VISION"))
                        .flat_map(|v| v.split('|'))
                        .map(|p| p.split('(').next().unwrap_or("").trim())
                        .find(|n| n.eq_ignore_ascii_case(t))
                        .unwrap_or(t)
                        .to_string();
                best.insert(t.to_lowercase(), (name, 0.0));
            }
        }
    }
    let mut items: Vec<(String, String)> = best
        .values()
        .map(|(n, d)| {
            let b = c.bonus.total("VISION", n);
            let d = (*d as i64) as f64 + if b > 0.0 { b } else { 0.0 };
            let key = if d > 0.0 {
                format!("{} ({}')", n, d as i64)
            } else {
                n.clone()
            };
            let text = if d > 0.0 {
                format!("{} ({})", n, units.distance(d))
            } else {
                n.clone()
            };
            (key, text)
        })
        .collect();
    items.sort();
    let mut e = Element::new("vision");
    for (_, t) in &items {
        e.add("vision", t);
    }
    let all: Vec<&str> = items.iter().map(|(_, t)| t.as_str()).collect();
    e.add("all", all.join(", "));
    e
}

fn notes(s: &SheetCtx) -> Element {
    struct Note {
        id: i64,
        parent: i64,
        name: String,
        value: String,
    }
    let notes: Vec<Note> =
        s.ch.pcg
            .lines_tagged("NOTE")
            .map(|l| Note {
                id: l.text("ID").and_then(|v| v.parse().ok()).unwrap_or(0),
                parent: l
                    .text("PARENTID")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(-1),
                name: l.head().map(|h| h.value_text()).unwrap_or_default(),
                value: l.text("VALUE").unwrap_or_default(),
            })
            .collect();
    let mut order = Vec::new();
    fn walk(notes: &[Note], parent: Option<i64>, order: &mut Vec<usize>) {
        for (i, n) in notes.iter().enumerate() {
            let is_child = match parent {
                None => n.parent < 0,
                Some(p) => n.parent == p,
            };
            if is_child {
                order.push(i);
                walk(notes, Some(n.id), order);
            }
        }
    }
    walk(&notes, None, &mut order);
    let mut e = Element::new("notes");
    for i in order {
        let n = &notes[i];
        let mut note = Element::new("note");
        note.add("name", &n.name);
        note.push(paragraphs("value", n.value.trim()));
        e.push(note);
    }
    e
}
