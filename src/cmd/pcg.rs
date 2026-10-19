//
// cmd/pcg.rs -- character file subcommands
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use super::{print_json, report, CmdResult};
use crate::GlobalOpts;
use pcgtools::context::Context;
use pcgtools::pcg::character::Character;
use pcgtools::Severity;
use std::path::Path;

pub fn show(o: &GlobalOpts, file: &Path) -> CmdResult {
    let c = Character::read(file)?;
    report(&c.pcg.diags);
    if o.json {
        print_json(&serde_json::json!({
            "name": c.name(),
            "gamemode": c.gamemode(),
            "campaigns": c.campaigns(),
            "race": c.race(),
            "alignment": c.alignment(),
            "deity": c.deity(),
            "experience": c.experience(),
            "stats": c.stats(),
            "classes": c.classes(),
            "levels": c.levels(),
            "skills": c.skills(),
            "abilities": c.abilities(),
            "languages": c.languages(),
            "templates": c.templates(),
            "domains": c.domains(),
            "equipment": c.equipment(),
            "equipped": c.equipped(),
        }));
        return Ok(true);
    }
    println!("{}", c.name().unwrap_or_default());
    println!("  game mode:  {}", c.gamemode().unwrap_or_default());
    println!("  campaigns:  {}", c.campaigns().join("; "));
    println!("  race:       {}", c.race().unwrap_or_default());
    println!("  alignment:  {}", c.alignment().unwrap_or_default());
    let classes: Vec<String> = c
        .classes()
        .iter()
        .map(|x| format!("{} {}", x.name, x.level))
        .collect();
    println!("  classes:    {}", classes.join(", "));
    println!("  experience: {}", c.experience());
    let stats: Vec<String> = c
        .stats()
        .iter()
        .map(|(a, s)| format!("{} {}", a, s))
        .collect();
    println!("  stats:      {}", stats.join(", "));
    let skills: Vec<String> = c
        .skills()
        .iter()
        .filter(|s| s.total_ranks() > 0.0)
        .map(|s| format!("{} {}", s.name, s.total_ranks()))
        .collect();
    println!("  skills:     {}", skills.join(", "));
    let abil: Vec<String> = c
        .abilities()
        .iter()
        .filter(|a| a.nature == "NORMAL")
        .map(|a| format!("{} ({})", a.key, a.category))
        .collect();
    println!("  abilities:  {}", abil.join(", "));
    println!("  languages:  {}", c.languages().join(", "));
    let eq: Vec<String> = c
        .equipped()
        .iter()
        .map(|e| format!("{}: {}", e.location, e.item.clone().unwrap_or_default()))
        .collect();
    println!("  equipped:   {}", eq.join("; "));
    Ok(true)
}

pub fn lint(o: &GlobalOpts, file: &Path) -> CmdResult {
    let c = Character::read(file)?;
    let ctx = Context::for_character(&c, o.dirs(None));
    let mut diags = c.pcg.diags.clone();
    diags.extend(ctx.diags.clone());
    diags.extend(ctx.check_references(&c));
    if o.json {
        print_json(&diags);
    } else {
        for d in &diags.items {
            println!("{}", d);
        }
        println!(
            "{} objects loaded from {} campaigns",
            ctx.dataset.len(),
            ctx.plan.campaigns.len()
        );
    }
    Ok(diags.count(Severity::Error) == 0)
}

pub fn stats(o: &GlobalOpts, file: &Path, verbose: bool, explain: &[String]) -> CmdResult {
    let c = Character::read(file)?;
    let ctx = Context::for_character(&c, o.dirs(None));
    report(&ctx.diags);
    let mut calc = pcgtools::rules::calc::Calc::new(&ctx.dataset, ctx.gamemode.as_ref(), &c);
    let sheet = calc.run();
    for key in explain {
        let (cat, tgt) = key.split_once('.').unwrap_or((key.as_str(), ""));
        println!("{}:", key);
        for line in calc.explain(cat, tgt) {
            println!("  {}", line);
        }
    }
    if !explain.is_empty() {
        return Ok(true);
    }
    if o.json {
        print_json(&sheet);
        return Ok(true);
    }
    println!("{} (level {})", c.name().unwrap_or_default(), sheet.level);
    let stats: Vec<String> = sheet
        .stats
        .iter()
        .map(|(k, (s, m))| format!("{} {} ({:+})", k, s, m))
        .collect();
    println!("  stats: {}", stats.join(", "));
    println!("  BAB {:+}  HP {}", sheet.bab, sheet.hp);
    let saves: Vec<String> = sheet
        .saves
        .iter()
        .map(|(k, v)| format!("{} {:+}", k, v))
        .collect();
    println!("  saves: {}", saves.join(", "));
    let ac: Vec<String> = sheet
        .ac
        .iter()
        .map(|(k, v)| format!("{} {}", k, v))
        .collect();
    println!("  AC: {}", ac.join(", "));
    let skills: Vec<String> = sheet
        .skills
        .iter()
        .filter(|(k, _)| verbose || calc.skill_ranks(k) > 0.0)
        .map(|(k, v)| format!("{} {:+}", k, v))
        .collect();
    println!("  skills: {}", skills.join(", "));
    if verbose {
        println!("  abilities: {}", sheet.abilities.join(", "));
        println!("  passes: {}", sheet.passes);
        let unk: Vec<String> = sheet
            .unknown
            .iter()
            .map(|(k, n)| format!("{}x{}", k, n))
            .collect();
        println!("  unknown terms: {}", unk.join(", "));
    }
    Ok(true)
}
