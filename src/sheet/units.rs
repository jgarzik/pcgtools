//
// sheet/units.rs -- unit sets and number patterns for sheet output
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! Distances, heights and weights are shown in the game mode's unit
//! set (UNITSET lines named by DEFAULTUNITSET), each with a unit, a
//! conversion factor and a number pattern such as `#.##`.

use crate::gamemode::GameMode;

#[derive(Debug, Clone, PartialEq)]
pub struct Units {
    pub height_unit: String,
    pub height_factor: f64,
    pub height_pattern: String,
    pub distance_unit: String,
    pub distance_factor: f64,
    pub distance_pattern: String,
    pub weight_unit: String,
    pub weight_factor: f64,
    pub weight_pattern: String,
}

impl Default for Units {
    /// Used when the game mode names no unit set it defines.
    fn default() -> Units {
        Units {
            height_unit: "ftin".into(),
            height_factor: 1.0,
            height_pattern: "#".into(),
            distance_unit: "~'".into(),
            distance_factor: 1.0,
            distance_pattern: "#".into(),
            weight_unit: "lbs.".into(),
            weight_factor: 1.0,
            weight_pattern: "#.###".into(),
        }
    }
}

impl Units {
    pub fn from_gamemode(gm: Option<&GameMode>) -> Units {
        let Some(gm) = gm else {
            return Units::default();
        };
        let want = gm.misc("DEFAULTUNITSET").unwrap_or("Imperial");
        let Some(line) = gm
            .lines("miscinfo.lst", "UNITSET")
            .find(|l| l.value.eq_ignore_ascii_case(want))
        else {
            return Units::default();
        };
        let mut u = Units::default();
        for (tag, val) in &line.tokens {
            let f = || val.parse().unwrap_or(1.0);
            match tag.as_str() {
                "HEIGHTUNIT" => u.height_unit = val.clone(),
                "HEIGHTFACTOR" => u.height_factor = f(),
                "HEIGHTPATTERN" => u.height_pattern = val.clone(),
                "DISTANCEUNIT" => u.distance_unit = val.clone(),
                "DISTANCEFACTOR" => u.distance_factor = f(),
                "DISTANCEPATTERN" => u.distance_pattern = val.clone(),
                "WEIGHTUNIT" => u.weight_unit = val.clone(),
                "WEIGHTFACTOR" => u.weight_factor = f(),
                "WEIGHTPATTERN" => u.weight_pattern = val.clone(),
                _ => {}
            }
        }
        u
    }

    /// A distance in feet, with its unit: `30 ft.`
    pub fn distance(&self, feet: f64) -> String {
        format!(
            "{}{}",
            pattern(feet * self.distance_factor, &self.distance_pattern),
            unit_suffix(&self.distance_unit)
        )
    }

    /// A weight in pounds, with its unit: `12.5 lbs.`
    pub fn weight(&self, lbs: f64) -> String {
        format!(
            "{}{}",
            self.weight_number(lbs),
            unit_suffix(&self.weight_unit)
        )
    }

    /// A weight in pounds, without its unit.
    pub fn weight_number(&self, lbs: f64) -> String {
        pattern(lbs * self.weight_factor, &self.weight_pattern)
    }

    /// A height in inches: `5' 11"` for feet and inches, else with the
    /// unit.
    pub fn height(&self, inches: i64) -> String {
        if self.height_unit == "ftin" {
            format!("{}' {}\"", inches / 12, inches % 12)
        } else {
            format!(
                "{}{}",
                pattern(inches as f64 * self.height_factor, &self.height_pattern),
                unit_suffix(&self.height_unit)
            )
        }
    }
}

/// The text after a number: a space and the unit, or the unit alone
/// when written with a leading `~`.
pub fn unit_suffix(unit: &str) -> String {
    match unit.strip_prefix('~') {
        Some(u) => u.to_string(),
        None => format!(" {}", unit),
    }
}

/// Format a number by a pattern of the form `#`, `#.#`, `#.##`, ...:
/// at most that many decimals, rounded half to even, no trailing zeros.
pub fn pattern(x: f64, pat: &str) -> String {
    let decimals = pat.split_once('.').map(|(_, d)| d.len()).unwrap_or(0);
    let scale = 10f64.powi(decimals as i32);
    let scaled = x * scale;
    // round half to even, tolerating representation error
    let floor = scaled.floor();
    let diff = scaled - floor;
    let r = if (diff - 0.5).abs() < 1e-9 {
        if floor % 2.0 == 0.0 {
            floor
        } else {
            floor + 1.0
        }
    } else {
        scaled.round()
    };
    let v = r / scale;
    let mut s = format!("{:.*}", decimals, v);
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if s == "-0" {
        s = "0".into();
    }
    s
}

/// A number written the way a single-precision float prints: always
/// with a decimal point (`4.0`, `3.5`).
pub fn float_text(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e7 {
        format!("{:.1}", x)
    } else {
        format!("{}", x as f32)
    }
}

/// A decimal with trailing zeros removed (`21`, `0.1`, `0`).
pub fn trim_zeros(x: f64) -> String {
    let s = format!("{:.6}", x);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() {
        "0".into()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns() {
        assert_eq!(pattern(5.0, "#.##"), "5");
        assert_eq!(pattern(6.25, "#.##"), "6.25");
        assert_eq!(pattern(112.5, "#.##"), "112.5");
        assert_eq!(pattern(0.125, "#.##"), "0.12");
        assert_eq!(pattern(0.135, "#.##"), "0.14");
        assert_eq!(pattern(2.5, "#"), "2");
        assert_eq!(pattern(3.5, "#"), "4");
        assert_eq!(pattern(1.0 / 3.0, "#.#"), "0.3");
        assert_eq!(pattern(0.0, "#.##"), "0");
        assert_eq!(pattern(-0.001, "#.##"), "0");
        assert_eq!(pattern(1300.0, "#.##"), "1300");
    }

    #[test]
    fn suffixes_and_units() {
        assert_eq!(unit_suffix("ft."), " ft.");
        assert_eq!(unit_suffix("~'"), "'");
        let u = Units::default();
        assert_eq!(u.distance(5.0), "5'");
        assert_eq!(u.height(71), "5' 11\"");
        assert_eq!(u.weight(185.0), "185 lbs.");
    }

    #[test]
    fn float_and_trim() {
        assert_eq!(float_text(4.0), "4.0");
        assert_eq!(float_text(3.5), "3.5");
        assert_eq!(trim_zeros(21.0), "21");
        assert_eq!(trim_zeros(0.1), "0.1");
        assert_eq!(trim_zeros(0.0), "0");
        assert_eq!(trim_zeros(49170.8), "49170.8");
    }
}
