//
// rules/statgen.rs -- generating ability scores
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! The ways a game mode lets ability scores be generated: buying them
//! from a point budget (`pointbuymethods_system.lst`), rolling dice
//! (miscinfo `ROLLMETHOD` lines), and the increases characters gain as
//! they level (miscinfo `BONUSSTATLEVELSTARTINTERVAL`).
//!
//! The ROLLMETHOD codes saved in a character file.
//! 0: scores entered by hand; 1: every score the same;
//! 2: point buy; 3: rolled.

use crate::gamemode::GameMode;

pub const METHOD_USER: u32 = 0;
pub const METHOD_SAME: u32 = 1;
pub const METHOD_PURCHASE: u32 = 2;
pub const METHOD_ROLLED: u32 = 3;

/// The lowest score PCGen offers when making a character.
const STAT_MIN: i64 = 3;

/// One row of a point-buy cost table.
#[derive(Debug, Clone, PartialEq)]
pub struct CostRow {
    pub score: i64,
    pub cost: i64,
    /// prerequisites (`PRExxx:...`) a character must meet for the row
    pub prereqs: Vec<String>,
}

/// A game mode's point-buy rules.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PointBuy {
    /// rows by ascending score
    pub rows: Vec<CostRow>,
    /// (method name, budget formula)
    pub methods: Vec<(String, String)>,
}

impl PointBuy {
    pub fn from_gamemode(gm: &GameMode) -> PointBuy {
        let mut rows: Vec<CostRow> = Vec::new();
        for l in gm.lines("pointbuymethods_system.lst", "STAT") {
            let Ok(score) = l.value.trim().parse::<i64>() else {
                continue;
            };
            let cost = l
                .get("COST")
                .and_then(|c| c.trim().parse().ok())
                .unwrap_or(0);
            let prereqs = l
                .tokens
                .iter()
                .filter(|(t, _)| t.starts_with("PRE") || t.starts_with("!PRE"))
                .map(|(t, v)| format!("{}:{}", t, v))
                .collect();
            // a later row for the same score replaces the earlier
            rows.retain(|r| r.score != score);
            rows.push(CostRow {
                score,
                cost,
                prereqs,
            });
        }
        rows.sort_by_key(|r| r.score);
        let methods = gm
            .lines("pointbuymethods_system.lst", "METHOD")
            .map(|l| {
                (
                    l.value.trim().to_string(),
                    l.get("POINTS").unwrap_or("0").trim().to_string(),
                )
            })
            .collect();
        PointBuy { rows, methods }
    }

    /// Can scores be bought at all?
    pub fn available(&self) -> bool {
        !self.rows.is_empty() && !self.methods.is_empty()
    }

    /// A method by name, ignoring case: (name, budget formula).
    pub fn method(&self, name: &str) -> Option<&(String, String)> {
        self.methods
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name.trim()))
    }

    /// Lowest and highest scores of the table.
    pub fn table_range(&self) -> (i64, i64) {
        match (self.rows.first(), self.rows.last()) {
            (Some(a), Some(b)) => (a.score, b.score),
            _ => (-1, -1),
        }
    }

    /// The lowest and highest scores whose rows a character qualifies
    /// for, given a test of a row's prerequisites.
    pub fn range(&self, qualifies: impl Fn(&CostRow) -> bool) -> (i64, i64) {
        let ok: Vec<i64> = self
            .rows
            .iter()
            .filter(|r| qualifies(r))
            .map(|r| r.score)
            .collect();
        (
            ok.first().copied().unwrap_or(-1),
            ok.last().copied().unwrap_or(-1),
        )
    }

    /// Costs of each score from the table's lowest to its highest; a
    /// score between two rows costs what the row below it does.
    pub fn costs(&self) -> Vec<i64> {
        let mut out = Vec::new();
        let mut last: Option<(i64, i64)> = None;
        for r in &self.rows {
            if let Some((s, c)) = last {
                for _ in s + 1..r.score {
                    out.push(c);
                }
            }
            out.push(r.cost);
            last = Some((r.score, r.cost));
        }
        out
    }

    /// What a score costs a character whose qualifying range is
    /// `min..=max`: scores above the range cost what its top does,
    /// scores below it nothing.  As PCGen does, the cost is found by
    /// the score's distance from the character's lowest score (which is
    /// the table's, unless prerequisites cut off its first rows).
    pub fn cost(&self, score: i64, min: i64, max: i64) -> i64 {
        let s = score.min(max);
        if s < min {
            return 0;
        }
        self.costs().get((s - min) as usize).copied().unwrap_or(0)
    }

    /// The score every stat starts at when buying: the lowest free
    /// score in the character's range, else just below the range (but
    /// at least 3).
    pub fn neutral(&self, min: i64) -> i64 {
        let (tmin, _) = self.table_range();
        for (i, c) in self.costs().iter().enumerate() {
            let s = tmin + i as i64;
            if *c == 0 && s >= min {
                return s;
            }
        }
        if tmin == min {
            (min - 1).max(STAT_MIN)
        } else {
            min
        }
    }
}

