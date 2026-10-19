//
// repl.rs -- character editing session and command language
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! A [`Session`] holds a character and the data it is built from, and
//! executes editing commands given as text lines.  The interactive
//! front end (line editing, history) lives in the command line tool;
//! everything here is testable without a terminal.

use crate::context::Context;
use crate::dataset::Object;
use crate::error::{Error, Result};
use crate::lst::kind::FileKind;
use crate::pcc::resolve::DataDirs;
use crate::pcg::character::Character;
use crate::query::Query;
use crate::rules::calc::{Calc, Sheet};
use std::path::{Path, PathBuf};

const MAX_UNDO: usize = 100;

/// Text fields that `set` may change, by command name.
const FIELDS: &[(&str, &str)] = &[
    ("name", "CHARACTERNAME"),
    ("player", "PLAYERNAME"),
    ("age", "AGE"),
    ("gender", "GENDER"),
    ("height", "HEIGHT"),
    ("weight", "WEIGHT"),
    ("eyes", "EYECOLOR"),
    ("hair", "HAIRCOLOR"),
    ("skin", "SKINCOLOR"),
    ("handed", "HANDED"),
    ("city", "CITY"),
    ("birthplace", "BIRTHPLACE"),
    ("location", "LOCATION"),
    ("catchphrase", "CATCHPHRASE"),
    ("bio", "CHARACTERBIO"),
    ("description", "CHARACTERDESC"),
];

const HELP: &str = "\
Commands (names may be quoted; most accept --force to skip checks):
  info                         summary of the character
  stats [-v]                   derived statistics
  explain CATEGORY.TARGET      where a bonus total comes from (e.g. SKILL.Ride)
  ls KIND [GLOB] [COND...]     search the data (e.g. ls feat \"Power*\" TYPE=Combat)
  show KIND KEY                show a data object
  prereq KIND KEY              check whether the character qualifies
  set stat ABB SCORE           set a base ability score
  set xp N | set align ABB     experience, alignment
  set FIELD VALUE              name, player, age, gender, city, bio, ...
  race KEY                     set race
  deity KEY | deity none       set or clear deity
  domain add KEY               add a domain
  class add KEY [HP]           gain a level (HP defaults to the hit die)
  class rm                     remove the last level gained
  ability add [CATEGORY|]KEY [CHOICE...]   add an ability (feat by default)
  ability rm [CATEGORY|]KEY    remove an ability
  skill KEY RANKS [CLASS]      set ranks bought in a skill
  lang add KEY                 add a language
  template add KEY             apply a template
  equip add NAME [QTY]         acquire equipment
  equip rm NAME                discard equipment
  equip wear NAME [LOCATION]   equip an item (location defaults by type)
  equip unwear NAME            stop using an item
  undo                         undo the last change
  save [PATH]                  write the character
  help                         this text
  quit                         leave";

/// Split a command line into words, honoring double quotes.
pub fn split_words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => {
                cur.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(cur);
    }
    out
}

/// What a command did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// text to show
    Text(String),
    /// the character changed; text to show
    Changed(String),
    /// leave the session
    Quit,
}

/// An editing session.
pub struct Session {
    pub ch: Character,
    pub ctx: Context,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    undo: Vec<Character>,
}

impl Session {
    /// Open an existing character file.
    pub fn open(path: &Path, dirs: DataDirs) -> Result<Session> {
        let ch = Character::read(path)?;
        let ctx = Context::for_character(&ch, dirs);
        Ok(Session {
            ch,
            ctx,
            path: Some(path.to_path_buf()),
            dirty: false,
            undo: Vec::new(),
        })
    }

    /// Start a new character.
    pub fn create(gamemode: &str, campaigns: &[String], name: &str, dirs: DataDirs) -> Session {
        let mut ch = Character::new(gamemode, campaigns, name);
        let ctx = Context::for_character(&ch, dirs);
        // start with every stat at the usual average
        for st in ctx.dataset.objects(FileKind::Stat) {
            ch.set_stat(&st.key, 10);
        }
        Session {
            ch,
            ctx,
            path: None,
            dirty: true,
            undo: Vec::new(),
        }
    }

