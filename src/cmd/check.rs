//
// cmd/check.rs -- lint and tags subcommands
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use super::data::{load_dataset, parse_kind};
use super::{print_json, CmdResult};
use crate::GlobalOpts;
use pcgtools::catalog::Catalog;
use pcgtools::lint::{lint_lst, LintOptions};
use pcgtools::lst::LstFile;
use pcgtools::{Diagnostics, Severity};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

pub struct LintArgs<'a> {
    pub files: &'a [PathBuf],
    pub kind: Option<&'a str>,
    pub summary: bool,
    pub opts: LintOptions,
    pub min: Severity,
}

pub fn lint(o: &GlobalOpts, a: LintArgs<'_>) -> CmdResult {
    let mut diags = Diagnostics::default();

    if let Some(k) = a.kind {
        let kind = parse_kind(k)?;
        for f in a.files {
            let lf = LstFile::read(f)?;
            diags.extend(lint_lst(f, kind, &lf, &a.opts));
        }
    } else {
        let (reg, plan, ds) = load_dataset(o, a.files);
        diags.extend(plan.diags.clone());
        if let Some(gm) = reg.dirs.gamemodes() {
            let modes = pcgtools::gamemode::GameModes::scan(&gm);
            let camps = plan.campaigns.iter().filter_map(|p| reg.get(p));
            diags.extend(pcgtools::lint::lint_gamemodes(camps, &modes));
        }
        let mut seen = HashSet::new();
        for pf in plan.ordered() {
            if !seen.insert((pf.kind, pf.path.clone())) {
                continue;
            }
            match LstFile::read(&pf.path) {
                Ok(lf) => diags.extend(lint_lst(&pf.path, pf.kind, &lf, &a.opts)),
                Err(e) => diags.push(pcgtools::Diagnostic::error(e.to_string())),
            }
        }
        diags.extend(ds.diags);
    }

    diags.items.retain(|d| d.severity >= a.min);
    let errors = diags.count(Severity::Error);

    if a.summary {
        // group by message shape: quoted names replaced
        let mut groups: BTreeMap<String, usize> = BTreeMap::new();
        for d in &diags.items {
            let mut shape = String::new();
            let mut quoted = false;
            for c in d.message.chars() {
                if c == '\'' {
                    quoted = !quoted;
                    shape.push_str(if quoted { "'…" } else { "'" });
                } else if !quoted {
                    shape.push(c);
                }
            }
            *groups
                .entry(format!("{}: {}", d.severity, shape))
                .or_default() += 1;
        }
        let mut v: Vec<_> = groups.into_iter().collect();
        v.sort_by_key(|e| std::cmp::Reverse(e.1));
        if o.json {
            print_json(&v);
        } else {
            for (k, n) in v {
                println!("{:>7}  {}", n, k);
            }
        }
    } else if o.json {
        print_json(&diags);
    } else {
        for d in &diags.items {
            println!("{}", d);
        }
    }
    Ok(errors == 0)
}

pub fn tags(o: &GlobalOpts, scope: Option<&str>) -> CmdResult {
    let cat = Catalog::get();
    let want = scope.map(|s| s.to_ascii_uppercase());
    let rows: Vec<_> = cat
        .entries
        .iter()
        .filter(|e| want.as_deref().is_none_or(|w| e.scope == w))
        .collect();
    if o.json {
        print_json(&rows);
    } else {
        for e in rows {
            let st = match e.status {
                pcgtools::catalog::Status::Active => "",
                pcgtools::catalog::Status::Deprecated => "\tdeprecated",
            };
            println!("{}\t{}{}", e.scope, e.tag, st);
        }
    }
    Ok(true)
}
