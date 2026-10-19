//
// gamemode.rs -- game mode system files
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! A game mode is a directory under `system/gameModes/` holding the
//! rules-system constants that data files build on.  Every file is a
//! list of lines of the form `HEAD:value<TAB>TAG:value...`; files
//! missing from a mode are taken from the `default` mode.

use crate::error::{Diagnostic, Diagnostics, Error, Result};
use crate::lst::line::split_token;
use crate::text::TextFile;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One line of a game mode file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GmLine {
    pub line: usize,
    pub tag: String,
    pub value: String,
    pub tokens: Vec<(String, String)>,
}

impl GmLine {
    pub fn parse(line: usize, text: &str) -> Option<GmLine> {
        let mut cells = text.split('\t').map(str::trim).filter(|c| !c.is_empty());
        let (tag, value) = split_token(cells.next()?);
        Some(GmLine {
            line,
            tag: tag.to_string(),
            value: value.unwrap_or("").to_string(),
            tokens: cells
                .map(|c| {
                    let (t, v) = split_token(c);
                    (t.to_string(), v.unwrap_or("").to_string())
                })
                .collect(),
        })
    }

    pub fn get(&self, tag: &str) -> Option<&str> {
        self.tokens
            .iter()
            .find(|(t, _)| t == tag)
            .map(|(_, v)| v.as_str())
    }
}

/// One game mode file.
#[derive(Debug, Clone, Serialize)]
pub struct GmFile {
    pub path: PathBuf,
    /// true if the file came from the default mode
    pub inherited: bool,
    pub lines: Vec<GmLine>,
}

/// The files that make up a game mode, by path relative to the mode
/// directory.
pub const MODE_FILES: [&str; 14] = [
    "miscinfo.lst",
    "statsandchecks.lst",
    "level.lst",
    "load.lst",
    "sizeAdjustment.lst",
    "rules.lst",
    "equipmentslots.lst",
    "codeControl.lst",
    "migration.lst",
    "pointbuymethods_system.lst",
    "paperInfo.lst",
    "equipIcons.lst",
    "bio/biosettings.lst",
    "bio/traits.lst",
];

/// A row of an experience table.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LevelRow {
    /// `LEVEL` value: a number, or `LEVEL` for the formula row
    pub level: String,
    pub min_xp: String,
    pub class_skill_max: String,
    pub cross_class_skill_max: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GameMode {
    pub dir: PathBuf,
    /// identity (GAMEMODEKEY, else the directory name)
    pub key: String,
    /// names accepted in PCC GAMEMODE tags (ALLOWEDMODES)
    pub aliases: Vec<String>,
    pub files: BTreeMap<String, GmFile>,
    pub diags: Diagnostics,
}

fn read_lines(path: &Path) -> Result<Vec<GmLine>> {
    let tf = TextFile::read(path)?;
    Ok(tf
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| {
            let t = l.text.trim();
            !t.is_empty() && !t.starts_with('#')
        })
        .filter_map(|(i, l)| GmLine::parse(i + 1, &l.text))
        .collect())
}

impl GameMode {
    /// Load the game mode in `dir`, falling back to `default_dir` for
    /// missing files.
    pub fn load(dir: &Path, default_dir: Option<&Path>) -> Result<GameMode> {
        if !dir.is_dir() {
            return Err(Error::Other(format!(
                "not a game mode directory: {}",
                dir.display()
            )));
        }
        let mut gm = GameMode {
            dir: dir.to_path_buf(),
            key: dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            aliases: Vec::new(),
            files: BTreeMap::new(),
            diags: Diagnostics::default(),
        };
        for name in MODE_FILES {
            let own = dir.join(name);
            let (path, inherited) = if own.is_file() {
                (own, false)
            } else if let Some(p) = default_dir.map(|d| d.join(name)).filter(|p| p.is_file()) {
                (p, true)
            } else {
                continue;
            };
            match read_lines(&path) {
                Ok(lines) => {
                    gm.files.insert(
                        name.to_string(),
                        GmFile {
                            path,
                            inherited,
                            lines,
                        },
                    );
                }
                Err(e) => gm.diags.push(Diagnostic::error(e.to_string())),
            }
        }
        if !gm.files.contains_key("miscinfo.lst") {
            gm.diags
                .push(Diagnostic::error("game mode has no miscinfo.lst").at(dir, 0));
        }
        if let Some(k) = gm.misc("GAMEMODEKEY") {
            gm.key = k.to_string();
        }
        gm.aliases = gm
            .misc("ALLOWEDMODES")
            .map(|v| v.split('|').map(str::to_string).collect())
            .unwrap_or_default();
        Ok(gm)
    }

