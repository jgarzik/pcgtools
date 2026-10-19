//
// sheet/combat.rs -- abilities, hit points, armor class, initiative,
// saves, attack
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use super::{signed, Element, SheetCtx};
use crate::dataset::Object;
use crate::lst::kind::FileKind;
use crate::rules::bonus::Source;
use crate::rules::vars::system::Owner;

pub fn build(s: &SheetCtx) -> Vec<Element> {
    vec![abilities(s), hit_points(s), armor_class(s), initiative(s)]
}

/// The sections that follow skills on the sheet.
pub fn build_after_skills(s: &SheetCtx) -> Vec<Element> {
    vec![saving_throws(s), attack(s)]
}

/// Stats in display order.
pub fn stats<'a>(s: &SheetCtx<'a>) -> Vec<&'a Object> {
    let mut v: Vec<&Object> = s.ctx.dataset.objects(FileKind::Stat).iter().collect();
    v.sort_by_key(|o| {
        o.get("SORTKEY")
            .and_then(|k| k.parse::<i64>().ok())
            .unwrap_or(i64::MAX)
    });
    v
}

fn stat_mod_at(s: &SheetCtx, i: usize) -> i64 {
    stats(s)
        .get(i)
        .map(|o| s.calc.stat_mod(&o.key) as i64)
        .unwrap_or(0)
}

/// Bonuses that come from equipment.
fn from_equipment(src: &Source) -> bool {
    matches!(src.kind.as_str(), "Equipment" | "EquipMod")
}

fn abilities(s: &SheetCtx) -> Element {
    let c = &s.calc;
    let mut out = Element::new("abilities");
    for st in stats(s) {
        let k = &st.key;
        let base = c.stat_base(k);
        let score = c.stat_score(k);
        let noequip = base
            + c.bonus.total_where("STAT", k, |src| {
                !from_equipment(src) && !equipment_ability(s, src)
            });
        let notemp = base + c.bonus.total_where("STAT", k, |src| !src.temp);
        let non = c.non_stat(k);
        let m = |v: f64| {
            if non {
                signed(0)
            } else {
                signed(c.stat_mod_for(k, v) as i64)
            }
        };
        let n = |v: f64| {
            if non {
                "*".to_string()
            } else {
                (v as i64).to_string()
            }
        };
        let mut a = Element::new("ability");
        a.push(
            Element::new("name")
                .with(Element::leaf("long", &st.name))
                .with(Element::leaf("short", k)),
        );
        a.add("score", n(score));
        a.add("modifier", m(score));
        a.add("base", n(base));
        a.add("basemod", m(base));
        a.add("noequip", n(noequip));
        a.add("noequip_mod", m(noequip));
        a.add("no_temp_score", n(notemp));
        a.add("no_temp_modifier", m(notemp));
        out.push(a);
    }
    out
}

/// An ability of type Equipment.
fn equipment_ability(s: &SheetCtx, src: &Source) -> bool {
    src.kind == "Ability"
        && s.ctx
            .dataset
            .get_in(
                FileKind::Ability,
                src.category.as_deref().unwrap_or(""),
                &src.key,
            )
            .is_some_and(|o| o.is_type("Equipment"))
}

fn hit_points(s: &SheetCtx) -> Element {
    let c = &s.calc;
    let con = c.hp_stat_bonus();
    let mut hp = Element::new("hit_points");
    hp.add("points", s.sheet.hp);
    hp.add("alternate", c.bonus.total("HP", "ALTHP") as i64);

    hp.add("die", hit_dice(s));
    hp.add("current", "");
    hp.add("subdual", "");
    hp.add("damage_reduction", c.damage_reduction());
    hp.add(
        "damage_threshold",
        c.var("DAMAGETHRESHOLD").unwrap_or(0.0) as i64,
    );
    let levels = s.ch.levels();
    let mut hist = Element::new("history");
    for n in 1..=c.ecl() as usize {
        let mut r = Element::new("roll");
        r.add("level", n);
        match levels.get(n - 1) {
            Some(l) => {
                let roll = l.hp.unwrap_or(0);
                r.add("roll", roll);
                r.add("stat", con);
                r.add("total", roll + con);
            }
            None => {
                r.add("roll", 0);
                r.add("stat", 0);
                r.add("total", 0);
            }
        }
        hist.push(r);
    }
    hp.push(hist);
    hp
}

