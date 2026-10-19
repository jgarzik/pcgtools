//
// sheet/gear.rs -- carrying capacity
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use super::skills::name_order;
use super::units::{float_text, pattern, trim_zeros, Units};
use super::{Element, SheetCtx};
use crate::lst::kind::FileKind;
use crate::pcg::character::{is_set_root, DEFAULT_SET_NAME};
use crate::rules::equip::{natural_weapons, EqItem};
use crate::token::pre::Prereq;

/// Weight limits per load category, whole pounds.
pub fn weight_allowance(s: &SheetCtx) -> Element {
    let c = &s.calc;
    let max = c.max_load();
    let limit = |name: &str| {
        let v = (max * c.load_fraction(name).unwrap_or(0.0)) as i64;
        pattern(v as f64, "#.##")
    };
    let mut e = Element::new("weight_allowance");
    e.add("light", limit("Light"));
    e.add("medium", limit("Medium"));
    e.add("heavy", limit("Heavy"));
    e.add("lift_over_head", limit("OverHead"));
    e.add("lift_off_ground", limit("OffGround"));
    e.add("push_drag", limit("PushDrag"));
    e
}

/// Where a copy of an unworn item carrying temporary bonuses is.
pub(super) const TEMP_BONUS_PLACE: &str = "Temp Bonus";

/// An item placed in the character's equipment set.
pub(super) struct Placed<'a> {
    pub item: EqItem<'a>,
    pub id: String,
    /// set name of the top-level node the item is under
    pub root: String,
    pub parent: Option<usize>,
    pub equipped: bool,
    pub carried: f64,
    pub qty: f64,
}

/// Master items (one per saved item) and the items placed in the
/// calculation equipment set, in set order.
pub(super) fn placed<'a>(s: &SheetCtx<'a>) -> (Vec<EqItem<'a>>, Vec<Placed<'a>>) {
    placed_in(s, &s.ch.calc_equip_set(), s.ch.use_temp_mods())
}

