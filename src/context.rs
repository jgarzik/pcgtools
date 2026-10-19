//
// context.rs -- the data a character needs
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! Given a character, find its game mode and campaigns (applying the
//! game mode's key migrations), load the dataset, and check that the
//! character's references resolve.

use crate::campaign::{LoadPlan, Registry};
use crate::dataset::Dataset;
use crate::error::{Diagnostic, Diagnostics};
use crate::gamemode::{GameMode, GameModes};
use crate::lst::kind::FileKind;
use crate::pcc::resolve::DataDirs;
use crate::pcg::character::Character;
use std::path::PathBuf;

/// Everything loaded on behalf of a character.
pub struct Context {
    pub registry: Registry,
    pub plan: LoadPlan,
    pub dataset: Dataset,
    pub gamemode: Option<GameMode>,
    /// the character's saved PCGen version, for key migration
    pub pcg_version: String,
    pub diags: Diagnostics,
}

impl Context {
    /// Load the game mode and campaigns a character names.
    pub fn for_character(c: &Character, dirs: DataDirs) -> Context {
        let mut diags = Diagnostics::default();
        let gamemode = match (dirs.gamemodes(), c.gamemode()) {
            (Some(gmdir), Some(name)) => {
                let mut modes = GameModes::scan(&gmdir);
                diags.extend(std::mem::take(&mut modes.diags));
                let idx = modes
                    .modes
                    .iter()
                    .position(|m| m.key.eq_ignore_ascii_case(&name))
                    .or_else(|| modes.modes.iter().position(|m| m.matches(&name)));
                match idx {
                    Some(i) => Some(modes.modes.swap_remove(i)),
                    None => {
                        diags.push(Diagnostic::warning(format!("unknown game mode '{}'", name)));
                        None
                    }
                }
            }
            (None, _) => {
                diags.push(Diagnostic::info(
                    "no system directory; game mode not loaded",
                ));
                None
            }
            (_, None) => {
                diags.push(Diagnostic::warning("character has no GAMEMODE"));
                None
            }
        };
        let pcg_version = c.pcg.value("VERSION").unwrap_or_default();
        Context::load(c.campaigns(), dirs, gamemode, pcg_version, diags)
    }

    /// Load a set of campaign keys under a game mode.
    pub fn load(
        keys: Vec<String>,
        dirs: DataDirs,
        gamemode: Option<GameMode>,
        pcg_version: String,
        mut diags: Diagnostics,
    ) -> Context {
        let mut registry = Registry::new(dirs.clone());
        for d in [&dirs.data, &dirs.vendor, &dirs.homebrew]
            .into_iter()
            .flatten()
        {
            registry.scan(d, &mut diags);
        }
        let mut selected: Vec<PathBuf> = Vec::new();
        for k in keys {
            let key = gamemode
                .as_ref()
                .and_then(|g| g.migrate("SOURCE", &k, &pcg_version))
                .unwrap_or(k);
            match registry.by_key(&key) {
                Some(c) => selected.push(c.path.clone()),
                None => diags.push(Diagnostic::error(format!("campaign '{}' not found", key))),
            }
        }
        let plan = LoadPlan::build(&mut registry, &selected);
        let dataset = Dataset::load(&plan);
        Context {
            registry,
            plan,
            dataset,
            gamemode,
            pcg_version,
            diags,
        }
    }

    /// Migrate an old object key (kind is SOURCE, RACE, ABILITY,
    /// EQUIPMENT or SPELL).
    pub fn migrate(&self, kind: &str, key: &str) -> String {
        self.gamemode
            .as_ref()
            .and_then(|g| g.migrate(kind, key, &self.pcg_version))
            .unwrap_or_else(|| key.to_string())
    }

    /// Check that everything the character refers to exists in the
    /// dataset.
    pub fn check_references(&self, c: &Character) -> Diagnostics {
        let mut d = Diagnostics::default();
        let ds = &self.dataset;
        let mut need = |kind: FileKind, key: &str, what: &str| {
            if ds.get(kind, key).is_none() {
                d.push(Diagnostic::warning(format!(
                    "{} '{}' not found in data",
                    what, key
                )));
            }
        };

        if let Some(r) = c.race() {
            if !r.eq_ignore_ascii_case("none") {
                need(FileKind::Race, &self.migrate("RACE", &r), "race");
            }
        }
        for cl in c.classes() {
            need(FileKind::Class, &cl.name, "class");
        }
        for (abb, _) in c.stats() {
            need(FileKind::Stat, &abb, "stat");
        }
        if let Some(a) = c.alignment() {
            need(FileKind::Alignment, &a, "alignment");
        }
        for t in c.templates() {
            need(FileKind::Template, &t, "template");
        }
        for s in c.skills() {
            need(FileKind::Skill, &s.name, "skill");
        }
        for l in c.languages() {
            need(FileKind::Language, &l, "language");
        }
        if let Some(dei) = c.deity() {
            need(FileKind::Deity, &dei, "deity");
        }
        for dom in c.domains() {
            need(FileKind::Domain, &dom, "domain");
        }
        for a in c.abilities() {
            if a.nature == "VIRTUAL" || a.key.starts_with('*') {
                continue;
            }
            let key = self.migrate("ABILITY", &format!("{}|{}", a.category, a.key));
            let key = key
                .rsplit_once('|')
                .map(|(_, k)| k.to_string())
                .unwrap_or(key);
            if ds.get_in(FileKind::Ability, &a.category, &key).is_none() {
                d.push(Diagnostic::warning(format!(
                    "ability '{}' ({}) not found in data",
                    a.key, a.category
                )));
            }
        }
        for e in c.equipment() {
            let key = e.base.clone().unwrap_or_else(|| e.name.clone());
            let key = self.migrate("EQUIPMENT", &key);
            if ds.get(FileKind::Equipment, &key).is_none() {
                d.push(Diagnostic::warning(format!(
                    "equipment '{}' not found in data",
                    key
                )));
            }
        }
        d
    }
}