    fn calc(&self) -> Calc<'_> {
        Calc::new(&self.ctx.dataset, self.ctx.gamemode.as_ref(), &self.ch)
    }

    pub fn sheet(&self) -> Sheet {
        self.calc().run()
    }

    /// Remember the current state before a change.
    fn checkpoint(&mut self) {
        self.dirty = true;
        self.undo.push(self.ch.clone());
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
    }

    /// One-line summary of key derived values.
    pub fn summary(&self) -> String {
        let s = self.sheet();
        let saves: Vec<String> = s
            .saves
            .iter()
            .map(|(k, v)| format!("{} {:+}", k, v))
            .collect();
        format!(
            "level {}  BAB {:+}  HP {}  AC {}  {}",
            s.level,
            s.bab,
            s.hp,
            s.ac.get("Total").copied().unwrap_or(0),
            saves.join(" ")
        )
    }

    /// Look up a data object by key, case-insensitively, with a
    /// helpful error.
    /// Find an object, optionally within an ability category.
    fn find_in(&self, kind: FileKind, category: Option<&str>, key: &str) -> Result<&Object> {
        if let Some(c) = category {
            if let Some(o) = self.ctx.dataset.get_in(kind, c, key) {
                return Ok(o);
            }
            let lk = key.to_lowercase();
            if let Some(o) = self.ctx.dataset.objects(kind).iter().find(|o| {
                o.name.to_lowercase() == lk
                    && o.category
                        .as_deref()
                        .is_some_and(|x| x.eq_ignore_ascii_case(c))
            }) {
                return Ok(o);
            }
        }
        self.find(kind, key)
    }

    fn find(&self, kind: FileKind, key: &str) -> Result<&Object> {
        if let Some(o) = self.ctx.dataset.get(kind, key) {
            return Ok(o);
        }
        // accept a display name too
        let lk = key.to_lowercase();
        if let Some(o) = self
            .ctx
            .dataset
            .objects(kind)
            .iter()
            .find(|o| o.name.to_lowercase() == lk)
        {
            return Ok(o);
        }
        let near: Vec<&str> = self
            .ctx
            .dataset
            .objects(kind)
            .iter()
            .filter(|o| o.key.to_lowercase().contains(&lk))
            .take(5)
            .map(|o| o.key.as_str())
            .collect();
        let hint = if near.is_empty() {
            String::new()
        } else {
            format!(" (did you mean: {}?)", near.join(", "))
        };
        Err(Error::Other(format!("no {} named '{}'{}", kind, key, hint)))
    }

    /// Prerequisites of `obj` the character fails.
    fn failing(&self, obj: &Object) -> Vec<String> {
        let mut c = self.calc();
        c.run();
        // the object borrowed from our own dataset outlives the calc
        let obj: &Object = self.ctx.dataset.get(obj.kind, &obj.key).unwrap_or(obj);
        c.failing_prereqs(obj)
    }

    fn check_prereqs(&self, obj: &Object, force: bool) -> Result<String> {
        let fails = self.failing(obj);
        if fails.is_empty() {
            return Ok(String::new());
        }
        let msg = format!("does not qualify: {}", fails.join(", "));
        if force {
            Ok(format!("warning: {}\n", msg))
        } else {
            Err(Error::Other(format!("{} (use --force to override)", msg)))
        }
    }

    /// Execute one command line.
    pub fn exec(&mut self, line: &str) -> Result<Outcome> {
        let mut words = split_words(line);
        let force = words.iter().any(|w| w == "--force");
        words.retain(|w| w != "--force");
        let w: Vec<&str> = words.iter().map(String::as_str).collect();
        let arg = |i: usize| -> Result<&str> {
            w.get(i)
                .copied()
                .ok_or_else(|| Error::Other("missing argument (try 'help')".into()))
        };
        let changed = |s: &Session, msg: String| -> Result<Outcome> {
            Ok(Outcome::Changed(format!("{}\n{}", msg, s.summary())))
        };

        match w.first().copied().unwrap_or("") {
            "" => Ok(Outcome::Text(String::new())),
            "help" | "?" => Ok(Outcome::Text(HELP.to_string())),
            "quit" | "exit" => Ok(Outcome::Quit),
            "info" => Ok(Outcome::Text(self.info())),
            "stats" | "sheet" => Ok(Outcome::Text(self.stats(w.get(1) == Some(&"-v")))),
            "explain" => {
                let key = arg(1)?;
                let (cat, tgt) = key.split_once('.').unwrap_or((key, ""));
                let mut c = self.calc();
                c.run();
                let lines = c.explain(cat, tgt);
                Ok(Outcome::Text(if lines.is_empty() {
                    format!("no bonuses to {}", key)
                } else {
                    lines.join("\n")
                }))
            }
            "ls" => {
                let (kind, cat) = parse_kind(arg(1)?)?;
                // an optional name glob comes before the conditions
                let (glob, conds) = match w.get(2) {
                    Some(g) if !is_condition(g) => (Some(*g), &w[3..]),
                    _ => (None, &w[2.min(w.len())..]),
                };
                let conds: Vec<String> = conds.iter().map(|s| s.to_string()).collect();
                let q = Query::new(glob, &conds).map_err(Error::Other)?;
                let hits: Vec<String> = self
                    .ctx
                    .dataset
                    .objects(kind)
                    .iter()
                    .filter(|o| {
                        cat.is_none_or(|c| {
                            o.category
                                .as_deref()
                                .is_some_and(|x| x.eq_ignore_ascii_case(c))
                        })
                    })
                    .filter(|o| q.matches(o))
                    .map(|o| match &o.category {
                        Some(c) => format!("{} ({})", o.key, c),
                        None => o.key.clone(),
                    })
                    .collect();
                let n = hits.len();
                let mut text: Vec<String> = hits.into_iter().take(50).collect();
                if n > 50 {
                    text.push(format!("... {} more", n - 50));
                }
                Ok(Outcome::Text(text.join("\n")))
            }
            "show" => {
                let (kind, cat) = parse_kind(arg(1)?)?;
                let o = self.find_in(kind, cat, arg(2)?)?;
                let mut out = format!("{} [{}]", o.name, o.key);
                for t in &o.tokens {
                    out.push_str(&format!("\n  {}", t.text()));
                }
                Ok(Outcome::Text(out))
            }
            "prereq" => {
                let (kind, cat) = parse_kind(arg(1)?)?;
                let o = self.find_in(kind, cat, arg(2)?)?;
                let fails = self.failing(o);
                Ok(Outcome::Text(if fails.is_empty() {
                    format!("qualifies for {}", o.key)
                } else {
                    format!("does not qualify for {}: {}", o.key, fails.join(", "))
                }))
            }
            "undo" => match self.undo.pop() {
                Some(prev) => {
                    self.ch = prev;
                    self.dirty = true;
                    changed(self, "undone".into())
                }
                None => Ok(Outcome::Text("nothing to undo".into())),
            },
            "save" => {
                let path = match w.get(1) {
                    Some(p) => PathBuf::from(p),
                    None => self
                        .path
                        .clone()
                        .ok_or_else(|| Error::Other("no file name; use 'save PATH'".into()))?,
                };
                self.ch.write(&path)?;
                self.path = Some(path.clone());
                self.dirty = false;
                Ok(Outcome::Text(format!("saved {}", path.display())))
            }
            "set" => self.cmd_set(&w, changed),
            "race" => {
                let o = self.find(FileKind::Race, arg(1)?)?;
                let key = o.key.clone();
                let warn = self.check_prereqs(o, force)?;
                self.checkpoint();
                self.ch.set_race(&key);
                changed(self, format!("{}race is now {}", warn, key))
            }
            "deity" => {
                let d = arg(1)?;
                self.checkpoint();
                if d.eq_ignore_ascii_case("none") {
                    self.ch.set_deity(None);
                    return changed(self, "deity cleared".into());
                }
                let o = self.find(FileKind::Deity, d)?;
                let key = o.key.clone();
                self.ch.set_deity(Some(&key));
                changed(self, format!("deity is now {}", key))
            }
            "domain" if w.get(1) == Some(&"add") => {
                let o = self.find(FileKind::Domain, arg(2)?)?;
                let key = o.key.clone();
                self.checkpoint();
                self.ch.add_domain(&key);
                changed(self, format!("added domain {}", key))
            }
            "class" => match arg(1)? {
                "add" => {
                    let o = self.find(FileKind::Class, arg(2)?)?;
                    let key = o.key.clone();
                    let hd: i64 = o.get("HD").and_then(|v| v.parse().ok()).unwrap_or(8);
                    let hp: i64 = match w.get(3) {
                        Some(v) => v
                            .parse()
                            .map_err(|_| Error::Other(format!("bad hit points '{}'", v)))?,
                        None => hd,
                    };
                    let skills: f64 = o
                        .get("STARTSKILLPTS")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0.0);
                    let warn = if self
                        .ch
                        .classes()
                        .iter()
                        .any(|c| c.name.eq_ignore_ascii_case(&key))
                    {
                        String::new()
                    } else {
                        self.check_prereqs(o, force)?
                    };
                    self.checkpoint();
                    let lvl = self.ch.add_level(&key, hp, skills);
                    changed(self, format!("{}{} is now level {}", warn, key, lvl))
                }
                "rm" | "remove" => {
                    self.checkpoint();
                    match self.ch.remove_last_level() {
                        Some(c) => changed(self, format!("removed a level of {}", c)),
                        None => {
                            self.undo.pop();
                            Ok(Outcome::Text("no levels to remove".into()))
                        }
                    }
                }
                other => Err(Error::Other(format!("unknown class command '{}'", other))),
            },
            "ability" | "feat" => {
                let (sub, rest) = (arg(1)?, 2);
                let spec = arg(rest)?;
                let (cat, key) = match spec.split_once('|') {
                    Some((c, k)) => (c.to_string(), k.to_string()),
                    None => ("FEAT".to_string(), spec.to_string()),
                };
                match sub {
                    "add" => {
                        let o = self.find_in(FileKind::Ability, Some(&cat), &key)?;
                        let (k, c) = (o.key.clone(), o.category.clone().unwrap_or(cat.clone()));
                        let multiple = o.get("MULT").is_some_and(|v| v.eq_ignore_ascii_case("YES"));
                        let has = self.ch.abilities().iter().any(|a| {
                            a.key.eq_ignore_ascii_case(&k) && a.category.eq_ignore_ascii_case(&c)
                        });
                        if has && !multiple && !force {
                            return Err(Error::Other(format!("the character already has {}", k)));
                        }
                        let warn = self.check_prereqs(o, force)?;
                        let choices: Vec<String> =
                            w[rest + 1..].iter().map(|s| s.to_string()).collect();
                        self.checkpoint();
                        self.ch.add_ability(&cat, &c, &k, &choices);
                        changed(self, format!("{}added {} ({})", warn, k, c))
                    }
                    "rm" | "remove" => {
                        self.checkpoint();
                        let catopt = spec.contains('|').then_some(cat.as_str());
                        let n = self.ch.remove_ability(catopt, &key);
                        if n == 0 {
                            self.undo.pop();
                            return Err(Error::Other(format!(
                                "the character has no ability '{}'",
                                key
                            )));
                        }
                        changed(self, format!("removed {}", key))
                    }
                    other => Err(Error::Other(format!("unknown ability command '{}'", other))),
                }
            }
            "skill" => {
                let o = self.find(FileKind::Skill, arg(1)?)?;
                let key = o.key.clone();
                let ranks: f64 = arg(2)?
                    .parse()
                    .map_err(|_| Error::Other("ranks must be a number".into()))?;
                let class = match w.get(3) {
                    Some(c) => c.to_string(),
                    None => self
                        .ch
                        .levels()
                        .last()
                        .map(|l| l.class.clone())
                        .ok_or_else(|| Error::Other("the character has no class".into()))?,
                };
                let calc = {
                    let mut c = self.calc();
                    c.run();
                    (c.is_class_skill(&key), c.max_ranks(&key))
                };
                let (class_skill, max) = calc;
                if let Some(m) = max {
                    if ranks > m && !force {
                        return Err(Error::Other(format!(
                            "{} ranks exceeds the maximum of {} (use --force)",
                            ranks, m
                        )));
                    }
                }
                self.checkpoint();
                self.ch.set_skill_ranks(&key, &class, ranks, class_skill);
                changed(self, format!("{} ranks in {}", ranks, key))
            }
            "lang" | "language" if w.get(1) == Some(&"add") => {
                let o = self.find(FileKind::Language, arg(2)?)?;
                let key = o.key.clone();
                self.checkpoint();
                self.ch.add_language(&key);
                changed(self, format!("added language {}", key))
            }
            "template" if w.get(1) == Some(&"add") => {
                let o = self.find(FileKind::Template, arg(2)?)?;
                let key = o.key.clone();
                let warn = self.check_prereqs(o, force)?;
                self.checkpoint();
                self.ch.add_template(&key);
                changed(self, format!("{}applied template {}", warn, key))
            }
            "equip" => self.cmd_equip(&w, force, changed),
            other => Err(Error::Other(format!(
                "unknown command '{}' (try 'help')",
                other
            ))),
        }
    }

    fn cmd_set(
        &mut self,
        w: &[&str],
        changed: impl Fn(&Session, String) -> Result<Outcome>,
    ) -> Result<Outcome> {
        let what = w.get(1).copied().unwrap_or("");
        let need = |i: usize| {
            w.get(i)
                .copied()
                .ok_or_else(|| Error::Other("missing value".into()))
        };
        match what {
            "stat" => {
                let abb = need(2)?;
                let st = self.find(FileKind::Stat, abb)?;
                let key = st.key.clone();
                let score: i64 = need(3)?
                    .parse()
                    .map_err(|_| Error::Other("score must be a number".into()))?;
                self.checkpoint();
                self.ch.set_stat(&key, score);
                changed(self, format!("{} is now {}", key, score))
            }
            "xp" => {
                let xp: i64 = need(2)?
                    .parse()
                    .map_err(|_| Error::Other("experience must be a number".into()))?;
                self.checkpoint();
                self.ch.set_experience(xp);
                changed(self, format!("experience is now {}", xp))
            }
            "align" | "alignment" => {
                let a = self.find(FileKind::Alignment, need(2)?)?;
                let key = a.key.clone();
                self.checkpoint();
                self.ch.set_alignment(&key);
                changed(self, format!("alignment is now {}", key))
            }
            field => {
                let Some((_, tag)) = FIELDS.iter().find(|(n, _)| *n == field) else {
                    return Err(Error::Other(format!("cannot set '{}' (try 'help')", field)));
                };
                let value = w[2..].join(" ");
                self.checkpoint();
                self.ch.set_field(tag, &value);
                changed(self, format!("{} is now '{}'", field, value))
            }
        }
    }

    fn cmd_equip(
        &mut self,
        w: &[&str],
        force: bool,
        changed: impl Fn(&Session, String) -> Result<Outcome>,
    ) -> Result<Outcome> {
        let sub = w.get(1).copied().unwrap_or("");
        let name = w
            .get(2)
            .copied()
            .ok_or_else(|| Error::Other("missing item name".into()))?;
        match sub {
            "add" => {
                let o = self.find(FileKind::Equipment, name)?;
                let key = o.key.clone();
                let qty: f64 = w.get(3).and_then(|q| q.parse().ok()).unwrap_or(1.0);
                self.checkpoint();
                self.ch.add_equipment(&key, qty);
                changed(self, format!("acquired {} x{}", key, qty))
            }
            "rm" | "remove" => {
                self.checkpoint();
                if !self.ch.remove_equipment(name) {
                    self.undo.pop();
                    return Err(Error::Other(format!("the character has no '{}'", name)));
                }
                changed(self, format!("discarded {}", name))
            }
            "wear" => {
                let Some(owned) = self
                    .ch
                    .equipment()
                    .into_iter()
                    .find(|e| e.name.eq_ignore_ascii_case(name))
                else {
                    return Err(Error::Other(format!(
                        "the character has no '{}' (use 'equip add' first)",
                        name
                    )));
                };
                let obj = self.ctx.dataset.get(
                    FileKind::Equipment,
                    owned.base.as_deref().unwrap_or(&owned.name),
                );
                let loc = match w.get(3) {
                    Some(l) => l.to_string(),
                    None => default_location(obj),
                };
                let warn = match obj {
                    Some(o) => self.check_prereqs(o, force)?,
                    None => String::new(),
                };
                self.checkpoint();
                self.ch.equip(&owned.name, &loc, 1.0);
                changed(self, format!("{}equipped {} ({})", warn, owned.name, loc))
            }
            "unwear" => {
                self.checkpoint();
                if !self.ch.unequip(name) {
                    self.undo.pop();
                    return Err(Error::Other(format!("'{}' is not equipped", name)));
                }
                changed(self, format!("unequipped {}", name))
            }
            other => Err(Error::Other(format!("unknown equip command '{}'", other))),
        }
    }

    fn info(&self) -> String {
        let c = &self.ch;
        let classes: Vec<String> = c
            .classes()
            .iter()
            .map(|x| format!("{} {}", x.name, x.level))
            .collect();
        let stats: Vec<String> = c
            .stats()
            .iter()
            .map(|(a, s)| format!("{} {}", a, s))
            .collect();
        let abilities: Vec<String> = c
            .abilities()
            .iter()
            .filter(|a| a.nature == "NORMAL")
            .map(|a| a.key.clone())
            .collect();
        let items: Vec<String> = c.equipment().iter().map(|e| e.name.clone()).collect();
        format!(
            "{}\n  game mode:  {}\n  campaigns:  {}\n  race:       {}\n  alignment:  {}\n  classes:    {}\n  stats:      {}\n  abilities:  {}\n  equipment:  {}\n  {}",
            c.name().unwrap_or_default(),
            c.gamemode().unwrap_or_default(),
            c.campaigns().join("; "),
            c.race().unwrap_or_default(),
            c.alignment().unwrap_or_default(),
            classes.join(", "),
            stats.join(", "),
            abilities.join(", "),
            items.join(", "),
            self.summary()
        )
    }

    fn stats(&self, verbose: bool) -> String {
        let mut c = self.calc();
        let s = c.run();
        let mut out = Vec::new();
        let stats: Vec<String> = s
            .stats
            .iter()
            .map(|(k, (v, m))| format!("{} {} ({:+})", k, v, m))
            .collect();
        out.push(format!("stats: {}", stats.join(", ")));
        out.push(format!("level {}  BAB {:+}  HP {}", s.level, s.bab, s.hp));
        let saves: Vec<String> = s
            .saves
            .iter()
            .map(|(k, v)| format!("{} {:+}", k, v))
            .collect();
        out.push(format!("saves: {}", saves.join(", ")));
        let ac: Vec<String> = s.ac.iter().map(|(k, v)| format!("{} {}", k, v)).collect();
        out.push(format!("AC: {}", ac.join(", ")));
        let skills: Vec<String> = s
            .skills
            .iter()
            .filter(|(k, _)| verbose || c.skill_ranks(k) > 0.0)
            .map(|(k, v)| format!("{} {:+}", k, v))
            .collect();
        out.push(format!("skills: {}", skills.join(", ")));
        if verbose {
            out.push(format!("abilities: {}", s.abilities.join(", ")));
            out.push(format!("equipped: {}", s.equipped.join("; ")));
        }
        out.join("\n")
    }
}

