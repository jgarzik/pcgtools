//
// campaign.rs -- campaign selection and LST load planning
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! Given a set of selected campaigns, work out which LST files will be
//! loaded, in what order, with which filters.

use crate::error::{Diagnostic, Diagnostics, Result};
use crate::lst::kind::FileKind;
use crate::pcc::fileref::Filter;
use crate::pcc::resolve::DataDirs;
use crate::pcc::{Campaign, PccFile};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Cache of parsed campaigns, keyed by file path.
#[derive(Debug, Default)]
pub struct Registry {
    pub dirs: DataDirs,
    campaigns: BTreeMap<PathBuf, Campaign>,
}

impl Registry {
    pub fn new(dirs: DataDirs) -> Registry {
        Registry {
            dirs,
            campaigns: BTreeMap::new(),
        }
    }

    /// Load (or fetch from cache) the campaign at `path`.
    pub fn load(&mut self, path: &Path) -> Result<&Campaign> {
        let key = canonical(path);
        if !self.campaigns.contains_key(&key) {
            let pcc = PccFile::read(&key)?;
            let c = Campaign::from_pcc(&pcc, &self.dirs);
            self.campaigns.insert(key.clone(), c);
        }
        Ok(&self.campaigns[&key])
    }

    pub fn get(&self, path: &Path) -> Option<&Campaign> {
        self.campaigns.get(&canonical(path))
    }

    /// Parse every `.pcc` file below `dir`, returning their paths.
    pub fn scan(&mut self, dir: &Path, diags: &mut Diagnostics) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for ent in WalkDir::new(dir).sort_by_file_name().into_iter().flatten() {
            let p = ent.path();
            if ent.file_type().is_file()
                && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pcc"))
            {
                match self.load(p) {
                    Ok(_) => out.push(canonical(p)),
                    Err(e) => diags.push(Diagnostic::error(e.to_string())),
                }
            }
        }
        out
    }

    pub fn campaigns(&self) -> impl Iterator<Item = &Campaign> {
        self.campaigns.values()
    }

    /// Find a loaded campaign by key (case-insensitive).
    pub fn by_key(&self, key: &str) -> Option<&Campaign> {
        self.campaigns
            .values()
            .find(|c| c.key.eq_ignore_ascii_case(key))
    }

    /// The campaign plus all campaigns it includes, transitively, in
    /// depth-first order.  Loads included campaigns as needed.
    pub fn closure(&mut self, path: &Path, diags: &mut Diagnostics) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        self.closure_rec(&canonical(path), &mut seen, &mut out, diags);
        out
    }

    fn closure_rec(
        &mut self,
        path: &Path,
        seen: &mut HashSet<PathBuf>,
        out: &mut Vec<PathBuf>,
        diags: &mut Diagnostics,
    ) {
        if !seen.insert(path.to_path_buf()) {
            return;
        }
        let includes: Vec<PathBuf> = match self.load(path) {
            Ok(c) => c.includes.iter().filter_map(|(_, p)| p.clone()).collect(),
            Err(e) => {
                diags.push(Diagnostic::error(e.to_string()));
                return;
            }
        };
        out.push(path.to_path_buf());
        for inc in includes {
            self.closure_rec(&canonical(&inc), seen, out, diags);
        }
    }
}