/// Stat increases a character gains on reaching a total level, from
/// the game mode's `start|interval[|count]` lines (all of them add
/// up).  `eval` works out an interval formula.
pub fn bonus_stats_for_level(specs: &[&str], level: i64, eval: impl Fn(&str) -> f64) -> u32 {
    let mut n = 0;
    for spec in specs {
        let mut parts = spec.split('|');
        let Some(Ok(start)) = parts.next().map(|s| s.trim().parse::<i64>()) else {
            continue;
        };
        let interval = parts.next().map(|f| eval(f.trim()) as i64).unwrap_or(0);
        let count = parts
            .next()
            .and_then(|c| c.trim().parse::<u32>().ok())
            .unwrap_or(1);
        if level == start || (level > start && interval > 0 && (level - start) % interval == 0) {
            n += count;
        }
    }
    n
}

/// The game mode's stat increase lines.
pub fn bonus_stat_specs(gm: &GameMode) -> Vec<String> {
    gm.lines("miscinfo.lst", "BONUSSTATLEVELSTARTINTERVAL")
        .map(|l| l.value.clone())
        .collect()
}

/// The game mode's dice methods for rolling scores, by SORTKEY:
/// (name, dice expression).
pub fn roll_methods(gm: &GameMode) -> Vec<(String, String)> {
    let mut v: Vec<(String, String, String)> = gm
        .lines("miscinfo.lst", "ROLLMETHOD")
        .filter_map(|l| {
            Some((
                l.get("SORTKEY").unwrap_or("").to_string(),
                l.value.trim().to_string(),
                l.get("METHOD")?.trim().to_string(),
            ))
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v.into_iter().map(|(_, n, m)| (n, m)).collect()
}

/// A small deterministic random number source (splitmix64).
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed)
    }

    /// Seeded from the clock.
    pub fn from_time() -> Rng {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        Rng(t ^ 0x9e37_79b9_7f4a_7c15)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// 1 to `sides`.
    pub fn die(&mut self, sides: i64) -> i64 {
        if sides <= 0 {
            return 0;
        }
        (self.next() % sides as u64) as i64 + 1
    }
}

/// Split at commas, plus and minus signs outside brackets.
fn split_top(s: &str, seps: &[char]) -> Vec<(char, String)> {
    let mut out = Vec::new();
    let mut depth = 0;
    let mut cur = String::new();
    let mut sign = '+';
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            _ => {}
        }
        if depth == 0 && seps.contains(&c) {
            // a sign with nothing before it belongs to what follows
            if c == ',' || !cur.is_empty() {
                out.push((sign, std::mem::take(&mut cur)));
            }
            sign = c;
            continue;
        }
        cur.push(c);
    }
    out.push((sign, cur));
    out
}

/// Roll `n` dice of `sides` faces, each rerolling results up to
/// `reroll`, sorted lowest first.
fn dice(rng: &mut Rng, n: i64, sides: i64, reroll: i64) -> Vec<i64> {
    let mut v: Vec<i64> = (0..n.max(0))
        .map(|_| rng.die(sides - reroll) + reroll)
        .collect();
    v.sort();
    v
}

