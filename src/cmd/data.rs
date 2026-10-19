//
// cmd/data.rs -- load, dump, show subcommands
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
use pcgtools::dataset::{Dataset, Object};
use pcgtools::lst::FileKind;
use pcgtools::{Error, Severity};
use std::path::PathBuf;

/// Load the dataset for a campaign selection.
pub fn load_dataset(o: &GlobalOpts, pcc: &[PathBuf]) -> (Registry, LoadPlan, Dataset) {
    let mut reg = Registry::new(o.dirs(pcc.first().map(PathBuf::as_path)));
    let plan = LoadPlan::build(&mut reg, pcc);
    let ds = Dataset::load(&plan);
    (reg, plan, ds)
}

pub fn parse_kind(s: &str) -> pcgtools::Result<FileKind> {
    FileKind::from_name(s).ok_or_else(|| Error::Other(format!("unknown object kind '{}'", s)))
}

pub fn load(o: &GlobalOpts, pcc: &[PathBuf], quiet: bool) -> CmdResult {
    let (_, plan, ds) = load_dataset(o, pcc);
    let mut diags = plan.diags.clone();
    diags.extend(ds.diags.clone());

    if o.json {
        let counts: std::collections::BTreeMap<String, usize> = ds
            .kinds
            .iter()
            .map(|(k, t)| (k.to_string(), t.len()))
            .collect();
        print_json(&serde_json::json!({
            "campaigns": plan.campaigns.len(),
            "files": ds.files.len(),
            "objects": counts,
            "tables": ds.tables.len(),
            "errors": diags.count(Severity::Error),
            "warnings": diags.count(Severity::Warning),
        }));
    } else {
        println!(
            "{} campaigns, {} files",
            plan.campaigns.len(),
            ds.files.len()
        );
        for (k, t) in &ds.kinds {
            println!("  {:<16} {}", k.to_string(), t.len());
        }
        if !ds.tables.is_empty() {
            println!("  {:<16} {}", "(tables)", ds.tables.len());
        }
        println!(
            "{} errors, {} warnings",
            diags.count(Severity::Error),
            diags.count(Severity::Warning)
        );
    }
    if !quiet {
        report(&diags);
    }
    Ok(true)
}

pub fn dump(o: &GlobalOpts, pcc: &[PathBuf]) -> CmdResult {
    let (_, _, ds) = load_dataset(o, pcc);
    print_json(&ds);
    Ok(true)
}

/// Render an object in LST-like text form, with provenance.
pub fn render(ds: &Dataset, obj: &Object, indent: &str) -> String {
    let mut s = String::new();
    let head = match &obj.head_tag {
        Some(t) => format!("{}:{}", t, obj.name),
        None => obj.name.clone(),
    };
    s.push_str(&format!(
        "{}{}\t# {}:{}\n",
        indent,
        head,
        ds.file_path(obj.origin).display(),
        obj.origin.line
    ));
    for t in &obj.tokens {
        s.push_str(&format!("{}\t{}\n", indent, t.text()));
    }
    for sl in &obj.sublines {
        let toks: Vec<String> = sl.tokens.iter().map(|t| t.text()).collect();
        s.push_str(&format!(
            "{}  [{}:{}]\t{}\n",
            indent,
            sl.tag,
            sl.value,
            toks.join("\t")
        ));
    }
    for c in &obj.children {
        s.push_str(&render(ds, c, &format!("{}    ", indent)));
    }
    for h in &obj.history {
        s.push_str(&format!(
            "{}\t# {} at {}:{}\n",
            indent,
            h.op,
            ds.file_path(h.origin).display(),
            h.origin.line
        ));
    }
    s
}

pub fn show(o: &GlobalOpts, pcc: &[PathBuf], kind: &str, key: &str) -> CmdResult {
    let kind = parse_kind(kind)?;
    let (_, _, ds) = load_dataset(o, pcc);
    let Some(obj) = ds.get(kind, key) else {
        eprintln!("{} '{}' not found", kind, key);
        return Ok(false);
    };
    if o.json {
        print_json(obj);
    } else {
        print!("{}", render(&ds, obj, ""));
    }
    Ok(true)
}