    pub fn file(&self, name: &str) -> Option<&GmFile> {
        self.files.get(name)
    }

    /// Lines of a file with the given head tag.
    pub fn lines<'a, 'b>(
        &'a self,
        file: &str,
        tag: &'b str,
    ) -> impl Iterator<Item = &'a GmLine> + use<'a, 'b> {
        self.files
            .get(file)
            .into_iter()
            .flat_map(|f| f.lines.iter())
            .filter(move |l| l.tag == tag)
    }

    /// First miscinfo value of a tag.
    pub fn misc(&self, tag: &str) -> Option<&str> {
        self.lines("miscinfo.lst", tag)
            .next()
            .map(|l| l.value.as_str())
    }

    /// Does this mode answer to `name` (its key or an alias)?
    pub fn matches(&self, name: &str) -> bool {
        self.key.eq_ignore_ascii_case(name)
            || self.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
    }

    /// Bonus types that stack with themselves (BONUSSTACKS).
    pub fn stacking_bonus_types(&self) -> Vec<String> {
        self.lines("miscinfo.lst", "BONUSSTACKS")
            .flat_map(|l| l.value.split('.'))
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// The experience table with the given name (or the first one).
    pub fn level_table(&self, name: Option<&str>) -> Vec<LevelRow> {
        let Some(f) = self.files.get("level.lst") else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut cur: Option<&str> = None;
        let mut taken: Option<&str> = None;
        for l in &f.lines {
            if l.tag == "XPTABLE" {
                cur = Some(l.value.as_str());
                continue;
            }
            if l.tag != "LEVEL" {
                continue;
            }
            let want = match (name, taken) {
                (Some(n), _) => cur == Some(n),
                (None, None) => {
                    taken = cur;
                    true
                }
                (None, Some(t)) => cur == Some(t),
            };
            if want {
                out.push(LevelRow {
                    level: l.value.clone(),
                    min_xp: l.get("MINXP").unwrap_or("").to_string(),
                    class_skill_max: l.get("CSKILLMAX").unwrap_or("").to_string(),
                    cross_class_skill_max: l.get("CCSKILLMAX").unwrap_or("").to_string(),
                });
            }
        }
        out
    }
}

/// Parse a version string such as `6.07.05` into numbers.  Missing or
/// non-numeric parts count as zero.
pub fn parse_version(s: &str) -> Vec<u32> {
    s.trim()
        .split(['.', '-', ' '])
        .take(3)
        .map(|p| {
            p.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse()
                .unwrap_or(0)
        })
        .collect()
}

fn cmp_version(a: &[u32], b: &[u32]) -> std::cmp::Ordering {
    for i in 0..3 {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x.cmp(&y);
        }
    }
    std::cmp::Ordering::Equal
}

/// Does a migration rule line apply to a character saved by `pcg_ver`?
/// The character must be no newer than MAXVER (or than MAXDEVVER
/// within the same major.minor series) and no older than MINVER.
pub fn rule_applies(rule: &GmLine, pcg_ver: &[u32]) -> bool {
    // MAXDEVVER sometimes follows MAXVER after a space, in one token
    let mut max = None;
    let mut maxdev = None;
    let mut min = None;
    for (t, v) in &rule.tokens {
        let mut parts = v.split_whitespace();
        let first = parts.next().unwrap_or("");
        match t.as_str() {
            "MAXVER" => max = Some(first.to_string()),
            "MAXDEVVER" => maxdev = Some(first.to_string()),
            "MINVER" => min = Some(first.to_string()),
            _ => {}
        }
        for extra in parts {
            if let Some(d) = extra.strip_prefix("MAXDEVVER:") {
                maxdev = Some(d.to_string());
            }
        }
    }
    if let Some(m) = max {
        if cmp_version(pcg_ver, &parse_version(&m)).is_gt() {
            let Some(d) = maxdev else {
                return false;
            };
            let dv = parse_version(&d);
            let same_series = pcg_ver.first() == dv.first() && pcg_ver.get(1) == dv.get(1);
            if cmp_version(pcg_ver, &dv).is_gt() || !same_series {
                return false;
            }
        }
    }
    if let Some(m) = min {
        if cmp_version(pcg_ver, &parse_version(&m)).is_lt() {
            return false;
        }
    }
    true
}