/// Master items and the items placed in an equipment set, with copies
/// of those carrying temporary bonuses if `temp`.
fn placed_in<'a>(s: &SheetCtx<'a>, set: &str, temp: bool) -> (Vec<EqItem<'a>>, Vec<Placed<'a>>) {
    let c = &s.calc;
    let pcg = &s.ch.pcg;
    let natural = natural_weapons(c);
    let master: Vec<EqItem> = pcg
        .lines_tagged("EQUIPNAME")
        .filter_map(|l| {
            let name = l.head()?.value_text();
            // natural weapons are made from the race and templates
            if let Some(n) = natural
                .iter()
                .find(|n| n.long_name().eq_ignore_ascii_case(&name))
            {
                let mut n = n.clone();
                n.qty = num(l.text("QUANTITY").as_deref());
                n.output_index = l
                    .text("OUTPUTORDER")
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0);
                return Some(n);
            }
            EqItem::from_pcg(c, l)
        })
        .fold(Vec::new(), |mut acc: Vec<EqItem>, it| {
            // an item saved twice under one name is one item
            match acc
                .iter_mut()
                .find(|x| x.long_name().eq_ignore_ascii_case(&it.long_name()))
            {
                Some(x) => x.qty = it.qty,
                None => acc.push(it),
            }
            acc
        });
    let prefix = format!("{}.", set);
    let mut sets: Vec<(String, String, Option<String>, f64)> = pcg
        .lines_tagged("EQUIPSET")
        .filter_map(|l| {
            let name = l.head()?.value_text();
            let id = l.text("ID")?;
            Some((
                id,
                name,
                l.text("VALUE"),
                num(l.text("QUANTITY").as_deref()),
            ))
        })
        .filter(|(id, _, _, _)| id.starts_with(&prefix))
        .collect();
    sets.sort_by_key(|a| a.0.to_lowercase());
    let names: Vec<(String, String)> = sets
        .iter()
        .map(|(i, n, _, _)| (i.clone(), n.clone()))
        .collect();
    let mut out: Vec<Placed> = Vec::new();
    for (id, _name, value, qty) in &sets {
        let Some(value) = value else { continue };
        let found = natural
            .iter()
            .find(|m| m.long_name().eq_ignore_ascii_case(value))
            .cloned()
            .or_else(|| {
                master
                    .iter()
                    .find(|m| m.long_name().eq_ignore_ascii_case(value))
                    .cloned()
            })
            .or_else(|| {
                c.ds.get(FileKind::Equipment, value)
                    .or_else(|| {
                        c.ds.get(FileKind::Equipment, &c.migrate("EQUIPMENT", value))
                    })
                    .map(|o| EqItem::from_data(c, o))
            });
        let Some(mut item) = found else { continue };
        if let Some(m) = master
            .iter()
            .find(|m| m.long_name().eq_ignore_ascii_case(&item.long_name()))
        {
            item.output_index = m.output_index;
        }
        item.qty = *qty;
        let parts: Vec<&str> = id.split('.').collect();
        let root_id = parts.iter().take(3).cloned().collect::<Vec<_>>().join(".");
        let root = names
            .iter()
            .find(|(i, _)| *i == root_id)
            .map(|(_, n)| n.clone())
            .unwrap_or_default();
        let parent_id = (parts.len() > 3).then(|| parts[..parts.len() - 1].join("."));
        let parent = parent_id.and_then(|p| out.iter().position(|x| x.id == p));
        let lroot = root.to_lowercase();
        let (equipped, carried) = if lroot.starts_with("not carried") {
            (false, 0.0)
        } else if lroot.starts_with("carried") || parts.len() > 3 {
            (false, *qty)
        } else {
            (true, *qty)
        };
        out.push(Placed {
            item,
            id: id.clone(),
            root,
            parent,
            equipped,
            carried,
            qty: *qty,
        });
    }
    // copies of items carrying temporary bonuses
    // (kept when switched off: only the character loses their bonuses)
    if temp {
        for tb in s.ch.temp_bonuses() {
            if tb.on_pc() || !tb.loads(s.ch) {
                continue;
            }
            let (source, bonuses, target) = (tb.key, tb.bonuses, tb.target);
            // the first item of that name, which may be an earlier copy
            let Some(i) = out
                .iter()
                .position(|p| p.item.long_name().eq_ignore_ascii_case(&target))
            else {
                continue;
            };
            let p = &out[i];
            let id = format!("{}.temp", p.id);
            if !p.equipped {
                // listed apart, outside any container, worth nothing
                out.push(Placed {
                    item: p.item.temporary_copy(&source, bonuses),
                    id,
                    root: TEMP_BONUS_PLACE.into(),
                    parent: None,
                    equipped: false,
                    carried: p.carried,
                    qty: p.qty,
                });
            } else if p.item.is_type(c, "WEAPON") {
                // a weapon is listed twice, the copy worth nothing
                out.push(Placed {
                    item: p.item.temporary_copy(&source, bonuses),
                    id,
                    root: p.root.clone(),
                    parent: p.parent,
                    equipped: true,
                    carried: p.carried,
                    qty: p.qty,
                });
            } else {
                // anything else worn is replaced by its copy
                let mut item = p.item.clone();
                item.temporary = Some((source, bonuses));
                out[i].item = item;
                out[i].id = id;
            }
        }
    }
    (master, out)
}

fn num(v: Option<&str>) -> f64 {
    v.and_then(|x| x.trim().parse().ok()).unwrap_or(0.0)
}

/// Weight of what a container holds (its CONTAINS reduction applied).
fn contained_weight(s: &SheetCtx, items: &[Placed], i: usize, effective: bool) -> f64 {
    let c = &s.calc;
    let spec = items[i].item.data.get("CONTAINS").unwrap_or("");
    let first = spec.split('|').next().unwrap_or("");
    if first.starts_with('*') && !effective {
        return 0.0;
    }
    let mut total = 0.0;
    for (j, p) in items.iter().enumerate() {
        if p.parent != Some(i) {
            continue;
        }
        let w = p.item.weight(c);
        if items.iter().any(|x| x.parent == Some(j)) {
            total += w + contained_weight(s, items, j, effective);
        } else {
            total += w * p.qty;
        }
    }
    if let Some((pct, _)) = first.trim_start_matches('*').split_once('%') {
        if let Ok(p) = pct.parse::<f64>() {
            total *= p / 100.0;
        }
    }
    total
}

