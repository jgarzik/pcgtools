//
// repl.rs -- character editing session and command language
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
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
use crate::gamemode::GameMode;
use crate::lst::kind::FileKind;
use crate::pcc::resolve::DataDirs;
use crate::pcg::character::{money_text, Character, SpellCopy, DEFAULT_SET_ID, DEFAULT_SET_NAME};
use crate::query::Query;
use crate::rules::calc::{Calc, Sheet};
use crate::rules::equip::EqItem;
use crate::rules::spells;
use crate::rules::statgen::{self, PointBuy, Rng};
use crate::token::pre::Prereq;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};

const MAX_UNDO: usize = 100;

/// PCGen's default rates, in percent of an item's price: buying,
/// selling, and selling coins, gems and trade goods.
const BUY_RATE: f64 = 100.0;
const SELL_RATE: f64 = 50.0;
const CASH_SELL_RATE: f64 = 100.0;

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
  vars [SCOPE]                 declared variables and their values
  ls KIND [GLOB] [COND...]     search the data (e.g. ls feat \"Power*\" TYPE=Combat)
  show KIND KEY                show a data object
  prereq KIND KEY              check whether the character qualifies
  set stat ABB SCORE           set a base ability score
  stats buy [METHOD]           buy scores with a point-buy method
  stats roll [METHOD] [--seed N]   roll scores with a dice method
  stats same SCORE | stats assign SCORE...   one score for all, or each
                               in stat order
  stats method                 how the scores were made; point costs
  set xp N | set align ABB     experience, alignment
  set gold N|+N|-N             set or change the character's funds
  set debt on|off              allow buying into debt
  set FIELD VALUE              name, player, age, gender, city, bio, ...
  race KEY                     set race
  deity KEY | deity none       set or clear deity
  domain add KEY               add a domain (up to the number allowed)
  class add KEY [HP] [--subclass SUB] [--choose CHOICE]... [--stat ABB]...
                               gain a level (HP: the highest roll allowed
                               by the hit die and HD bonuses, by default);
                               choices for the level's ADD tokens, in order;
                               the stats its stat increases raise
  class rm                     remove the last level gained
  class subclass KEY SUB       choose a class's subclass
  ability add [CATEGORY|]KEY [CHOICE...]   add an ability (feat by default)
  ability rm [CATEGORY|]KEY    remove an ability
  skill KEY RANKS [CLASS]      set ranks bought in a skill
  lang add KEY                 add a bonus language (from those offered,
                               up to the number allowed)
  template add KEY             apply a template
  equip add NAME [QTY]         acquire equipment
  equip rm NAME                discard equipment
  equip buy NAME [QTY]         acquire equipment, paying for it
  equip sell NAME [QTY|all]    give up equipment for half its price
  equip wear NAME [LOCATION] [--set SET]
                               equip an item (location defaults by type) in
                               a set (by default the one used), if the
                               body slot has room
  equip unwear NAME [--set SET]   stop using an item
  equip customize NAME +MOD[|CHOICE] | -MOD | size SIZE
                               add or remove an equipment modifier, or resize
  eqset ls | eqset show [SET]  equipment sets (* the one used); a set's items
  eqset use SET                use a set (by name or id) for calculations
  eqset add NAME | eqset copy SET NAME   add an empty or copied set, and use it
  eqset rename SET NAME | eqset rm SET   rename or remove a set
  kit apply KEY                apply a starting kit
  spell add CLASS SPELL [LEVEL] [BOOK]   add a spell (book: Known Spells)
  spell rm CLASS SPELL [BOOK]  remove a spell (one copy)
  spell prep CLASS SPELL [LIST] [--meta FEAT[,FEAT]] [--level N] [--domain D]
                               prepare a known spell (list: Prepared Spells),
                               with metamagic feats; needs a free slot
  spell unprep CLASS SPELL [LIST] [--meta FEAT[,FEAT]]
                               remove a prepared copy
  spell scribe CLASS SPELL BOOK [--level N] | spell unscribe CLASS SPELL BOOK
                               write a known spell in a spellbook the
                               character has, if it has the pages; or
                               remove a copy
  spellbook add NAME | spellbook rm NAME | spellbook ls
                               spell lists and books
  temp ls [GLOB]               temporary bonuses offered, and those applied
  temp add [KIND=]SOURCE [pc|ITEM] [--value N]
                               apply one to the character or to an item
                               in the equipment set used (KIND: feat,
                               spell, template, skill or equip)
  temp rm SOURCE [pc|ITEM] | temp on|off SOURCE [pc|ITEM]
                               remove one, or switch it on or off
  temp mods on|off             use temporary modifiers, or not
  follower add NAME TYPE RACE [FILE]     add a follower or companion (as
                               many of a TYPE as the rules allow)
  follower rm NAME | follower ls
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
        // the file names the mode by its key, whatever it was called here
        if let Some(gm) = &ctx.gamemode {
            ch.set_field("GAMEMODE", &gm.key);
        }
        // a file without an age gets a random one each time PCGen loads it
        ch.set_field("AGE", "0");
        // start with every stat at the usual average, as PCGen's
        // all-the-same method does
        for st in ctx.dataset.objects(FileKind::Stat) {
            ch.set_stat(&st.key, 10);
        }
        ch.set_roll_method(1, "10");
        Session {
            ch,
            ctx,
            path: None,
            dirty: true,
            undo: Vec::new(),
        }
    }

    /// The saved name of an item the character has, given as saved or
    /// by anything `find` accepts for the data item (a display name).
    fn owned_item(&self, name: &str) -> String {
        let owned = self.ch.equipment();
        if owned.iter().any(|e| e.name.eq_ignore_ascii_case(name)) {
            return name.to_string();
        }
        match self.find(FileKind::Equipment, name) {
            Ok(o) if owned.iter().any(|e| e.name == o.key) => o.key.clone(),
            _ => name.to_string(),
        }
    }

    /// The character's class (and the level of it) that grants domains:
    /// the first with a BONUS:DOMAIN|NUMBER on its class line or on a
    /// level line the character has reached.
    fn domain_grant(&self) -> Option<(String, u32)> {
        let grants = |t: &crate::dataset::Tok| {
            t.tag == "BONUS" && t.value.to_ascii_uppercase().starts_with("DOMAIN|NUMBER")
        };
        for x in self.ch.classes() {
            let Some(o) = self.ctx.dataset.get(FileKind::Class, &x.name) else {
                continue;
            };
            if o.tokens.iter().any(grants) {
                return Some((o.key.clone(), 1));
            }
            for sl in o.sublines.iter().filter(|sl| sl.tag == "LEVEL") {
                let n: u32 = sl
                    .value
                    .split(':')
                    .next()
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0);
                if n >= 1 && n <= x.level && sl.tokens.iter().any(grants) {
                    return Some((o.key.clone(), n));
                }
            }
        }
        None
    }

    /// May the character choose language `key`?  One it does not have
    /// already, or by AUTO:LANG, must be among its bonus languages
    /// (LANGBONUS), and fewer than the number allowed taken, counting
    /// those not chosen through an ADD:LANGUAGE.  PCGen drops a
    /// language it cannot account for when it loads a character.  As
    /// in PCGen, bonus languages are chosen at first level only, unless
    /// the game mode's INTBONUSLANG rule allows them later.
    fn check_bonus_language(&self, key: &str) -> Result<()> {
        let mut c = self.calc();
        c.run();
        let has = |list: &[String], k: &str| list.iter().any(|x| x.eq_ignore_ascii_case(k));
        let known = self.ch.languages();
        let auto = c.auto_languages();
        if has(&known, key) || has(&auto, key) {
            return Ok(());
        }
        let offered = c.bonus_language_choices();
        if !has(&offered, key) {
            return Err(Error::Other(format!(
                "{} is not a bonus language for the character (use --force)",
                key
            )));
        }
        // languages chosen through ADD tokens
        let mut added: Vec<String> = Vec::new();
        for l in &self.ch.pcg.lines {
            for e in l.elems.iter().flatten().filter(|e| e.tag == "ADD") {
                let kids = e.children();
                if kids.iter().any(|k| k.tag == "LANGUAGE") {
                    added.extend(
                        kids.iter()
                            .filter(|k| k.tag == "CHOICE")
                            .map(|k| k.value_text()),
                    );
                }
            }
        }
        let taken = known
            .iter()
            .filter(|l| has(&offered, l) && !has(&auto, l) && !has(&added, l))
            .count() as i64;
        let allowed = if c.rule("INTBONUSLANG") || self.ch.total_level() <= 1 {
            c.bonus_language_count()
        } else {
            0
        };
        if taken >= allowed {
            return Err(Error::Other(format!(
                "the character may choose {} bonus language{} now (use --force)",
                allowed.max(0),
                if allowed == 1 { "" } else { "s" }
            )));
        }
        Ok(())
    }

    /// The range a hit point roll for a new level of `cls` may take:
    /// 1 to the hit die `hd`, moved by BONUS:HD|MIN and HD|MAX (and
    /// their `;CLASS.key` forms for this class), counting the bonuses
    /// the new level itself brings.  A class without a hit die rolls
    /// nothing.
    ///
    /// The top is also held to what PCGen keeps when it loads the
    /// character: it lowers a roll above the hit die plus the HD|MAX
    /// bonuses of the class's own line and of its levels below this
    /// one (so a bonus a class grants at level 1 first counts at 2).
    ///
    /// `work` is the character already holding the new level.
    fn hit_point_range(&self, work: &Character, cls: &Object, hd: i64) -> (i64, i64) {
        if hd <= 0 {
            return (0, 0);
        }
        let level = work
            .classes()
            .iter()
            .find(|x| x.name.eq_ignore_ascii_case(&cls.key))
            .map_or(1, |x| x.level);
        let mut c = Calc::new(&self.ctx.dataset, self.ctx.gamemode.as_ref(), work);
        c.run();
        let b = |t: &str| {
            let own = format!("{};CLASS.{}", t, cls.key);
            (c.bonus.total("HD", t) + c.bonus.total("HD", &own)) as i64
        };
        let mut kept = 0.0;
        let below = |sl: &crate::dataset::SubLine| {
            sl.tag == "LEVEL" && (1..level).any(|l| crate::rules::adds::level_line_at(&sl.value, l))
        };
        let lines = std::iter::once(&cls.tokens).chain(
            cls.sublines
                .iter()
                .filter(|sl| below(sl))
                .map(|sl| &sl.tokens),
        );
        for t in lines.flatten().filter(|t| t.tag == "BONUS") {
            let Ok(bonus) = crate::token::bonus::Bonus::parse(&t.value) else {
                continue;
            };
            if bonus.category.eq_ignore_ascii_case("HD")
                && bonus.targets.iter().any(|x| x.eq_ignore_ascii_case("MAX"))
                && bonus.prereqs().all(|p| c.passes(p, None))
            {
                kept += c.eval_formula(&bonus.formula);
            }
        }
        let lo = 1 + b("MIN");
        let hi = (hd + b("MAX")).min(hd + kept as i64);
        (lo, hi.max(lo))
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
            "stats"
                if matches!(
                    w.get(1).copied(),
                    Some("buy" | "roll" | "same" | "assign" | "method")
                ) =>
            {
                self.cmd_stats(&w, force, changed)
            }
            "stats" | "sheet" => Ok(Outcome::Text(self.stats(w.get(1) == Some(&"-v")))),
            "vars" => {
                let mut calc = self.calc();
                calc.run();
                let sys = crate::rules::vars::system::VarSystem::new(&calc);
                let scope = w.get(1).map(|s| s.to_ascii_uppercase());
                let mut out = Vec::new();
                for (inst, vars) in sys.results() {
                    if scope
                        .as_deref()
                        .is_some_and(|s| !inst.to_ascii_uppercase().starts_with(s))
                    {
                        continue;
                    }
                    out.push(inst);
                    for (k, v) in vars {
                        out.push(format!("  {} = {}", k, v));
                    }
                }
                Ok(Outcome::Text(if out.is_empty() {
                    "no declared variables".into()
                } else {
                    out.join("\n")
                }))
            }
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
                self.ch.path = Some(path.clone());
                self.path = Some(path.clone());
                self.dirty = false;
                Ok(Outcome::Text(format!("saved {}", path.display())))
            }
            "set" => self.cmd_set(&w, force, changed),
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
                let have = self.ch.domains();
                if have.iter().any(|d| d.eq_ignore_ascii_case(&key)) {
                    return Err(Error::Other(format!("the character already has {}", key)));
                }
                // no more than BONUS:DOMAIN|NUMBER allows
                let max = {
                    let mut c = self.calc();
                    c.run();
                    c.max_domains()
                };
                if have.len() as i64 >= max && !force {
                    return Err(Error::Other(format!(
                        "the character may have {} domain{} (use --force)",
                        max,
                        if max == 1 { "" } else { "s" }
                    )));
                }
                let grant = self.domain_grant();
                self.checkpoint();
                match grant {
                    Some((class, level)) => self.ch.add_class_domain(&key, &class, level),
                    None => self.ch.add_domain(&key),
                }
                changed(self, format!("added domain {}", key))
            }
            "class" => match arg(1)? {
                "add" => {
                    let o = self.find(FileKind::Class, arg(2)?)?;
                    let key = o.key.clone();
                    // stat increases the level grants, and those chosen
                    let mut raise = Vec::new();
                    for (i, _) in w.iter().enumerate().filter(|(_, x)| **x == "--stat") {
                        let abb = w
                            .get(i + 1)
                            .ok_or_else(|| Error::Other("--stat needs a stat".into()))?;
                        raise.push(self.find(FileKind::Stat, abb)?.key.clone());
                    }
                    let grants = self.stat_increases_for_new_level(o);
                    let pending = self.check_stat_increases(grants, &raise, force)?;
                    let o = self.ctx.dataset.get(FileKind::Class, &key).unwrap_or(o);
                    let (hd, skills, work) = self.new_level_worth(o, &raise);
                    let (lo, hi) = self.hit_point_range(&work, o, hd);
                    let hp: i64 = match w.get(3).filter(|v| !v.starts_with("--")) {
                        Some(v) => v
                            .parse()
                            .map_err(|_| Error::Other(format!("bad hit points '{}'", v)))?,
                        None => hi,
                    };
                    if hd > 0 && (hp < lo || hp > hi) && !force {
                        return Err(Error::Other(format!(
                            "hit points must be {} to {} (use --force)",
                            lo, hi
                        )));
                    }
                    let subclass = w
                        .iter()
                        .position(|x| *x == "--subclass")
                        .and_then(|i| w.get(i + 1).map(|s| s.to_string()));
                    if let Some(sub) = &subclass {
                        if !o.children.iter().any(|c| c.key.eq_ignore_ascii_case(sub)) {
                            return Err(Error::Other(format!("{} has no subclass '{}'", key, sub)));
                        }
                    }
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
                    let mut choices: VecDeque<String> = w
                        .iter()
                        .enumerate()
                        .filter(|(_, x)| **x == "--choose")
                        .filter_map(|(i, _)| w.get(i + 1).map(|s| s.to_string()))
                        .collect();
                    let was_dirty = self.dirty;
                    self.checkpoint();
                    let lvl = self.ch.add_level(&key, hp, skills);
                    raise_stats(&mut self.ch, &raise);
                    for abb in &raise {
                        self.ch.add_stat_change(abb, 1);
                    }
                    if pending > 0 {
                        let n = self.ch.pool_points() + pending;
                        self.ch.set_pool_points(n);
                    }
                    if let Some(sub) = &subclass {
                        self.ch.set_subclass(&key, sub);
                    }
                    let chosen = match self.level_choices(&key, lvl, &mut choices, force) {
                        Ok(_) if !choices.is_empty() => Err(Error::Other(format!(
                            "unused choices: {}",
                            Vec::from(choices).join(", ")
                        ))),
                        r => r,
                    };
                    let chosen = match chosen {
                        Ok(c) => c,
                        Err(e) => {
                            if let Some(prev) = self.undo.pop() {
                                self.ch = prev;
                            }
                            self.dirty = was_dirty;
                            return Err(e);
                        }
                    };
                    let chosen: String = chosen.iter().map(|c| format!("\nchose {}", c)).collect();
                    changed(
                        self,
                        format!("{}{} is now level {}{}", warn, key, lvl, chosen),
                    )
                }
                "subclass" => {
                    let o = self.find(FileKind::Class, arg(2)?)?;
                    let key = o.key.clone();
                    let sub = arg(3)?;
                    let Some(s) = o.children.iter().find(|c| {
                        c.key.eq_ignore_ascii_case(sub) || c.name.eq_ignore_ascii_case(sub)
                    }) else {
                        return Err(Error::Other(format!("{} has no subclass '{}'", key, sub)));
                    };
                    let sub = s.key.clone();
                    self.checkpoint();
                    if !self.ch.set_subclass(&key, &sub) {
                        self.undo.pop();
                        return Err(Error::Other(format!("the character has no {} levels", key)));
                    }
                    changed(self, format!("{} subclass is {}", key, sub))
                }
                "rm" | "remove" => {
                    let deltas = self.ch.last_level_stat_changes();
                    self.checkpoint();
                    match self.ch.remove_last_level() {
                        Some(c) => {
                            // the stat changes made with the level go
                            // with it (but not a monster level's)
                            let monster = self
                                .ctx
                                .dataset
                                .get(FileKind::Class, &c)
                                .is_some_and(|o| self.calc().is_monster_class(o));
                            if !monster {
                                for (abb, d) in deltas {
                                    let now = self.ch.stat(&abb).unwrap_or(0);
                                    self.ch.set_stat(&abb, now - d);
                                }
                            }
                            changed(self, format!("removed a level of {}", c))
                        }
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
                        self.check_choices(o, &choices, force)?;
                        self.check_pool(&cat, o, force)?;
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
                // what the change costs from the class's skill points
                let per_rank = {
                    let mut c = self.calc();
                    c.run();
                    self.rank_cost(c.is_class_skill_for(&key, &class))
                };
                let had: f64 = self
                    .ch
                    .skills()
                    .iter()
                    .find(|e| e.name.eq_ignore_ascii_case(&key))
                    .and_then(|e| {
                        e.ranks
                            .iter()
                            .find(|(c, _)| c.eq_ignore_ascii_case(&class))
                            .map(|(_, r)| *r)
                    })
                    .unwrap_or(0.0);
                let spend = (ranks - had) * per_rank;
                let left = self.ch.skill_points_left(&class);
                if spend > left && !force {
                    return Err(Error::Other(format!(
                        "{} skill points needed, {} left for {} (use --force)",
                        spend, left, class
                    )));
                }
                self.checkpoint();
                self.ch.set_skill_ranks(&key, &class, ranks, class_skill);
                self.ch.spend_skill_points(&class, spend);
                changed(self, format!("{} ranks in {}", ranks, key))
            }
            "lang" | "language" if w.get(1) == Some(&"add") => {
                let o = self.find(FileKind::Language, arg(2)?)?;
                let key = o.key.clone();
                if !force {
                    self.check_bonus_language(&key)?;
                }
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
            "equip" if w.get(1) == Some(&"customize") => self.cmd_customize(&w, changed),
            "equip" => self.cmd_equip(&w, force, changed),
            "eqset" => self.cmd_eqset(&w, changed),
            "kit" if w.get(1) == Some(&"apply") => {
                let o = self.find(FileKind::Kit, arg(2)?)?.clone();
                self.checkpoint();
                let msg = self.apply_kit(&o)?;
                changed(self, msg)
            }
            "spell" => match arg(1)? {
                "add" => {
                    let cls = self.find(FileKind::Class, arg(2)?)?.key.clone();
                    let sp = self.find(FileKind::Spell, arg(3)?)?;
                    let key = sp.key.clone();
                    let level: i64 = match w.get(4) {
                        Some(v) => v
                            .parse()
                            .map_err(|_| Error::Other(format!("bad spell level '{}'", v)))?,
                        None => {
                            let c = self.calc();
                            let sps = crate::rules::spells::Spells::new(&c);
                            let cls_obj = self.ctx.dataset.get(FileKind::Class, &cls).unwrap();
                            sps.levels_on('C', &cls_obj.key, sp)
                                .first()
                                .copied()
                                .ok_or_else(|| {
                                    Error::Other(format!(
                                        "{} is not on the {} list; give a level",
                                        key, cls
                                    ))
                                })?
                        }
                    };
                    let book = w
                        .get(5)
                        .copied()
                        .unwrap_or(crate::rules::spells::KNOWN_BOOK);
                    if !self
                        .ch
                        .classes()
                        .iter()
                        .any(|c| c.name.eq_ignore_ascii_case(&cls))
                    {
                        return Err(Error::Other(format!("the character has no {} levels", cls)));
                    }
                    // a class knows a spell once
                    let known = crate::rules::spells::KNOWN_BOOK;
                    if book.eq_ignore_ascii_case(known)
                        && self.ch.spells().iter().any(|(spell, class, b, _, _)| {
                            spell.eq_ignore_ascii_case(&key)
                                && class.eq_ignore_ascii_case(&cls)
                                && b.eq_ignore_ascii_case(known)
                        })
                    {
                        return Err(Error::Other(format!("{} already knows {}", cls, key)));
                    }
                    self.checkpoint();
                    self.ch.add_spell(&key, &cls, book, level, 1);
                    changed(self, format!("added {} (level {}) to {}", key, level, book))
                }
                "rm" | "remove" => {
                    let cls = self.find(FileKind::Class, arg(2)?)?.key.clone();
                    let spell = arg(3)?.to_string();
                    self.checkpoint();
                    if !self.ch.remove_spell(&spell, &cls, w.get(4).copied()) {
                        self.undo.pop();
                        return Err(Error::Other(format!("{} has no spell '{}'", cls, spell)));
                    }
                    changed(self, format!("removed {}", spell))
                }
                "prep" | "unprep" => self.cmd_prep(&w, force, changed),
                "scribe" | "unscribe" => self.cmd_scribe(&w, force, changed),
                other => Err(Error::Other(format!("unknown spell command '{}'", other))),
            },
            "spellbook" => self.cmd_spellbook(&w, changed),
            "temp" => self.cmd_temp(&w, force, changed),
            "follower" => match arg(1)? {
                "add" => {
                    let (name, kind) = (arg(2)?.to_string(), arg(3)?.to_string());
                    let race = self.find(FileKind::Race, arg(4)?)?.key.clone();
                    let file = w
                        .get(5)
                        .map(|f| f.to_string())
                        .unwrap_or_else(|| format!("{}.pcg", name));
                    // as many of a companion list as the rules allow
                    let max = {
                        let mut c = self.calc();
                        c.run();
                        c.max_followers(&kind)
                    };
                    let have = self
                        .ch
                        .followers()
                        .iter()
                        .filter(|f| f.1.eq_ignore_ascii_case(&kind))
                        .count() as i64;
                    if let Some(m) = max.filter(|m| have >= *m && !force) {
                        return Err(Error::Other(format!(
                            "the character may have {} {} follower{} (use --force)",
                            m,
                            kind,
                            if m == 1 { "" } else { "s" }
                        )));
                    }
                    self.checkpoint();
                    self.ch.add_follower(&name, &kind, &race, &file);
                    changed(self, format!("added {} {} ({})", kind, name, race))
                }
                "rm" | "remove" => {
                    let name = arg(2)?.to_string();
                    self.checkpoint();
                    if !self.ch.remove_follower(&name) {
                        self.undo.pop();
                        return Err(Error::Other(format!("no follower '{}'", name)));
                    }
                    changed(self, format!("removed {}", name))
                }
                "ls" | "list" => {
                    let lines: Vec<String> = self
                        .ch
                        .followers()
                        .iter()
                        .map(|(n, k, r, f)| format!("{}\t{}\t{}\t{}", n, k, r, f))
                        .collect();
                    Ok(Outcome::Text(lines.join("\n")))
                }
                other => Err(Error::Other(format!(
                    "unknown follower command '{}'",
                    other
                ))),
            },
            other => Err(Error::Other(format!(
                "unknown command '{}' (try 'help')",
                other
            ))),
        }
    }

    fn cmd_set(
        &mut self,
        w: &[&str],
        force: bool,
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
                self.check_stat_range(&key, score, force)?;
                // a bought score, while scores are still bought
                let buying = self.purchase().filter(|_| self.nonmonster_levels() < 2);
                if let Some(p) = &buying {
                    if !force && (score < p.min || score > p.max) {
                        return Err(Error::Other(format!(
                            "{} can buy scores between {} and {} (use --force)",
                            p.method, p.min, p.max
                        )));
                    }
                    let spent = self.points_spent(p, Some((&key, score)));
                    if !force && p.budget >= 0 && spent > p.budget {
                        return Err(Error::Other(format!(
                            "those scores cost {} points, over the budget of {} (use --force)",
                            spent, p.budget
                        )));
                    }
                }
                let old = self.ch.stat(&key).unwrap_or(0);
                self.checkpoint();
                self.ch.set_stat(&key, score);
                // a change after levels are gained is recorded with the
                // latest, as PCGen does
                if score != old {
                    self.ch.add_stat_change(&key, score - old);
                }
                if let Some(p) = &buying {
                    if self.nonmonster_levels() <= 1 {
                        let spent = self.points_spent(p, None);
                        self.ch.set_pool_points(spent);
                    }
                }
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
            "gold" | "funds" => {
                let v = need(2)?;
                let n: f64 = v
                    .trim_start_matches('+')
                    .parse()
                    .map_err(|_| Error::Other(format!("'{}' is not an amount of gold", v)))?;
                // +N and -N change the funds, N sets them
                let gold = if v.starts_with(['+', '-']) {
                    self.ch.gold() + n
                } else {
                    n
                };
                self.checkpoint();
                self.ch.set_gold(gold);
                changed(self, format!("gold is now {}", money_text(gold)))
            }
            "debt" => {
                let on = match need(2)?.to_ascii_lowercase().as_str() {
                    "on" | "yes" | "y" => true,
                    "off" | "no" | "n" => false,
                    other => {
                        return Err(Error::Other(format!("debt is on or off, not '{}'", other)))
                    }
                };
                self.checkpoint();
                self.ch.set_flag("ALLOWDEBT", on);
                changed(
                    self,
                    format!(
                        "buying into debt is {}",
                        if on { "allowed" } else { "refused" }
                    ),
                )
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

    /// Price of one of a data item.
    fn price_of(&self, key: &str) -> f64 {
        let mut c = self.calc();
        c.run();
        match self.ctx.dataset.get(FileKind::Equipment, key) {
            Some(o) => EqItem::from_data(&c, o).cost(&c),
            None => 0.0,
        }
    }

    /// Price of one of an item the character has, as customized, and
    /// whether it sells as cash (coins, gems and trade goods).
    fn owned_price(&self, name: &str) -> (f64, bool) {
        let mut c = self.calc();
        c.run();
        self.ch
            .pcg
            .lines_tagged("EQUIPNAME")
            .find(|l| l.head().is_some_and(|h| h.value_text() == name))
            .and_then(|l| EqItem::from_pcg(&c, l))
            .map(|it| {
                let cash = ["Coin", "Gem", "Trade"].iter().any(|t| it.is_type(&c, t));
                (it.cost(&c), cash)
            })
            .unwrap_or((0.0, false))
    }

    /// Work out a kit's formula, rolling any dice in it.
    fn kit_formula(&self, f: &str, rng: &mut Rng) -> f64 {
        let f = statgen::roll_calls(f, rng);
        f.trim().parse().unwrap_or_else(|_| {
            let mut c = self.calc();
            c.run();
            c.eval_formula(&f)
        })
    }

    /// Stats whose scores are generated (all but those with
    /// ROLLED:NO), by key.
    fn rolled_stats(&self) -> Vec<String> {
        self.ctx
            .dataset
            .objects(FileKind::Stat)
            .iter()
            .filter(|o| {
                !o.get("ROLLED")
                    .is_some_and(|v| v.eq_ignore_ascii_case("NO"))
            })
            .map(|o| o.key.clone())
            .collect()
    }

    /// The scores a stat may have (STATRANGE), by default 0 to 1000.
    fn stat_range(&self, key: &str) -> (i64, i64) {
        self.ctx
            .dataset
            .get(FileKind::Stat, key)
            .and_then(|o| o.get("STATRANGE"))
            .and_then(|r| {
                let (a, b) = r.split_once('|')?;
                Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
            })
            .unwrap_or((0, 1000))
    }

    fn check_stat_range(&self, key: &str, score: i64, force: bool) -> Result<()> {
        let (lo, hi) = self.stat_range(key);
        if !force && (score < lo || score > hi) {
            return Err(Error::Other(format!(
                "{} must be between {} and {} (use --force)",
                key, lo, hi
            )));
        }
        Ok(())
    }

    /// Levels in classes other than monster classes.
    fn nonmonster_levels(&self) -> u32 {
        let c = self.calc();
        self.ch
            .classes()
            .iter()
            .filter(|x| {
                !self
                    .ctx
                    .dataset
                    .get(FileKind::Class, &x.name)
                    .is_some_and(|o| c.is_monster_class(o))
            })
            .map(|x| x.level)
            .sum()
    }

    /// The game mode's point-buy rules, if it has any.
    fn point_buy(&self) -> Option<PointBuy> {
        self.ctx
            .gamemode
            .as_ref()
            .map(PointBuy::from_gamemode)
            .filter(PointBuy::available)
    }

    /// The scores the character can buy: those of the cost rows whose
    /// prerequisites it meets.
    fn buy_range(&self, pb: &PointBuy) -> (i64, i64) {
        let mut c = self.calc();
        c.run();
        pb.range(|r| {
            r.prereqs
                .iter()
                .all(|p| Prereq::parse(p).map_or(true, |p| c.passes(&p, None)))
        })
    }

    /// Work out a point-buy budget formula.
    fn budget_of(&self, formula: &str) -> i64 {
        statgen::roll(formula, &mut Rng::from_time()).unwrap_or_else(|| {
            let mut c = self.calc();
            c.run();
            c.eval_formula(formula) as i64
        })
    }

    /// The point buy the character's scores were made with.
    fn purchase(&self) -> Option<Purchase> {
        let (code, method) = self.ch.roll_method()?;
        if code != statgen::METHOD_PURCHASE {
            return None;
        }
        let pb = self.point_buy()?;
        let (name, formula) = pb.method(&method)?.clone();
        let (min, max) = self.buy_range(&pb);
        let budget = match self.ch.pool_budget() {
            b if b >= 0 => b,
            _ => self.budget_of(&formula),
        };
        Some(Purchase {
            pb,
            method: name,
            min,
            max,
            budget,
        })
    }

    /// Points the generated stats cost (with one score as given),
    /// plus POINTBUY|SPENT bonuses.
    fn points_spent(&self, p: &Purchase, over: Option<(&str, i64)>) -> i64 {
        let mut c = self.calc();
        c.run();
        let bonus = c.bonus.total("POINTBUY", "SPENT") as i64;
        let scores: i64 = self
            .rolled_stats()
            .iter()
            .map(|k| {
                let s = match over {
                    Some((o, v)) if o.eq_ignore_ascii_case(k) => v,
                    _ => self.ch.stat(k).unwrap_or(0),
                };
                p.pb.cost(s, p.min, p.max)
            })
            .sum();
        scores + bonus
    }

    /// Stat increases gaining a level of `cls` brings: the game mode's
    /// for the new total level, but none for a monster class's levels
    /// the race already accounts for.
    fn stat_increases_for_new_level(&self, cls: &Object) -> u32 {
        let Some(gm) = &self.ctx.gamemode else {
            return 0;
        };
        let specs = statgen::bonus_stat_specs(gm);
        if specs.is_empty() {
            return 0;
        }
        let level = self.ch.total_level() + 1;
        let mut c = self.calc();
        c.run();
        if c.is_monster_class(cls) {
            let racial = self
                .ch
                .race()
                .and_then(|r| self.ctx.dataset.get(FileKind::Race, &r))
                .and_then(|r| r.get("MONSTERCLASS"))
                .and_then(|v| v.rsplit(':').next())
                .and_then(|n| n.trim().parse::<u32>().ok())
                .unwrap_or(0);
            if level <= racial {
                return 0;
            }
        }
        let specs: Vec<&str> = specs.iter().map(String::as_str).collect();
        statgen::bonus_stats_for_level(&specs, level as i64, |f| c.eval_formula(f))
    }

    /// Check the stats chosen to raise against the increases a level
    /// grants.  Returns how many are left to take later.
    fn check_stat_increases(&self, grants: u32, raise: &[String], force: bool) -> Result<i64> {
        if raise.len() as u32 != grants && !force {
            return Err(Error::Other(format!(
                "this level grants {} stat increase{}; {} (or use --force{})",
                grants,
                if grants == 1 { "" } else { "s" },
                if grants > 0 {
                    "name the stat of each with --stat ABB"
                } else {
                    "drop --stat"
                },
                if grants > 0 {
                    " to leave them for later"
                } else {
                    ""
                }
            )));
        }
        let stack = self
            .ctx
            .gamemode
            .as_ref()
            .and_then(|g| g.misc("BONUSSTATALLOWSSTACK"))
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("YES"));
        let twice = raise
            .iter()
            .enumerate()
            .any(|(i, a)| raise[..i].contains(a));
        if twice && !stack && !force {
            return Err(Error::Other(
                "a stat can be raised only once a level (or use --force)".into(),
            ));
        }
        Ok(grants.saturating_sub(raise.len() as u32) as i64)
    }

    /// `stats buy|roll|same|assign|method`: generate the scores of a
    /// new character, or show how they were made.
    fn cmd_stats(
        &mut self,
        w: &[&str],
        force: bool,
        changed: impl Fn(&Session, String) -> Result<Outcome>,
    ) -> Result<Outcome> {
        let sub = w[1];
        if sub == "method" {
            return Ok(Outcome::Text(self.stat_method()));
        }
        if self.nonmonster_levels() > 1 && !force {
            return Err(Error::Other(
                "scores are generated before the second level (use --force)".into(),
            ));
        }
        let mut seed = None;
        let mut args = Vec::new();
        let mut it = w[2..].iter();
        while let Some(a) = it.next() {
            if *a == "--seed" {
                seed = Some(
                    it.next()
                        .and_then(|v| v.parse::<u64>().ok())
                        .ok_or_else(|| Error::Other("--seed needs a number".into()))?,
                );
            } else {
                args.push(*a);
            }
        }
        let stats = self.rolled_stats();
        let clamp = |s: &Session, k: &str, v: i64| {
            let (lo, hi) = s.stat_range(k);
            v.clamp(lo, hi.max(lo))
        };
        let (code, expr, scores): (u32, String, Vec<i64>) = match sub {
            "buy" => {
                let pb = self
                    .point_buy()
                    .ok_or_else(|| Error::Other("the game mode has no point buy".into()))?;
                let names: Vec<&str> = pb.methods.iter().map(|m| m.0.as_str()).collect();
                let names = names.join(", ");
                let (name, formula) = match args.first() {
                    Some(m) => pb.method(m).cloned().ok_or_else(|| {
                        Error::Other(format!(
                            "no point-buy method '{}'; choose from: {}",
                            m, names
                        ))
                    })?,
                    None if pb.methods.len() == 1 => pb.methods[0].clone(),
                    None => {
                        return Err(Error::Other(format!("name a point-buy method: {}", names)))
                    }
                };
                let (min, _) = self.buy_range(&pb);
                let neutral = pb.neutral(min);
                let budget = self.budget_of(&formula);
                self.checkpoint();
                for k in &stats {
                    let v = clamp(self, k, neutral);
                    self.ch.set_stat(k, v);
                }
                self.ch.set_roll_method(statgen::METHOD_PURCHASE, &name);
                self.ch.set_pool_budget(budget);
                if let Some(p) = self.purchase() {
                    let spent = self.points_spent(&p, None);
                    self.ch.set_pool_points(spent);
                }
                return changed(
                    self,
                    format!(
                        "buying scores with {} ({} points); every score is {}",
                        name, budget, neutral
                    ),
                );
            }
            "roll" => {
                let methods = self
                    .ctx
                    .gamemode
                    .as_ref()
                    .map(statgen::roll_methods)
                    .unwrap_or_default();
                let names: Vec<&str> = methods.iter().map(|m| m.0.as_str()).collect();
                let method = match args.first() {
                    Some(m) => methods.iter().find(|x| x.0.eq_ignore_ascii_case(m)),
                    None => methods.first(),
                };
                let Some((_, expr)) = method else {
                    return Err(Error::Other(format!(
                        "no roll method{}; choose from: {}",
                        args.first()
                            .map(|m| format!(" '{}'", m))
                            .unwrap_or_default(),
                        names.join(", ")
                    )));
                };
                let mut rng = seed.map(Rng::new).unwrap_or_else(Rng::from_time);
                let mut scores = Vec::new();
                for k in &stats {
                    let v = statgen::roll(expr, &mut rng)
                        .ok_or_else(|| Error::Other(format!("cannot roll '{}'", expr)))?;
                    scores.push(clamp(self, k, v));
                }
                (statgen::METHOD_ROLLED, expr.clone(), scores)
            }
            "same" => {
                let n: i64 = args
                    .first()
                    .and_then(|v| v.parse().ok())
                    .ok_or_else(|| Error::Other("give the score for every stat".into()))?;
                let scores = stats.iter().map(|k| clamp(self, k, n)).collect();
                (statgen::METHOD_SAME, n.to_string(), scores)
            }
            _ => {
                if args.len() != stats.len() {
                    return Err(Error::Other(format!(
                        "give {} scores, for {}",
                        stats.len(),
                        stats.join(", ")
                    )));
                }
                let mut scores = Vec::new();
                for (k, v) in stats.iter().zip(&args) {
                    let n: i64 = v
                        .parse()
                        .map_err(|_| Error::Other(format!("'{}' is not a score", v)))?;
                    self.check_stat_range(k, n, force)?;
                    scores.push(n);
                }
                (statgen::METHOD_USER, "0".into(), scores)
            }
        };
        self.checkpoint();
        for (k, v) in stats.iter().zip(&scores) {
            self.ch.set_stat(k, *v);
        }
        self.ch.set_roll_method(code, &expr);
        self.ch.set_pool_points(0);
        let shown: Vec<String> = stats
            .iter()
            .zip(&scores)
            .map(|(k, v)| format!("{} {}", k, v))
            .collect();
        changed(self, format!("scores: {}", shown.join(", ")))
    }

    /// How the character's scores were made.
    fn stat_method(&self) -> String {
        let mut out = match self.ch.roll_method() {
            Some((statgen::METHOD_SAME, n)) => format!("every score {}", n),
            Some((statgen::METHOD_ROLLED, e)) => format!("rolled ({})", e),
            Some((statgen::METHOD_PURCHASE, m)) => match self.purchase() {
                Some(p) => {
                    let costs: Vec<String> = (p.min..=p.max)
                        .map(|s| format!("{} {}", s, p.pb.cost(s, p.min, p.max)))
                        .collect();
                    format!(
                        "point buy ({}): {} of {} points spent; scores {} to {}\ncosts: {}",
                        p.method,
                        self.points_spent(&p, None),
                        p.budget,
                        p.min,
                        p.max,
                        costs.join(", ")
                    )
                }
                None => format!("point buy ({}), which the game mode does not have", m),
            },
            _ => "scores entered by hand".to_string(),
        };
        let pending = self.ch.pool_points();
        if self.purchase().is_none() && pending > 0 {
            out.push_str(&format!("\n{} stat increase(s) not taken", pending));
        }
        out
    }

    fn cmd_equip(
        &mut self,
        w: &[&str],
        force: bool,
        changed: impl Fn(&Session, String) -> Result<Outcome>,
    ) -> Result<Outcome> {
        // the equipment set worn or unworn in (by default the one used)
        let mut set: Option<&str> = None;
        let mut pos: Vec<&str> = Vec::new();
        let mut it = w.iter().copied();
        while let Some(x) = it.next() {
            match x {
                "--set" => {
                    set = Some(
                        it.next()
                            .ok_or_else(|| Error::Other("--set needs a set".into()))?,
                    )
                }
                o if o.starts_with("--") => {
                    return Err(Error::Other(format!("unknown option {}", o)));
                }
                _ => pos.push(x),
            }
        }
        let w = pos.as_slice();
        let sub = w.get(1).copied().unwrap_or("");
        if set.is_some() && !matches!(sub, "wear" | "unwear") {
            return Err(Error::Other(format!("equip {} takes no --set", sub)));
        }
        let set_id = match set {
            Some(s) => Some(self.find_equip_set(s)?.0),
            None => None,
        };
        let name = w
            .get(2)
            .copied()
            .ok_or_else(|| Error::Other("missing item name".into()))?;
        let resolved = if sub == "add" || sub == "buy" {
            name.to_string()
        } else {
            self.owned_item(name)
        };
        let name = resolved.as_str();
        match sub {
            "add" => {
                let o = self.find(FileKind::Equipment, name)?;
                let key = o.key.clone();
                let qty: f64 = w.get(3).and_then(|q| q.parse().ok()).unwrap_or(1.0);
                self.checkpoint();
                self.ch.add_equipment(&key, qty);
                changed(self, format!("acquired {} x{}", key, qty))
            }
            "buy" => {
                let key = self.find(FileKind::Equipment, name)?.key.clone();
                let qty: f64 = match w.get(3) {
                    Some(q) => q
                        .parse()
                        .ok()
                        .filter(|q: &f64| *q > 0.0)
                        .ok_or_else(|| Error::Other(format!("bad quantity '{}'", q)))?,
                    None => 1.0,
                };
                let cost = qty * self.price_of(&key) * BUY_RATE / 100.0;
                let gold = self.ch.gold();
                if cost > gold && !self.ch.flag("ALLOWDEBT", false) && !force {
                    return Err(Error::Other(format!(
                        "{} x{} costs {}, and the character has {} (use --force, or 'set debt on')",
                        key,
                        qty,
                        money_text(cost),
                        money_text(gold)
                    )));
                }
                self.checkpoint();
                self.ch.add_equipment(&key, qty);
                self.ch.set_gold(gold - cost);
                changed(
                    self,
                    format!("bought {} x{} for {}", key, qty, money_text(cost)),
                )
            }
            "sell" => {
                let Some(have) = self
                    .ch
                    .equipment()
                    .into_iter()
                    .find(|e| e.name == name)
                    .map(|e| e.quantity)
                else {
                    return Err(Error::Other(format!("the character has no '{}'", name)));
                };
                let qty: f64 = match w.get(3).copied() {
                    Some("all") => have,
                    Some(q) => q
                        .parse()
                        .ok()
                        .filter(|q: &f64| *q > 0.0)
                        .ok_or_else(|| Error::Other(format!("bad quantity '{}'", q)))?,
                    None => 1.0,
                };
                let (price, cash) = self.owned_price(name);
                let rate = if cash { CASH_SELL_RATE } else { SELL_RATE };
                let spellbook = self.item_book(name).is_some();
                self.checkpoint();
                let sold = self.ch.reduce_equipment(name, qty);
                if spellbook {
                    self.drop_item_books(name, have - sold);
                }
                let refund = sold * price * rate / 100.0;
                let gold = self.ch.gold() + refund;
                self.ch.set_gold(gold);
                changed(
                    self,
                    format!("sold {} x{} for {}", name, sold, money_text(refund)),
                )
            }
            "rm" | "remove" => {
                let spellbook = self.item_book(name).is_some();
                self.checkpoint();
                if !self.ch.remove_equipment(name) {
                    self.undo.pop();
                    return Err(Error::Other(format!("the character has no '{}'", name)));
                }
                if spellbook {
                    self.drop_item_books(name, 0.0);
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
                    None => default_location(obj, self.ctx.gamemode.as_ref()),
                };
                let warn = match obj {
                    Some(o) => self.check_prereqs(o, force)?,
                    None => String::new(),
                };
                // as in PCGen, a set holds no more of an item than the
                // character has
                let placed: f64 = self
                    .ch
                    .set_items(&set_id.clone().unwrap_or_else(|| self.ch.calc_equip_set()))
                    .iter()
                    .filter(|e| e.item.as_deref() == Some(owned.name.as_str()))
                    .map(|e| e.quantity.unwrap_or(1.0))
                    .sum();
                if placed + 1.0 > owned.quantity && !force {
                    return Err(Error::Other(format!(
                        "{} of {} {} are already in the set (use --force)",
                        placed, owned.quantity, owned.name
                    )));
                }
                if !force {
                    let set = set_id.clone().unwrap_or_else(|| self.ch.calc_equip_set());
                    self.check_slot_room(&set, &loc)?;
                }
                self.checkpoint();
                let calc = self.ch.ensure_equip_set();
                let set = set_id.unwrap_or(calc);
                self.ch.equip_in(&set, &owned.name, &loc, 1.0);
                changed(self, format!("{}equipped {} ({})", warn, owned.name, loc))
            }
            "unwear" => {
                let set = set_id.unwrap_or_else(|| self.ch.calc_equip_set());
                self.checkpoint();
                if !self.ch.unequip_in(&set, name) {
                    self.undo.pop();
                    return Err(Error::Other(format!("'{}' is not equipped", name)));
                }
                changed(self, format!("unequipped {}", name))
            }
            other => Err(Error::Other(format!("unknown equip command '{}'", other))),
        }
    }

    /// Is there room in body slot `loc` of an equipment set for one
    /// more item?  A slot (EQSLOT) holds its CONTAINS count times the
    /// game mode's NUMSLOTS number of its NUMBER kind, plus
    /// BONUS:SLOTS|slot; each item placed there takes its SLOTS (1 by
    /// default).  As in PCGen, an item fits while any room is left.
    /// Locations that are not body slots are not limited.
    fn check_slot_room(&self, set: &str, loc: &str) -> Result<()> {
        let Some(g) = self.ctx.gamemode.as_ref() else {
            return Ok(());
        };
        let Some(slot) = g
            .lines("equipmentslots.lst", "EQSLOT")
            .find(|l| l.value.eq_ignore_ascii_case(loc))
        else {
            return Ok(());
        };
        let tok = |t: &str| {
            slot.tokens
                .iter()
                .find(|(k, _)| k == t)
                .map(|(_, v)| v.as_str())
        };
        let per: i64 = tok("CONTAINS")
            .and_then(|v| v.rsplit_once('='))
            .and_then(|(_, n)| n.trim().parse().ok())
            .unwrap_or(1);
        let kind = tok("NUMBER").unwrap_or("");
        let count: i64 = g
            .lines("equipmentslots.lst", "NUMSLOTS")
            .flat_map(|l| l.tokens.iter())
            .find(|(k, _)| k.eq_ignore_ascii_case(kind))
            .and_then(|(_, v)| v.trim().parse().ok())
            .unwrap_or(0);
        let mut c = self.calc();
        c.run();
        let room = per * count + c.bonus.total("SLOTS", &slot.value) as i64;
        let owned = self.ch.equipment();
        let used: i64 = self
            .ch
            .set_items(set)
            .iter()
            .filter(|e| e.location.eq_ignore_ascii_case(&slot.value))
            .filter_map(|e| e.item.as_deref())
            .map(|name| {
                let key = owned
                    .iter()
                    .find(|o| o.name == name)
                    .and_then(|o| o.base.clone())
                    .unwrap_or_else(|| name.to_string());
                self.ctx
                    .dataset
                    .get(FileKind::Equipment, &key)
                    .and_then(|o| o.get("SLOTS"))
                    .and_then(|v| v.trim().parse::<i64>().ok())
                    .unwrap_or(1)
                    .max(0)
            })
            .sum();
        if used >= room {
            return Err(Error::Other(format!(
                "the {} slot holds {} and is full (use --force)",
                slot.value, room
            )));
        }
        Ok(())
    }

    /// The character's equipment sets as (id, name); PCGen's default
    /// set if it has none.
    fn equip_sets(&self) -> Vec<(String, String)> {
        let roots = self.ch.equip_set_roots();
        if roots.is_empty() {
            return vec![(DEFAULT_SET_ID.into(), DEFAULT_SET_NAME.into())];
        }
        roots
    }

    /// An equipment set, by name (in any case) or id.
    fn find_equip_set(&self, name: &str) -> Result<(String, String)> {
        let sets = self.equip_sets();
        sets.iter()
            .find(|(_, n)| n.eq_ignore_ascii_case(name))
            .or_else(|| sets.iter().find(|(id, _)| id == name))
            .cloned()
            .ok_or_else(|| Error::Other(format!("no equipment set named '{}'", name)))
    }

    /// Refuse a set name another set has, or one PCGen could not
    /// read back.
    fn check_set_name(&self, name: &str) -> Result<()> {
        if name.trim().is_empty() || name.contains(['|', '\t', '\n', '\r']) {
            return Err(Error::Other(format!("bad equipment set name '{}'", name)));
        }
        if self
            .equip_sets()
            .iter()
            .any(|(_, n)| n.eq_ignore_ascii_case(name))
        {
            return Err(Error::Other(format!(
                "there is already an equipment set named {}",
                name
            )));
        }
        Ok(())
    }

    /// `eqset ...`: equipment sets.
    fn cmd_eqset(
        &mut self,
        w: &[&str],
        changed: impl Fn(&Session, String) -> Result<Outcome>,
    ) -> Result<Outcome> {
        let arg = |i: usize| {
            w.get(i)
                .copied()
                .ok_or_else(|| Error::Other("missing argument (try 'help')".into()))
        };
        match arg(1)? {
            "ls" => {
                let calc = self.ch.calc_equip_set();
                let out: Vec<String> = self
                    .equip_sets()
                    .into_iter()
                    .map(|(id, name)| {
                        let n = self.ch.set_items(&id).len();
                        format!(
                            "{} {}\t{}\t{} item{}",
                            if id == calc { "*" } else { " " },
                            id,
                            name,
                            n,
                            if n == 1 { "" } else { "s" }
                        )
                    })
                    .collect();
                Ok(Outcome::Text(out.join("\n")))
            }
            "show" => {
                let set = match w.get(2) {
                    Some(s) => self.find_equip_set(s)?.0,
                    None => self.ch.calc_equip_set(),
                };
                let mut items = self.ch.set_items(&set);
                items.sort_by_key(|e| e.id.to_lowercase());
                let out: Vec<String> = items
                    .iter()
                    .map(|e| {
                        format!(
                            "{}\t{}\t{} x{}",
                            e.id,
                            e.location,
                            e.item.as_deref().unwrap_or(""),
                            e.quantity.unwrap_or(1.0)
                        )
                    })
                    .collect();
                Ok(Outcome::Text(if out.is_empty() {
                    "the set is empty".into()
                } else {
                    out.join("\n")
                }))
            }
            "add" => {
                let name = arg(2)?;
                self.check_set_name(name)?;
                self.checkpoint();
                // a new set is the one used, as in PCGen
                let id = self.ch.add_equip_set(name);
                self.ch.use_equip_set(&id);
                changed(self, format!("added equipment set {} ({})", name, id))
            }
            "copy" => {
                let (from, from_name) = self.find_equip_set(arg(2)?)?;
                let name = arg(3)?;
                self.check_set_name(name)?;
                self.checkpoint();
                self.ch.ensure_equip_set();
                let id = self.ch.copy_equip_set(&from, name);
                self.ch.use_equip_set(&id);
                changed(
                    self,
                    format!("copied equipment set {} to {} ({})", from_name, name, id),
                )
            }
            "rename" => {
                let (id, old) = self.find_equip_set(arg(2)?)?;
                let name = arg(3)?;
                if !old.eq_ignore_ascii_case(name) {
                    self.check_set_name(name)?;
                }
                self.checkpoint();
                self.ch.ensure_equip_set();
                self.ch.rename_equip_set(&id, name);
                changed(self, format!("renamed equipment set {} to {}", old, name))
            }
            "rm" | "remove" => {
                let (id, name) = self.find_equip_set(arg(2)?)?;
                if self.equip_sets().len() < 2 {
                    return Err(Error::Other(format!("{} is the last equipment set", name)));
                }
                self.checkpoint();
                self.ch.remove_equip_set(&id);
                changed(self, format!("removed equipment set {}", name))
            }
            "use" => {
                let (id, name) = self.find_equip_set(arg(2)?)?;
                self.checkpoint();
                self.ch.ensure_equip_set();
                self.ch.use_equip_set(&id);
                changed(self, format!("using equipment set {}", name))
            }
            other => Err(Error::Other(format!("unknown eqset command '{}'", other))),
        }
    }

    /// Make the choices the ADD tokens of a newly gained class level
    /// offer (those of its level lines, then on the class's first level
    /// those of the class line), taking `choices` in order, and record
    /// them with their effects.  A token offering no more than it
    /// grants takes everything.  Returns what was chosen.
    fn level_choices(
        &mut self,
        class: &str,
        level: u32,
        choices: &mut VecDeque<String>,
        force: bool,
    ) -> Result<Vec<String>> {
        use crate::rules::adds::{level_line_at, AddKind, AddToken};
        let Some(o) = self.ctx.dataset.get(FileKind::Class, class) else {
            return Ok(Vec::new());
        };
        let adds = |toks: &[crate::dataset::Tok]| -> Vec<AddToken> {
            toks.iter()
                .filter(|t| t.tag == "ADD")
                .filter_map(|t| AddToken::parse(&t.value))
                .collect()
        };
        let mut tokens: Vec<(bool, AddToken)> = o
            .sublines
            .iter()
            .filter(|sl| sl.tag == "LEVEL" && level_line_at(&sl.value, level))
            .flat_map(|sl| adds(&sl.tokens))
            .map(|t| (false, t))
            .collect();
        // the chosen subclass's own line for this level adds to it
        let sub = self.calc().subclass_of(o).map(|s| s.key.clone());
        if let Some(sub) = sub.and_then(|k| o.children.iter().find(|c| c.key == k)) {
            tokens.extend(
                sub.sublines
                    .iter()
                    .filter(|sl| {
                        sl.tag == "SUBCLASSLEVEL"
                            && sl
                                .value
                                .split(':')
                                .next()
                                .and_then(|v| v.trim().parse().ok())
                                == Some(level)
                    })
                    .flat_map(|sl| adds(&sl.tokens))
                    .map(|t| (false, t)),
            );
        }
        if level == 1 {
            tokens.extend(adds(&o.tokens).into_iter().map(|t| (true, t)));
        }
        let mut done = Vec::new();
        for (class_line, add) in tokens {
            let (offered, count) = {
                let mut c = self.calc();
                c.run();
                if !add.passes(&c) {
                    continue;
                }
                (add.options(&c, class), add.count(&c))
            };
            let what = format!("ADD:{}|{}", add.set_name(), add.lst_format());
            let listed = || {
                let shown: Vec<&str> = offered.iter().take(30).map(String::as_str).collect();
                let more = offered.len().saturating_sub(shown.len());
                format!(
                    "choose from: {}{}",
                    shown.join(", "),
                    if more > 0 {
                        format!(" ... ({} more)", more)
                    } else {
                        String::new()
                    }
                )
            };
            let picked: Vec<String> = if offered.len() <= count {
                // nothing to choose, though choices naming what is
                // taken anyway are accepted
                for _ in 0..offered.len() {
                    match choices.front() {
                        Some(ch) if offered.iter().any(|x| x.eq_ignore_ascii_case(ch)) => {
                            choices.pop_front();
                        }
                        _ => break,
                    }
                }
                offered.clone()
            } else {
                let mut v: Vec<String> = Vec::new();
                for _ in 0..count {
                    let found = choices.pop_front().map(|ch| {
                        let hit = offered
                            .iter()
                            .find(|x| x.eq_ignore_ascii_case(&ch) && !v.iter().any(|y| y == *x));
                        (ch, hit.cloned())
                    });
                    match found {
                        Some((_, Some(k))) => v.push(k),
                        _ if force => {
                            v.clear();
                            break;
                        }
                        Some((ch, None)) => {
                            return Err(Error::Other(format!(
                                "'{}' is not a choice for {}; {} (or use --force)",
                                ch,
                                what,
                                listed()
                            )))
                        }
                        None => {
                            return Err(Error::Other(format!(
                                "{} needs {} choice{} (--choose); {} (or use --force)",
                                what,
                                count,
                                if count == 1 { "" } else { "s" },
                                listed()
                            )))
                        }
                    }
                }
                v
            };
            if picked.is_empty() {
                continue;
            }
            let encoded: Vec<String> = picked.iter().map(|k| add.encode(k)).collect();
            let lst = add.lst_format();
            if class_line {
                self.ch
                    .add_class_choice(class, add.set_name(), &lst, &encoded);
            } else {
                self.ch
                    .add_level_choice(class, level, add.set_name(), &lst, &encoded);
            }
            for k in &picked {
                match &add.kind {
                    // the chosen class's level counts one higher for spells
                    AddKind::SpellCaster => self.ch.add_level_save(
                        class,
                        level,
                        &format!("BONUS|PCLEVEL|{}|1", k.to_uppercase()),
                    ),
                    AddKind::Ability { category, nature } => {
                        let parent = self.calc().pool_category(category);
                        let Some(ab) = self.ctx.dataset.get_in(FileKind::Ability, &parent, k)
                        else {
                            continue;
                        };
                        let cat = ab.category.clone().unwrap_or(parent);
                        // a granted ability is paid for by the pool
                        if nature == "NORMAL" {
                            let cost = crate::rules::pools::ability_cost(ab);
                            self.ch.adjust_user_pool(category, cost);
                        }
                        self.ch.add_ability_as(category, &cat, k, nature, &[]);
                    }
                    AddKind::Language => self.ch.add_language(k),
                    AddKind::ClassSkills => {
                        if let Some(n) = add.auto_rank() {
                            if self.buy_auto_ranks(class, k, n) {
                                done.push(format!("{} ({}, {} ranks)", k, add.set_name(), n));
                                continue;
                            }
                        }
                    }
                }
                done.push(format!("{} ({})", k, add.set_name()));
            }
        }
        Ok(done)
    }

    /// Buy `n` ranks of a skill just made a class skill of `class` by
    /// ADD:CLASSSKILLS with AUTORANK, as an ordinary purchase: only if
    /// the character qualifies for the skill, the class has the points
    /// at class skill cost, and the ranks stay within the maximum
    /// (unless the game mode's SKILLMAX rule lifts it).  Otherwise the
    /// skill is only a class skill, as in PCGen.  Whether ranks were
    /// bought.
    fn buy_auto_ranks(&mut self, class: &str, skill: &str, n: u32) -> bool {
        let Some(sk) = self.ctx.dataset.get(FileKind::Skill, skill) else {
            return false;
        };
        let key = sk.key.clone();
        let n = f64::from(n);
        let cost = self
            .ctx
            .gamemode
            .as_ref()
            .and_then(|g| g.misc("SKILLCOST_CLASS"))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(1.0);
        let (qualifies, within) = {
            let mut c = self.calc();
            c.run();
            let within = c.rule("SKILLMAX")
                || c.max_ranks(&key)
                    .is_none_or(|m| c.base_ranks(&key) + c.bonus.total("SKILLRANK", &key) + n <= m);
            (c.failing_prereqs(sk).is_empty(), within)
        };
        if !qualifies || !within || self.ch.skill_points_left(class) < n * cost {
            return false;
        }
        let had: f64 = self
            .ch
            .skills()
            .iter()
            .find(|e| e.name.eq_ignore_ascii_case(&key))
            .and_then(|e| {
                e.ranks
                    .iter()
                    .find(|(c, _)| c.eq_ignore_ascii_case(class))
                    .map(|(_, r)| *r)
            })
            .unwrap_or(0.0);
        self.ch.set_skill_ranks(&key, class, had + n, true);
        self.ch.spend_skill_points(class, n * cost);
        true
    }

    /// Skill points a rank costs: of a class skill, or another.
    fn rank_cost(&self, class_skill: bool) -> f64 {
        let (tag, d) = if class_skill {
            ("SKILLCOST_CLASS", 1.0)
        } else {
            ("SKILLCOST_CROSSCLASS", 2.0)
        };
        self.ctx
            .gamemode
            .as_ref()
            .and_then(|g| g.misc(tag))
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(d)
    }

    /// Buy the ranks of a kit's SKILL line, as a level's skill points
    /// are spent: RANK ranks (one by default) in each of its first
    /// COUNT skills (one by default; nobody is asked to choose), paid
    /// from the points of the class the line names (CLASS) and then of
    /// each class in turn, at that class's cost and within the maximum
    /// ranks.  A free line (FREE:YES) costs no points.  A skill the
    /// character does not qualify for, or an exclusive one the class
    /// lacks, is not bought.  A skill that chooses languages takes
    /// those of SELECTION it offers, one a rank.
    fn kit_skill(
        &mut self,
        sl: &crate::dataset::SubLine,
        done: &mut Vec<String>,
        short: &mut Vec<String>,
    ) -> Result<()> {
        let tok = |t: &str| {
            sl.tokens
                .iter()
                .find(|x| x.tag == t)
                .map(|x| x.value.trim().to_string())
        };
        let want: f64 = tok("RANK")
            .and_then(|v| v.parse().ok())
            .filter(|n: &f64| n.is_finite() && *n >= 0.0)
            .unwrap_or(1.0);
        let free = tok("FREE").is_some_and(|v| v.to_ascii_uppercase().starts_with('Y'));
        let count: usize = tok("COUNT").and_then(|v| v.parse().ok()).unwrap_or(1);
        let mut skills: Vec<String> = Vec::new();
        for part in sl.value.split('|').map(str::trim) {
            if let Some(t) = part
                .strip_prefix("TYPE=")
                .or_else(|| part.strip_prefix("TYPE."))
            {
                for o in self.ctx.dataset.objects(FileKind::Skill) {
                    if t.split('.').all(|x| o.is_type(x)) && !skills.contains(&o.key) {
                        skills.push(o.key.clone());
                    }
                }
            } else {
                let key = self.find(FileKind::Skill, part)?.key.clone();
                if !skills.contains(&key) {
                    skills.push(key);
                }
            }
        }
        skills.truncate(count.max(1));
        // the named class pays first
        let mut classes: Vec<String> = Vec::new();
        if let Some(c) = tok("CLASS") {
            match self
                .ch
                .classes()
                .into_iter()
                .find(|x| x.name.eq_ignore_ascii_case(&c))
            {
                Some(x) => classes.push(x.name),
                None => short.push(format!("the character has no {} levels to buy skills", c)),
            }
        }
        for x in self.ch.classes() {
            if !classes.contains(&x.name) {
                classes.push(x.name);
            }
        }
        for key in skills {
            let mut left = want;
            for class in &classes {
                if left <= 0.0 {
                    break;
                }
                let pool = self.ch.skill_points_left(class);
                if !free && pool <= 0.0 {
                    continue;
                }
                let (class_skill, room, ok) = {
                    let mut c = self.calc();
                    c.run();
                    let sk = self.ctx.dataset.get(FileKind::Skill, &key);
                    let class_skill = c.is_class_skill_for(&key, class);
                    let exclusive =
                        sk.is_some_and(|s| s.get("EXCLUSIVE").is_some_and(|v| v == "YES"));
                    let qualifies = sk.is_none_or(|s| c.failing_prereqs(s).is_empty());
                    let room = match c.max_ranks(&key) {
                        Some(m) if !c.rule("SKILLMAX") => {
                            m - c.base_ranks(&key) - c.bonus.total("SKILLRANK", &key)
                        }
                        _ => f64::INFINITY,
                    };
                    (class_skill, room, qualifies && (class_skill || !exclusive))
                };
                if !ok {
                    continue;
                }
                let cost = self.rank_cost(class_skill);
                let mut n = left.min(room);
                if !free && cost > 0.0 {
                    n = n.min((pool / cost).floor());
                }
                if n <= 0.0 {
                    continue;
                }
                let had: f64 = self
                    .ch
                    .skills()
                    .iter()
                    .find(|e| e.name.eq_ignore_ascii_case(&key))
                    .and_then(|e| {
                        e.ranks
                            .iter()
                            .find(|(c, _)| c.eq_ignore_ascii_case(class))
                            .map(|(_, r)| *r)
                    })
                    .unwrap_or(0.0);
                self.ch.set_skill_ranks(&key, class, had + n, class_skill);
                if !free {
                    self.ch.spend_skill_points(class, n * cost);
                }
                left -= n;
            }
            let bought = want - left;
            if left > 0.0 {
                short.push(format!("could not add {} ranks to {}", left, key));
            }
            if bought <= 0.0 {
                continue;
            }
            done.push(format!("{} {}", key, bought));
            if let Some(sel) = tok("SELECTION") {
                self.kit_skill_languages(&key, &sel, bought);
            }
        }
        Ok(())
    }

    /// Languages a kit chooses with ranks in a skill that offers them
    /// (CHOOSE:LANG): those listed that it offers and the character
    /// lacks, one a rank.
    fn kit_skill_languages(&mut self, skill: &str, list: &str, ranks: f64) {
        let offered = {
            let mut c = self.calc();
            c.run();
            self.ctx
                .dataset
                .get(FileKind::Skill, skill)
                .filter(|s| {
                    s.get("CHOOSE")
                        .is_some_and(|v| v.to_ascii_uppercase().starts_with("LANG"))
                })
                .and_then(|s| crate::rules::pools::candidates(&c, s))
                .unwrap_or_default()
        };
        let mut limit = ranks.floor() as usize;
        for name in list.split(',').map(str::trim) {
            if limit == 0 {
                break;
            }
            let Some(lang) = offered.iter().find(|x| x.eq_ignore_ascii_case(name)) else {
                continue;
            };
            if self
                .ch
                .languages()
                .iter()
                .any(|l| l.eq_ignore_ascii_case(lang))
            {
                continue;
            }
            let lang = lang.clone();
            self.ch.add_language(&lang);
            self.ch.add_skill_choice(skill, &lang);
            limit -= 1;
        }
    }

    /// Check choices against what the ability's CHOOSE offers.
    fn check_choices(&self, o: &Object, choices: &[String], force: bool) -> Result<()> {
        let Some(choose) = o.get("CHOOSE") else {
            return Ok(());
        };
        if choose.starts_with("NOCHOICE") || force {
            return Ok(());
        }
        let mut c = self.calc();
        c.run();
        let o = self.ctx.dataset.get(o.kind, &o.key).unwrap_or(o);
        let cands = crate::rules::pools::candidates(&c, o);
        if choices.is_empty() {
            let hint = match &cands {
                Some(v) if !v.is_empty() => {
                    let shown: Vec<&str> = v.iter().take(20).map(String::as_str).collect();
                    format!("; choose from: {}", shown.join(", "))
                }
                _ => String::new(),
            };
            return Err(Error::Other(format!(
                "{} needs a choice{} (or use --force)",
                o.key, hint
            )));
        }
        if let Some(v) = cands {
            for ch in choices {
                if !v.iter().any(|x| x.eq_ignore_ascii_case(ch)) {
                    return Err(Error::Other(format!(
                        "'{}' is not a choice for {} (use --force)",
                        ch, o.key
                    )));
                }
            }
        }
        Ok(())
    }

    /// Check there are pool points for a chosen ability.
    fn check_pool(&self, pool: &str, o: &Object, force: bool) -> Result<()> {
        if force {
            return Ok(());
        }
        let mut c = self.calc();
        c.run();
        let pools = crate::rules::pools::pools(&c);
        let Some(p) = pools.iter().find(|p| p.category.eq_ignore_ascii_case(pool)) else {
            return Ok(());
        };
        let cost = crate::rules::pools::ability_cost(o);
        if p.total > 0.0 && p.spent + cost > p.total {
            return Err(Error::Other(format!(
                "no {} points left ({} of {} spent; use --force)",
                p.category, p.spent, p.total
            )));
        }
        Ok(())
    }

    /// The hit die (as the rules adjust it; a class without one gains
    /// no rolled hit points) and skill points a new level of `class` is
    /// worth.  The level is held while it is counted, so what the class
    /// defines (a variable its points read) applies, and the stats in
    /// `raise` rise first if the game mode counts them before.  Also
    /// returns the working copy of the character holding the level.
    fn new_level_worth(&self, class: &Object, raise: &[String]) -> (i64, f64, Character) {
        let int_before = {
            let mut c = self.calc();
            c.run();
            c.rule("INTBEFORE")
        };
        let mut work = self.ch.clone();
        if int_before {
            raise_stats(&mut work, raise);
        }
        work.add_level(&class.key, 0, 0.0);
        let mut c = Calc::new(&self.ctx.dataset, self.ctx.gamemode.as_ref(), &work);
        c.run();
        let char_level = self.ch.total_level() + 1;
        let class_level = self
            .ch
            .classes()
            .iter()
            .find(|x| x.name.eq_ignore_ascii_case(&class.key))
            .map_or(1, |x| x.level + 1);
        let hd = c.hit_die(class);
        let skills = c.skill_points_for_level(class, char_level)
            + c.bonus_skill_points(class, class_level, char_level);
        drop(c);
        (hd, skills, work)
    }

    /// Apply a starting kit's lines: funds, gear, stats, abilities,
    /// skills, languages, templates, deity, alignment and class levels.
    /// Gear is paid for at the kit's rate (EQUIPBUY, the full price by
    /// default), as much of it as the funds allow, unless the kit has
    /// one price (TOTALCOST), paid first.  A kit applied at once
    /// (APPLY:INSTANT) is not recorded.
    fn apply_kit(&mut self, kit: &Object) -> Result<String> {
        let mut done = Vec::new();
        let mut short = Vec::new();
        let mut rng = Rng::from_time();
        let formula = |v: &str| v.split('|').next().unwrap_or("").to_string();
        let mut rate = match kit.get("EQUIPBUY") {
            Some(f) => self.kit_formula(&formula(f), &mut rng),
            None => BUY_RATE,
        };
        if let Some(t) = kit.get("TOTALCOST") {
            let charge = (self.kit_formula(&formula(t), &mut rng) * BUY_RATE / 100.0).floor();
            let gold = self.ch.gold();
            self.ch.set_gold(gold - charge);
            done.push(format!("paid {}", money_text(charge)));
            rate = 0.0;
        }
        for sl in &kit.sublines {
            let val = sl.value.as_str();
            let tok = |t: &str| {
                sl.tokens
                    .iter()
                    .find(|x| x.tag == t)
                    .map(|x| x.value.clone())
            };
            // a line applies only if the character meets its
            // prerequisites
            let prereqs: Vec<Prereq> = sl
                .tokens
                .iter()
                .filter(|t| t.tag.starts_with("PRE") || t.tag.starts_with("!PRE"))
                .filter_map(|t| Prereq::from_token(&t.tag, &t.value).ok())
                .collect();
            if !prereqs.is_empty() {
                let mut c = self.calc();
                c.run();
                if !prereqs.iter().all(|p| c.passes(p, None)) {
                    continue;
                }
            }
            match sl.tag.as_str() {
                "FUNDS" => {
                    let n = match tok("QTY") {
                        Some(f) => self.kit_formula(&f, &mut rng),
                        None => 0.0,
                    };
                    let gold = self.ch.gold();
                    self.ch.set_gold(gold + n);
                    done.push(format!("{} {}", money_text(n), val));
                }
                "GEAR" => {
                    let want = match tok("QTY") {
                        Some(f) => self.kit_formula(&f, &mut rng).floor(),
                        None => 1.0,
                    };
                    if !want.is_finite() || want > MAX_KIT_QTY {
                        return Err(Error::Other(format!(
                            "kit {} asks for an unreasonable quantity of {}",
                            kit.key, val
                        )));
                    }
                    let key = self.find(FileKind::Equipment, val)?.key.clone();
                    // as many as the funds pay for (never into debt)
                    let mut qty = want;
                    if rate != 0.0 {
                        let price = self.price_of(&key) * rate / 100.0;
                        let gold = self.ch.gold();
                        if price > 0.0 && price * qty > gold {
                            qty = want.min((gold / price).floor()).max(0.0);
                        }
                        if qty < want {
                            short.push(format!(
                                "could not buy {} {}: not enough funds",
                                want - qty,
                                key
                            ));
                        }
                        self.ch.set_gold(gold - price * qty.max(0.0));
                    }
                    if qty <= 0.0 {
                        continue;
                    }
                    self.ch.add_equipment(&key, qty);
                    done.push(format!("{} x{}", key, qty));
                }
                "STAT" => {
                    for part in val.split('|') {
                        if let Some((a, v)) = part.split_once('=') {
                            if let Ok(n) = v.trim().parse::<i64>() {
                                self.ch.set_stat(a, n);
                                done.push(format!("{} {}", a, n));
                            }
                        }
                    }
                }
                "ABILITY" | "FEAT" => {
                    let (cat, list) = match sl.tag.as_str() {
                        "FEAT" => ("FEAT".to_string(), val.to_string()),
                        _ => {
                            let rest = val.strip_prefix("CATEGORY=").unwrap_or(val);
                            match rest.split_once('|') {
                                Some((c, l)) => (c.to_string(), l.to_string()),
                                None => ("FEAT".to_string(), rest.to_string()),
                            }
                        }
                    };
                    for k in list.split('|') {
                        let o = self.find_in(FileKind::Ability, Some(&cat), k)?;
                        let (key, c) = (o.key.clone(), o.category.clone().unwrap_or(cat.clone()));
                        self.ch.add_ability(&cat, &c, &key, &[]);
                        done.push(key);
                    }
                }
                "SKILL" => self.kit_skill(sl, &mut done, &mut short)?,
                "LANGUAGE" | "LANGAUTO" => {
                    for k in val.split('|') {
                        let key = self.find(FileKind::Language, k)?.key.clone();
                        self.ch.add_language(&key);
                        done.push(key);
                    }
                }
                "TEMPLATE" => {
                    let key = self.find(FileKind::Template, val)?.key.clone();
                    self.ch.add_template(&key);
                    done.push(key);
                }
                "DEITY" => {
                    let key = self.find(FileKind::Deity, val)?.key.clone();
                    self.ch.set_deity(Some(&key));
                    done.push(key);
                }
                "ALIGN" => {
                    self.ch.set_alignment(val);
                    done.push(val.to_string());
                }
                "RACE" => {
                    let key = self.find(FileKind::Race, val)?.key.clone();
                    self.ch.set_race(&key);
                    done.push(key);
                }
                "CLASS" => {
                    let key = self.find(FileKind::Class, val)?.key.clone();
                    let levels: u32 = tok("LEVEL").and_then(|v| v.parse().ok()).unwrap_or(1);
                    if levels > MAX_KIT_LEVELS {
                        return Err(Error::Other(format!(
                            "kit {} asks for an unreasonable number of {} levels",
                            kit.key, key
                        )));
                    }
                    // each level is gained as one added by hand is
                    for _ in 0..levels {
                        let (hd, skills, _) = {
                            let o = self.find(FileKind::Class, &key)?;
                            self.new_level_worth(o, &[])
                        };
                        self.ch.add_level(&key, hd, skills);
                    }
                    done.push(format!("{} {}", key, levels));
                }
                _ => {}
            }
        }
        if !kit
            .get("APPLY")
            .is_some_and(|a| a.trim().eq_ignore_ascii_case("INSTANT"))
        {
            self.ch.add_kit(&kit.key);
        }
        let short: String = short.iter().map(|s| format!("\n{}", s)).collect();
        Ok(format!(
            "applied kit {}: {}{}",
            kit.key,
            done.join(", "),
            short
        ))
    }

    /// `equip customize NAME +MOD[|CHOICE] | -MOD | size SIZE`
    fn cmd_customize(
        &mut self,
        w: &[&str],
        changed: impl Fn(&Session, String) -> Result<Outcome>,
    ) -> Result<Outcome> {
        let name = self.owned_item(
            w.get(2)
                .ok_or_else(|| Error::Other("missing item name".into()))?,
        );
        let change = w
            .get(3)
            .ok_or_else(|| Error::Other("missing change (+MOD, -MOD or size SIZE)".into()))?;
        if !self.ch.equipment().iter().any(|e| e.name == name) {
            return Err(Error::Other(format!("the character has no '{}'", name)));
        }
        let (base, data) = self.ch.customization(&name).unwrap_or_else(|| {
            let base = self
                .ctx
                .dataset
                .get(FileKind::Equipment, &name)
                .map(|o| o.key.clone())
                .unwrap_or(name.clone());
            (base, format!("NAME={}", name))
        });
        let mut parts: Vec<String> = data.split('$').map(str::to_string).collect();
        let mods_at = parts.iter().position(|p| p.starts_with("EQMOD="));
        let mut mods: Vec<String> = mods_at
            .map(|i| {
                parts[i]["EQMOD=".len()..]
                    .split('.')
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let msg = if let Some(m) = change.strip_prefix('+') {
            let key = m.split('|').next().unwrap_or(m);
            let eqmod = self.find(FileKind::EquipMod, key)?;
            let entry = match m.split_once('|') {
                Some((_, c)) => format!("{}|{}", eqmod.key, c),
                None => eqmod.key.clone(),
            };
            mods.push(entry);
            format!("added {} to {}", eqmod.key, name)
        } else if let Some(m) = change.strip_prefix('-') {
            let before = mods.len();
            mods.retain(|x| !x.split('|').next().unwrap_or(x).eq_ignore_ascii_case(m));
            if mods.len() == before {
                return Err(Error::Other(format!("{} has no modifier '{}'", name, m)));
            }
            format!("removed {} from {}", m, name)
        } else if *change == "size" {
            let size = w
                .get(4)
                .ok_or_else(|| Error::Other("missing size".into()))?;
            let key = self.find(FileKind::Size, size)?.key.clone();
            parts.retain(|p| !p.starts_with("SIZE="));
            parts.push(format!("SIZE={}", key));
            format!("{} is now size {}", name, key)
        } else {
            return Err(Error::Other(format!("unknown change '{}'", change)));
        };
        parts.retain(|p| !p.starts_with("EQMOD="));
        if !mods.is_empty() {
            parts.push(format!("EQMOD={}", mods.join(".")));
        }
        self.checkpoint();
        self.ch.set_customization(&name, &base, &parts.join("$"));
        let msg = match self.container_capacity(&name) {
            Some(cap) => format!("{}; it holds {}", msg, cap),
            None => msg,
        };
        changed(self, msg)
    }

    /// What a container the character has holds, as text.
    fn container_capacity(&self, name: &str) -> Option<String> {
        let mut c = self.calc();
        c.run();
        let line = self
            .ch
            .pcg
            .lines_tagged("EQUIPNAME")
            .find(|l| l.head().is_some_and(|h| h.value_text() == name))?;
        let cap = EqItem::from_pcg(&c, line)?.capacity(&c)?;
        let amount = |n: Option<f64>| n.map_or("any number".to_string(), |n| format!("{}", n));
        let mut out = vec![match cap.weight {
            Some(w) => format!("{} lbs", w),
            None => "any weight".to_string(),
        }];
        for (t, n) in &cap.items {
            out.push(format!("{} {}", amount(*n), t));
        }
        Some(out.join(", "))
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
        let mut calc = self.calc();
        calc.run();
        let mut points: Vec<String> = crate::rules::pools::pools(&calc)
            .iter()
            .map(|p| format!("{} {}/{}", p.category, p.spent, p.total))
            .collect();
        for x in c.classes() {
            let left = c.skill_points_left(&x.name);
            if left != 0.0 {
                points.push(format!("{} skill points {}", x.name, left));
            }
        }
        let currency = self
            .ctx
            .gamemode
            .as_ref()
            .and_then(|g| g.misc("CURRENCYUNITABBREV"))
            .map(|c| format!(" {}", c))
            .unwrap_or_default();
        format!(
            "{}\n  game mode:  {}\n  campaigns:  {}\n  race:       {}\n  alignment:  {}\n  classes:    {}\n  stats:      {}\n  abilities:  {}\n  equipment:  {}\n  gold:       {}{}\n  pools:      {}\n  {}",
            c.name().unwrap_or_default(),
            c.gamemode().unwrap_or_default(),
            c.campaigns().join("; "),
            c.race().unwrap_or_default(),
            c.alignment().unwrap_or_default(),
            classes.join(", "),
            stats.join(", "),
            abilities.join(", "),
            items.join(", "),
            money_text(c.gold()),
            currency,
            points.join(", "),
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

/// Point buy in effect for a character.
struct Purchase {
    pb: PointBuy,
    method: String,
    /// the lowest and highest scores the character can buy
    min: i64,
    max: i64,
    /// points to spend (-1: none)
    budget: i64,
}

/// Raise each stat by one.
fn raise_stats(ch: &mut Character, keys: &[String]) {
    for k in keys {
        let v = ch.stat(k).unwrap_or(0);
        ch.set_stat(k, v + 1);
    }
}

/// Most of one item a kit may hand out.
const MAX_KIT_QTY: f64 = 10_000.0;

/// Most class levels one kit line may grant.
const MAX_KIT_LEVELS: u32 = 100;

/// Spell books and lists, and preparing spells.
impl Session {
    /// A name for a new spell list: not a class's, nor holding the
    /// characters that separate a saved line's parts.
    fn check_book_name(&self, name: &str) -> Result<()> {
        if name.trim().is_empty() || name.contains(['|', ':', '[', ']']) {
            return Err(Error::Other(
                "a spell list name may not contain | : [ or ]".into(),
            ));
        }
        if self.ctx.dataset.get(FileKind::Class, name).is_some() {
            return Err(Error::Other(format!(
                "a spell list may not be named for the class {}",
                name
            )));
        }
        Ok(())
    }

    /// Names of the items placed in the equipment set used, in set
    /// order, each once.
    fn set_items(&self) -> Vec<String> {
        let calc = self
            .ch
            .pcg
            .value("CALCEQUIPSET")
            .unwrap_or_else(|| "0.1".into());
        let inside = format!("{}.", calc);
        let mut out: Vec<String> = Vec::new();
        for e in self.ch.equipsets() {
            if let Some(it) = e.item.filter(|_| e.id.starts_with(&inside)) {
                if !out.iter().any(|x| x.eq_ignore_ascii_case(&it)) {
                    out.push(it);
                }
            }
        }
        out
    }

    fn cmd_temp(
        &mut self,
        w: &[&str],
        force: bool,
        changed: impl Fn(&Session, String) -> Result<Outcome>,
    ) -> Result<Outcome> {
        let sub = w.get(1).copied().unwrap_or("");
        if sub == "mods" {
            let on = match w.get(2).map(|v| v.to_ascii_lowercase()).as_deref() {
                Some("on" | "y" | "yes") => true,
                Some("off" | "n" | "no") => false,
                _ => return Err(Error::Other("use 'temp mods on' or 'temp mods off'".into())),
            };
            self.checkpoint();
            self.ch.set_temp_mods(on);
            return changed(
                self,
                format!(
                    "temporary modifiers {}",
                    if on { "used" } else { "not used" }
                ),
            );
        }
        // --value N, then the source and the target
        let mut value: Option<i64> = None;
        let mut args: Vec<&str> = Vec::new();
        let mut i = 2;
        while i < w.len() {
            if w[i] == "--value" {
                let v = w
                    .get(i + 1)
                    .ok_or_else(|| Error::Other("--value needs a number".into()))?;
                value = Some(
                    v.parse()
                        .map_err(|_| Error::Other(format!("bad value '{}'", v)))?,
                );
                i += 2;
            } else {
                args.push(w[i]);
                i += 1;
            }
        }
        // a source, as [KIND=]KEY
        let source = |s: &str| -> (Option<&'static str>, String) {
            if let Some((k, key)) = s.split_once('=') {
                let kind = match k.to_ascii_lowercase().as_str() {
                    "feat" | "ability" => Some("FEAT"),
                    "spell" => Some("SPELL"),
                    "template" => Some("TEMPLATE"),
                    "skill" => Some("SKILL"),
                    "equip" | "equipment" => Some("EQUIPMENT"),
                    _ => None,
                };
                if kind.is_some() {
                    return (kind, key.to_string());
                }
            }
            (None, s.to_string())
        };
        let on_pc = |t: &str| t.eq_ignore_ascii_case("pc");
        match sub {
            "ls" => {
                let mut c = self.calc();
                c.run();
                let q = match args.first() {
                    Some(g) => Some(Query::new(Some(g), &[]).map_err(Error::Other)?),
                    None => None,
                };
                let items: Vec<(String, Option<EqItem>)> = self
                    .set_items()
                    .into_iter()
                    .map(|n| {
                        let line = self
                            .ch
                            .pcg
                            .lines_tagged("EQUIPNAME")
                            .find(|l| l.head().is_some_and(|h| h.value_text() == n));
                        let it = line.and_then(|l| EqItem::from_pcg(&c, l));
                        (n, it)
                    })
                    .collect();
                let mut out = Vec::new();
                for o in crate::rules::temp::offers(&c) {
                    if q.as_ref().is_some_and(|q| !q.matches(o.obj)) {
                        continue;
                    }
                    let mut to: Vec<String> = Vec::new();
                    if !o.pc.is_empty() {
                        to.push("PC".into());
                    }
                    for (n, it) in &items {
                        if it
                            .as_ref()
                            .is_some_and(|it| o.suits(&|t| it.is_type(&c, t)))
                        {
                            to.push(n.clone());
                        }
                    }
                    let range = match o.value_range(&c) {
                        Some(Ok((a, b))) => format!("  value {}-{}", a, b),
                        Some(Err(e)) => format!("  ({})", e),
                        None => String::new(),
                    };
                    let to = if to.is_empty() {
                        "no target here".to_string()
                    } else {
                        to.join(", ")
                    };
                    out.push(format!("{} ({}): {}{}", o.key(), o.kind, to, range));
                }
                let applied: Vec<String> = self
                    .ch
                    .temp_bonuses()
                    .iter()
                    .map(|t| {
                        format!(
                            "{} ({}) on {}{}",
                            t.key,
                            t.kind,
                            t.target,
                            if t.active { "" } else { " (off)" }
                        )
                    })
                    .collect();
                if out.is_empty() {
                    out.push("no temporary bonuses offered".into());
                }
                if !applied.is_empty() {
                    out.push("applied:".into());
                    out.extend(applied.into_iter().map(|a| format!("  {}", a)));
                }
                if !self.ch.use_temp_mods() {
                    out.push("temporary modifiers are off".into());
                }
                Ok(Outcome::Text(out.join("\n")))
            }
            "add" => {
                let (kind, key) = source(
                    args.first()
                        .ok_or_else(|| Error::Other("missing source".into()))?,
                );
                let mut c = self.calc();
                c.run();
                let offers = crate::rules::temp::offers(&c);
                let found: Vec<_> = offers
                    .iter()
                    .filter(|o| kind.is_none_or(|k| k == o.kind))
                    .filter(|o| {
                        o.obj.key.eq_ignore_ascii_case(&key)
                            || o.obj.name.eq_ignore_ascii_case(&key)
                    })
                    .collect();
                let o = match found.as_slice() {
                    [o] => *o,
                    [] => {
                        return Err(Error::Other(format!(
                        "nothing named '{}' offers the character a temporary bonus (see 'temp ls')",
                        key
                    )))
                    }
                    _ => {
                        return Err(Error::Other(format!(
                            "'{}' is ambiguous; name its kind (e.g. spell={})",
                            key, key
                        )))
                    }
                };
                // the target: the character, or an item in the set used
                let target = match args.get(1) {
                    None if !o.pc.is_empty() => "PC".to_string(),
                    None => {
                        return Err(Error::Other(format!(
                            "{} applies only to items; name one",
                            o.key()
                        )))
                    }
                    Some(t) if on_pc(t) => {
                        if o.pc.is_empty() {
                            return Err(Error::Other(format!("{} applies only to items", o.key())));
                        }
                        "PC".to_string()
                    }
                    Some(t) => {
                        if o.eq.is_empty() {
                            return Err(Error::Other(format!(
                                "{} applies only to the character",
                                o.key()
                            )));
                        }
                        let name = self.owned_item(t);
                        let Some(name) = self
                            .set_items()
                            .into_iter()
                            .find(|n| n.eq_ignore_ascii_case(&name))
                        else {
                            return Err(Error::Other(format!(
                                "'{}' is not in the equipment set used",
                                t
                            )));
                        };
                        let line = self
                            .ch
                            .pcg
                            .lines_tagged("EQUIPNAME")
                            .find(|l| l.head().is_some_and(|h| h.value_text() == name));
                        let suits = line
                            .and_then(|l| EqItem::from_pcg(&c, l))
                            .is_some_and(|it| o.suits(&|t| it.is_type(&c, t)));
                        if !suits && !force {
                            let conds: Vec<&str> = o.eq.iter().map(|(c, _)| c.as_str()).collect();
                            return Err(Error::Other(format!(
                                "{} does not suit {} (it needs {}; use --force)",
                                o.key(),
                                name,
                                conds.join(" or ")
                            )));
                        }
                        name
                    }
                };
                let choice = match o.value_range(&c) {
                    None if value.is_some() => {
                        return Err(Error::Other(format!("{} takes no value", o.key())))
                    }
                    None => None,
                    Some(range) => {
                        let v = value.ok_or_else(|| {
                            Error::Other(match &range {
                                Ok((a, b)) => {
                                    format!("{} needs --value, from {} to {}", o.key(), a, b)
                                }
                                Err(e) => format!("{} needs --value ({})", o.key(), e),
                            })
                        })?;
                        match range {
                            Ok((a, b)) if v < a || v > b => {
                                if !force {
                                    return Err(Error::Other(format!(
                                        "{} must be from {} to {} (use --force)",
                                        v, a, b
                                    )));
                                }
                            }
                            Err(e) if !force => {
                                return Err(Error::Other(format!("{} (use --force)", e)))
                            }
                            _ => {}
                        }
                        Some(v)
                    }
                };
                let tb = crate::pcg::character::TempBonus {
                    kind: o.kind.to_string(),
                    key: o.obj.key.clone(),
                    target: target.clone(),
                    active: true,
                    bonuses: o.applied(target != "PC", choice),
                };
                drop(c);
                let mut all = self.ch.temp_bonuses();
                if all.iter().any(|t| {
                    t.kind == tb.kind
                        && t.key.eq_ignore_ascii_case(&tb.key)
                        && t.target.eq_ignore_ascii_case(&tb.target)
                }) {
                    return Err(Error::Other(format!(
                        "{} is already applied to {}",
                        tb.key, tb.target
                    )));
                }
                if tb.bonuses.is_empty() {
                    return Err(Error::Other(format!(
                        "{} has no bonus PCGen would keep",
                        tb.key
                    )));
                }
                let msg = format!("applied {} to {}", tb.key, tb.target);
                all.push(tb);
                self.checkpoint();
                self.ch.set_temp_bonuses(&all);
                changed(self, msg)
            }
            "rm" | "remove" | "on" | "off" => {
                let (kind, key) = source(
                    args.first()
                        .ok_or_else(|| Error::Other("missing source".into()))?,
                );
                let target = args.get(1).map(|t| {
                    if on_pc(t) {
                        "PC".to_string()
                    } else {
                        self.owned_item(t)
                    }
                });
                let mut all = self.ch.temp_bonuses();
                let hits: Vec<usize> = (0..all.len())
                    .filter(|&i| {
                        let t = &all[i];
                        kind.is_none_or(|k| k == t.kind)
                            && t.key.eq_ignore_ascii_case(&key)
                            && target
                                .as_ref()
                                .is_none_or(|x| t.target.eq_ignore_ascii_case(x))
                    })
                    .collect();
                let i = match hits.as_slice() {
                    [i] => *i,
                    [] => {
                        return Err(Error::Other(format!(
                            "no temporary bonus from '{}'{} is applied",
                            key,
                            target.map(|t| format!(" to {}", t)).unwrap_or_default()
                        )))
                    }
                    _ => {
                        return Err(Error::Other(format!(
                            "'{}' is applied to more than one target; name it",
                            key
                        )))
                    }
                };
                let msg = match sub {
                    "on" | "off" => {
                        all[i].active = sub == "on";
                        format!("{} on {} switched {}", all[i].key, all[i].target, sub)
                    }
                    _ => {
                        let t = all.remove(i);
                        format!("removed {} from {}", t.key, t.target)
                    }
                };
                self.checkpoint();
                self.ch.set_temp_bonuses(&all);
                changed(self, msg)
            }
            other => Err(Error::Other(format!("unknown temp command '{}'", other))),
        }
    }

    fn cmd_spellbook(
        &mut self,
        w: &[&str],
        changed: impl Fn(&Session, String) -> Result<Outcome>,
    ) -> Result<Outcome> {
        let name = || {
            w.get(2)
                .copied()
                .ok_or_else(|| Error::Other("missing spell list name".into()))
        };
        let fixed = |n: &str| {
            n.eq_ignore_ascii_case(spells::KNOWN_BOOK)
                || n.eq_ignore_ascii_case(spells::INNATE_BOOK)
        };
        match w.get(1).copied().unwrap_or("") {
            "ls" => {
                let mut c = self.calc();
                c.run();
                let sp = spells::Spells::new(&c);
                let types = self.ch.spell_books();
                let out: Vec<String> = sp
                    .books
                    .iter()
                    .map(|b| {
                        let item = sp.item_books.iter().find(|(n, _)| n == b).map(|x| x.1);
                        let kind = match types.iter().find(|(n, _)| n == b).map(|(_, t)| *t) {
                            _ if b == spells::KNOWN_BOOK => 1,
                            _ if b == spells::INNATE_BOOK => 4,
                            Some(t) => t,
                            None if item.is_some() => 3,
                            None => 2,
                        };
                        let kind = match kind {
                            1 => "known spells",
                            3 => "spellbook",
                            4 => "innate",
                            _ => "prepared list",
                        };
                        let count: i64 = sp
                            .by_caster
                            .iter()
                            .flatten()
                            .flat_map(|cs| cs.infos.iter())
                            .filter(|i| i.book == *b)
                            .map(|i| i.times.max(1))
                            .sum();
                        let pages = item.map_or(String::new(), |o| {
                            format!(
                                "\t{}/{} pages",
                                self.ch.spell_book_pages(b),
                                spells::book_pages(o)
                            )
                        });
                        format!("{}\t{}\t{}{}", b, kind, count, pages)
                    })
                    .collect();
                Ok(Outcome::Text(out.join("\n")))
            }
            "add" => {
                let name = name()?;
                let taken = fixed(name)
                    || self
                        .ch
                        .spell_books()
                        .iter()
                        .any(|(b, _)| b.eq_ignore_ascii_case(name));
                if taken {
                    return Err(Error::Other(format!("there is already a list {}", name)));
                }
                self.check_book_name(name)?;
                self.checkpoint();
                self.ch.add_spell_book(name, 2);
                changed(self, format!("added spell list {}", name))
            }
            "rm" | "remove" => {
                let name = name()?;
                if fixed(name) {
                    return Err(Error::Other(format!("{} cannot be removed", name)));
                }
                if self.item_book(name).is_some() {
                    return Err(Error::Other(format!(
                        "{} is an item the character has; it stays a book until discarded",
                        name
                    )));
                }
                self.checkpoint();
                if !self.ch.remove_spell_book(name) {
                    self.undo.pop();
                    return Err(Error::Other(format!("no spell book named '{}'", name)));
                }
                changed(self, format!("removed spell list {}", name))
            }
            other => Err(Error::Other(format!(
                "unknown spellbook command '{}'",
                other
            ))),
        }
    }

    /// `spell prep` and `spell unprep`.
    fn cmd_prep(
        &mut self,
        w: &[&str],
        force: bool,
        changed: impl Fn(&Session, String) -> Result<Outcome>,
    ) -> Result<Outcome> {
        // options with a value, then the positional words
        let mut pos: Vec<&str> = Vec::new();
        let mut meta: Option<Vec<String>> = None;
        let mut level: Option<i64> = None;
        let mut domain: Option<String> = None;
        let mut it = w.iter().copied();
        while let Some(x) = it.next() {
            let mut val = || {
                it.next()
                    .ok_or_else(|| Error::Other(format!("{} needs a value", x)))
            };
            match x {
                "--meta" => {
                    let v = val()?;
                    meta.get_or_insert_with(Vec::new)
                        .extend(v.split(',').map(|f| f.trim().to_string()));
                }
                "--level" => {
                    let v = val()?;
                    level = Some(
                        v.parse()
                            .map_err(|_| Error::Other(format!("bad spell level '{}'", v)))?,
                    );
                }
                "--domain" => domain = Some(val()?.to_string()),
                o if o.starts_with("--") => {
                    return Err(Error::Other(format!("unknown option {}", o)));
                }
                _ => pos.push(x),
            }
        }
        let arg = |i: usize| {
            pos.get(i)
                .copied()
                .ok_or_else(|| Error::Other("missing argument (try 'help')".into()))
        };
        let cls = self.find(FileKind::Class, arg(2)?)?.key.clone();
        if !self
            .ch
            .classes()
            .iter()
            .any(|c| c.name.eq_ignore_ascii_case(&cls))
        {
            return Err(Error::Other(format!("the character has no {} levels", cls)));
        }
        let spell = self.find(FileKind::Spell, arg(3)?)?.key.clone();
        let book = pos
            .get(4)
            .copied()
            .unwrap_or(spells::PREPARED_BOOK)
            .to_string();
        let feats: Vec<String> = meta
            .iter()
            .flatten()
            .map(|f| {
                self.find_in(FileKind::Ability, Some("FEAT"), f)
                    .map(|o| o.key.clone())
            })
            .collect::<Result<_>>()?;

        if arg(1)? == "unprep" {
            self.checkpoint();
            let only = meta.is_some().then_some(feats.as_slice());
            if !self.ch.remove_spell_copy(&spell, &cls, Some(&book), only) {
                self.undo.pop();
                return Err(Error::Other(format!(
                    "{} is not prepared in {}",
                    spell, book
                )));
            }
            return changed(self, format!("unprepared {} from {}", spell, book));
        }

        if book.eq_ignore_ascii_case(spells::KNOWN_BOOK)
            || book.eq_ignore_ascii_case(spells::INNATE_BOOK)
        {
            return Err(Error::Other(format!(
                "spells are not prepared in {}; use 'spell add'",
                book
            )));
        }
        let books = self.ch.spell_books();
        if self.item_book(&book).is_some() || books.iter().any(|(b, t)| *b == book && *t == 3) {
            return Err(Error::Other(format!(
                "{} is a spellbook; use 'spell scribe'",
                book
            )));
        }
        if !books.iter().any(|(b, _)| *b == book) {
            self.check_book_name(&book)?;
        }
        let (copy, warn) = {
            let mut c = self.calc();
            c.run();
            let ds = &self.ctx.dataset;
            let cls_obj = ds.get(FileKind::Class, &cls).unwrap();
            let sp_obj = ds.get(FileKind::Spell, &spell).unwrap();
            if cls_obj
                .get("MEMORIZE")
                .is_some_and(|v| v.eq_ignore_ascii_case("NO"))
            {
                return Err(Error::Other(format!(
                    "{} can only add to {}",
                    cls,
                    spells::KNOWN_BOOK
                )));
            }
            let sps = spells::Spells::new(&c);
            let Some(ci) = sps.caster_index(cls_obj) else {
                return Err(Error::Other(format!("{} casts no spells", cls)));
            };
            // the list it is cast from: a domain's if asked for, else
            // the class's, else one of the class's domains'
            let class_domains: Vec<&Object> = self
                .ch
                .class_domains_of(&cls)
                .iter()
                .filter_map(|d| ds.get(FileKind::Domain, d))
                .collect();
            let on_domain = |d: &Object| sps.levels_on('D', &d.key, sp_obj);
            let (owner_domain, levels): (Option<&Object>, Vec<i64>) = match &domain {
                Some(d) => {
                    let d = self.find(FileKind::Domain, d)?;
                    (Some(d), on_domain(d))
                }
                None => {
                    let own: Vec<i64> = sps
                        .class_lists(&c, cls_obj)
                        .iter()
                        .flat_map(|l| sps.levels_on('C', l, sp_obj))
                        .collect();
                    match class_domains.iter().find(|d| !on_domain(d).is_empty()) {
                        Some(d) if own.is_empty() => (Some(*d), on_domain(d)),
                        _ => (None, own),
                    }
                }
            };
            let Some(base) = level.or_else(|| levels.iter().copied().min()) else {
                return Err(Error::Other(match owner_domain {
                    Some(d) => format!("{} is not on the {} domain's list", spell, d.key),
                    None => format!("{} is not on the {} list", spell, cls),
                }));
            };
            let owner = match owner_domain {
                Some(d) => spells::Owner::Domain(d, Some(cls_obj.key.clone())),
                None => spells::Owner::Class(cls_obj),
            };
            let known = sps.by_caster[ci].iter().any(|cs| {
                std::ptr::eq(cs.spell, sp_obj)
                    && cs.owner == owner
                    && cs.infos.iter().any(|i| i.book == spells::KNOWN_BOOK)
            });
            let mut warn = String::new();
            let mut fail = |msg: String| -> Result<()> {
                if force {
                    warn.push_str(&format!("warning: {}\n", msg));
                    Ok(())
                } else {
                    Err(Error::Other(format!("{} (use --force to override)", msg)))
                }
            };
            if !known {
                fail(format!("{} does not know {}", cls, spell))?;
            }
            let probe = spells::CharSpell {
                spell: sp_obj,
                owner: owner.clone(),
                fixed_cl: None,
                infos: Vec::new(),
            };
            let specialty = spells::is_specialty(&c, &probe);
            if !specialty && spells::prohibited(&c, cls_obj, sp_obj) {
                fail(format!("{} is prohibited", spell))?;
            }
            let fails = c.failing_prereqs(sp_obj);
            if !fails.is_empty() {
                fail(format!("does not qualify: {}", fails.join(", ")))?;
            }
            for f in &feats {
                let o = ds.get_in(FileKind::Ability, "FEAT", f).unwrap();
                if !o.is_type("Metamagic") {
                    fail(format!("{} is not a metamagic feat", f))?;
                } else if !c.has_ability(Some("FEAT"), f) {
                    fail(format!("the character does not have {}", f))?;
                }
            }
            let adjusted = base + spells::metamagic_levels(&c, &feats);
            if let Err(e) =
                spells::check_prepared_slot(&c, &sps, cls_obj, &book, adjusted, specialty)
            {
                fail(e)?;
            }
            (
                SpellCopy {
                    spell: spell.clone(),
                    class: cls.clone(),
                    book: book.clone(),
                    level: adjusted,
                    feats: feats.clone(),
                    domain: owner_domain.map(|d| d.key.clone()),
                    pages: 0,
                },
                warn,
            )
        };
        self.checkpoint();
        let times = self.ch.add_spell_copy(&copy, 1);
        changed(
            self,
            format!(
                "{}prepared {} (level {}) in {}, {} in all",
                warn, spell, copy.level, book, times
            ),
        )
    }

    /// The item a spellbook of the character is.
    fn item_book(&self, name: &str) -> Option<&Object> {
        let c = self.calc();
        let sp = spells::Spells::new(&c);
        sp.item_books
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, o)| *o)
    }

    /// Remove the books of a spellbook item (`Name`, `Name #2`, ...)
    /// past the `left` copies still had, and the spells written in
    /// them, as PCGen does when copies are given up.
    fn drop_item_books(&mut self, item: &str, left: f64) {
        let left = left.trunc().max(0.0);
        let copy = |book: &str| -> Option<f64> {
            if book == item {
                return Some(1.0);
            }
            book.strip_prefix(item)?
                .strip_prefix(" #")?
                .parse::<u32>()
                .ok()
                .map(f64::from)
        };
        let mut books: Vec<String> = self
            .ch
            .spell_books()
            .into_iter()
            .map(|(b, _)| b)
            .chain(self.ch.spells().into_iter().map(|s| s.2))
            .filter(|b| copy(b).is_some_and(|k| k > left))
            .collect();
        books.sort();
        books.dedup();
        for b in books {
            self.ch.remove_spell_book(&b);
        }
    }

    /// `spell scribe` and `spell unscribe`.
    fn cmd_scribe(
        &mut self,
        w: &[&str],
        force: bool,
        changed: impl Fn(&Session, String) -> Result<Outcome>,
    ) -> Result<Outcome> {
        let mut pos: Vec<&str> = Vec::new();
        let mut level: Option<i64> = None;
        let mut it = w.iter().copied();
        while let Some(x) = it.next() {
            match x {
                "--level" => {
                    let v = it
                        .next()
                        .ok_or_else(|| Error::Other("--level needs a value".into()))?;
                    level = Some(
                        v.parse()
                            .map_err(|_| Error::Other(format!("bad spell level '{}'", v)))?,
                    );
                }
                o if o.starts_with("--") => {
                    return Err(Error::Other(format!("unknown option {}", o)));
                }
                _ => pos.push(x),
            }
        }
        let arg = |i: usize| {
            pos.get(i)
                .copied()
                .ok_or_else(|| Error::Other("missing argument (try 'help')".into()))
        };
        let cls = self.find(FileKind::Class, arg(2)?)?.key.clone();
        if !self
            .ch
            .classes()
            .iter()
            .any(|c| c.name.eq_ignore_ascii_case(&cls))
        {
            return Err(Error::Other(format!("the character has no {} levels", cls)));
        }
        let spell = self.find(FileKind::Spell, arg(3)?)?.key.clone();
        let book = arg(4)?.to_string();
        let Some(item) = self.item_book(&book) else {
            return Err(Error::Other(format!(
                "the character has no spellbook named {}",
                book
            )));
        };
        let capacity = spells::book_pages(item);
        if arg(1)? == "unscribe" {
            self.checkpoint();
            if !self.ch.remove_spell_copy(&spell, &cls, Some(&book), None) {
                self.undo.pop();
                return Err(Error::Other(format!("{} is not in {}", spell, book)));
            }
            return changed(self, format!("removed a copy of {} from {}", spell, book));
        }
        let (copy, warn) = {
            let mut c = self.calc();
            c.run();
            let ds = &self.ctx.dataset;
            let cls_obj = ds.get(FileKind::Class, &cls).unwrap();
            let sp_obj = ds.get(FileKind::Spell, &spell).unwrap();
            let sps = spells::Spells::new(&c);
            let base = level.or_else(|| {
                sps.class_lists(&c, cls_obj)
                    .iter()
                    .flat_map(|l| sps.levels_on('C', l, sp_obj))
                    .min()
            });
            let Some(base) = base else {
                return Err(Error::Other(format!(
                    "{} is not on the {} list; give a level",
                    spell, cls
                )));
            };
            let known = sps.caster_index(cls_obj).is_some_and(|ci| {
                sps.by_caster[ci].iter().any(|cs| {
                    std::ptr::eq(cs.spell, sp_obj)
                        && cs.owner == spells::Owner::Class(cls_obj)
                        && cs.infos.iter().any(|i| i.book == spells::KNOWN_BOOK)
                })
            });
            let mut warn = String::new();
            let mut fail = |msg: String| -> Result<()> {
                if force {
                    warn.push_str(&format!("warning: {}\n", msg));
                    Ok(())
                } else {
                    Err(Error::Other(format!("{} (use --force to override)", msg)))
                }
            };
            if !known {
                fail(format!("{} does not know {}", cls, spell))?;
            }
            let pages = spells::page_cost(&c, item, base);
            let used = self.ch.spell_book_pages(&book);
            if used + pages > capacity {
                fail(format!(
                    "not enough pages left in {}: {} of {} used, {} needed",
                    book, used, capacity, pages
                ))?;
            }
            (
                SpellCopy {
                    spell: spell.clone(),
                    class: cls.clone(),
                    book: book.clone(),
                    level: base,
                    feats: Vec::new(),
                    domain: None,
                    pages,
                },
                warn,
            )
        };
        self.checkpoint();
        // recorded before its spells, so PCGen keeps it a spellbook
        if self.ch.spell_books().iter().all(|(b, _)| *b != book) {
            self.ch.add_spell_book(&book, 3);
        }
        let times = self.ch.add_spell_copy(&copy, 1);
        changed(
            self,
            format!(
                "{}scribed {} (level {}, {} pages) in {}, {} in all",
                warn, spell, copy.level, copy.pages, book, times
            ),
        )
    }
}

/// Default equipment-set location for an item, by its type: weapons
/// in the primary hand, other items (shields too, though they are also
/// weapons) in the first of the game mode's body slots (EQSLOT) whose
/// CONTAINS lists one of the item's types.
fn default_location(obj: Option<&Object>, gm: Option<&GameMode>) -> String {
    let Some(o) = obj else {
        return "Carried".into();
    };
    if o.is_type("Weapon") && !o.is_type("Shield") {
        return "Primary Hand".into();
    }
    if let Some(g) = gm {
        for l in g.lines("equipmentslots.lst", "EQSLOT") {
            let holds = l
                .tokens
                .iter()
                .filter(|(t, _)| t == "CONTAINS")
                .flat_map(|(_, v)| v.split('=').next().unwrap_or("").split(','))
                .any(|t| o.is_type(t.trim()));
            if holds {
                return l.value.clone();
            }
        }
    }
    // without slots in the game mode
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
        assert_eq!(default_location(None, None), "Carried");
    }
}
