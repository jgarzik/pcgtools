//
// sheet/weapons.rs -- the weapons section: attacks and damage per
// weapon and way of wielding it
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use super::combat::{bab_sequence, bab_sequence_for};
use super::gear::{merged_by, placed, Placed};
use super::units::{pattern, Units};
use super::{signed, Element, SheetCtx};
use crate::dataset::Object;
use crate::lst::kind::FileKind;
use crate::rules::bonus::Source;
use crate::rules::equip::EqItem;

/// Most extra attacks listed for one weapon, whatever the data says.
const MAX_EXTRA_ATTACKS: i64 = 50;

/// Where a weapon is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hand {
    Primary,
    Secondary,
    Both,
    TwoWeapons,
    Equipped,
    Carried,
    NotCarried,
}

impl Hand {
    fn text(self) -> &'static str {
        match self {
            Hand::Primary => "Primary",
            Hand::Secondary => "Off-hand",
            Hand::Both => "Both",
            Hand::TwoWeapons => "Two-Weapons",
            Hand::Equipped => "Equipped",
            Hand::Carried => "Carried",
            Hand::NotCarried => "Not Carried",
        }
    }
}

/// Ways of attacking with a weapon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// whatever the weapon's place implies
    Total,
    /// one weapon in the primary hand
    Base,
    /// one weapon in the off hand
    OffHand,
    /// one weapon, two hands
    TwoHands,
    /// two weapons, this one primary, the other heavy
    PrimaryHeavy,
    /// two weapons, this one primary, the other light
    PrimaryLight,
    /// two weapons, this one in the off hand
    TwoWeapons,
    /// off hand with a light weapon while also using the primary
    OffLight,
    /// off hand with a heavy weapon while also using the primary
    OffHeavy,
}

/// How damage is figured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dmg {
    Basic,
    OffHand,
    TwoHands,
}

/// A weapon as listed: an item, or one of the melee and thrown copies
/// of a weapon that can be used both ways.
#[derive(Clone)]
struct Weapon<'a> {
    item: EqItem<'a>,
    hand: Hand,
    equipped: bool,
    /// listed position (sequence)
    index: usize,
    /// types after making melee/thrown copies
    types: Vec<String>,
    /// the base range in feet (0 for melee)
    range: f64,
    /// a thrown copy's name
    name: String,
    /// how the character wields it (Light, OneHanded, TwoHanded, ...)
    wield: String,
    /// distinct items the weapon holds (ammunition)
    contents: Vec<EqItem<'a>>,
    /// the ammunition a range listing is for
    ammo: Option<EqItem<'a>>,
}

impl Weapon<'_> {
    fn is(&self, t: &str) -> bool {
        self.types.iter().any(|x| x.eq_ignore_ascii_case(t))
    }
    fn natural(&self) -> bool {
        self.is("Natural")
    }
    fn melee(&self) -> bool {
        self.is("Melee")
    }
    fn ranged(&self) -> bool {
        self.is("Ranged")
    }
}

fn gm_num(s: &SheetCtx, tag: &str, default: f64) -> f64 {
    s.ctx
        .gamemode
        .as_ref()
        .and_then(|g| g.misc(tag))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(default)
}

fn hand_of(p: &Placed, items: &[Placed]) -> Hand {
    let root = match p.parent {
        Some(_) => {
            let mut up = p;
            while let Some(x) = up.parent {
                up = &items[x];
            }
            up.root.to_lowercase()
        }
        None => p.root.to_lowercase(),
    };
    if root.starts_with("not carried") {
        Hand::NotCarried
    } else if root.starts_with("carried") {
        Hand::Carried
    } else if p.parent.is_some() {
        Hand::Equipped
    } else if root == "primary hand" || root == "natural-primary" {
        Hand::Primary
    } else if root.starts_with("secondary hand") || root == "natural-secondary" {
        Hand::Secondary
    } else if root == "both hands" {
        Hand::Both
    } else if root == "double weapon" || root == "two weapons" {
        Hand::TwoWeapons
    } else {
        Hand::Equipped
    }
}

