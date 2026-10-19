//
// rules/equip.rs -- equipment items: customization, size, cost, weight
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! An equipment item as a character holds it: a data item, possibly
//! customized (renamed, resized, with equipment modifiers and choices,
//! cost and weight adjustments), with its quantity and place in an
//! equipment set.  Cost, weight, types and special properties are
//! computed from the data; the cost and weight saved in a character
//! file are not used.

use super::calc::Calc;
use crate::dataset::Object;
use crate::lst::kind::FileKind;
use crate::pcg::PcgLine;
use crate::token::bonus::Bonus;
use crate::token::pre::{PreBody, Prereq};
use std::borrow::Cow;

/// An equipment modifier on an item, with its choices.
#[derive(Debug, Clone)]
pub struct EqMod<'a> {
    pub obj: &'a Object,
    pub choices: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EqItem<'a> {
    /// display name
    pub name: String,
    pub key: String,
    pub output_name: Option<String>,
    /// the data item this one is built from (or a made-up one, for
    /// natural weapons)
    pub data: Cow<'a, Object>,
    /// the item resizing is measured from (BASEITEM, else the item)
    pub base: Cow<'a, Object>,
    /// a customized item not in the data as such
    pub custom: bool,
    pub size: String,
    /// cost after resizing, if resized
    pub resized_cost: Option<f64>,
    /// weight, resized
    pub weight: f64,
    pub cost_mod: f64,
    pub weight_mod: f64,
    /// the item's own special properties (data, or a saved override)
    pub sprops: Vec<String>,
    pub eqmods: Vec<EqMod<'a>>,
    pub alt_eqmods: Vec<EqMod<'a>>,
    pub qty: f64,
    pub output_index: i64,
    /// order among items of the same output index (natural weapons)
    pub sub_index: i64,
    /// shown after the name in parentheses (`Natural/Primary`)
    pub modified: Option<String>,
    /// a copy carrying temporary bonuses: the name of what applies
    /// them, and the bonuses (BONUS values)
    pub temporary: Option<(String, Vec<String>)>,
    pub note: String,
}

fn num(v: Option<&str>) -> f64 {
    v.and_then(|x| x.trim().parse().ok()).unwrap_or(0.0)
}

fn upper(s: &str) -> String {
    s.to_ascii_uppercase()
}