/// One term of a dice expression.
fn term(t: &str, rng: &mut Rng) -> Option<i64> {
    let t = t.trim();
    if let Ok(n) = t.parse::<i64>() {
        return Some(n);
    }
    let lower = t.to_ascii_lowercase();
    if let Some(args) = lower
        .strip_prefix("roll(")
        .and_then(|a| a.strip_suffix(')'))
    {
        let parts: Vec<String> = split_top(args, &[',']).into_iter().map(|p| p.1).collect();
        let n: i64 = parts.first()?.trim().parse().ok()?;
        let sides: i64 = parts.get(1)?.trim().parse().ok()?;
        let (mut keep, mut reroll, mut picks) = (n, 0, Vec::new());
        for o in &parts[2..] {
            let o = o.trim();
            if let Some(k) = o.strip_prefix("top(").and_then(|x| x.strip_suffix(')')) {
                keep = k.trim().parse().ok()?;
            } else if let Some(r) = o.strip_prefix("reroll(").and_then(|x| x.strip_suffix(')')) {
                reroll = r.trim().parse().ok()?;
            } else {
                // positions (from 1) among the dice sorted lowest first
                let l = o.strip_prefix('[').and_then(|x| x.strip_suffix(']'))?;
                for p in l.split(',') {
                    picks.push(p.trim().parse::<usize>().ok()?);
                }
            }
        }
        let v = dice(rng, n, sides, reroll);
        if !picks.is_empty() {
            return Some(picks.iter().filter_map(|p| v.get(p.wrapping_sub(1))).sum());
        }
        let skip = (n - keep).max(0) as usize;
        return Some(v.iter().skip(skip).sum());
    }
    let (n, sides) = lower.split_once('d')?;
    let n: i64 = if n.is_empty() { 1 } else { n.parse().ok()? };
    let sides: i64 = if sides == "%" {
        100
    } else {
        sides.parse().ok()?
    };
    Some(dice(rng, n, sides, 0).iter().sum())
}

/// Roll a dice expression: `3d6`, `roll(4,6,top(3))`,
/// `roll(4,6,reroll(1))`, `roll(4,6,[2,3,4])`, with whole numbers
/// added or taken away.  None if it cannot be read.
pub fn roll(expr: &str, rng: &mut Rng) -> Option<i64> {
    let s: String = expr.chars().filter(|c| !c.is_whitespace()).collect();
    if s.is_empty() {
        return None;
    }
    let mut total = 0;
    for (sign, t) in split_top(&s, &['+', '-']) {
        let v = term(&t, rng)?;
        total += if sign == '-' { -v } else { v };
    }
    Some(total)
}

