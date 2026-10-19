//
// rules/temp.rs -- temporary bonuses offered and applied
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! Temporary bonuses: which objects offer them (TEMPBONUS, with the
//! number TEMPVALUE asks for), which items they suit, and the bonus
//! text PCGen records when one is applied.

use super::calc::{Calc, SrcKind};
use crate::dataset::Object;
use crate::lst::kind::FileKind;

/// An object offering temporary bonuses.
#[derive(Debug, Clone)]
pub struct Offer<'a> {
    /// the kind PCGen records it as: FEAT, SPELL, TEMPLATE, SKILL or
    /// EQUIPMENT
    pub kind: &'static str,
    pub obj: &'a Object,
    /// bonuses for the character (TEMPBONUS:ANYPC, then :PC)
    pub pc: Vec<String>,
    /// whether any of them are ANYPC
    pub any_pc: bool,
    /// bonuses for an item: the item types it needs, and the bonus
    pub eq: Vec<(String, String)>,
}

impl<'a> Offer<'a> {
    /// The temporary bonuses of an object, if it has any.
    pub fn of(obj: &'a Object, kind: &'static str) -> Option<Offer<'a>> {
        let mut any = Vec::new();
        let mut pc = Vec::new();
        let mut eq = Vec::new();
        for v in obj.values("TEMPBONUS") {
            let v = v.replace("<this>", &obj.key);
            let Some((sub, rest)) = v.split_once('|') else {
                continue;
            };
            match sub.to_ascii_uppercase().as_str() {
                "ANYPC" => any.push(rest.to_string()),
                "PC" => pc.push(rest.to_string()),
                "EQ" => {
                    if let Some((cond, b)) = rest.split_once('|') {
                        eq.push((cond.to_string(), b.to_string()));
                    }
                }
                _ => {}
            }
        }
        if any.is_empty() && pc.is_empty() && eq.is_empty() {
            return None;
        }
        let any_pc = !any.is_empty();
        any.extend(pc);
        Some(Offer {
            kind,
            obj,
            pc: any,
            any_pc,
            eq,
        })
    }

    pub fn key(&self) -> &str {
        &self.obj.key
    }

    /// Whether the bonuses suit an item of these types.
    pub fn suits(&self, is_type: &dyn Fn(&str) -> bool) -> bool {
        self.eq.iter().any(|(c, _)| passes_conditions(c, is_type))
    }

    /// The range TEMPVALUE allows, worked out now: None when the
    /// object asks for no number; an error when the range is empty.
    pub fn value_range(&self, calc: &Calc) -> Option<Result<(i64, i64), String>> {
        let spec = self.obj.get("TEMPVALUE")?;
        let mut parts = spec.split('|');
        let mut part = |name: &str| {
            parts
                .next()
                .and_then(|p| p.strip_prefix(name))
                .map(str::to_string)
        };
        let (Some(min), Some(max)) = (part("MIN="), part("MAX=")) else {
            return Some(Err(format!("bad TEMPVALUE '{}'", spec)));
        };
        let (min, max) = (
            calc.eval_formula(&min) as i64,
            calc.eval_formula(&max) as i64,
        );
        Some(if max < min || max <= 0 {
            Err(format!("no number fits: {} to {}", min, max))
        } else {
            Ok((min, max))
        })
    }

    /// The bonuses applied to the character or to an item, with the
    /// number chosen, as PCGen records them.
    pub fn applied(&self, to_item: bool, value: Option<i64>) -> Vec<String> {
        let list: Vec<&str> = if to_item {
            self.eq.iter().map(|(_, b)| b.as_str()).collect()
        } else {
            self.pc.iter().map(String::as_str).collect()
        };
        list.into_iter()
            .filter_map(normalize_bonus)
            .map(|b| match value {
                Some(v) => b.replace("%CHOICE", &v.to_string()),
                None => b,
            })
            .filter_map(|b| normalize_bonus(&b))
            .collect()
    }
}

