//
// sheet/skills.rs -- the skills section
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use super::{exported, Element, SheetCtx};
use crate::lst::kind::FileKind;
use std::collections::HashMap;

pub fn build(s: &SheetCtx) -> Element {
    let mut skills = Element::new("skills");
    let assoc: HashMap<String, String> =
        s.ch.skills()
            .into_iter()
            .filter_map(|e| Some((e.name.to_lowercase(), e.associated?)))
            .collect();
    // skills shown on output: hidden ones and display-only ones (used
    // in the editor, with an export copy elsewhere) are left out
    for sk_obj in s.ctx.dataset.objects(FileKind::Skill) {
        if !exported(sk_obj) {
            continue;
        }
        let v = s.sheet.skills.get(&sk_obj.key).copied().unwrap_or(0);
        let mut sk = Element::new("skill");
        // a skill with sub-choices shows them after the name
        match assoc.get(&sk_obj.name.to_lowercase()) {
            Some(a) => sk.add("name", format!("{}({})", sk_obj.name, a)),
            None => sk.add("name", &sk_obj.name),
        }
        sk.add("skill_mod", v);
        skills.push(sk);
    }
    skills
}