/// Number of an item carried: none if under a set that is not carried.
fn number_carried(items: &[Placed], i: usize) -> f64 {
    let p = &items[i];
    let Some(mut up) = p.parent else {
        return p.carried;
    };
    if p.equipped {
        return p.carried;
    }
    loop {
        let a = &items[up];
        if a.equipped {
            return p.carried;
        }
        match a.parent {
            Some(x) => up = x,
            None => {
                return if a.carried as i64 != 0 {
                    p.carried
                } else {
                    0.0
                };
            }
        }
    }
}

fn item_element(s: &SheetCtx, items: &[Placed], i: usize, full: bool, qty: f64) -> Element {
    let c = &s.calc;
    let units = Units::from_gamemode(s.ctx.gamemode.as_ref());
    let p = &items[i];
    let it = &p.item;
    let mut e = Element::new("item");
    if full {
        e.add("longname", it.long_name());
        e.add("id", "");
    }
    e.add("name", it.output_name());
    let carried = number_carried(items, i);
    e.add("carried", float_text(carried));
    e.add("charges", "");
    e.add("charges_used", "");
    let kids: Vec<usize> = (0..items.len())
        .filter(|j| items[*j].parent == Some(i))
        .collect();
    let contents = if it.is_container() {
        let mut t = units.weight(contained_weight(s, items, i, true));
        for j in &kids {
            if items[*j].qty > 0.0 {
                t.push_str(&format!(
                    ", {} {}",
                    trim_zeros(items[*j].qty),
                    items[*j].item.output_name()
                ));
            }
        }
        t
    } else {
        String::new()
    };
    e.add("contents", contents);
    if full {
        e.add("contents_num", kids.len());
        let cw = if kids.is_empty() {
            0.0
        } else {
            contained_weight(s, items, i, true)
        };
        e.add("content_weight", pattern(cw, "#.##"));
    }
    e.add("cost", trim_zeros(it.cost(c)));
    e.add("equipped", if p.equipped { "Y" } else { "N" });
    let location = match p.parent {
        Some(x) => items[x].item.output_name(),
        None if p.equipped => "Equipped".into(),
        None if carried as i64 > 0 => "Carried".into(),
        None => String::new(),
    };
    e.add("location", location);
    e.add("maxcharges", 0);
    e.add("note", &it.note);
    e.add("quantity", trim_zeros(qty));
    if full {
        let bq = num(it.data.get("BASEQTY")).max(1.0);
        e.add("checkbox", trim_zeros(qty * bq));
    }
    let (long, short) = it.size_names(c);
    e.push(
        Element::new("size")
            .with(Element::leaf("long", long))
            .with(Element::leaf("short", short)),
    );
    e.add("special_properties", it.special_properties(c));
    e.add("type", it.types(c).join("."));
    e.add("weight", pattern(it.weight(c), "#.##"));
    e.add("bonuslist", "");
    e
}

/// Placed items in sheet order, with quantities merged for identical
/// items in the same place.
pub(super) fn merged(items: &[Placed]) -> Vec<(usize, f64)> {
    merged_by(items, false)
}

/// [`merged`]; with `by_name`, identical items merge wherever they are.
pub(super) fn merged_by(items: &[Placed], by_name: bool) -> Vec<(usize, f64)> {
    let parent_name = |p: &Placed| {
        p.parent
            .map(|x| items[x].item.name.clone())
            .unwrap_or_default()
    };
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by(|a, b| {
        let (pa, pb) = (&items[*a], &items[*b]);
        let idx = |p: &Placed| {
            if p.item.output_index == 0 {
                999
            } else {
                p.item.output_index
            }
        };
        idx(pa)
            .cmp(&idx(pb))
            .then_with(|| pa.item.sub_index.cmp(&pb.item.sub_index))
            .then_with(|| name_order(&pa.item.name, &pb.item.name))
            .then_with(|| name_order(&parent_name(pa), &parent_name(pb)))
    });
    let mut list: Vec<(usize, f64)> = order.into_iter().map(|i| (i, items[i].qty)).collect();
    let place = |p: &Placed| (p.equipped, p.root.to_lowercase(), p.parent.is_some());
    let mut i = 0;
    while i < list.len() {
        let a = list[i].0;
        if !items[a].item.is_container() && items[a].item.temporary.is_none() {
            let mut j = i + 1;
            while j < list.len() {
                let b = list[j].0;
                if items[b].item.long_name() == items[a].item.long_name()
                    && (by_name
                        || place(&items[b]) == place(&items[a])
                            && parent_name(&items[b]) == parent_name(&items[a]))
                    && items[b].item.temporary.is_none()
                {
                    list[i].1 += list[j].1;
                    list.remove(j);
                }
                // the element after a merged one is passed over
                j += 1;
            }
        }
        i += 1;
    }
    list.retain(|(i, _)| items[*i].item.output_index != -1);
    list
}