impl<'a> EqItem<'a> {
    /// An item as defined in the data.
    pub fn from_data(calc: &Calc<'a>, obj: &'a Object) -> EqItem<'a> {
        let base = obj
            .get("BASEITEM")
            .and_then(|b| calc.ds.get(FileKind::Equipment, b))
            .unwrap_or(obj);
        let mut it = EqItem {
            name: obj.name.clone(),
            key: obj.key.clone(),
            output_name: obj.get("OUTPUTNAME").map(str::to_string),
            data: Cow::Borrowed(obj),
            base: Cow::Borrowed(base),
            custom: false,
            size: obj.get("SIZE").unwrap_or("").to_string(),
            resized_cost: None,
            weight: num(obj.get("WT")),
            cost_mod: 0.0,
            weight_mod: 0.0,
            sprops: obj.values("SPROP").map(str::to_string).collect(),
            eqmods: Vec::new(),
            alt_eqmods: Vec::new(),
            qty: 1.0,
            output_index: 0,
            sub_index: 0,
            modified: None,
            temporary: None,
            note: String::new(),
        };
        if it.size.is_empty() {
            it.size = default_size(calc);
        }
        for v in obj.values("EQMOD") {
            it.add_eqmods(calc, v, false);
        }
        for v in obj.values("ALTEQMOD") {
            it.add_eqmods(calc, v, true);
        }
        it
    }

    /// An item from a character file's EQUIPNAME line.
    pub fn from_pcg(calc: &Calc<'a>, line: &PcgLine) -> Option<EqItem<'a>> {
        let ds = calc.ds;
        let key = line.head()?.value_text();
        let lookup = |k: &str| {
            ds.get(FileKind::Equipment, k)
                .or_else(|| ds.get(FileKind::Equipment, &calc.migrate("EQUIPMENT", k)))
        };
        let direct = lookup(&key).filter(|o| !o.is_type("CUSTOM"));
        let cust = line.get("CUSTOMIZATION");
        let mut item = match (direct, cust) {
            (Some(o), None) => EqItem::from_data(calc, o),
            (direct, Some(c)) => {
                let base_key = c
                    .children()
                    .iter()
                    .find(|e| e.tag == "BASEITEM")
                    .map(|e| e.value_text())
                    .unwrap_or_default();
                let data: String = c
                    .children()
                    .iter()
                    .find(|e| e.tag == "DATA")
                    .map(|e| e.value_text())
                    .unwrap_or_default();
                // the item is itself a data item with a saved
                // customization, or a customized copy of a base item
                let same = direct.is_some_and(|o| {
                    let b = o
                        .get("BASEITEM")
                        .and_then(|b| ds.get(FileKind::Equipment, b));
                    let bname = b.map(|b| b.name.as_str()).unwrap_or(o.key.as_str());
                    bname.eq_ignore_ascii_case(&base_key)
                });
                let mut it = match (same, direct) {
                    (true, Some(o)) => EqItem::from_data(calc, o),
                    _ => {
                        let b = lookup(&base_key)?;
                        if b.is_type("CUSTOM") {
                            return None;
                        }
                        let mut it = EqItem::from_data(calc, b);
                        it.custom = true;
                        it
                    }
                };
                it.eqmods.clear();
                it.load(calc, &data);
                if it.custom {
                    it.output_name = None;
                }
                it
            }
            (None, None) => return None,
        };
        item.qty = num(line.text("QUANTITY").as_deref());
        item.output_index = line
            .text("OUTPUTORDER")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0);
        item.note = line.text("NOTE").unwrap_or_default();
        Some(item)
    }

    /// Apply a saved customization (`NAME=..$SIZE=..$EQMOD=..`).
    fn load(&mut self, calc: &Calc<'a>, data: &str) {
        let mut new_size = None;
        let mut sprop_cleared = false;
        for part in data.split('$') {
            if let Some(v) = part.strip_prefix("NAME=") {
                self.name = v.to_string();
                self.key = v.to_string();
                self.output_name = Some(v.to_string());
            } else if let Some(v) = part.strip_prefix("KEY=") {
                self.key = v.to_string();
            } else if let Some(v) = part.strip_prefix("SIZE=") {
                new_size = Some(v.to_string());
            } else if let Some(v) = part.strip_prefix("EQMOD=") {
                self.add_eqmods(calc, v, false);
            } else if let Some(v) = part.strip_prefix("ALTEQMOD=") {
                self.add_eqmods(calc, v, true);
            } else if let Some(v) = part.strip_prefix("SPROP=") {
                if !sprop_cleared {
                    self.sprops.clear();
                    sprop_cleared = true;
                }
                self.sprops.push(v.to_string());
            } else if let Some(v) = part.strip_prefix("COSTMOD=") {
                self.cost_mod = v.trim().parse().unwrap_or(0.0);
            } else if let Some(v) = part.strip_prefix("WEIGHTMOD=") {
                self.weight_mod = v.trim().parse().unwrap_or(0.0);
            }
        }
        if let Some(sz) = new_size {
            if !sz.eq_ignore_ascii_case(&self.size) {
                self.resize(calc, &sz);
            }
        }
    }

    /// Add modifiers written `KEY|choice|choice.KEY2...`.
    fn add_eqmods(&mut self, calc: &Calc<'a>, list: &str, alt: bool) {
        for m in list.split('.') {
            let mut parts = m.split('|');
            let key = parts.next().unwrap_or("");
            if key.is_empty() || key.eq_ignore_ascii_case("NONE") {
                continue;
            }
            let rest: Vec<String> = parts.map(|c| c.replace('=', "|")).collect();
            if key == "_WEIGHTADD" {
                self.weight_mod = rest
                    .first()
                    .map(|v| v.replace(',', "."))
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.0);
                continue;
            }
            if key == "_DAMAGE" {
                continue;
            }
            let Some(obj) = calc.find_eqmod(key) else {
                continue;
            };
            let head = if alt {
                &mut self.alt_eqmods
            } else {
                &mut self.eqmods
            };
            let i = match head.iter().position(|e| std::ptr::eq(e.obj, obj)) {
                Some(i) => i,
                None => {
                    head.push(EqMod {
                        obj,
                        choices: Vec::new(),
                    });
                    head.len() - 1
                }
            };
            if obj.has("CHOOSE") {
                head[i].choices.extend(rest);
            }
        }
    }

    /// Resize: cost and weight scale by the sizes' multipliers.
    fn resize(&mut self, calc: &Calc<'a>, new: &str) {
        let b = self.base.clone();
        let types = self.types(calc);
        let base_size = b
            .get("SIZE")
            .map(str::to_string)
            .unwrap_or_else(|| default_size(calc));
        let cost_ratio = size_bonus(calc, new, "ITEMCOST", &types)
            / size_bonus(calc, &base_size, "ITEMCOST", &types);
        let weight_ratio = size_bonus(calc, new, "ITEMWEIGHT", &types)
            / size_bonus(calc, &base_size, "ITEMWEIGHT", &types);
        self.resized_cost = Some(num(b.get("COST")) * cost_ratio);
        self.weight = num(b.get("WT")) * weight_ratio;
        self.size = new.to_string();
    }

    pub fn is_type(&self, calc: &Calc<'a>, t: &str) -> bool {
        self.types(calc).iter().any(|x| x.eq_ignore_ascii_case(t))
    }

    /// The item's types, upper case: its own, CUSTOM, then those its
    /// modifiers add; MAGIC first.
    pub fn types(&self, _calc: &Calc<'a>) -> Vec<String> {
        let mut l: Vec<String> = Vec::new();
        for t in self.data.types() {
            let t = upper(t);
            if !l.contains(&t) {
                l.push(t);
            }
        }
        if self.custom && !l.iter().any(|t| t == "CUSTOM") {
            l.push("CUSTOM".into());
        }
        if self.temporary.is_some() && !l.iter().any(|t| t == "TEMPORARY") {
            l.push("TEMPORARY".into());
        }
        let mut added: Vec<String> = Vec::new();
        for m in &self.eqmods {
            if m.obj.key.eq_ignore_ascii_case("ADDTYPE") {
                for c in &m.choices {
                    let c = upper(c);
                    if !l.contains(&c) && !added.contains(&c) {
                        added.push(c);
                    }
                }
            }
        }
        for m in &self.eqmods {
            for v in m.obj.values("ARMORTYPE") {
                let (src, dst) = v.split_once('|').unwrap_or((v, ""));
                let src = upper(src);
                if let Some(i) = l.iter().position(|t| *t == src) {
                    if dst.is_empty() {
                        l.remove(i);
                    } else {
                        l[i] = upper(dst);
                    }
                    added.retain(|t| *t != src);
                }
            }
            for t in m.obj.values("ITYPE").flat_map(|v| v.split('.')) {
                let t = upper(t);
                if t.is_empty() {
                    continue;
                }
                let both_melee =
                    l.contains(&"BOTH".to_string()) && l.contains(&"MELEE".to_string());
                if both_melee && (t == "RANGED" || t == "THROWN") {
                    continue;
                }
                if !l.contains(&t) && !added.contains(&t) {
                    added.push(t);
                }
            }
        }
        l.extend(added);
        if let Some(i) = l.iter().position(|t| t == "MAGIC") {
            let m = l.remove(i);
            l.insert(0, m);
        }
        l
    }

    fn base_qty(&self) -> f64 {
        let q = num(self.data.get("BASEQTY"));
        if q > 0.0 {
            q
        } else {
            1.0
        }
    }

    fn icount(&self, m: &EqMod) -> f64 {
        let select: f64 = m
            .obj
            .get("SELECT")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(1.0)
            .max(1.0);
        ((m.choices.len() as f64) / select).floor().max(1.0)
    }

    fn plus_total(&self, head: &[EqMod]) -> f64 {
        head.iter()
            .map(|m| num(m.obj.get("PLUS")) * self.icount(m))
            .sum()
    }

    /// Terms available to equipment formulas.
    fn term(&self, calc: &Calc<'a>, name: &str, head: &[EqMod]) -> Option<f64> {
        match upper(name).as_str() {
            "BASECOST" => Some(num(self.data.get("COST"))),
            "WT" => Some(self.weight),
            "PLUSTOTAL" => Some(self.plus_total(&self.eqmods)),
            "HEADPLUSTOTAL" => Some(self.plus_total(head)),
            "ALTPLUSTOTAL" => Some(self.plus_total(&self.alt_eqmods)),
            "BASEQTY" => Some(self.base_qty()),
            "SIZE" | "EQUIP.SIZE.INT" => calc
                .ds
                .objects(FileKind::Size)
                .iter()
                .position(|o| o.key.eq_ignore_ascii_case(&self.size))
                .map(|i| i as f64),
            _ => None,
        }
    }

    fn eval(&self, calc: &Calc<'a>, formula: &str, head: &[EqMod]) -> f64 {
        calc.eval_with(formula, &|n| self.term(calc, n, head))
    }

    /// Does a prerequisite on a modifier's bonus hold for this item?
    fn passes(&self, calc: &Calc<'a>, p: &Prereq, choice: Option<&str>) -> bool {
        let r = match (&p.body, p.kind.as_str()) {
            (PreBody::Mult { count, subs, .. }, _) => {
                let need: usize = count.trim().parse().unwrap_or(1);
                subs.iter().filter(|q| self.passes(calc, q, choice)).count() >= need
            }
            (PreBody::Args(args), "TYPE") => {
                let need: usize = args.first().and_then(|a| a.parse().ok()).unwrap_or(1);
                let hits = args
                    .iter()
                    .skip(1)
                    .filter(|a| {
                        if let Some(t) = a.strip_prefix("EQMODTYPE=") {
                            self.eqmods.iter().any(|m| m.obj.is_type(t))
                        } else if let Some(k) = a.strip_prefix("EQMOD=") {
                            self.eqmods
                                .iter()
                                .any(|m| m.obj.key.eq_ignore_ascii_case(k))
                        } else {
                            let t = a.strip_prefix("TYPE=").unwrap_or(a);
                            t.split('.').all(|x| self.is_type(calc, x))
                        }
                    })
                    .count();
                hits >= need
            }
            (PreBody::Args(args), "WIELD") => {
                let w = self.data.get("WIELD").unwrap_or("");
                args.iter()
                    .skip(1)
                    .any(|a| a.split('|').any(|x| x.eq_ignore_ascii_case(w)))
            }
            (PreBody::Args(args), k) if k.starts_with("VAR") => {
                let op = &k[3..];
                let vals: Vec<f64> = args
                    .iter()
                    .map(|a| {
                        let a = match choice {
                            Some(c) => a.replace("%CHOICE", c),
                            None => a.clone(),
                        };
                        self.eval(calc, &a, &self.eqmods)
                    })
                    .collect();
                vals.chunks(2).all(|c| match c {
                    [l, r] => match op {
                        "GT" => l > r,
                        "GTEQ" => l >= r,
                        "LT" => l < r,
                        "LTEQ" => l <= r,
                        "EQ" => l == r,
                        "NEQ" => l != r,
                        _ => true,
                    },
                    _ => true,
                })
            }
            _ => return calc.passes(p, None),
        };
        r != p.negated
    }

    /// BONUS:ITEMCOST of a modifier that applies to this item.
    fn item_cost_bonus(&self, calc: &Calc<'a>, m: &EqMod, n: f64) -> f64 {
        let mut total = 0.0;
        for v in m.obj.values("BONUS") {
            let Ok(b) = Bonus::parse(v) else { continue };
            if !b.category.eq_ignore_ascii_case("ITEMCOST") {
                continue;
            }
            let choices: Vec<Option<&str>> = if v.contains("%CHOICE") && !m.choices.is_empty() {
                m.choices.iter().map(|c| Some(c.as_str())).collect()
            } else {
                vec![None]
            };
            for choice in choices {
                let applies = b.targets.iter().any(|t| {
                    let t = t.strip_prefix("TYPE.").or_else(|| t.strip_prefix("TYPE="));
                    t.is_some_and(|t| t.split('.').all(|x| self.is_type(calc, x)))
                });
                if !applies || !b.prereqs().all(|p| self.passes(calc, p, choice)) {
                    continue;
                }
                let f = match choice {
                    Some(c) => b.formula.replace("%CHOICE", &choice_number(c)),
                    None => b.formula.clone(),
                };
                total += self.eval(calc, &f, &self.eqmods);
            }
        }
        total * n
    }

    /// Cost of one item.
    pub fn cost(&self, calc: &Calc<'a>) -> f64 {
        let bq = self.base_qty();
        let mut pre = 0.0;
        for head in [&self.eqmods, &self.alt_eqmods] {
            for m in head.iter() {
                let n = bq * self.icount(m);
                if let Some(f) = m.obj.get("COSTPRE") {
                    pre += self.eval(calc, f, head) * n;
                }
                pre += self.item_cost_bonus(calc, m, n);
            }
        }
        let item_cost = self
            .resized_cost
            .unwrap_or_else(|| num(self.data.get("COST")))
            + pre;

        let mut doublable: Vec<f64> = Vec::new();
        let mut non_double = 0.0;
        let mut post = 0.0;
        for (hi, head) in [&self.eqmods, &self.alt_eqmods].iter().enumerate() {
            for m in head.iter() {
                let f = m.obj.get("COST").unwrap_or("0");
                let f = f.replace("BASECOST", &format!("(BASECOST/{})", bq));
                let dbl = cost_doubles(m.obj);
                let mut c = 0.0;
                let spell = |name: &str| {
                    calc.ds
                        .get(FileKind::Spell, name)
                        .map(|sp| (num(sp.get("COST")), num(sp.get("XPCOST"))))
                };
                let per_choice =
                    !m.choices.is_empty() && substitute_with(&f, &m.choices[0], &spell) != f;
                if per_choice {
                    for a in &m.choices {
                        let ca = self.eval(calc, &substitute_with(&f, a, &spell), head);
                        c += ca;
                        if dbl {
                            if hi == 0 {
                                doublable.push(ca);
                            }
                        } else {
                            non_double += ca;
                        }
                    }
                } else {
                    c = self.eval(calc, &f, head);
                    if dbl {
                        if hi == 0 {
                            doublable.push(c);
                        }
                    } else {
                        non_double += c;
                    }
                }
                if m.obj.is_type("BaseMaterial") {
                    c *= bq;
                }
                post += c;
            }
        }
        let t = |x: &str| self.is_type(calc, x);
        if !(t("AMMUNITION") || t("ARMOR") || t("SHIELD") || t("WEAPON")) {
            let limited = [
                "HEADGEAR", "EYEGEAR", "CAPE", "AMULET", "SUIT", "ROBE", "SHIRT", "BRACER",
                "GLOVE", "RING", "BELT", "BOOT",
            ]
            .iter()
            .any(|x| t(x));
            if !limited {
                if !["MANTLE", "POTION", "SCROLL", "STAFF", "WAND"]
                    .iter()
                    .any(|x| t(x))
                {
                    post = 2.0 * (post - non_double) + non_double;
                }
            } else {
                doublable.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                doublable.pop();
                post += doublable.iter().sum::<f64>();
            }
        }
        if post >= 0.0 && post + item_cost + self.cost_mod < 0.0 {
            return 0.0;
        }
        round_cents(post + item_cost + self.cost_mod)
    }

    /// Weight of one item: its weight with the modifiers' multipliers,
    /// divisors and additions.
    pub fn weight(&self, calc: &Calc<'a>) -> f64 {
        let mut w = self.weight;
        let m = self.eqm_bonus(calc, "WEIGHTMULT");
        if m != 0.0 {
            w *= m;
        }
        let d = self.eqm_bonus(calc, "WEIGHTDIV");
        if d != 0.0 {
            w /= d;
        }
        w + self.eqm_bonus(calc, "WEIGHTADD") + self.weight_mod
    }

    /// Total of BONUS:EQM|target on the item and its modifiers.
    pub fn eqm_bonus(&self, calc: &Calc<'a>, target: &str) -> f64 {
        self.bonus(calc, "EQM", target, true)
    }

    /// Total of BONUS:category|target on the item and, if asked, its
    /// modifiers.
    pub fn bonus(&self, calc: &Calc<'a>, category: &str, target: &str, eqmods: bool) -> f64 {
        let mut total = 0.0;
        let mods = if eqmods { &self.eqmods[..] } else { &[] };
        let objs = std::iter::once(&*self.data).chain(mods.iter().map(|m| m.obj));
        for o in objs {
            for v in o.values("BONUS") {
                let Ok(b) = Bonus::parse(v) else { continue };
                if !b.category.eq_ignore_ascii_case(category)
                    || !b.targets.iter().any(|t| t.eq_ignore_ascii_case(target))
                {
                    continue;
                }
                if !b.prereqs().all(|p| self.passes(calc, p, None)) {
                    continue;
                }
                total += self.eval(calc, &b.formula, &self.eqmods);
            }
        }
        total
    }

    /// Special properties: the item's own, then its modifiers'.
    pub fn special_properties(&self, calc: &Calc<'a>) -> String {
        let mut parts: Vec<String> = Vec::new();
        for sp in &self.sprops {
            if let Some(t) = self.sprop_text(calc, sp, None) {
                if !t.is_empty() {
                    parts.push(t);
                }
            }
        }
        let all_heads = |m: &&EqMod| {
            m.obj
                .get("ASSIGNTOALL")
                .is_some_and(|v| v.eq_ignore_ascii_case("YES"))
        };
        let common: Vec<&EqMod> = self.eqmods.iter().filter(all_heads).collect();
        let rest: Vec<&EqMod> = self.eqmods.iter().filter(|m| !all_heads(m)).collect();
        for group in [common, rest] {
            let mut texts: Vec<String> = Vec::new();
            for m in group {
                for sp in m.obj.values("SPROP") {
                    let Some(mut t) = self.sprop_text(calc, sp, Some(m)) else {
                        continue;
                    };
                    for c in &m.choices {
                        t = t.replacen("%CHOICE", c, 1);
                    }
                    if !t.is_empty() {
                        texts.push(t);
                    }
                }
            }
            let mut uniq: Vec<(String, usize)> = Vec::new();
            for t in texts {
                match uniq.iter_mut().find(|(x, _)| *x == t) {
                    Some(e) => e.1 += 1,
                    None => uniq.push((t, 1)),
                }
            }
            let list: Vec<String> = uniq
                .into_iter()
                .map(|(t, n)| if n > 1 { format!("{} ({})", t, n) } else { t })
                .collect();
            if !list.is_empty() {
                parts.push(list.join(", "));
            }
        }
        parts.join(", ")
    }

    fn sprop_text(&self, calc: &Calc<'a>, sp: &str, _m: Option<&EqMod>) -> Option<String> {
        let mut parts = sp.split('|');
        let text = parts.next().unwrap_or("");
        let mut vars = Vec::new();
        for p in parts {
            if Prereq::is_pre_tag(p.split(':').next().unwrap_or("")) || p.starts_with("!PRE") {
                if let Ok(pr) = Prereq::parse(p) {
                    if !self.passes(calc, &pr, None) {
                        return None;
                    }
                }
            } else {
                vars.push(p);
            }
        }
        if vars.is_empty() {
            return Some(text.to_string());
        }
        let vals: Vec<i64> = vars
            .iter()
            .map(|v| self.eval(calc, v, &self.eqmods) as i64)
            .collect();
        if vals.iter().all(|v| *v == 0) {
            return Some(String::new());
        }
        let mut it = vals.iter();
        let mut out = String::new();
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

    /// Name shown on the sheet: the output name with `[NAME]`/`[BASE]`
    /// filled in.
    pub fn output_name(&self) -> String {
        let Some(out) = &self.output_name else {
            return self.name.clone();
        };
        let mut out = out.clone();
        if out.eq_ignore_ascii_case("[BASE]") {
            out = self
                .name
                .split('(')
                .next()
                .unwrap_or(&self.name)
                .trim()
                .to_string();
        }
        if out.contains("[NAME]") {
            let inner = match (self.name.find('('), self.name.rfind(')')) {
                (Some(a), Some(b)) if b > a => {
                    let parts: Vec<&str> = self.name[a + 1..b].split('/').rev().collect();
                    parts.join(" ")
                }
                _ => self.name.clone(),
            };
            out = out.replace("[NAME]", &inner);
        }
        out
    }

    /// Size category name and key.
    pub fn size_names(&self, calc: &Calc<'a>) -> (String, String) {
        let name = calc
            .ds
            .get(FileKind::Size, &self.size)
            .map(|o| o.name.clone())
            .unwrap_or_default();
        (name, self.size.clone())
    }

    /// An item made up for the sheet (such as the unarmed strike).
    pub fn synthetic(obj: Object, size: &str) -> EqItem<'a> {
        EqItem {
            name: obj.name.clone(),
            key: obj.key.clone(),
            output_name: None,
            data: Cow::Owned(obj.clone()),
            base: Cow::Owned(obj),
            custom: false,
            size: size.to_string(),
            resized_cost: None,
            weight: 0.0,
            cost_mod: 0.0,
            weight_mod: 0.0,
            sprops: Vec::new(),
            eqmods: Vec::new(),
            alt_eqmods: Vec::new(),
            qty: 1.0,
            output_index: 0,
            sub_index: 0,
            modified: None,
            temporary: None,
            note: String::new(),
        }
    }

    /// A copy of this item carrying temporary bonuses from `source`:
    /// it costs only what its modifiers add and weighs nothing.
    pub fn temporary_copy(&self, source: &str, bonuses: Vec<String>) -> EqItem<'a> {
        let mut t = self.clone();
        t.temporary = Some((source.to_string(), bonuses));
        t.resized_cost = Some(0.0);
        t.weight = 0.0;
        t.weight_mod = 0.0;
        t
    }

    /// Name with the modified part: `Claw (Natural/Primary)`.
    pub fn long_name(&self) -> String {
        match &self.modified {
            Some(m) if !m.is_empty() => format!("{} ({})", self.name, m),
            _ => self.name.clone(),
        }
    }

    pub fn is_container(&self) -> bool {
        self.data.has("CONTAINS")
    }
}