impl GameMode {
    /// Map an old key to its current key using `migration.lst`.
    /// `kind` is the rule type (SOURCE, RACE, ABILITY, EQUIPMENT,
    /// SPELL); ability keys are written `Category|Key`.  Returns None
    /// if no rule applies.
    pub fn migrate(&self, kind: &str, key: &str, pcg_version: &str) -> Option<String> {
        let ver = parse_version(pcg_version);
        self.lines("migration.lst", kind)
            .find(|l| l.value.eq_ignore_ascii_case(key) && rule_applies(l, &ver))
            .and_then(|l| l.get("NEWKEY"))
            .map(str::to_string)
    }
}

/// All game modes of an installation.
#[derive(Debug, Default)]
pub struct GameModes {
    pub modes: Vec<GameMode>,
    pub diags: Diagnostics,
}

impl GameModes {
    /// Load every mode below `system/gameModes`.
    pub fn scan(gamemodes_dir: &Path) -> GameModes {
        let mut out = GameModes::default();
        let default = gamemodes_dir.join("default");
        let default = default.is_dir().then_some(default);
        let mut dirs: Vec<PathBuf> = match std::fs::read_dir(gamemodes_dir) {
            Ok(rd) => rd
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir() && p.join("miscinfo.lst").is_file())
                .collect(),
            Err(e) => {
                out.diags
                    .push(Diagnostic::error(Error::io(gamemodes_dir, e).to_string()));
                return out;
            }
        };
        dirs.sort();
        for d in dirs {
            match GameMode::load(&d, default.as_deref()) {
                Ok(gm) => out.modes.push(gm),
                Err(e) => out.diags.push(Diagnostic::error(e.to_string())),
            }
        }
        out
    }

    /// Find the mode a PCC GAMEMODE value refers to.
    pub fn find(&self, name: &str) -> Option<&GameMode> {
        self.modes
            .iter()
            .find(|m| m.key.eq_ignore_ascii_case(name))
            .or_else(|| self.modes.iter().find(|m| m.matches(name)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version("6.07.05"), vec![6, 7, 5]);
        assert_eq!(parse_version("5.17.9"), vec![5, 17, 9]);
        assert_eq!(parse_version("6.2"), vec![6, 2]);
        assert_eq!(parse_version("6.09.00-RC1"), vec![6, 9, 0]);
        assert!(cmp_version(&[6, 2], &[6, 2, 0]).is_eq());
        assert!(cmp_version(&[5, 17, 9], &[6, 2, 0]).is_lt());
    }

    #[test]
    fn migration_rules() {
        let r = GmLine::parse(1, "SOURCE:Old\tNEWKEY:New\tMAXVER:6.2.0 MAXDEVVER:6.1.8").unwrap();
        assert!(rule_applies(&r, &[5, 17, 9]));
        assert!(rule_applies(&r, &[6, 2, 0]));
        assert!(!rule_applies(&r, &[6, 3, 0]));
        // dev series 6.1.x up to 6.1.8 is also covered (already below max)
        let r = GmLine::parse(1, "SOURCE:Old\tMAXVER:6.0.0\tMAXDEVVER:6.1.8").unwrap();
        assert!(rule_applies(&r, &[6, 1, 5]));
        assert!(!rule_applies(&r, &[6, 1, 9]));
        assert!(!rule_applies(&r, &[6, 2, 0]));
        let r = GmLine::parse(1, "SOURCE:Old\tMAXVER:7.0.0\tMINVER:6.0.0").unwrap();
        assert!(!rule_applies(&r, &[5, 0, 0]));
        assert!(rule_applies(&r, &[6, 5, 0]));
        let r = GmLine::parse(1, "SOURCE:Old\tNEWKEY:New").unwrap();
        assert!(rule_applies(&r, &[9, 9, 9]));
    }

    #[test]
    fn line_parse() {
        let l = GmLine::parse(3, "TAB:CLASS\t\tNAME:in_class\tCONTEXT:a\\b.html").unwrap();
        assert_eq!(l.line, 3);
        assert_eq!(l.tag, "TAB");
        assert_eq!(l.value, "CLASS");
        assert_eq!(l.get("NAME"), Some("in_class"));
        assert_eq!(l.get("CONTEXT"), Some("a\\b.html"));
        let l = GmLine::parse(1, "LEVELMSG:Well done: you leveled").unwrap();
        assert_eq!(l.value, "Well done: you leveled");
        let l = GmLine::parse(1, "FLAG").unwrap();
        assert_eq!((l.tag.as_str(), l.value.as_str()), ("FLAG", ""));
        assert!(GmLine::parse(1, "\t\t").is_none());
    }
}
