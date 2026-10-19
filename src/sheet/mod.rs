//
// sheet/mod.rs -- character sheet output
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! Builds a structured character sheet: the character's own data plus
//! every derived value, organized in sections (basics, abilities, hit
//! points, armor class, saves, skills, attacks, equipment, abilities,
//! spells, ...).

pub mod tree;

use crate::context::Context;
use crate::pcg::character::Character;
use crate::rules::calc::Calc;
pub use tree::{compare, Diff, Element};

/// Format a modifier with an explicit sign.
pub fn signed(v: i64) -> String {
    format!("{:+}", v)
}

/// Is an object shown on output (VISIBLE is not NO or DISPLAY)?
pub fn exported(o: &crate::dataset::Object) -> bool {
    !matches!(
        o.get("VISIBLE").map(|v| v.to_ascii_uppercase()).as_deref(),
        Some("NO") | Some("DISPLAY")
    )
}

/// Build the sheet for a character.
pub fn build(ch: &Character, ctx: &Context) -> Element {
    let mut calc = Calc::new(&ctx.dataset, ctx.gamemode.as_ref(), ch);
    let s = calc.run();
    let mut root = Element::new("character");

    let mut basics = Element::new("basics");
    basics.add("name", ch.name().unwrap_or_default());
    basics.add("bab", signed(s.bab));
    root.push(basics);

    let mut hp = Element::new("hit_points");
    hp.add("points", s.hp);
    root.push(hp);

    let mut ac = Element::new("armor_class");
    ac.add("total", s.ac.get("Total").copied().unwrap_or(0));
    ac.add("flat", s.ac.get("Flatfooted").copied().unwrap_or(0));
    ac.add("touch", s.ac.get("Touch").copied().unwrap_or(0));
    root.push(ac);

    let mut skills = Element::new("skills");
    let assoc: std::collections::HashMap<String, String> = ch
        .skills()
        .into_iter()
        .filter_map(|e| Some((e.name.to_lowercase(), e.associated?)))
        .collect();
    // skills shown on output: hidden ones and display-only ones (used
    // in the editor, with an export copy elsewhere) are left out
    for sk_obj in ctx.dataset.objects(crate::lst::kind::FileKind::Skill) {
        if !exported(sk_obj) {
            continue;
        }
        let v = s.skills.get(&sk_obj.key).copied().unwrap_or(0);
        let mut sk = Element::new("skill");
        // a skill with a sub-choice shows it after the name
        match assoc.get(&sk_obj.name.to_lowercase()) {
            Some(a) => sk.add("name", format!("{}({})", sk_obj.name, a)),
            None => sk.add("name", &sk_obj.name),
        }
        sk.add("skill_mod", v);
        skills.push(sk);
    }
    root.push(skills);

    let mut saves = Element::new("saving_throws");
    for (k, v) in &s.saves {
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
    root.push(saves);
    root
}