/// Hit dice per class, `(3d8)+(2d10)`, with the total stat and
/// maximum hit point bonus.
pub fn hit_dice(s: &SheetCtx) -> String {
    let c = &s.calc;
    let mut dice: Vec<String> = Vec::new();
    for (cls, lvl) in c.classes() {
        let die = c.hit_die(cls);
        if die != 0 && *lvl > 0 {
            dice.push(format!("({}d{})", lvl, die));
        }
    }
    let mut die = dice.join("+");
    if !c.classes().is_empty() {
        let t =
            c.hp_stat_bonus() * c.total_level() as i64 + c.bonus.total("HP", "CURRENTMAX") as i64;
        if t != 0 {
            die.push_str(&signed(t));
        }
    }
    die
}

fn armor_class(s: &SheetCtx) -> Element {
    let c = &s.calc;
    let mut ac = Element::new("armor_class");
    let t = |n: &str| c.ac_of_type(n);
    ac.add("total", t("Total"));
    ac.add("listing", c.bonus.listing("COMBAT", "AC"));
    ac.add("flat", t("Flatfooted"));
    ac.add("touch", t("Touch"));
    ac.add("base", t("Base"));
    ac.add("armor_bonus", t("Armor"));
    ac.add("shield_bonus", t("Shield"));
    ac.add("stat_mod", t("Ability"));
    ac.add("size_mod", t("Size"));
    ac.add("natural", t("NaturalArmor"));
    ac.add("class_bonus", t("ClassDefense"));
    ac.add("dodge_bonus", t("Dodge"));
    ac.add("misc", t("Misc"));
    ac.add("miss_chance", "");
    let md = c.max_dex();
    ac.add(
        "max_dex",
        if md >= 100.0 {
            String::new()
        } else {
            signed(md as i64)
        },
    );
    ac.add("spell_failure", c.spell_failure());
    ac.add("check_penalty", signed(c.check_penalty()));
    ac.add("spell_resistance", c.spell_resistance());
    ac
}

fn initiative(s: &SheetCtx) -> Element {
    let c = &s.calc;
    let misc = c.bonus.total("COMBAT", "INITIATIVE") as i64;
    let comp = c.var("INITCOMP").unwrap_or(0.0) as i64;
    let mut e = Element::new("initiative");
    e.add("total", signed(misc + comp));
    e.add("dex_mod", signed(stat_mod_at(s, 1)));
    e.add("misc_mod", signed(misc));
    e
}

fn saving_throws(s: &SheetCtx) -> Element {
    let c = &s.calc;
    let ds = &s.ctx.dataset;
    let mut saves: Vec<&Object> = ds.objects(FileKind::Save).iter().collect();
    saves.sort_by_key(|o| {
        o.get("SORTKEY")
            .and_then(|k| k.parse::<i64>().ok())
            .unwrap_or(i64::MAX)
    });
    let mut out = Element::new("saving_throws");
    out.add("conditional_modifiers", "");
    let layout = [
        ("fortitude", "fort", "constitution", 2),
        ("reflex", "ref", "dexterity", 1),
        ("will", "will", "wisdom", 4),
    ];
    for (i, (long, short, ability, stat)) in layout.iter().enumerate() {
        let name = saves.get(i).map(|o| o.key.as_str()).unwrap_or("");
        let base = c.bonus.total("SAVE", &format!("BASE.{}", name)) as i64;
        // bonuses to all saves count toward each
        let total = |f: &dyn Fn(&str) -> f64| f(name) + f("ALL");
        let misc = total(&|t| c.bonus.total("SAVE", t)) as i64;
        let magic = total(&|t| c.bonus.raw_sum("SAVE", t, from_equipment)) as i64;
        let feats = total(&|t| {
            c.bonus.raw_sum("SAVE", t, |src| {
                src.kind == "Ability"
                    && src
                        .category
                        .as_deref()
                        .is_some_and(|k| k.eq_ignore_ascii_case("FEAT"))
            })
        }) as i64;
        let race = (c
            .bonus
            .raw_sum("SAVE", &format!("BASE.{}", name), |src| src.kind == "Race")
            + total(&|t| c.bonus.raw_sum("SAVE", t, |src| src.kind == "Race")))
            as i64;
        let epic = c.bonus.by_type("SAVE", name, "EPIC") as i64;
        // the stat part of a save is either a variable named by the
        // game mode or the bonuses on the save object itself
        let statmod = match s.vars.controls.get("STATMODSAVE") {
            Some(var) => s
                .vars
                .get(&Owner::new("PC.SAVE", name), var)
                .map(|v| v.as_f64())
                .unwrap_or(0.0) as i64,
            None => c.bonus.raw_sum("SAVE", name, |src| {
                src.kind == "Save" && src.key.eq_ignore_ascii_case(name)
            }) as i64,
        };
        let mut st = Element::new("saving_throw");
        st.push(
            Element::new("name")
                .with(Element::leaf("long", long))
                .with(Element::leaf("short", short)),
        );
        st.add("ability", ability);
        st.add("total", signed(base + misc));
        st.add("base", signed(base));
        let abil = signed(stat_mod_at(s, *stat));
        if *long == "reflex" {
            st.add("feats", signed(feats));
            st.add("abil_mod", abil);
        } else {
            st.add("abil_mod", abil);
            st.add("feats", signed(feats));
        }
        st.add("magic_mod", signed(magic));
        st.add("misc_mod", signed(misc - magic - statmod));
        st.add("race", signed(race));
        st.add("epic_mod", signed(epic));
        st.add("temp_mod", "");
        out.push(st);
    }
    out
}

