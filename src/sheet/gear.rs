//
// sheet/gear.rs -- carrying capacity
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use super::units::pattern;
use super::{Element, SheetCtx};

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