/// The weapons to list: weapon items in sheet order, each weapon
/// usable both in melee and thrown split into a melee and a thrown
/// copy.
fn weapons<'a>(s: &SheetCtx<'a>) -> Vec<Weapon<'a>> {
    let c = &s.calc;
    let (_, items) = placed(s);
    let mut out = Vec::new();
    for (i, _) in merged_by(&items, true) {
        let p = &items[i];
        if !p.item.is_type(c, "Weapon") {
            continue;
        }
        let types = p.item.types(c);
        // merged items show where the last of them is
        let last = items
            .iter()
            .rev()
            .find(|x| {
                x.item.long_name() == p.item.long_name()
                    && x.item.temporary.is_none() == p.item.temporary.is_none()
            })
            .unwrap_or(p);
        let hand = hand_of(last, &items);
        let mut contents: Vec<EqItem> = Vec::new();
        for x in items.iter().filter(|x| x.parent == Some(i)) {
            if !contents.iter().any(|y| y.long_name() == x.item.long_name()) {
                contents.push(x.item.clone());
            }
        }
        let range = p
            .item
            .data
            .get("RANGE")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0)
            + p.item.bonus(c, "EQMWEAPON", "RANGEADD", true);
        let has = |t: &str| types.iter().any(|x| x == t);
        if has("MELEE") && has("RANGED") {
            let mut melee: Vec<String> = types
                .iter()
                .filter(|t| *t != "RANGED" && *t != "THROWN")
                .cloned()
                .collect();
            melee.push("BOTH".into());
            let mut thrown: Vec<String> = types.iter().filter(|t| *t != "MELEE").cloned().collect();
            for t in ["RANGED", "THROWN", "BOTH"] {
                if !thrown.iter().any(|x| x == t) {
                    thrown.push(t.into());
                }
            }
            let tname = match p.item.name.strip_suffix(')') {
                Some(n) => format!("{}/Thrown)", n),
                None => format!("{} (Thrown)", p.item.name),
            };
            let mut mi = p.item.clone();
            mi.weight = 0.0;
            mi.weight_mod = 0.0;
            let mut ti = mi.clone();
            ti.output_name = None;
            // the thrown copy is not a melee weapon, for prerequisites too
            for t in ti
                .data
                .to_mut()
                .tokens
                .iter_mut()
                .filter(|t| t.tag == "TYPE")
            {
                t.value = t
                    .value
                    .split('.')
                    .filter(|x| !x.eq_ignore_ascii_case("Melee"))
                    .collect::<Vec<_>>()
                    .join(".");
            }
            out.push(Weapon {
                name: p.item.name.clone(),
                item: mi,
                hand,
                equipped: p.equipped,
                index: 0,
                types: melee,
                range: 0.0,
                wield: String::new(),
                contents: contents.clone(),
                ammo: None,
            });
            out.push(Weapon {
                name: tname,
                item: ti,
                hand,
                equipped: p.equipped,
                index: 0,
                types: thrown,
                range,
                wield: String::new(),
                contents: contents.clone(),
                ammo: None,
            });
        } else {
            let ranged = has("RANGED");
            out.push(Weapon {
                name: p.item.name.clone(),
                item: p.item.clone(),
                hand,
                equipped: p.equipped,
                index: 0,
                types,
                range: if ranged { range } else { 0.0 },
                wield: String::new(),
                contents: contents.clone(),
                ammo: None,
            });
        }
    }
    for (i, w) in out.iter_mut().enumerate() {
        w.index = i;
        w.wield = w.item.wield_category(c);
        w.wield = wield_bumped(s, w);
    }
    out
}

/// The wield category after bonuses that make a weapon easier or
/// harder to wield: `WEAPONPROF=key|WIELDCATEGORY`, the most negative
/// `WEAPONPROF=TYPE.x|WIELDCATEGORY` over the item's types, and the
/// item's own `WEAPON|WIELDCATEGORY`.  Each step moves along the game
/// mode's `UP` and `DOWN` lists; a step off the list leaves no
/// category.
fn wield_bumped(s: &SheetCtx, w: &Weapon) -> String {
    let c = &s.calc;
    let b = &c.bonus;
    let mut bump = 0i64;
    if let Some(p) = prof_of(s, w) {
        bump += b.total(&format!("WEAPONPROF={}", p.key), "WIELDCATEGORY") as i64;
        bump += w
            .item
            .types(c)
            .iter()
            .map(|t| b.total(&format!("WEAPONPROF=TYPE.{}", t), "WIELDCATEGORY") as i64)
            .min()
            .unwrap_or(0)
            .min(0);
    }
    bump += w.item.bonus(c, "WEAPON", "WIELDCATEGORY", true) as i64;
    if bump == 0 {
        return w.wield.clone();
    }
    let Some(gm) = s.ctx.gamemode.as_ref() else {
        return w.wield.clone();
    };
    let dir = if bump > 0 { "UP" } else { "DOWN" };
    gm.lines("miscinfo.lst", "WIELDCATEGORY")
        .filter(|l| l.value.eq_ignore_ascii_case(&w.wield))
        .find_map(|l| l.get(dir))
        .and_then(|list| {
            list.split('|')
                .nth(bump.unsigned_abs() as usize - 1)
                .map(str::to_string)
        })
        .unwrap_or_default()
}

/// Weapon proficiency of an item.
fn prof_of<'a>(s: &SheetCtx<'a>, w: &Weapon) -> Option<&'a Object> {
    let ds = &s.ctx.dataset;
    let named = w
        .item
        .data
        .get("PROFICIENCY")
        .map(|v| v.split('|').next_back().unwrap_or(v).to_string())
        .unwrap_or_else(|| w.item.base.name.clone());
    ds.get(FileKind::WeaponProf, &named)
        .or_else(|| ds.get(FileKind::WeaponProf, &w.item.data.key))
}

/// Character bonus totals for a weapon's proficiency (the proficiency
/// itself and each of its types).
fn prof_bonus(
    s: &SheetCtx,
    prof: Option<&Object>,
    target: &str,
    keep: &dyn Fn(&Source) -> bool,
) -> f64 {
    let Some(p) = prof else { return 0.0 };
    prof_bonus_named(s, &p.key, &p.types(), target, keep)
}

fn prof_bonus_named(
    s: &SheetCtx,
    key: &str,
    types: &[&str],
    target: &str,
    keep: &dyn Fn(&Source) -> bool,
) -> f64 {
    let b = &s.calc.bonus;
    let mut t = b.total_where(&format!("WEAPONPROF={}", key), target, keep);
    for ty in types {
        t += b.total_where(&format!("WEAPONPROF=TYPE.{}", ty), target, keep);
    }
    t
}

/// Bonuses to a weapon's proficiency; natural attacks are their own
/// proficiency even when the data defines none.
fn weapon_prof_bonus(
    s: &SheetCtx,
    w: &Weapon,
    target: &str,
    keep: &dyn Fn(&Source) -> bool,
) -> f64 {
    match prof_of(s, w) {
        Some(p) => prof_bonus(s, Some(p), target, keep),
        None if w.natural() => prof_bonus_named(s, &w.item.name, &[], target, keep),
        None => 0.0,
    }
}

