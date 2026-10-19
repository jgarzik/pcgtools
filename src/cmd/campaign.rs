//
// cmd/campaign.rs -- campaigns and deps subcommands
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use super::{print_json, report, CmdResult};
use crate::GlobalOpts;
use pcgtools::campaign::{LoadPlan, Registry};
use pcgtools::{Diagnostics, Error};
use serde_json::json;
use std::path::{Path, PathBuf};

pub fn campaigns(o: &GlobalOpts, dir: Option<&Path>, gamemode: Option<&str>) -> CmdResult {
    let dirs = o.dirs(dir);
    let root = match dir.map(Path::to_path_buf).or_else(|| dirs.data.clone()) {
        Some(r) => r,
        None => {
            return Err(Error::Other(
                "no directory given and no data dir known".into(),
            ))
        }
    };
    let mut reg = Registry::new(dirs);
    let mut diags = Diagnostics::default();
    let paths = reg.scan(&root, &mut diags);

    let mut rows = Vec::new();
    for p in &paths {
        let c = reg.get(p).expect("scanned");
        if let Some(g) = gamemode {
            if !c.supports_gamemode(g) {
                continue;
            }
        }
        rows.push(c);
    }

    if o.json {
        let v: Vec<_> = rows
            .iter()
            .map(|c| {
                json!({
                    "path": c.path, "key": c.key, "name": c.name,
                    "gamemodes": c.gamemodes, "booktypes": c.booktypes,
                    "rank": c.rank, "showinmenu": c.meta("SHOWINMENU"),
                })
            })
            .collect();
        print_json(&v);
    } else {
        for c in &rows {
            let rel = c.path.strip_prefix(&root).unwrap_or(&c.path);
            println!(
                "{}\t{}\t{}\t{}\t{}",
                c.key,
                c.gamemodes.join("|"),
                c.rank,
                c.booktypes.join("|"),
                rel.display()
            );
        }
    }
    report(&diags);
    Ok(true)
}

pub fn deps(o: &GlobalOpts, pcc: &[PathBuf]) -> CmdResult {
    let mut reg = Registry::new(o.dirs(pcc.first().map(PathBuf::as_path)));
    let plan = LoadPlan::build(&mut reg, pcc);

    if o.json {
        print_json(&plan);
    } else {
        println!("campaigns:");
        for c in &plan.campaigns {
            let camp = reg.get(c).expect("loaded");
            println!("  {} [{}] {}", camp.key, camp.rank, c.display());
        }
        for (kind, files) in &plan.files {
            println!("{}:", kind);
            for f in files {
                let filt = match &f.filter {
                    Some(flt) => format!(" ({} filter items)", flt.items().len()),
                    None => String::new(),
                };
                println!("  {}{}", f.path.display(), filt);
            }
        }
    }
    let errs = report(&plan.diags);
    Ok(!errs)
}