fn canonical(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// One LST file scheduled for loading.
#[derive(Debug, Clone, Serialize)]
pub struct PlannedFile {
    pub kind: FileKind,
    pub path: PathBuf,
    pub filter: Option<Filter>,
    /// campaign whose PCC named this file
    pub campaign: PathBuf,
    /// line within that PCC
    pub line: usize,
}

/// The result of campaign selection: what to load, in order.
#[derive(Debug, Clone, Default, Serialize)]
pub struct LoadPlan {
    /// selected campaigns, in rank order
    pub selected: Vec<PathBuf>,
    /// every campaign involved (selected plus included), in load order
    pub campaigns: Vec<PathBuf>,
    /// files grouped by kind, kinds in load order
    pub files: BTreeMap<FileKind, Vec<PlannedFile>>,
    /// kinds whose objects may share keys (ALLOWDUPES)
    pub allow_dupes: BTreeSet<FileKind>,
    /// objects declared to be referenced before (or without) being
    /// defined (FORWARDREF): (kind as written, key)
    pub forward_refs: Vec<(String, String)>,
    /// each campaign's SOURCEDATE
    pub source_dates: BTreeMap<PathBuf, String>,
    /// the campaigns in use, for PRECAMPAIGN on data objects
    #[serde(skip)]
    pub campaigns_in_use: PreCampaignContext,
    pub diags: Diagnostics,
}

impl LoadPlan {
    /// Build a load plan for the selected campaign files.
    pub fn build(reg: &mut Registry, selected: &[PathBuf]) -> LoadPlan {
        let mut plan = LoadPlan::default();

        let mut sel: Vec<(PathBuf, i32)> = Vec::new();
        for p in selected {
            match reg.load(p) {
                Ok(c) => sel.push((canonical(p), c.rank)),
                Err(e) => plan.diags.push(Diagnostic::error(e.to_string())),
            }
        }
        // higher rank first; stable, so equal ranks keep user order
        sel.sort_by_key(|b| std::cmp::Reverse(b.1));
        plan.selected = sel.into_iter().map(|(p, _)| p).collect();

        // full include closure of each selected campaign
        let closures: Vec<Vec<PathBuf>> = plan
            .selected
            .clone()
            .iter()
            .map(|p| reg.closure(p, &mut plan.diags))
            .collect();

        let ctx = PreCampaignContext::new(reg, &plan.selected, &closures);
        plan.campaigns_in_use = ctx.clone();

        let mut seen_campaigns = HashSet::new();
        let mut seen_entries = HashSet::new();
        let mut excludes = HashSet::new();
        for closure in &closures {
            for cpath in closure {
                let first_seen = seen_campaigns.insert(cpath.clone());
                if first_seen {
                    plan.campaigns.push(cpath.clone());
                }
                let c = reg.get(cpath).expect("loaded by closure");
                if first_seen {
                    plan.diags.extend(c.diags.clone());
                    for (tag, value) in &c.meta {
                        match tag.as_str() {
                            "ALLOWDUPES" => {
                                if let Some(k) = FileKind::from_tag(value.trim()) {
                                    plan.allow_dupes.insert(k);
                                }
                            }
                            "FORWARDREF" => {
                                if let Some((kind, keys)) = value.split_once('|') {
                                    for k in keys.split(',') {
                                        plan.forward_refs.push((kind.to_string(), k.to_string()));
                                    }
                                }
                            }
                            "SOURCEDATE" => {
                                plan.source_dates.insert(cpath.clone(), value.clone());
                            }
                            _ => {}
                        }
                    }
                }
                excludes.extend(c.lst_excludes.iter().cloned());
                for f in &c.files {
                    let Some(path) = &f.path else { continue };
                    let pass = f.fref.prereqs.iter().all(|pre| {
                        ctx.eval(pre).unwrap_or_else(|| {
                            plan.diags.push(
                                Diagnostic::info(format!(
                                    "prerequisite ignored for file selection: {}",
                                    pre
                                ))
                                .at(&c.path, f.line),
                            );
                            true
                        })
                    });
                    if !pass {
                        continue;
                    }
                    let id = (f.kind, path.clone(), format!("{:?}", f.fref.filter));
                    if !seen_entries.insert(id) {
                        continue;
                    }
                    plan.files.entry(f.kind).or_default().push(PlannedFile {
                        kind: f.kind,
                        path: path.clone(),
                        filter: f.fref.filter.clone(),
                        campaign: cpath.clone(),
                        line: f.line,
                    });
                }
            }
        }

        // LSTEXCLUDE is global
        for list in plan.files.values_mut() {
            list.retain(|f| !excludes.contains(&f.path));
        }
        plan
    }

    /// All planned files, in load order.
    pub fn ordered(&self) -> impl Iterator<Item = &PlannedFile> {
        self.files.values().flatten()
    }
}

/// Evaluates PRECAMPAIGN against a campaign selection.
#[derive(Debug, Clone, Default)]
pub struct PreCampaignContext {
    /// (key, booktypes) of selected campaigns only
    selected: Vec<(String, Vec<String>)>,
    /// (key, booktypes) of selected campaigns and their includes
    included: Vec<(String, Vec<String>)>,
}

impl PreCampaignContext {
    fn new(reg: &Registry, selected: &[PathBuf], closures: &[Vec<PathBuf>]) -> Self {
        let info = |p: &PathBuf| {
            let c = reg.get(p).expect("loaded");
            (c.key.clone(), c.booktypes.clone())
        };
        PreCampaignContext {
            selected: selected.iter().map(info).collect(),
            included: closures.iter().flatten().map(info).collect(),
        }
    }

    /// Evaluate a `[!]PRECAMPAIGN:n,item,...` prerequisite.  Returns
    /// None for other prerequisite kinds.
    pub fn eval(&self, pre: &str) -> Option<bool> {
        let (neg, body) = match pre.strip_prefix('!') {
            Some(b) => (true, b),
            None => (false, pre),
        };
        let body = body.strip_prefix("PRECAMPAIGN:")?;
        let mut parts = body.split(',');
        let need: usize = parts.next()?.trim().parse().ok()?;
        let mut count = 0;
        for item in parts {
            let item = item.trim();
            let (inv, item) = match item.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                Some(i) => (true, i),
                None => (false, item),
            };
            let hit = if let Some(bt) = item.strip_prefix("INCLUDESBOOKTYPE=") {
                has_booktype(&self.included, bt)
            } else if let Some(bt) = item.strip_prefix("BOOKTYPE=") {
                has_booktype(&self.selected, bt)
            } else if let Some(k) = item.strip_prefix("INCLUDES=") {
                has_key(&self.included, k)
            } else {
                has_key(&self.selected, item)
            };
            if hit != inv {
                count += 1;
            }
        }
        Some((count >= need) != neg)
    }
}