/// A weapon with no proficiency matches every bonus to a named
/// proficiency, each bonus counted once however many it names.
fn unnamed_prof_bonus(s: &SheetCtx, target: &str, keep: &dyn Fn(&Source) -> bool) -> f64 {
    let suffix = format!(".{}", target.to_ascii_uppercase());
    let mut seen: Vec<&str> = Vec::new();
    let mut total = 0.0;
    for c in &s.calc.bonus.log {
        let Some(name) = c
            .key
            .strip_prefix("WEAPONPROF=")
            .and_then(|k| k.strip_suffix(&suffix))
        else {
            continue;
        };
        if name.starts_with("TYPE.") || !keep(&c.source) || seen.contains(&c.source.origin.as_str())
        {
            continue;
        }
        seen.push(&c.source.origin);
        total += c.value;
    }
    total
}

fn all(_: &Source) -> bool {
    true
}

fn light(w: &Weapon) -> bool {
    w.wield.eq_ignore_ascii_case("Light") || w.is("Light")
}

fn two_handed(w: &Weapon) -> bool {
    w.wield.eq_ignore_ascii_case("TwoHanded")
}

/// Whether PCGen fails to find how the weapon is wielded: with no
/// `Light` or `OneHanded` wield category in the game mode, a weapon
/// with a proficiency (the unarmed strike always has one) has none,
/// and its attacks and damage come out empty.
fn wield_unknown(s: &SheetCtx, w: &Weapon) -> bool {
    let defined = s.ctx.gamemode.as_ref().is_some_and(|g| {
        g.lines("miscinfo.lst", "WIELDCATEGORY").any(|l| {
            l.value.eq_ignore_ascii_case("Light") || l.value.eq_ignore_ascii_case("OneHanded")
        })
    });
    !defined && (w.is("Unarmed") || prof_of(s, w).is_some())
}

fn finessable(w: &Weapon) -> bool {
    w.is("Finesseable") || light(w)
}

/// The weapon's to-hit bonus apart from the mode: types, finesse,
/// proficiency, range, general and item bonuses.
fn to_hit_bonus(s: &SheetCtx, w: &Weapon, distance: Option<f64>) -> i64 {
    let c = &s.calc;
    let b = &c.bonus;
    let ranged = distance.is_some();
    let mut t = 0.0;
    for ty in &w.types {
        let u = ty.to_ascii_uppercase();
        if u == "FINESSEABLE" || (u == "MELEE" && ranged) {
            continue;
        }
        if (u == "THROWN" || u == "RANGED") && !ranged && w.melee() {
            continue;
        }
        t += b.total("TOHIT", &format!("TYPE.{}", ty)) as i64 as f64;
        t += b.total("COMBAT", &format!("TOHIT.{}", ty)) as i64 as f64;
    }
    if !ranged && w.melee() && finessable(w) {
        t += b.total("COMBAT", "TOHIT.FINESSEABLE") as i64 as f64;
    }
    let prof = prof_of(s, w);
    let known = super::lists::weapon_prof_keys(s);
    let prof_key = prof
        .map(|p| p.key.clone())
        .or_else(|| w.is("Unarmed").then(|| "Unarmed Strike".to_string()));
    let proficient = prof_key.is_some_and(|p| known.iter().any(|k| k.eq_ignore_ascii_case(&p)));
    if !w.natural() && !proficient {
        t += gm_num(s, "WEAPONNONPROFPENALTY", -4.0);
    }
    t += weapon_prof_bonus(s, w, "TOHIT", &all) as i64 as f64;
    if let Some(d) = distance {
        if d <= gm_num(s, "SHORTRANGE", 30.0) {
            t += b.total("COMBAT", "TOHIT-SHORTRANGE") as i64 as f64;
            t += b.total("TOHIT", "SHORTRANGE") as i64 as f64;
            t += w.item.bonus(c, "WEAPON", "TOHIT-SHORTRANGE", true);
        }
        if w.range > 0.0 {
            let steps = ((d / w.range).ceil() - 1.0).max(0.0);
            t += (gm_num(s, "RANGEPENALTY", -2.0) + b.total("COMBAT", "RANGEPENALTY")) * steps;
        }
    }
    t += b.total("TOHIT", "TOHIT") as i64 as f64;
    t += b.total("COMBAT", "TOHIT") as i64 as f64;
    t += nonproficient_armor_penalty(s) as f64;
    t += item_bonus(s, w, "TOHIT");
    if let (Some(a), true) = (&w.ammo, distance.is_some()) {
        t += a.bonus(c, "WEAPON", "TOHIT", true);
    }
    t as i64
}

/// Armor and shields worn without proficiency apply their check
/// penalty to attacks.
fn nonproficient_armor_penalty(s: &SheetCtx) -> i64 {
    let c = &s.calc;
    let (_, items) = placed(s);
    let mut total = 0;
    for p in items.iter().filter(|p| p.equipped && p.parent.is_none()) {
        let it = &p.item;
        let (kind, grants) = if it.is_type(c, "Shield") {
            (FileKind::ShieldProf, "SHIELDPROF")
        } else if it.is_type(c, "Armor") {
            (FileKind::ArmorProf, "ARMORPROF")
        } else {
            continue;
        };
        let prof = it
            .data
            .get("PROFICIENCY")
            .and_then(|v| v.split_once('|'))
            .map(|(_, n)| n.to_string())
            .unwrap_or_else(|| it.base.name.clone());
        let shield = kind == FileKind::ShieldProf;
        let _ = grants;
        if crate::rules::profs::armor_proficient(c, shield, &prof, &it.types(c)) {
            continue;
        }
        let acp = num(it.data.get("ACCHECK")) + it.bonus(c, "EQMARMOR", "ACCHECK", true);
        total += acp.min(0.0) as i64;
    }
    total
}

fn num(v: Option<&str>) -> f64 {
    v.and_then(|x| x.trim().parse().ok()).unwrap_or(0.0)
}

