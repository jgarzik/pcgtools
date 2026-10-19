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
//! spells, ...).  Each section lives in its own module.

mod basics;
mod combat;
mod skills;
pub mod tree;

use crate::context::Context;
use crate::dataset::Object;
use crate::pcg::character::Character;
use crate::rules::calc::{Calc, Sheet};
use crate::rules::vars::system::VarSystem;
pub use tree::{compare, Diff, Element};

/// Everything the section builders need.
pub struct SheetCtx<'a> {
    pub ch: &'a Character,
    pub ctx: &'a Context,
    pub calc: Calc<'a>,
    pub sheet: Sheet,
    pub vars: VarSystem<'a>,
}

impl<'a> SheetCtx<'a> {
    pub fn new(ch: &'a Character, ctx: &'a Context) -> SheetCtx<'a> {
        let mut calc = Calc::new(&ctx.dataset, ctx.gamemode.as_ref(), ch);
        let sheet = calc.run();
        let vars = VarSystem::new(&calc);
        SheetCtx {
            ch,
            ctx,
            calc,
            sheet,
            vars,
        }
    }
}

/// Format a modifier with an explicit sign.
pub fn signed(v: i64) -> String {
    format!("{:+}", v)
}

/// Is an object shown on output (VISIBLE is not NO or DISPLAY)?
pub fn exported(o: &Object) -> bool {
    !matches!(
        o.get("VISIBLE").map(|v| v.to_ascii_uppercase()).as_deref(),
        Some("NO") | Some("DISPLAY")
    )
}

/// Build the sheet for a character.
pub fn build(ch: &Character, ctx: &Context) -> Element {
    let s = SheetCtx::new(ch, ctx);
    let mut root = Element::new("character");
    root.push(basics::build(&s));
    for e in combat::build(&s) {
        root.push(e);
    }
    root.push(skills::build(&s));
    for e in combat::build_after_skills(&s) {
        root.push(e);
    }
    root
}