/// Natural weapons from NATURALATTACKS on the race, templates,
/// abilities and other granted objects
/// (`Name,Types,Attacks,Damage[,...]`, entries separated by `|`): the
/// first entry of each is the primary attack, the rest secondary.
pub fn natural_weapons<'a>(calc: &Calc<'a>) -> Vec<EqItem<'a>> {
    use super::calc::SrcKind;
    let mut out = Vec::new();
    // natural weapons are the race's size as written (the default size
    // when the race's size is a formula)
    let size = calc
        .sources
        .iter()
        .find(|s| s.kind == SrcKind::Race)
        .and_then(|r| r.obj.get("SIZE"))
        .filter(|v| calc.ds.get(FileKind::Size, v).is_some())
        .map(str::to_string)
        .unwrap_or_else(|| default_size(calc));
    for src in calc.sources.iter().filter(|s| {
        !matches!(
            s.kind,
            SrcKind::Stat | SrcKind::Save | SrcKind::Skill | SrcKind::Equipment | SrcKind::EquipMod
        )
    }) {
        // only the race's and templates' attacks take the race's size
        let size = if matches!(src.kind, SrcKind::Race | SrcKind::Template) {
            size.clone()
        } else {
            default_size(calc)
        };
        for t in src.tokens.iter().filter(|t| t.tag == "NATURALATTACKS") {
            for (i, entry) in t.value.split('|').enumerate() {
                let f: Vec<&str> = entry.split(',').collect();
                if f.len() < 4 {
                    continue;
                }
                let tok = |tag: &str, value: &str| crate::dataset::Tok {
                    tag: tag.into(),
                    value: value.into(),
                    origin: t.origin,
                };
                let mut tokens = vec![
                    tok("TYPE", f[1]),
                    tok("DAMAGE", f[3]),
                    tok("ATTACKS", f[2]),
                    tok("CRITRANGE", "1"),
                    tok("CRITMULT", "x2"),
                    tok("SIZE", &size),
                ];
                let mut sprops = Vec::new();
                for extra in &f[4..] {
                    match extra.split_once('=') {
                        Some(("SPROP", v)) => sprops.push(v.to_string()),
                        Some(_) => {}
                        None => tokens.push(tok("SLOTS", extra)),
                    }
                }
                let obj = Object {
                    kind: FileKind::Equipment,
                    head_tag: None,
                    name: f[0].to_string(),
                    key: f[0].to_string(),
                    category: None,
                    tokens,
                    sublines: Vec::new(),
                    children: Vec::new(),
                    source: Vec::new(),
                    origin: t.origin,
                    history: Vec::new(),
                };
                out.push(EqItem {
                    name: obj.name.clone(),
                    key: obj.key.clone(),
                    output_name: None,
                    data: Cow::Owned(obj.clone()),
                    base: Cow::Owned(obj),
                    custom: false,
                    size: size.clone(),
                    resized_cost: None,
                    weight: 0.0,
                    cost_mod: 0.0,
                    weight_mod: 0.0,
                    sprops,
                    eqmods: Vec::new(),
                    alt_eqmods: Vec::new(),
                    qty: 1.0,
                    output_index: 0,
                    sub_index: i as i64 + 1,
                    modified: Some(
                        if i == 0 {
                            "Natural/Primary"
                        } else {
                            "Natural/Secondary"
                        }
                        .into(),
                    ),
                    temporary: None,
                    note: String::new(),
                });
            }
        }
    }
    out
}