/// An item's own WEAPON bonus, with temporary bonuses on a copy.
fn item_bonus(s: &SheetCtx, w: &Weapon, target: &str) -> f64 {
    let c = &s.calc;
    let mut t = w.item.bonus(c, "WEAPON", target, true);
    if let Some((_, bonuses)) = &w.item.temporary {
        for v in bonuses {
            if let Ok(b) = crate::token::bonus::Bonus::parse(v) {
                if b.category.eq_ignore_ascii_case("WEAPON")
                    && b.targets.iter().any(|x| x.eq_ignore_ascii_case(target))
                {
                    t += c.eval_formula(&b.formula);
                }
            }
        }
    }
    t
}

/// Mode-dependent part of the to-hit bonus.
fn mode_bonus(s: &SheetCtx, w: &Weapon, mode: Mode, ranged: bool) -> i64 {
    let b = &s.calc.bonus;
    if w.natural() {
        return match w.hand {
            Hand::Secondary => -5 + b.total("COMBAT", "TOHIT-SECONDARY") as i64,
            _ => 0,
        };
    }
    let off_light = s.calc.var("OFFHANDLIGHTBONUS").unwrap_or(0.0) as i64;
    let mut t: i64 = match mode {
        Mode::Total if ranged => 0,
        Mode::Base | Mode::TwoHands => 0,
        Mode::PrimaryHeavy | Mode::PrimaryLight => -6,
        Mode::OffHand => -4,
        _ => -10,
    };
    if matches!(mode, Mode::PrimaryLight | Mode::OffLight) {
        t += off_light;
    }
    if mode == Mode::TwoWeapons && light(w) {
        t += off_light;
    }
    let secondary = b.total("COMBAT", "TOHIT-SECONDARY") as i64;
    let primary = b.total("COMBAT", "TOHIT-PRIMARY") as i64;
    match mode {
        Mode::TwoWeapons | Mode::OffLight | Mode::OffHeavy => t += secondary,
        Mode::OffHand => t += secondary - primary,
        Mode::PrimaryHeavy | Mode::PrimaryLight => t += primary,
        _ => {}
    }
    t
}

/// Attack sequence for a weapon in a mode, at a distance (ranged).
fn to_hit(s: &SheetCtx, w: &Weapon, all_w: &[Weapon], mode: Mode, distance: Option<f64>) -> String {
    let b = &s.calc.bonus;
    if wield_unknown(s, w) {
        return String::new();
    }
    let two = two_handed(w);
    // weapons too big for one hand can only be used two-handed
    if w.melee() && two && !matches!(mode, Mode::TwoHands) && distance.is_none() {
        let resolved = resolve_mode(w, all_w, mode);
        if resolved != Mode::TwoHands {
            return "N/A".into();
        }
    }
    let mode = resolve_mode(w, all_w, mode);
    let bonus = to_hit_bonus(s, w, distance) + mode_bonus(s, w, mode, distance.is_some());
    let seq = if w.is("Unarmed") {
        bab_sequence_for(s, Some("UAB"))
    } else {
        bab_sequence(s)
    };
    let first = seq.first().copied().unwrap_or(0);
    let (progress, extra) = attacks(s, w);
    // counts come from data files: keep them sane
    let mut list: Vec<i64> = vec![first; extra.clamp(0, MAX_EXTRA_ATTACKS) as usize];
    if progress {
        list.extend(seq.iter().copied());
    } else {
        list.push(first);
    }
    // the off hand gets one attack, more with feats
    let sec_to_add = 1
        + b.total("COMBAT", "ATTACKS-SECONDARY") as i64
        + b.total("COMBAT", "SECONDARYATTACKS") as i64;
    if mode == Mode::TwoWeapons || matches!(mode, Mode::OffLight | Mode::OffHeavy) {
        list.truncate(sec_to_add.max(1) as usize);
    }
    list.iter()
        .map(|v| signed(v + bonus))
        .collect::<Vec<_>>()
        .join("/")
}

/// The attack mode a weapon's place implies.
fn resolve_mode(w: &Weapon, all_w: &[Weapon], mode: Mode) -> Mode {
    if mode != Mode::Total {
        return mode;
    }
    let slots = |x: &Weapon| !x.natural();
    let primaries = all_w.iter().any(|x| {
        slots(x)
            && matches!(x.hand, Hand::Primary | Hand::Both | Hand::TwoWeapons)
            && x.name == x.item.name
            && x.equipped
    });
    let secondary: Option<&Weapon> = all_w
        .iter()
        .find(|x| slots(x) && x.hand == Hand::Secondary && x.name == x.item.name);
    if w.is("Unarmed") {
        return Mode::Base;
    }
    if !w.natural() && w.hand == Hand::Primary && w.name == w.item.name {
        if let Some(sec) = secondary {
            return if light(sec) {
                Mode::PrimaryLight
            } else {
                Mode::PrimaryHeavy
            };
        }
        return Mode::Base;
    }
    if !w.natural() && w.hand == Hand::Secondary && w.name == w.item.name {
        if primaries {
            return if light(w) {
                Mode::OffLight
            } else {
                Mode::OffHeavy
            };
        }
        return Mode::OffHand;
    }
    if w.hand == Hand::Both {
        return Mode::TwoHands;
    }
    // a pair held as two weapons attacks as primary with the other
    // in the off hand
    if w.hand == Hand::TwoWeapons && !w.is("Double") && w.name == w.item.name {
        return if light(w) {
            Mode::PrimaryLight
        } else {
            Mode::PrimaryHeavy
        };
    }
    if two_handed(w) {
        Mode::TwoHands
    } else {
        Mode::Base
    }
}