pub fn equipment(s: &SheetCtx) -> Element {
    let c = &s.calc;
    let units = Units::from_gamemode(s.ctx.gamemode.as_ref());
    let (master, items) = placed(s);
    let mut out = Element::new("equipment");
    for (i, qty) in merged(&items) {
        out.push(item_element(s, &items, i, true, qty));
    }
    // carried weight: items carried at top level, containers with what
    // they hold
    let mut weight = 0.0;
    // unless the game mode counts it, the first clothing worn is free
    let mut free_clothing = !c.rule("CLOTHINGENCUMBRANCE");
    for (i, p) in items.iter().enumerate() {
        if p.parent.is_some() || p.carried <= 0.0 {
            continue;
        }
        let w = p.item.weight(c);
        if items.iter().any(|x| x.parent == Some(i)) {
            weight += w + contained_weight(s, &items, i, false);
        } else if free_clothing && p.equipped && p.item.is_type(c, "CLOTHING") {
            weight += w * (p.carried - 1.0).max(0.0);
            free_clothing = false;
        } else {
            weight += w * p.carried;
        }
    }
    // equipment granted outright is not listed or carried, but its
    // value counts twice (once granted, once as owned)
    let value: f64 = master.iter().map(|m| m.cost(c) * m.qty).sum::<f64>()
        + auto_equipment(s)
            .iter()
            .map(|m| 2.0 * m.cost(c) * m.qty)
            .sum::<f64>();
    let currency = s
        .ctx
        .gamemode
        .as_ref()
        .and_then(|g| g.misc("CURRENCYUNITABBREV"))
        .unwrap_or("gp")
        .to_string();
    let max = c.max_load();
    let ratio = if max > 0.0 { weight / max } else { 0.0 };
    let no_load = !c.rule("SYS_LDPACSK") && c.rules_known("SYS_LDPACSK");
    let load = if no_load || ratio <= c.load_fraction("Light").unwrap_or(1.0 / 3.0) {
        "Light"
    } else if ratio <= c.load_fraction("Medium").unwrap_or(2.0 / 3.0) {
        "Medium"
    } else if ratio <= c.load_fraction("Heavy").unwrap_or(1.0) {
        "Heavy"
    } else {
        "Overload"
    };
    out.push(
        Element::new("total")
            .with(Element::leaf("weight", units.weight(weight)))
            .with(Element::leaf(
                "value",
                format!(
                    "{} {}",
                    trim_zeros((value * 10000.0).round() / 10000.0),
                    currency
                ),
            ))
            .with(Element::leaf("load", load))
            .with(Element::leaf("capacity", pattern(max, "#.##"))),
    );
    // every set in turn, each with its own items and temporary-bonus
    // setting (bonuses stay those of the set used)
    let mut sets = Element::new("equipmentsets");
    let roots: Vec<(String, String, bool)> =
        s.ch.pcg
            .lines_tagged("EQUIPSET")
            .filter_map(|l| {
                let id = l.text("ID")?;
                is_set_root(&id).then(|| {
                    (
                        id,
                        l.head().map(|h| h.value_text()).unwrap_or_default(),
                        // a set lists copies carrying temporary bonuses
                        // only when marked to use them
                        l.text("USETEMPMODS")
                            .is_some_and(|v| v.trim_end().ends_with('Y')),
                    )
                })
            })
            .collect();
    if roots.is_empty() {
        // a character without equipment sets gets a default one
        let mut set = Element::new("equipmentset").attr("name", DEFAULT_SET_NAME);
        for (i, qty) in merged(&items) {
            set.push(item_element(s, &items, i, false, qty));
        }
        sets.push(set);
    }
    for (id, name, temp) in roots {
        let (_, items) = placed_in(s, &id, temp);
        let mut set = Element::new("equipmentset").attr("name", name);
        for (i, qty) in merged(&items) {
            set.push(item_element(s, &items, i, false, qty));
        }
        sets.push(set);
    }
    out.push(sets);
    out
}