/// Default equipment-set location for an item, by its type.
fn default_location(obj: Option<&Object>) -> String {
    let Some(o) = obj else {
        return "Carried".into();
    };
    for (t, loc) in [
        ("Armor", "Armor"),
        ("Shield", "Shield"),
        ("Weapon", "Primary Hand"),
        ("Ring", "Ring"),
        ("Amulet", "Neck"),
        ("Belt", "Belt"),
        ("Cloak", "Shoulders"),
        ("Boots", "Feet"),
        ("Headband", "Headband"),
    ] {
        if o.is_type(t) {
            return loc.to_string();
        }
    }
    "Equipped".into()
}

/// Does a word look like a query condition (TAG, !TAG, TAG=v, TAG~re)
/// rather than a name?
fn is_condition(s: &str) -> bool {
    s.contains('=')
        || s.contains('~')
        || s.starts_with('!')
        || (!s.is_empty() && s.chars().all(|c| c.is_ascii_uppercase() || c == '_'))
}

/// Parse a kind name.  "feat" means abilities in the FEAT category.
fn parse_kind(s: &str) -> Result<(FileKind, Option<&'static str>)> {
    if s.eq_ignore_ascii_case("feat") || s.eq_ignore_ascii_case("feats") {
        return Ok((FileKind::Ability, Some("FEAT")));
    }
    FileKind::from_name(s)
        .map(|k| (k, None))
        .ok_or_else(|| Error::Other(format!("unknown object kind '{}'", s)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words() {
        assert_eq!(split_words("a b  c"), vec!["a", "b", "c"]);
        assert_eq!(
            split_words("ability add \"Power Attack\" x"),
            vec!["ability", "add", "Power Attack", "x"]
        );
        assert_eq!(split_words("set city \"\""), vec!["set", "city", ""]);
        assert_eq!(split_words("  "), Vec::<String>::new());
        assert_eq!(split_words("say \"a \"b"), vec!["say", "a b"]);
    }

    #[test]
    fn conditions() {
        for c in ["TYPE=Combat", "DESC", "!PRE", "NAME~x"] {
            assert!(is_condition(c), "{}", c);
        }
        for g in ["Power*", "dodge", "Dodge"] {
            assert!(!is_condition(g), "{}", g);
        }
    }

    #[test]
    fn locations() {
        assert_eq!(default_location(None), "Carried");
    }
}