/// Dice for an item's damage at its size.
fn item_dice(s: &SheetCtx, w: &Weapon) -> String {
    let c = &s.calc;
    let dice = w.item.data.get("DAMAGE").unwrap_or("0d0").to_string();
    if w.natural() {
        return dice;
    }
    let sizes: Vec<String> =
        c.ds.objects(FileKind::Size)
            .iter()
            .map(|o| o.key.clone())
            .collect();
    let pos = |k: &str| {
        sizes
            .iter()
            .position(|x| x.eq_ignore_ascii_case(k))
            .map(|i| i as i64)
    };
    let data_size = w
        .item
        .data
        .get("SIZE")
        .map(str::to_string)
        .unwrap_or_else(|| crate::rules::equip::default_size(c));
    let diff = match (pos(&w.item.size), pos(&data_size)) {
        (Some(a), Some(b)) => a - b,
        _ => 0,
    } + w.item.bonus(c, "EQMWEAPON", "DAMAGESIZE", true) as i64;
    adjust_dice(s, &dice, diff)
}

/// Step dice up or down the game mode's BASEDICE table.
fn adjust_dice(s: &SheetCtx, dice: &str, diff: i64) -> String {
    if diff == 0 || dice.eq_ignore_ascii_case("special") || dice == "-" {
        return dice.to_string();
    }
    let Some(gm) = s.ctx.gamemode.as_ref() else {
        return dice.to_string();
    };
    let row = gm
        .lines("miscinfo.lst", "BASEDICE")
        .find(|l| l.value == dice);
    if let Some(row) = row {
        let list: Vec<&str> = if diff > 0 {
            row.get("UP").unwrap_or("").split(',').collect()
        } else {
            row.get("DOWN").unwrap_or("").split(',').collect()
        };
        let i = (diff.unsigned_abs() as usize).min(list.len()) - 1;
        return list
            .get(i)
            .map(|x| x.to_string())
            .unwrap_or_else(|| dice.to_string());
    }
    // NdS with no row: step 1dS and multiply the count
    if let Some((n, sides)) = dice.split_once('d') {
        if let Ok(n) =
            n.parse::<i64>()
                .map_err(|_| ())
                .and_then(|n| if n > 1 { Ok(n) } else { Err(()) })
        {
            let one = adjust_dice(s, &format!("1d{}", sides), diff);
            if let Some((m, sd)) = one.split_once('d') {
                if let Ok(m) = m.parse::<i64>() {
                    return format!("{}d{}", m.saturating_mul(n), sd);
                }
            }
        }
    }
    dice.to_string()
}

/// Damage for a weapon in a damage mode, at a distance (ranged).
fn damage(
    s: &SheetCtx,
    w: &Weapon,
    all_w: &[Weapon],
    forced: Option<Dmg>,
    distance: Option<f64>,
) -> String {
    let c = &s.calc;
    let b = &c.bonus;
    if wield_unknown(s, w) {
        return String::new();
    }
    let two = two_handed(w);
    let mode = forced.unwrap_or_else(|| {
        if w.natural() && w.hand == Hand::Secondary {
            Dmg::OffHand
        } else if w.is("Unarmed") {
            Dmg::Basic
        } else if w.hand == Hand::Secondary && w.name == w.item.name {
            Dmg::OffHand
        } else if w.hand == Hand::Both || two {
            Dmg::TwoHands
        } else {
            Dmg::Basic
        }
    });
    let _ = all_w;
    if w.melee() && two && mode != Dmg::TwoHands && distance.is_none() {
        return "N/A".into();
    }
    let mut hands = match mode {
        Dmg::Basic => 1,
        Dmg::OffHand => 0,
        Dmg::TwoHands => 2,
    };
    if light(w) && hands == 2 {
        hands = 1;
    }
    let mut dice = item_dice(s, w);
    // monk-style unarmed weapons use unarmed strike damage
    if w.is("Unarmed") && w.is("Monk") {
        dice = unarmed_dice(s);
    }
    let ranged = distance.is_some();
    let mut total = item_bonus(s, w, "DAMAGE");
    let mut general = 0.0;
    for ty in &w.types {
        let u = ty.to_ascii_uppercase();
        if u == "MELEE" && ranged {
            continue;
        }
        if (u == "THROWN" || u == "RANGED") && !ranged {
            continue;
        }
        general += b.total("COMBAT", &format!("DAMAGE.{}", ty));
        general += b.total("DAMAGE", &format!("TYPE.{}", ty));
    }
    general += weapon_prof_bonus(s, w, "STATDAMAGE", &all);
    if finessable(w) && !w.is("Finesseable") {
        general += b.total("COMBAT", "DAMAGE.FINESSEABLE");
    }
    // strength damage multiplied by how the weapon is held
    let stat = |src: &Source| src.kind == "Stat";
    let str_part = b.total_where("COMBAT", "DAMAGE.MELEE", stat)
        + b.total_where("DAMAGE", "TYPE.MELEE", stat)
        + weapon_prof_bonus(s, w, "STATDAMAGE", &all);
    if w.melee() && !ranged && str_part > 0.0 {
        let mult = b.total("COMBAT", &format!("DAMAGEMULT:{}", hands))
            + weapon_prof_bonus(s, w, &format!("DAMAGEMULT:{}", hands), &all)
            + item_bonus(s, w, &format!("DAMAGEMULT:{}", hands));
        general = (general - str_part + mult * str_part).trunc();
    }
    if let Some(d) = distance {
        if d <= gm_num(s, "SHORTRANGE", 30.0) {
            general += item_bonus(s, w, "DAMAGE-SHORTRANGE")
                + b.total("DAMAGE", "SHORTRANGE")
                + b.total("COMBAT", "DAMAGE-SHORTRANGE");
        }
    }
    total += general;
    total += weapon_prof_bonus(s, w, "DAMAGE", &all);
    let total = total as i64;
    if dice == "0d0" {
        return "0".into();
    }
    if total != 0 {
        format!("{}{}", dice, signed(total))
    } else {
        dice
    }
}

/// The die of a UDAM list for a size: one value serves every size,
/// otherwise the list has one value per size.
fn udam_at(list: &str, size: usize) -> Option<&str> {
    let v: Vec<&str> = list.split(',').collect();
    if v.len() == 1 {
        Some(v[0])
    } else {
        v.get(size).copied()
    }
}