/// Replace each `roll("dice")` in a formula with a roll of the dice
/// (0 if they cannot be read).
pub fn roll_calls(formula: &str, rng: &mut Rng) -> String {
    let mut out = String::new();
    let mut rest = formula;
    while let Some(i) = rest.find("roll(\"") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 6..];
        let Some(j) = after.find("\")") else {
            out.push_str(&rest[i..]);
            rest = "";
            break;
        };
        out.push_str(&roll(&after[..j], rng).unwrap_or(0).to_string());
        rest = &after[j + 2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rolls_inside_formulas() {
        let mut rng = Rng::new(1);
        assert_eq!(roll_calls("2*roll(\"1d1\")+23", &mut rng), "2*1+23");
        assert_eq!(roll_calls("roll(\"2d1\")+roll(\"3\")", &mut rng), "2+3");
        assert_eq!(roll_calls("10", &mut rng), "10");
        assert_eq!(roll_calls("roll(\"x\")", &mut rng), "0");
        assert_eq!(roll_calls("roll(\"1d6", &mut rng), "roll(\"1d6");
        let v: i64 = roll_calls("10*roll(\"5d4\")", &mut rng)[3..]
            .parse()
            .unwrap();
        assert!((5..=20).contains(&v));
    }

    fn table(rows: &[(i64, i64)]) -> PointBuy {
        PointBuy {
            rows: rows
                .iter()
                .map(|&(score, cost)| CostRow {
                    score,
                    cost,
                    prereqs: Vec::new(),
                })
                .collect(),
            methods: vec![("Standard".into(), "15".into())],
        }
    }

    #[test]
    fn costs_fill_gaps_from_below() {
        let t = table(&[(8, -2), (10, 0), (11, 1), (14, 5)]);
        assert_eq!(t.costs(), vec![-2, -2, 0, 1, 1, 1, 5]);
        assert_eq!(t.table_range(), (8, 14));
        assert_eq!(t.cost(10, 8, 14), 0);
        assert_eq!(t.cost(13, 8, 14), 1);
        // above the top costs the top; below the bottom costs nothing
        assert_eq!(t.cost(18, 8, 14), 5);
        assert_eq!(t.cost(5, 8, 14), 0);
        // a narrower range counts from its own lowest score
        assert_eq!(t.cost(10, 9, 14), -2);
    }

    #[test]
    fn neutral_scores() {
        let t = table(&[(7, -4), (8, -2), (9, -1), (10, 0), (11, 1)]);
        assert_eq!(t.neutral(7), 10);
        assert_eq!(t.neutral(11), 11);
        // no free score: just below the table, but not under 3
        let t = table(&[(3, -6), (4, -2), (10, 10)]);
        assert_eq!(t.neutral(3), 3);
        let t = table(&[(6, 1), (7, 2)]);
        assert_eq!(t.neutral(6), 5);
        assert_eq!(t.neutral(7), 7);
    }

    #[test]
    fn methods_and_ranges() {
        let mut t = table(&[(8, -2), (10, 0), (12, 2)]);
        assert_eq!(t.method("standard").unwrap().1, "15");
        assert!(t.method("Epic").is_none());
        assert!(t.available());
        t.rows[0].prereqs.push("PRERACE:1,Gnome".into());
        assert_eq!(t.range(|r| r.prereqs.is_empty()), (10, 12));
        assert_eq!(t.range(|_| false), (-1, -1));
    }

    #[test]
    fn stat_increases_by_level() {
        let at = |level| bonus_stats_for_level(&["4|4"], level, |f| f.parse().unwrap());
        let got: Vec<u32> = (1..=12).map(at).collect();
        assert_eq!(got, [0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1]);
        // counts, several lines, and no interval
        assert_eq!(bonus_stats_for_level(&["4|4|2"], 8, |_| 4.0), 2);
        assert_eq!(bonus_stats_for_level(&["4|4", "2|0"], 4, |_| 4.0), 1);
        assert_eq!(bonus_stats_for_level(&["2|0"], 2, |_| 0.0), 1);
        assert_eq!(bonus_stats_for_level(&["2|0"], 4, |_| 0.0), 0);
        assert_eq!(bonus_stats_for_level(&["0|0"], 4, |_| 0.0), 0);
        assert_eq!(bonus_stats_for_level(&["x|4"], 4, |_| 4.0), 0);
    }

    #[test]
    fn dice_expressions() {
        let mut rng = Rng::new(7);
        for _ in 0..200 {
            let v = roll("3d6", &mut rng).unwrap();
            assert!((3..=18).contains(&v), "{}", v);
            let v = roll("roll(4,6,top(3))", &mut rng).unwrap();
            assert!((3..=18).contains(&v), "{}", v);
            let v = roll("roll(4,6,reroll(1))", &mut rng).unwrap();
            assert!((8..=24).contains(&v), "{}", v);
            let v = roll("roll(4,6,top(3),reroll(1))", &mut rng).unwrap();
            assert!((6..=18).contains(&v), "{}", v);
            let v = roll("roll(4,6,[2,3,4])", &mut rng).unwrap();
            assert!((3..=18).contains(&v), "{}", v);
            let v = roll("2d4+10", &mut rng).unwrap();
            assert!((12..=18).contains(&v), "{}", v);
            let v = roll("d%", &mut rng).unwrap();
            assert!((1..=100).contains(&v), "{}", v);
        }
        assert_eq!(roll("12", &mut rng), Some(12));
        assert_eq!(roll("10 - 2", &mut rng), Some(8));
        assert_eq!(roll("-3+5", &mut rng), Some(2));
        assert_eq!(roll("roll(1,1,top(1))", &mut rng), Some(1));
        assert_eq!(roll("", &mut rng), None);
        assert_eq!(roll("lots", &mut rng), None);
        assert_eq!(roll("roll(4,6,weird(1))", &mut rng), None);
        // the same seed rolls the same
        let a: Vec<_> = (0..5)
            .map(|_| roll("3d6", &mut Rng::new(42)).unwrap())
            .collect();
        assert!(a.iter().all(|x| *x == a[0]));
    }
}