/// Costs to the hundredth (formulas work in single precision).
fn round_cents(x: f64) -> f64 {
    (x * 10000.0).round() / 10000.0
}

/// The size marked as default (ISDEFAULTSIZE), else M.
pub fn default_size(calc: &Calc) -> String {
    calc.ds
        .objects(FileKind::Size)
        .iter()
        .find(|o| {
            o.get("ISDEFAULTSIZE")
                .is_some_and(|v| v.eq_ignore_ascii_case("Y") || v.eq_ignore_ascii_case("YES"))
        })
        .map(|o| o.key.clone())
        .unwrap_or_else(|| "M".into())
}

/// A size's multiplier for item cost or weight: the first of the item's
/// types with a BONUS:ITEMCOST/ITEMWEIGHT on the size, else 1.
pub fn size_bonus(calc: &Calc, size: &str, what: &str, types: &[String]) -> f64 {
    let Some(sz) = calc.ds.get(FileKind::Size, size) else {
        return 1.0;
    };
    for t in types {
        let mut total = 0.0;
        for v in sz.values("BONUS") {
            let Ok(b) = Bonus::parse(v) else { continue };
            if !b.category.eq_ignore_ascii_case(what) {
                continue;
            }
            let hit = b.targets.iter().any(|x| {
                x.strip_prefix("TYPE.")
                    .or_else(|| x.strip_prefix("TYPE="))
                    .is_some_and(|y| y.eq_ignore_ascii_case(t))
            });
            if hit {
                total += calc.eval_formula(&b.formula);
            }
        }
        if total != 0.0 {
            return total;
        }
    }
    1.0
}