/// Unarmed damage dice: the best of 1d2, the classes' unarmed damage
/// (UDAM on level lines), unarmed damage from the race, templates and
/// abilities (UDAM), and the race's (1d3 at medium size,
/// scaled by size).
fn unarmed_dice(s: &SheetCtx) -> String {
    let c = &s.calc;
    let sizes: Vec<String> =
        c.ds.objects(FileKind::Size)
            .iter()
            .map(|o| o.key.clone())
            .collect();
    let pos = |k: &str| {
        sizes
            .iter()
            .position(|x| x.eq_ignore_ascii_case(k))
            .map(|i| i as i64)
            .unwrap_or(0)
    };
    let medium = crate::rules::equip::default_size(c);
    let race_size = c
        .sources
        .iter()
        .find(|x| x.kind == crate::rules::calc::SrcKind::Race)
        .and_then(|r| r.obj.get("SIZE"))
        .filter(|v| c.ds.get(FileKind::Size, v).is_some())
        .map(str::to_string)
        .unwrap_or_else(|| medium.clone());
    let pc_size = c.size_key();
    let apply = race_size.eq_ignore_ascii_case(&pc_size);
    let size_at = pos(&pc_size).max(0) as usize;
    let sides = |d: &str| {
        d.split('d')
            .nth(1)
            .and_then(|x| x.parse::<i64>().ok())
            .unwrap_or(0)
    };
    let mut best = "1d2".to_string();
    let mut consider = |d: &str| {
        if sides(d) > sides(&best) {
            best = d.to_string();
        }
    };
    for (cls, lvl) in c.classes() {
        // the last level line at or below the class level with UDAM
        let line = cls
            .sublines
            .iter()
            .filter(|sl| sl.tag == "LEVEL")
            .filter(|sl| {
                sl.value
                    .split(':')
                    .next()
                    .and_then(|x| x.trim().parse::<u32>().ok())
                    .is_some_and(|n| n > 0 && n <= *lvl)
            })
            .filter_map(|sl| sl.tokens.iter().find(|t| t.tag == "UDAM"))
            .next_back();
        if let Some(t) = line {
            if let Some(d) = udam_at(&t.value, size_at) {
                consider(d);
            }
        }
    }
    for src in &c.sources {
        if src.kind == crate::rules::calc::SrcKind::Class {
            continue;
        }
        if let Some(d) = src.obj.get("UDAM").and_then(|u| udam_at(u, size_at)) {
            consider(d);
        }
    }
    let race_die = adjust_dice(s, "1d3", pos(&race_size) - pos(&medium));
    consider(&race_die);
    let from = if apply { pos(&race_size) } else { pos(&medium) };
    adjust_dice(s, &best, pos(&pc_size) - from)
}

/// The weapon's category: `Simple-Melee`, `Natural-Natural,Melee`, ...
fn category(s: &SheetCtx, w: &Weapon) -> String {
    let cats: Vec<String> = s
        .ctx
        .gamemode
        .as_ref()
        .map(|g| {
            g.lines("miscinfo.lst", "WEAPONCATEGORY")
                .map(|l| l.value.clone())
                .collect()
        })
        .unwrap_or_default();
    let mine: Vec<&String> = cats.iter().filter(|c| w.is(c)).collect();
    let mut out = if mine.is_empty() {
        "Non-Standard".to_string()
    } else {
        mine.iter()
            .map(|x| x.as_str())
            .collect::<Vec<_>>()
            .join("/")
    };
    out.push('-');
    let both = w.is("Both");
    let any = w.natural() || both || w.melee() || w.ranged();
    if w.natural() {
        out.push_str("Natural");
        if both || w.melee() || w.ranged() {
            out.push(',');
        }
    }
    if both {
        out.push_str(if w.ranged() {
            "Both (Ranged)"
        } else {
            "Both (Melee)"
        });
    } else if w.melee() {
        out.push_str("Melee");
    } else if w.ranged() {
        out.push_str("Ranged");
    }
    if !any {
        out.push_str("Non-Standard");
    }
    out
}

fn crit_range(s: &SheetCtx, w: &Weapon) -> String {
    let r: i64 = w
        .item
        .data
        .get("CRITRANGE")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if r == 0 {
        return "none".into();
    }
    let dbl = (weapon_prof_bonus(s, w, "CRITRANGEDOUBLE", &all)
        + w.item.bonus(&s.calc, "EQMWEAPON", "CRITRANGEDOUBLE", true)) as i64;
    let add = (weapon_prof_bonus(s, w, "CRITRANGEADD", &all)
        + w.item.bonus(&s.calc, "EQMWEAPON", "CRITRANGEADD", true)) as i64;
    let low = 21 - (r * (dbl + 1) + add);
    if low < 20 {
        format!("{}-20", low)
    } else {
        low.to_string()
    }
}

fn crit_mult(s: &SheetCtx, w: &Weapon) -> String {
    let m = w.item.data.get("CRITMULT").unwrap_or("0");
    let m: i64 = m.trim_start_matches('x').parse().unwrap_or(0);
    (m + weapon_prof_bonus(s, w, "CRITMULTADD", &all) as i64).to_string()
}