type PlacedTest<'t> = dyn Fn(&Placed) -> bool + 't;

/// Armor, shields and other items that change armor class.
pub fn protection(s: &SheetCtx) -> Element {
    let c = &s.calc;
    let (_, items) = placed(s);
    let mut out = Element::new("protection");
    let order = merged(&items);
    let alters_ac = |p: &Placed| p.item.bonus(c, "COMBAT", "AC", true) != 0.0;
    let groups: [(&str, &PlacedTest); 3] = [
        ("armor", &|p: &Placed| {
            p.item.is_type(c, "Armor") && alters_ac(p)
        }),
        ("shield", &|p: &Placed| p.item.is_type(c, "Shield")),
        ("item", &|p: &Placed| {
            !p.item.is_type(c, "Armor") && !p.item.is_type(c, "Shield") && alters_ac(p)
        }),
    ];
    for (tag, keep) in groups {
        for (i, _) in &order {
            let p = &items[*i];
            if !keep(p) {
                continue;
            }
            let it = &p.item;
            let signed = super::signed;
            let mut e = Element::new(tag);
            e.add(
                "name",
                format!(
                    "{}{}{}",
                    if p.equipped { "*" } else { "" },
                    it.output_name(),
                    it.applied_name()
                ),
            );
            let ac = it.bonus(c, "COMBAT", "AC", true) as i64;
            e.add("acbonus", signed(ac));
            let acp = (num(it.data.get("ACCHECK")) + it.bonus(c, "EQMARMOR", "ACCHECK", true))
                .min(0.0) as i64;
            e.add("accheck", signed(acp));
            e.add("baseac", signed(it.bonus(c, "COMBAT", "AC", false) as i64));
            let edr = (num(it.data.get("EDR")) + it.bonus(c, "EQMARMOR", "EDR", true)).max(0.0);
            e.add("edr", signed(edr as i64));
            let maxdex = if tag == "armor" {
                let md = it
                    .data
                    .get("MAXDEX")
                    .and_then(|v| v.trim().parse::<f64>().ok())
                    .unwrap_or(100.0)
                    + it.bonus(c, "EQMARMOR", "MAXDEX", true);
                let md = md.clamp(0.0, 100.0);
                if md >= 100.0 {
                    String::new()
                } else {
                    signed(md as i64)
                }
            } else {
                String::new()
            };
            e.add("maxdex", maxdex);
            e.add("move", "");
            let sf = (num(it.data.get("SPELLFAILURE"))
                + it.bonus(c, "EQMARMOR", "SPELLFAILURE", true))
            .max(0.0) as i64;
            e.add("spellfail", sf);
            e.add("special_properties", it.special_properties(c));
            e.add("totalac", signed(ac));
            let kind = if it.is_type(c, "Light") {
                "Light"
            } else if it.is_type(c, "Medium") {
                "Medium"
            } else if it.is_type(c, "Heavy") {
                "Heavy"
            } else if it.is_type(c, "Shield") {
                "Shield"
            } else if it.is_type(c, "Extra") {
                "Extra"
            } else {
                ""
            };
            e.add("type", kind);
            e.add("wt", trim_zeros(it.weight(c) as f32 as f64));
            out.push(e);
        }
    }
    out
}

/// Equipment granted by AUTO:EQUIP whose prerequisites pass.
fn auto_equipment<'a>(s: &SheetCtx<'a>) -> Vec<EqItem<'a>> {
    let c = &s.calc;
    let is_pre =
        |i: &str| Prereq::is_pre_tag(i.split(':').next().unwrap_or("")) || i.starts_with("!PRE");
    let mut out = Vec::new();
    for src in &c.sources {
        for t in src.tokens.iter().filter(|t| t.tag == "AUTO") {
            let Some(rest) = t.value.strip_prefix("EQUIP|") else {
                continue;
            };
            let parts: Vec<&str> = rest.split('|').collect();
            let ok = parts
                .iter()
                .filter(|i| is_pre(i))
                .filter_map(|i| Prereq::parse(i).ok())
                .all(|p| c.passes(&p, Some(src)));
            if !ok {
                continue;
            }
            for name in parts.iter().filter(|i| !is_pre(i)) {
                if let Some(o) = c.ds.get(FileKind::Equipment, name) {
                    out.push(EqItem::from_data(c, o));
                }
            }
        }
    }
    out
}
