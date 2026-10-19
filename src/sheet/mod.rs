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
mod gear;
mod lists;
mod skills;
pub mod tree;
pub mod units;

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

/// The name an object is shown under: OUTPUTNAME (with `[NAME]`
/// standing for the parenthesized part of the name, its `/`-separated
/// pieces reversed), else the name.
pub fn output_name(o: &Object) -> String {
    let Some(out) = o.get("OUTPUTNAME") else {
        return o.name.clone();
    };
    if out.eq_ignore_ascii_case("[BASE]") {
        return o.name.split(" (").next().unwrap_or(&o.name).to_string();
    }
    if out.contains("[NAME]") {
        let inner = o
            .name
            .split_once('(')
            .map(|(_, r)| r.trim_end_matches(')'))
            .unwrap_or("");
        let parts: Vec<&str> = inner.split('/').map(str::trim).rev().collect();
        return out.replace("[NAME]", &parts.join(" "));
    }
    out.to_string()
}

/// A text element that is split into `<para>` children when the text
/// has line breaks (each CR or LF starts a new paragraph).
pub fn paragraphs(tag: &str, text: &str) -> Element {
    if !text.contains(['\r', '\n']) {
        return Element::leaf(tag, text);
    }
    let mut e = Element::new(tag);
    for p in text.split(['\r', '\n']) {
        e.add("para", p);
    }
    e
}

/// Rendered DESC text of an object: each description whose
/// prerequisites pass, with `%1`.. replaced by its variables, joined
/// with spaces.
pub fn describe(s: &SheetCtx, o: &Object) -> String {
    describe_with(s, o, &[])
}

/// [`describe`] for an object taken with the given choices.
pub fn describe_with(s: &SheetCtx, o: &Object, choices: &[String]) -> String {
    let mut out: Vec<String> = Vec::new();
    for v in o.values("DESC") {
        if v == ".CLEAR" {
            out.clear();
            continue;
        }
        let mut parts = v.split('|');
        let text = crate::pcg::decode(parts.next().unwrap_or(""));
        let mut vars = Vec::new();
        let mut ok = true;
        for p in parts {
            if crate::token::pre::Prereq::is_pre_tag(p.split(':').next().unwrap_or(""))
                || p.starts_with("!PRE")
            {
                if let Ok(pr) = crate::token::pre::Prereq::parse(p) {
                    ok &= s.calc.passes(&pr, None);
                }
            } else {
                vars.push(p);
            }
        }
        if !ok {
            continue;
        }
        let value = |n: usize| -> String {
            let Some(v) = vars.get(n) else {
                return String::new();
            };
            match *v {
                "%NAME" => output_name(o),
                "%CHOICE" => choices.first().cloned().unwrap_or_default(),
                "%LIST" => {
                    let mut c: Vec<&String> = choices.iter().collect();
                    c.sort();
                    let c: Vec<&str> = c.iter().map(|x| x.as_str()).collect();
                    if c.len() == 2 {
                        c.join(" and ")
                    } else {
                        c.join(", ")
                    }
                }
                q if q.len() >= 2 && q.starts_with('"') && q.ends_with('"') => {
                    q[1..q.len() - 1].to_string()
                }
                f => (s.calc.eval_formula(f) as i64).to_string(),
            }
        };
        out.push(substitute(&text, value));
    }
    out.retain(|t| !t.is_empty());
    out.join(" ")
}

/// Replace `%1`, `%{1}` with the 1-based variable values; `%%` is a
/// literal percent sign.
pub fn substitute(text: &str, value: impl Fn(usize) -> String) -> String {
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c != '%' {
            out.push(c);
            i += 1;
            continue;
        }
        if chars.get(i + 1) == Some(&'%') {
            out.push('%');
            i += 2;
            continue;
        }
        let (digits, end) = if chars.get(i + 1) == Some(&'{') {
            let close = chars[i..].iter().position(|&x| x == '}').map(|p| p + i);
            match close {
                Some(e) => (chars[i + 2..e].iter().collect::<String>(), e + 1),
                None => (String::new(), i + 1),
            }
        } else {
            let mut e = i + 1;
            while e < chars.len() && chars[e].is_ascii_digit() {
                e += 1;
            }
            (chars[i + 1..e].iter().collect::<String>(), e)
        };
        match digits.parse::<usize>() {
            Ok(n) if n >= 1 => {
                out.push_str(&value(n - 1));
                i = end;
            }
            _ => {
                out.push('%');
                i += 1;
            }
        }
    }
    out
}

/// Where an object comes from: `Publisher - Book, page`.
pub fn source_text(s: &SheetCtx, o: &Object) -> String {
    let ds = &s.ctx.dataset;
    let origin = o.history.last().map(|c| c.origin).unwrap_or(o.origin);
    let file = ds.file_path(origin);
    let campaign = s
        .ctx
        .plan
        .files
        .values()
        .flatten()
        .find(|f| f.path == file)
        .and_then(|f| s.ctx.registry.get(&f.campaign));
    let meta = |tag: &str| -> Option<String> {
        campaign.and_then(|c| {
            c.meta
                .iter()
                .find(|(t, _)| t == tag)
                .map(|(_, v)| v.clone())
        })
    };
    // a later .MOD from another book makes that book the source
    let modified = !o.history.is_empty();
    let src = |tag: &str| -> Option<String> {
        o.get(tag).map(str::to_string).or_else(|| {
            if modified {
                return None;
            }
            o.source
                .iter()
                .find(|(t, _)| t == tag)
                .map(|(_, v)| v.clone())
        })
    };
    let mut out = String::new();
    if o.is_type("Custom") {
        out.push_str("Custom - ");
    }
    if let Some(p) = meta("PUBNAMELONG").filter(|p| !p.trim().is_empty()) {
        out.push_str(&p);
        out.push_str(" - ");
    }
    if let Some(l) = src("SOURCELONG").or_else(|| meta("SOURCELONG")) {
        out.push_str(&l);
    }
    if let Some(p) = src("SOURCEPAGE").filter(|p| !p.is_empty()) {
        out.push_str(", ");
        out.push_str(&p);
    }
    out
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
    root.push(lists::class_features(&s));
    root.push(gear::equipment(&s));
    root.push(gear::weight_allowance(&s));
    root.push(lists::special_abilities(&s));
    root.push(lists::feats(&s));
    root.push(lists::ability_objects(&s));
    if let Some(e) = lists::domains(&s) {
        root.push(e);
    }
    root.push(lists::weapon_proficiencies(&s));
    root.push(lists::languages(&s));
    if let Some(e) = lists::templates(&s) {
        root.push(e);
    }
    root.push(lists::misc(&s));
    root.push(lists::companions(&s));
    if let Some(e) = lists::prohibited_schools(&s) {
        root.push(e);
    }
    root
}