fn type_abbrevs(s: &SheetCtx, w: &Weapon) -> String {
    let mut out: Vec<String> = s
        .ctx
        .gamemode
        .as_ref()
        .map(|g| {
            g.lines("miscinfo.lst", "WEAPONTYPE")
                .filter_map(|l| {
                    let (name, abb) = l.value.split_once('|')?;
                    w.is(name).then(|| abb.to_string())
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out.dedup();
    out.concat()
}

/// Whether attacks follow the attack bonus progression, and how many
/// extra attacks at the first bonus the weapon gives.  A natural
/// attack written `*n` makes n attacks at full bonus and no more.
fn attacks(s: &SheetCtx, w: &Weapon) -> (bool, i64) {
    let spec = w.item.data.get("ATTACKS").unwrap_or("");
    let progress = !spec.starts_with('*');
    let natural_extra = if w.natural() {
        spec.trim_start_matches('*').parse::<i64>().unwrap_or(1) - 1
    } else {
        0
    };
    let extra = natural_extra
        + item_bonus(s, w, "ATTACKS") as i64
        + s.calc.bonus.total("COMBAT", "ATTACKS") as i64;
    (progress, extra)
}

fn num_attacks(s: &SheetCtx, w: &Weapon) -> i64 {
    let (progress, extra) = attacks(s, w);
    let n = if progress {
        bab_sequence(s).len() as i64
    } else {
        1
    };
    let natural_extra = if w.natural() {
        w.item
            .data
            .get("ATTACKS")
            .unwrap_or("")
            .trim_start_matches('*')
            .parse::<i64>()
            .unwrap_or(1)
            - 1
    } else {
        0
    };
    let _ = extra;
    (n + natural_extra + item_bonus(s, w, "ATTACKS") as i64).clamp(0, MAX_EXTRA_ATTACKS + n)
}

fn reach(s: &SheetCtx, w: &Weapon) -> String {
    let c = &s.calc;
    let race_reach = c
        .sources
        .iter()
        .filter(|x| {
            matches!(
                x.kind,
                crate::rules::calc::SrcKind::Race | crate::rules::calc::SrcKind::Template
            )
        })
        .filter_map(|x| x.obj.get("REACH"))
        .next_back()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(5.0)
        + c.bonus.total("COMBAT", "REACH");
    let item_reach: f64 = w
        .item
        .data
        .get("REACH")
        .and_then(|v| v.parse().ok())
        .unwrap_or(5.0);
    let mult: f64 = w
        .item
        .data
        .get("REACHMULT")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0);
    let r =
        (race_reach + (item_reach - 5.0).max(0.0)) * mult + weapon_prof_bonus(s, w, "REACH", &all);
    pattern(r, "#.##")
}

/// Distances for a ranged weapon's five range rows.
fn range_list(w: &Weapon) -> Vec<f64> {
    let base = w.range;
    if base <= 0.0 {
        return vec![0.0; 5];
    }
    let mut out = Vec::new();
    let mut insert = true;
    let mut r = base;
    while out.len() < 5 {
        if r == 30.0 {
            insert = false;
        }
        if r > 30.0 && insert {
            out.push(30.0);
            insert = false;
            if out.len() == 5 {
                break;
            }
        }
        out.push(r);
        r += base;
    }
    out
}

fn magic(s: &SheetCtx, w: &Weapon, target: &str) -> String {
    signed(item_bonus(s, w, target) as i64)
}

/// A weapon bonus from feats or from templates.
fn from_kind(s: &SheetCtx, target: &str, keep: &dyn Fn(&Source) -> bool, extra: i64) -> String {
    let b = &s.calc.bonus;
    let mut v = b.total_where("WEAPON", target, keep);
    if target == "DAMAGE" {
        v -= b.total_where("WEAPON", "DAMAGE-SHORTRANGE", keep);
    }
    signed(v as i64 + extra)
}

fn feat(src: &Source) -> bool {
    src.kind == "Ability"
        && src
            .category
            .as_deref()
            .is_some_and(|c| c.eq_ignore_ascii_case("FEAT"))
}

fn template(src: &Source) -> bool {
    src.kind == "Template"
}

fn common(s: &SheetCtx, w: &Weapon, all_w: &[Weapon]) -> Element {
    let c = &s.calc;
    let star = if w.equipped { "*" } else { "" };
    let applied = match &w.item.temporary {
        Some((src, _)) => format!(" [{}]", src),
        None => String::new(),
    };
    let short = if w.name == w.item.name {
        w.item.output_name()
    } else {
        w.name.clone()
    } + &applied;
    let long = if w.name == w.item.name {
        w.item.long_name()
    } else {
        w.name.clone()
    } + &applied;
    let mut e = Element::new("common");
    e.push(
        Element::new("name")
            .with(Element::leaf("short", format!("{}{}", star, short)))
            .with(Element::leaf("long", format!("{}{}", star, long))),
    );
    e.add("category", category(s, w));
    e.push(
        Element::new("critical")
            .with(Element::leaf("range", crit_range(s, w)))
            .with(Element::leaf("multiplier", crit_mult(s, w))),
    );
    let total = to_hit(s, w, all_w, Mode::Total, (w.range > 0.0).then_some(w.range));
    e.push(
        Element::new("to_hit")
            .with(Element::leaf("hit", &total))
            .with(Element::leaf("magic_hit", magic(s, w, "TOHIT")))
            .with(Element::leaf("total_hit", &total)),
    );
    // the feat column of a weapon without a proficiency also shows
    // bonuses to named proficiencies (the total does not)
    let unnamed = |target: &str| {
        if prof_of(s, w).is_none() && !w.natural() {
            unnamed_prof_bonus(s, target, &feat) as i64
        } else {
            0
        }
    };
    e.push(
        Element::new("feat")
            .with(Element::leaf(
                "hit",
                from_kind(s, "TOHIT", &feat, unnamed("TOHIT")),
            ))
            .with(Element::leaf(
                "damage",
                from_kind(s, "DAMAGE", &feat, unnamed("DAMAGE")),
            )),
    );
    e.add("hand", w.hand.text());
    e.add("num_attacks", num_attacks(s, w));
    e.add("reach", reach(s, w));
    e.add("size", &w.item.size);
    e.add("special_properties", w.item.special_properties(c));
    e.push(
        Element::new("template")
            .with(Element::leaf("hit", from_kind(s, "TOHIT", &template, 0)))
            .with(Element::leaf(
                "damage",
                from_kind(s, "DAMAGE", &template, 0),
            )),
    );
    e.add("type", type_abbrevs(s, w));
    let wt = if w.name == w.item.name && !w.is("Both") {
        w.item.weight(c)
    } else {
        0.0
    };
    e.add("weight", pattern(wt, "#.##"));
    e.add("sequence", w.index);
    e
}

fn melee(s: &SheetCtx, w: &Weapon, all_w: &[Weapon]) -> Element {
    let row = |tag: &str, mode: Mode, dmg: Dmg| {
        Element::new(tag)
            .with(Element::leaf("to_hit", to_hit(s, w, all_w, mode, None)))
            .with(Element::leaf(
                "damage",
                damage(s, w, all_w, Some(dmg), None),
            ))
    };
    Element::new("melee")
        .with(row("w1_h1_p", Mode::Base, Dmg::Basic))
        .with(row("w1_h1_o", Mode::OffHand, Dmg::OffHand))
        .with(row("w1_h2", Mode::TwoHands, Dmg::TwoHands))
        .with(row("w2_p_oh", Mode::PrimaryHeavy, Dmg::Basic))
        .with(row("w2_p_ol", Mode::PrimaryLight, Dmg::Basic))
        .with(row("w2_o", Mode::TwoWeapons, Dmg::OffHand))
}

fn ranges(s: &SheetCtx, w: &Weapon, all_w: &[Weapon]) -> Element {
    let units = Units::from_gamemode(s.ctx.gamemode.as_ref());
    let mut e = Element::new("ranges");
    if let Some(a) = &w.ammo {
        e.push(
            Element::new("ammunition")
                .with(Element::leaf("name", &a.name))
                .with(Element::leaf(
                    "special_properties",
                    a.special_properties(&s.calc),
                )),
        );
    }
    for d in range_list(w) {
        let dist = if w.range > 0.0 { Some(d) } else { None };
        e.push(
            Element::new("range")
                .with(Element::leaf("distance", units.distance(d)))
                .with(Element::leaf(
                    "to_hit",
                    to_hit(s, w, all_w, Mode::Total, dist.or(Some(0.0))),
                ))
                .with(Element::leaf(
                    "damage",
                    damage(s, w, all_w, None, dist.or(Some(0.0))),
                )),
        );
    }
    e
}

fn simple(s: &SheetCtx, w: &Weapon, all_w: &[Weapon]) -> Element {
    let units = Units::from_gamemode(s.ctx.gamemode.as_ref());
    let dist = (w.range > 0.0).then_some(w.range);
    Element::new("simple")
        .with(Element::leaf(
            "to_hit",
            to_hit(s, w, all_w, Mode::Total, dist),
        ))
        .with(Element::leaf("damage", damage(s, w, all_w, None, dist)))
        .with(Element::leaf("range", units.distance(w.range)))
}

/// The unarmed strike every character has.
fn unarmed(s: &SheetCtx) -> Element {
    let c = &s.calc;
    let obj = Object {
        kind: FileKind::Equipment,
        head_tag: None,
        name: "Unarmed Strike".into(),
        key: "Unarmed Strike".into(),
        category: None,
        tokens: vec![crate::dataset::Tok {
            tag: "WIELD".into(),
            value: "Light".into(),
            origin: crate::dataset::Origin { file: 0, line: 0 },
        }],
        sublines: Vec::new(),
        children: Vec::new(),
        source: Vec::new(),
        origin: crate::dataset::Origin { file: 0, line: 0 },
        history: Vec::new(),
    };
    let item = EqItem::synthetic(obj, &c.size_key());
    let w = Weapon {
        name: "Unarmed Strike".into(),
        item,
        hand: Hand::Primary,
        equipped: true,
        index: 0,
        types: [
            "WEAPON",
            "MELEE",
            "SIMPLE",
            "UNARMED",
            "SUBDUAL",
            "STANDARD",
            "MONK",
            "BLUDGEONING",
        ]
        .iter()
        .map(|x| x.to_string())
        .collect(),
        range: 0.0,
        wield: "Light".into(),
        contents: Vec::new(),
        ammo: None,
    };
    let all_w = std::slice::from_ref(&w);
    Element::new("unarmed")
        .with(Element::leaf(
            "total",
            to_hit(s, &w, all_w, Mode::Base, None),
        ))
        .with(Element::leaf(
            "damage",
            damage(s, &w, all_w, Some(Dmg::Basic), None),
        ))
        .with(Element::leaf("critical", "20/x2"))
}

pub fn build(s: &SheetCtx) -> Element {
    let all_w = weapons(s);
    let mut out = Element::new("weapons");
    out.push(unarmed(s));
    let mut i = 0;
    while i < all_w.len() {
        let w = &all_w[i];
        let mut e = Element::new("weapon");
        if w.is("Both") && !w.ranged() {
            e.push(common(s, w, &all_w));
            e.push(melee(s, w, &all_w));
            if let Some(t) = all_w.get(i + 1).filter(|t| t.is("Both") && t.ranged()) {
                e.push(ranges(s, t, &all_w));
                i += 1;
            }
        } else if w.ranged() {
            e.push(common(s, w, &all_w));
            if w.contents.is_empty() {
                e.push(ranges(s, w, &all_w));
            }
            for a in &w.contents {
                let mut with = w.clone();
                with.ammo = Some(a.clone());
                e.push(ranges(s, &with, &all_w));
            }
        } else if w.is("Double") || w.natural() || category(s, w).contains("Non-Standard") {
            e.push(common(s, w, &all_w));
            e.push(simple(s, w, &all_w));
        } else {
            e.push(common(s, w, &all_w));
            e.push(melee(s, w, &all_w));
        }
        out.push(e);
        i += 1;
    }
    out
}
