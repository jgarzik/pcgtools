//
// rules/bonus.rs -- bonus accumulation and stacking
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! Bonuses are filed under a key `CATEGORY.TARGET` plus an optional
//! type.  Within one key and type:
//!
//! * untyped bonuses, types the game mode declares stacking, negative
//!   values, and `.STACK`/`.REPLACE` variants all add up;
//! * any other typed bonuses do not stack: only the largest counts.
//!
//! The total for a key adds, per type, the larger of the plain value
//! and the `.REPLACE` value, plus the `.STACK` value.  Each bonus is
//! truncated toward zero before it is filed, except for a few
//! categories that allow fractions.

use std::collections::{BTreeMap, HashSet};

/// Non-stacking bonuses keep their maximum; stacking ones (including
/// all negatives) are summed separately and added on top.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Acc {
    best: Option<f64>,
    sum: Option<f64>,
}

impl Acc {
    fn value(&self) -> Option<f64> {
        match (self.best, self.sum) {
            (None, None) => None,
            (b, s) => Some(b.unwrap_or(0.0) + s.unwrap_or(0.0)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Mode {
    Plain,
    Stack,
    Replace,
}

/// Accumulated bonuses.
#[derive(Debug, Clone, Default)]
pub struct BonusTable {
    /// (KEY, TYPE, mode) -> value; KEY is `CATEGORY.TARGET` upper
    /// case; TYPE is upper case, empty for untyped
    entries: BTreeMap<(String, String, Mode), Acc>,
    stacking: HashSet<String>,
    /// every bonus filed, with a note on where it came from
    pub log: Vec<(String, String, f64, String)>,
}

fn fractional(cat: &str) -> bool {
    ["ITEMWEIGHT", "ITEMCOST", "ITEMCAPACITY", "LOADMULT", "FEAT"]
        .iter()
        .any(|p| cat.starts_with(p))
        || cat.contains("DAMAGEMULT")
}

impl BonusTable {
    /// `stacking` lists the bonus types that stack with themselves.
    pub fn new<S: AsRef<str>>(stacking: &[S]) -> BonusTable {
        BonusTable {
            entries: BTreeMap::new(),
            log: Vec::new(),
            stacking: stacking
                .iter()
                .map(|s| s.as_ref().to_ascii_uppercase())
                .collect(),
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.log.clear();
    }

    /// File a bonus and remember its origin.
    pub fn add_from(
        &mut self,
        category: &str,
        target: &str,
        btype: Option<&str>,
        value: f64,
        origin: &str,
    ) {
        self.add(category, target, btype, value);
        self.log.push((
            format!(
                "{}.{}",
                category.to_ascii_uppercase(),
                target.to_ascii_uppercase()
            ),
            btype.unwrap_or("").to_string(),
            value,
            origin.to_string(),
        ));
    }

    /// Contributions to `category.target`: (type, value, origin).
    pub fn explain(&self, category: &str, target: &str) -> Vec<(String, f64, String)> {
        let key = format!(
            "{}.{}",
            category.to_ascii_uppercase(),
            target.to_ascii_uppercase()
        );
        self.log
            .iter()
            .filter(|(k, _, _, _)| *k == key)
            .map(|(_, t, v, o)| (t.clone(), *v, o.clone()))
            .collect()
    }

    /// File a bonus.  `btype` may carry a `.STACK` or `.REPLACE`
    /// suffix, as written in data.
    pub fn add(&mut self, category: &str, target: &str, btype: Option<&str>, value: f64) {
        let cat = category.to_ascii_uppercase();
        let value = if fractional(&cat) {
            value
        } else {
            value.trunc()
        };
        let key = format!("{}.{}", cat, target.to_ascii_uppercase());
        let t = btype.unwrap_or("").to_ascii_uppercase();
        let (t, mode) = if let Some(x) = t.strip_suffix(".STACK") {
            (x.to_string(), Mode::Stack)
        } else if let Some(x) = t.strip_suffix(".REPLACE") {
            (x.to_string(), Mode::Replace)
        } else {
            (t, Mode::Plain)
        };
        let stacks =
            t.is_empty() || self.stacking.contains(&t) || mode != Mode::Plain || value < 0.0;
        let e = self.entries.entry((key, t, mode)).or_default();
        if stacks {
            e.sum = Some(e.sum.unwrap_or(0.0) + value);
        } else {
            e.best = Some(e.best.map_or(value, |b| b.max(value)));
        }
    }

    fn get(&self, key: &str, t: &str, mode: Mode) -> Option<f64> {
        self.entries
            .get(&(key.to_string(), t.to_string(), mode))
            .and_then(Acc::value)
    }

    /// Combined value of one type under a key.
    fn type_total(&self, key: &str, t: &str) -> f64 {
        let plain = self.get(key, t, Mode::Plain);
        let replace = self.get(key, t, Mode::Replace);
        let stack = self.get(key, t, Mode::Stack).unwrap_or(0.0);
        let base = match (plain, replace) {
            (None, None) => 0.0,
            (Some(p), None) => p,
            (None, Some(r)) => r,
            (Some(p), Some(r)) => p.max(r),
        };
        base + stack
    }

    /// Total of all bonuses to `category.target`.
    pub fn total(&self, category: &str, target: &str) -> f64 {
        let key = format!(
            "{}.{}",
            category.to_ascii_uppercase(),
            target.to_ascii_uppercase()
        );
        let types: Vec<String> = self
            .entries
            .keys()
            .filter(|(k, _, _)| *k == key)
            .map(|(_, t, _)| t.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        types.iter().map(|t| self.type_total(&key, t)).sum()
    }

    /// Bonuses of one type to `category.target` (`TYPE` matched
    /// case-insensitively).
    pub fn by_type(&self, category: &str, target: &str, btype: &str) -> f64 {
        let key = format!(
            "{}.{}",
            category.to_ascii_uppercase(),
            target.to_ascii_uppercase()
        );
        self.type_total(&key, &btype.to_ascii_uppercase())
    }

    /// All targets with bonuses in a category.
    pub fn targets(&self, category: &str) -> Vec<String> {
        let prefix = format!("{}.", category.to_ascii_uppercase());
        let mut v: Vec<String> = self
            .entries
            .keys()
            .filter_map(|(k, _, _)| k.strip_prefix(&prefix).map(str::to_string))
            .collect();
        v.dedup();
        v
    }

    /// Number of filed entries (for change detection).
    pub fn snapshot(&self) -> Vec<f64> {
        self.entries.values().filter_map(Acc::value).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> BonusTable {
        BonusTable::new(&["Dodge", "Circumstance"])
    }

    #[test]
    fn untyped_stack() {
        let mut b = t();
        b.add("COMBAT", "AC", None, 1.0);
        b.add("combat", "ac", None, 2.0);
        assert_eq!(b.total("COMBAT", "AC"), 3.0);
    }

    #[test]
    fn typed_highest_wins() {
        let mut b = t();
        b.add("COMBAT", "AC", Some("Deflection"), 2.0);
        b.add("COMBAT", "AC", Some("Deflection"), 3.0);
        b.add("COMBAT", "AC", Some("Armor"), 4.0);
        assert_eq!(b.total("COMBAT", "AC"), 7.0);
        assert_eq!(b.by_type("COMBAT", "AC", "deflection"), 3.0);
    }

    #[test]
    fn declared_stacking_types() {
        let mut b = t();
        b.add("COMBAT", "AC", Some("Dodge"), 1.0);
        b.add("COMBAT", "AC", Some("Dodge"), 1.0);
        assert_eq!(b.total("COMBAT", "AC"), 2.0);
    }

    #[test]
    fn negatives_always_add() {
        let mut b = t();
        b.add("SKILL", "Ride", Some("Armor"), -2.0);
        b.add("SKILL", "Ride", Some("Armor"), -3.0);
        assert_eq!(b.total("SKILL", "Ride"), -5.0);
    }

    #[test]
    fn negatives_add_to_best_positive() {
        let mut b = t();
        b.add("COMBAT", "AC", Some("Deflection"), 3.0);
        b.add("COMBAT", "AC", Some("Deflection"), -2.0);
        b.add("COMBAT", "AC", Some("Deflection"), 4.0);
        assert_eq!(b.total("COMBAT", "AC"), 2.0);
    }

    #[test]
    fn stack_and_replace() {
        let mut b = t();
        // multiclass base attack: REPLACE values add up among themselves
        b.add("COMBAT", "BASEAB", Some("Base.REPLACE"), 3.0);
        b.add("COMBAT", "BASEAB", Some("Base.REPLACE"), 2.0);
        assert_eq!(b.total("COMBAT", "BASEAB"), 5.0);
        // larger of plain and replace
        b.add("COMBAT", "BASEAB", Some("Base"), 7.0);
        assert_eq!(b.total("COMBAT", "BASEAB"), 7.0);
        // stack adds on top
        b.add("COMBAT", "BASEAB", Some("Base.STACK"), 1.0);
        assert_eq!(b.total("COMBAT", "BASEAB"), 8.0);
    }

    #[test]
    fn truncation() {
        let mut b = t();
        b.add("COMBAT", "BASEAB", None, 2.25);
        b.add("COMBAT", "BASEAB", None, -1.5);
        assert_eq!(b.total("COMBAT", "BASEAB"), 1.0);
        b.add("ITEMWEIGHT", "X", None, 0.5);
        assert_eq!(b.total("ITEMWEIGHT", "X"), 0.5);
    }

    #[test]
    fn keys_do_not_bleed() {
        let mut b = t();
        b.add("COMBAT", "AC", None, 1.0);
        b.add("COMBAT", "ACCHECK", None, -2.0);
        assert_eq!(b.total("COMBAT", "AC"), 1.0);
        assert_eq!(b.total("COMBAT", "ACCHECK"), -2.0);
        assert_eq!(b.total("COMBAT", "TOHIT"), 0.0);
        assert_eq!(b.targets("COMBAT"), vec!["AC", "ACCHECK"]);
    }
}