fn has_key(list: &[(String, Vec<String>)], key: &str) -> bool {
    list.iter().any(|(k, _)| k.eq_ignore_ascii_case(key))
}

fn has_booktype(list: &[(String, Vec<String>)], bt: &str) -> bool {
    list.iter()
        .any(|(_, b)| b.iter().any(|x| x.eq_ignore_ascii_case(bt)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precampaign() {
        let ctx = PreCampaignContext {
            selected: vec![("Core".into(), vec!["Core Rules".into()])],
            included: vec![
                ("Core".into(), vec!["Core Rules".into()]),
                ("Sub".into(), vec!["Supplement".into()]),
            ],
        };
        assert_eq!(ctx.eval("PRECAMPAIGN:1,Core"), Some(true));
        assert_eq!(ctx.eval("PRECAMPAIGN:1,Sub"), Some(false));
        assert_eq!(ctx.eval("PRECAMPAIGN:1,INCLUDES=Sub"), Some(true));
        assert_eq!(ctx.eval("!PRECAMPAIGN:1,Core"), Some(false));
        assert_eq!(ctx.eval("PRECAMPAIGN:1,BOOKTYPE=Core Rules"), Some(true));
        assert_eq!(
            ctx.eval("PRECAMPAIGN:1,INCLUDESBOOKTYPE=Supplement"),
            Some(true)
        );
        assert_eq!(ctx.eval("PRECAMPAIGN:2,Core,Sub"), Some(false));
        assert_eq!(ctx.eval("PRECAMPAIGN:1,[Missing]"), Some(true));
        assert_eq!(ctx.eval("PRERACE:1,Elf"), None);
    }
}