/// Does a modifier's cost double on items without a body slot?
fn cost_doubles(m: &Object) -> bool {
    if let Some(v) = m.get("COSTDOUBLE") {
        return v.eq_ignore_ascii_case("YES") || v.eq_ignore_ascii_case("Y");
    }
    if m.is_type("MagicalEnhancement") || m.is_type("BaseMaterial") {
        return false;
    }
    if m.values("ITYPE")
        .flat_map(|v| v.split('.'))
        .any(|t| t.eq_ignore_ascii_case("Magic"))
    {
        return true;
    }
    m.values("PRETYPE").any(|v| {
        v.to_ascii_uppercase()
            .contains("EQMODTYPE=MAGICALENHANCEMENT")
    })
}

/// The number a choice stands for (`+4` is 4), for %CHOICE in costs.
fn choice_number(c: &str) -> String {
    let last = c.rsplit('|').next().unwrap_or(c).trim();
    last.trim_start_matches('+')
        .parse::<i64>()
        .map(|v| v.to_string())
        .unwrap_or_else(|_| "0".into())
}

/// A `NAME[value]` part of a built choice
/// (`SPELLNAME[Cure Light Wounds]SPELLLEVEL[1]CASTERLEVEL[1]`).
fn choice_part<'c>(choice: &'c str, name: &str) -> Option<&'c str> {
    choice
        .split_once(&format!("{}[", name))
        .and_then(|(_, r)| r.split(']').next())
}