/// Temporary bonuses the character can apply, as PCGen offers them:
/// those of its abilities, templates, skills and worn items, any
/// ability's for any character, the character's spells', and any
/// spell's or template's for any character or for items.  Sorted by
/// key.  (Sources PCGen cannot record, such as the race or a class,
/// are left out.)
pub fn offers<'a>(calc: &Calc<'a>) -> Vec<Offer<'a>> {
    let mut out: Vec<Offer<'a>> = Vec::new();
    let mut push = |o: Option<Offer<'a>>, keep: &dyn Fn(&Offer) -> bool| {
        if let Some(o) = o.filter(|o| keep(o)) {
            if !out
                .iter()
                .any(|x| x.kind == o.kind && x.obj.key.eq_ignore_ascii_case(&o.obj.key))
            {
                out.push(o);
            }
        }
    };
    for s in &calc.sources {
        let kind = match s.kind {
            SrcKind::Ability => "FEAT",
            SrcKind::Template => "TEMPLATE",
            SrcKind::Skill => "SKILL",
            SrcKind::Equipment => "EQUIPMENT",
            _ => continue,
        };
        push(Offer::of(s.obj, kind), &|_| true);
    }
    for o in calc.ds.objects(FileKind::Ability) {
        push(Offer::of(o, "FEAT"), &|o| o.any_pc);
    }
    for (spell, ..) in calc.ch.spells() {
        if let Some(o) = calc.ds.get(FileKind::Spell, &spell) {
            push(Offer::of(o, "SPELL"), &|_| true);
        }
    }
    for (kind, name) in [(FileKind::Spell, "SPELL"), (FileKind::Template, "TEMPLATE")] {
        for o in calc.ds.objects(kind) {
            push(Offer::of(o, name), &|o| o.any_pc || !o.eq.is_empty());
        }
    }
    out.sort_by_key(|o| o.obj.key.to_lowercase());
    out
}

/// Whether an item passes TEMPBONUS:EQ conditions: `,` joins
/// conditions that must all hold, `;` alternatives, and `[TYPE]`
/// asks for an item not of that type.
pub fn passes_conditions(cond: &str, is_type: &dyn Fn(&str) -> bool) -> bool {
    cond.split(',').all(|and| {
        and.split(';').any(|t| match t.strip_prefix('[') {
            Some(not) => !is_type(not.strip_suffix(']').unwrap_or(not)),
            None => is_type(t),
        })
    })
}

