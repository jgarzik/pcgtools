//
// sheet/combat.rs -- hit points, armor class, initiative, saves, attack
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use super::{signed, Element, SheetCtx};

pub fn build(s: &SheetCtx) -> Vec<Element> {
    let sh = &s.sheet;
    let mut hp = Element::new("hit_points");
    hp.add("points", sh.hp);

    let mut ac = Element::new("armor_class");
    ac.add("total", sh.ac.get("Total").copied().unwrap_or(0));
    ac.add("flat", sh.ac.get("Flatfooted").copied().unwrap_or(0));
    ac.add("touch", sh.ac.get("Touch").copied().unwrap_or(0));

    let mut saves = Element::new("saving_throws");
    for (k, v) in &sh.saves {
        let mut st = Element::new("saving_throw");
        st.push(
            Element::new("name")
                .with(Element::leaf("long", k.to_lowercase()))
                .with(Element::leaf(
                    "short",
                    k.to_lowercase().chars().take(4).collect::<String>(),
                )),
        );
        st.add("total", signed(*v));
        saves.push(st);
    }
    vec![hp, ac, saves]
}