/// Fill a modifier cost formula's per-choice terms.
#[cfg(test)]
fn substitute(f: &str, choice: &str) -> String {
    substitute_with(f, choice, &|_| None)
}

/// [`substitute`], looking up spell costs.
fn substitute_with(f: &str, choice: &str, spell: &dyn Fn(&str) -> Option<(f64, f64)>) -> String {
    let mut out = f.replace("%CHOICE", &choice_number(choice));
    if out.contains("%CHARGES") {
        out = out.replace("%CHARGES", choice_part(choice, "CHARGES").unwrap_or("0"));
    }
    if out.contains("%SPELLLEVEL") {
        let l = choice_part(choice, "SPELLLEVEL").unwrap_or("0");
        // a 0-level spell counts as half a level
        let l = if l.trim() == "0" { "0.5" } else { l };
        out = out.replace("%SPELLLEVEL", l);
    }
    if out.contains("%CASTERLEVEL") {
        let cl = choice_part(choice, "CASTERLEVEL").unwrap_or("0");
        out = out.replace("%CASTERLEVEL", &format!("({})", cl));
    }
    if out.contains("%SPELLCOST") || out.contains("%SPELLXPCOST") {
        let (cost, xp) = choice_part(choice, "SPELLNAME")
            .and_then(spell)
            .unwrap_or((0.0, 0.0));
        out = out
            .replace("%SPELLXPCOST", &xp.to_string())
            .replace("%SPELLCOST", &cost.to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choice_numbers() {
        assert_eq!(choice_number("+4"), "4");
        assert_eq!(choice_number("STR|+2"), "2");
        assert_eq!(choice_number("Knowledge (Geography)"), "0");
        assert_eq!(choice_number("-1"), "-1");
    }

    #[test]
    fn substitutions() {
        assert_eq!(substitute("100*%CHOICE", "+4"), "100*4");
        assert_eq!(
            substitute("((BASECOST/10)*%CHARGES)", "CHARGES[5]"),
            "((BASECOST/10)*5)"
        );
        assert_eq!(substitute("50", "x"), "50");
    }

    #[test]
    fn spell_choices() {
        let c = "SPELLNAME[Cure Light Wounds]SPELLLEVEL[1]CASTERLEVEL[1]";
        let f = "(50*(max(%SPELLLEVEL,(1/2)))*%CASTERLEVEL)+%SPELLCOST+(5*%SPELLXPCOST)";
        assert_eq!(
            substitute_with(f, c, &|n| (n == "Cure Light Wounds").then_some((0.0, 0.0))),
            "(50*(max(1,(1/2)))*(1))+0+(5*0)"
        );
        assert_eq!(substitute("%SPELLLEVEL", "SPELLLEVEL[0]"), "0.5");
    }

    #[test]
    fn cents() {
        assert_eq!(round_cents(0.30000001), 0.3);
        assert_eq!(round_cents(2302.0), 2302.0);
    }
}