/// Split at `|` outside parentheses and brackets.
fn split_top(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            '|' if depth <= 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

fn is_pre(s: &str) -> bool {
    s.starts_with("PRE") || s.starts_with("!PRE")
}

/// A bonus as PCGen writes it back: category, targets (upper case,
/// but for skill names), a whole-number formula as a number, the
/// rest upper case, the type before the prerequisites.  None for a
/// bonus PCGen would not accept.
pub fn normalize_bonus(text: &str) -> Option<String> {
    if text.matches('|').count() < 2 && !text.contains('%') {
        return None;
    }
    let mut parts = split_top(text).into_iter();
    let mut name = parts.next()?.to_string();
    if name.trim().parse::<i64>().is_ok() {
        // an old level number before the category
        name = parts.next()?.to_string();
    }
    let name = name.to_uppercase();
    let info = parts.next()?;
    let value = parts.next().unwrap_or("0");
    if is_pre(value) {
        return None;
    }
    let value = value.to_uppercase();
    let formula = if let Ok(n) = value.parse::<i32>() {
        n.to_string()
    } else if let Some(f) = value.parse::<f64>().ok().filter(|f| f.is_finite()) {
        if f.fract() == 0.0 {
            format!("{:.1}", f)
        } else {
            f.to_string()
        }
    } else {
        value
    };
    let mut btype: Option<String> = None;
    let mut prereqs = Vec::new();
    for p in parts {
        let p = p.to_uppercase();
        if is_pre(&p) {
            prereqs.push(p);
        } else if let Some(t) = p.strip_prefix("TYPE=").or_else(|| p.strip_prefix("TYPE.")) {
            if btype.is_some() {
                return None;
            }
            btype = Some(t.to_string());
        }
    }
    let skill = name == "SKILL";
    let targets: Vec<String> = info
        .split(',')
        .filter(|t| !t.is_empty())
        .map(|t| {
            if !skill {
                return t.to_uppercase();
            }
            match t.split_once('=') {
                Some((a, b)) if a == "STAT" || a == "TYPE" => format!("{}.{}", a, b),
                _ => t.to_string(),
            }
        })
        .collect();
    if targets.is_empty() {
        return None;
    }
    let mut out = format!("{}|{}|{}", name, targets.join(","), formula);
    if let Some(t) = btype {
        out.push_str("|TYPE=");
        out.push_str(&t);
    }
    for p in prereqs {
        out.push('|');
        out.push_str(&p);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_like_pcgen() {
        assert_eq!(
            normalize_bonus("WEAPON|TOHIT|-PowerAttackModifier|TYPE=Temporary").as_deref(),
            Some("WEAPON|TOHIT|-POWERATTACKMODIFIER|TYPE=TEMPORARY")
        );
        assert_eq!(
            normalize_bonus("Var|ActionUsed|4").as_deref(),
            Some("VAR|ACTIONUSED|4")
        );
        // the type goes before the prerequisites
        assert_eq!(
            normalize_bonus("COMBAT|AC|2|PREFEAT:1,Alertness|TYPE=Dodge").as_deref(),
            Some("COMBAT|AC|2|TYPE=DODGE|PREFEAT:1,ALERTNESS")
        );
        // skill names keep their case
        assert_eq!(
            normalize_bonus("SKILL|Listen,TYPE=Craft|2").as_deref(),
            Some("SKILL|Listen,TYPE.Craft|2")
        );
    }

    #[test]
    fn normalizes_numbers() {
        assert_eq!(
            normalize_bonus("COMBAT|AC|+2").as_deref(),
            Some("COMBAT|AC|2")
        );
        assert_eq!(
            normalize_bonus("COMBAT|AC|1.5").as_deref(),
            Some("COMBAT|AC|1.5")
        );
        assert_eq!(
            normalize_bonus("COMBAT|AC|2.0").as_deref(),
            Some("COMBAT|AC|2.0")
        );
        // grouping keeps a pipe inside parentheses
        assert_eq!(
            normalize_bonus("COMBAT|AC|(min(%CHOICE/6+3,5))|TYPE=NaturalArmor").as_deref(),
            Some("COMBAT|AC|(MIN(%CHOICE/6+3,5))|TYPE=NATURALARMOR")
        );
    }

    #[test]
    fn rejects_what_pcgen_rejects() {
        assert_eq!(normalize_bonus("COMBAT|AC"), None);
        assert_eq!(normalize_bonus("COMBAT|AC|PREFEAT:1,X"), None);
        assert_eq!(normalize_bonus("COMBAT|AC|1|TYPE=A|TYPE=B"), None);
        assert_eq!(normalize_bonus("COMBAT||1"), None);
    }

    #[test]
    fn old_level_number_is_dropped() {
        assert_eq!(
            normalize_bonus("1|COMBAT|AC|1").as_deref(),
            Some("COMBAT|AC|1")
        );
    }

    #[test]
    fn conditions() {
        let types = ["WEAPON", "MELEE", "ONEHANDED"];
        let is = |t: &str| types.iter().any(|x| x.eq_ignore_ascii_case(t));
        assert!(passes_conditions("Weapon", &is));
        assert!(passes_conditions("Weapon,Melee,TwoHanded;OneHanded", &is));
        assert!(!passes_conditions("Weapon,Ranged", &is));
        assert!(passes_conditions("Weapon,[Ranged]", &is));
        assert!(!passes_conditions("[Weapon]", &is));
        assert!(passes_conditions("Armor;Shield;Weapon", &is));
    }
}