/// Iterative attack string: one entry per attack, `/`-separated.
fn attack_string(s: &SheetCtx, to_hit: i64) -> String {
    let c = &s.calc;
    let bab = c.bonus.total("COMBAT", "BASEAB") as i64;
    let max_attacks: i64 = s
        .ctx
        .gamemode
        .as_ref()
        .and_then(|g| g.misc("BABMAXATT"))
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    let cycle: i64 = s
        .ctx
        .gamemode
        .as_ref()
        .and_then(|g| g.misc("BABATTCYC"))
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let mut class_bab = 0i64;
    for (cls, _) in c.classes() {
        class_bab += c.bonus.total_where("COMBAT", "BASEAB", |src| {
            src.kind == "Class" && src.key == cls.key
        }) as i64;
    }
    let mut working = bab + to_hit;
    let mut attack_total = class_bab;
    let mut sub_total = bab;
    let mut left = max_attacks;
    let mut parts = Vec::new();
    loop {
        parts.push(signed(working));
        working -= cycle;
        attack_total -= cycle;
        sub_total -= cycle;
        left -= 1;
        if !((attack_total >= 1 || sub_total >= 1) && left > 0) {
            break;
        }
    }
    parts.join("/")
}

fn attack(s: &SheetCtx) -> Element {
    let c = &s.calc;
    let b = &c.bonus;
    let bab = b.total("COMBAT", "BASEAB") as i64;
    let mut out = Element::new("attack");
    for (tag, at) in [
        ("melee", "MELEE"),
        ("ranged", "RANGED"),
        ("grapple", "GRAPPLE"),
    ] {
        let tt = format!("TYPE.{}", at);
        let ct = format!("TOHIT.{}", at);
        let stat_only = |src: &Source| src.kind == "Stat";
        let stat_tt = b.total_where("TOHIT", &tt, stat_only) as i64;
        let stat_ct = b.total_where("COMBAT", &ct, stat_only) as i64;
        let size = |cat: &str, tg: &str| b.by_type(cat, tg, "SIZE") as i64;
        let epic = b.by_type("COMBAT", "TOHIT", "EPIC") as i64;
        let tohit = b.total("TOHIT", "TOHIT") as i64;
        let tohit_t = b.total("TOHIT", &tt) as i64;
        let combat = b.total("COMBAT", "TOHIT") as i64;
        let combat_t = b.total("COMBAT", &ct) as i64;
        let size_all = size("TOHIT", "TOHIT")
            + size("TOHIT", &tt)
            + size("COMBAT", "TOHIT")
            + size("COMBAT", &ct);
        let misc = (tohit + tohit_t - stat_tt - size("TOHIT", "TOHIT"))
            + (combat + combat_t - stat_ct - size("COMBAT", "TOHIT") - size("COMBAT", &ct) - epic);
        let total = tohit + tohit_t + combat + combat_t;
        let mut e = Element::new(tag);
        e.add("total", attack_string(s, total));
        e.add("bab", signed(bab));
        e.add("base_attack_bonus", attack_string(s, 0));
        e.add("stat_mod", signed(stat_tt + stat_ct));
        e.add("size_mod", signed(size_all));
        e.add("misc_mod", signed(misc));
        e.add("epic_mod", signed(epic));
        e.add("temp_mod", "");
        out.push(e);
    }
    out
}
