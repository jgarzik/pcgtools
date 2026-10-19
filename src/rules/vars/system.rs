//
// rules/vars/system.rs -- declared variables, modifiers and solving
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! The declared-variable system.  Variables are declared in VARIABLE
//! files with a scope and a format; MODIFY and MODIFYOTHER tokens on
//! active objects (and GLOBALMODIFIER files) change them.  A variable
//! in a local scope has one value per object of that scope (each stat,
//! skill, equipped item, ...).  A variable's value starts at its
//! format's default and passes through its modifiers in priority order.

use super::value::{arith, Format, Value};
use crate::dataset::{Dataset, Object};
use crate::error::{Diagnostic, Diagnostics};
use crate::gamemode::GameMode;
use crate::lst::kind::FileKind;
use crate::rules::calc::{Calc, SrcKind};
use crate::token::formula::{BinOp, Expr, UnOp};
use crate::token::split::split_top;
use serde::Serialize;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};

/// The global scope.
pub const PC: &str = "PC";

fn lower(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// The scope a scope draws on (`PC.EQUIPMENT.PART` -> `PC.EQUIPMENT`,
/// `PC.STAT` -> `PC`).
pub fn parent_scope(scope: &str) -> Option<&str> {
    scope.rfind('.').map(|i| &scope[..i])
}

/// A variable declaration.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Decl {
    pub scope: String,
    pub name: String,
    pub format: Format,
    /// declared with CHANNEL: stored as `CHANNEL*name`
    pub channel: bool,
}

/// Parse a declaration value: `[FORMAT=]Name`.
fn parse_fmt_name(s: &str) -> Option<(Format, String)> {
    match s.split_once('=') {
        Some((f, n)) => Some((Format::parse(f)?, n.trim().to_string())),
        None => Some((Format::Number, s.trim().to_string())),
    }
}

/// Is this a legal variable name?
pub fn legal_name(n: &str) -> bool {
    let mut c = n.chars();
    c.next().is_some_and(|x| x.is_ascii_alphabetic())
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

/// Parse one VARIABLE-file declaration head (`GLOBAL:...`, `LOCAL:...`,
/// `CHANNEL:...`, or a bare name meaning GLOBAL).
pub fn parse_decl(tag: Option<&str>, value: &str) -> Result<Decl, String> {
    let tag = tag.unwrap_or("GLOBAL");
    let (scope, rest, channel) = match tag {
        "GLOBAL" => (PC.to_string(), value, false),
        "LOCAL" | "CHANNEL" => {
            let (s, r) = value
                .split_once('|')
                .ok_or_else(|| format!("{} needs Scope|Name: {}", tag, value))?;
            (s.trim().to_string(), r, tag == "CHANNEL")
        }
        other => return Err(format!("unknown declaration {}", other)),
    };
    let (format, name) =
        parse_fmt_name(rest).ok_or_else(|| format!("bad variable format in '{}'", rest))?;
    if !legal_name(&name) {
        return Err(format!("illegal variable name '{}'", name));
    }
    let name = if channel {
        format!("CHANNEL*{}", name)
    } else {
        name
    };
    Ok(Decl {
        scope,
        name,
        format,
        channel,
    })
}

/// A modifier operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Op {
    Set,
    Multiply,
    Divide,
    Add,
    Max,
    Min,
}

impl Op {
    pub fn parse(s: &str) -> Option<Op> {
        Some(match s.to_ascii_uppercase().as_str() {
            "SET" => Op::Set,
            "MULTIPLY" => Op::Multiply,
            "DIVIDE" => Op::Divide,
            "ADD" => Op::Add,
            "MAX" => Op::Max,
            "MIN" => Op::Min,
            _ => return None,
        })
    }

    /// Order among modifiers of equal user priority.
    pub fn inherent(self) -> i64 {
        match self {
            Op::Set => 0,
            Op::Multiply => 1,
            Op::Divide => 2,
            Op::Add => 3,
            Op::Max => 4,
            Op::Min => 5,
        }
    }
}

/// A scope instance: the global scope, or one object of a local scope.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct Owner {
    pub scope: String,
    /// object key; empty for the global scope
    pub key: String,
}

impl Owner {
    pub fn global() -> Owner {
        Owner {
            scope: PC.into(),
            key: String::new(),
        }
    }

    pub fn new(scope: &str, key: &str) -> Owner {
        Owner {
            scope: scope.into(),
            key: key.into(),
        }
    }

    /// The instance this one draws on.
    pub fn parent(&self) -> Option<Owner> {
        let p = parent_scope(&self.scope)?;
        Some(if p == PC {
            Owner::global()
        } else {
            Owner::new(p, &self.key)
        })
    }
}

/// A modifier attached to a variable instance.
#[derive(Debug, Clone, Serialize)]
pub struct Modifier {
    pub op: Op,
    pub value: String,
    pub priority: i64,
    pub order: usize,
    /// the object carrying the modifier (formulas evaluate in its scope)
    pub source: Owner,
    /// description for explanations
    pub origin: String,
}

impl Modifier {
    fn sort_key(&self) -> (i64, i64, usize) {
        (self.priority, self.op.inherent(), self.order)
    }
}

/// A data-defined function.
#[derive(Debug, Clone)]
struct UserFunction {
    body: Expr,
}

/// Game mode code controls (`codeControl.lst`): names of variables
/// that take over core calculations.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Controls {
    pub map: BTreeMap<String, String>,
}

impl Controls {
    pub fn from_gamemode(gm: Option<&GameMode>) -> Controls {
        let mut c = Controls::default();
        if let Some(g) = gm {
            if let Some(f) = g.file("codeControl.lst") {
                for l in &f.lines {
                    c.map.insert(l.tag.to_ascii_uppercase(), l.value.clone());
                }
            }
        }
        c
    }

    pub fn get(&self, control: &str) -> Option<&str> {
        self.map
            .get(&control.to_ascii_uppercase())
            .map(String::as_str)
    }

    pub fn enabled(&self, feature: &str) -> bool {
        self.get(feature)
            .is_some_and(|v| v.eq_ignore_ascii_case("YES"))
    }
}

/// Default values of channels and variables (DEFAULTVARIABLEVALUE) from
/// the data's data controls, or from the game modes' compatibility
/// defaults when the data declares none; keys are upper case.
pub fn data_defaults(ds: &Dataset, gm: Option<&GameMode>) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for o in ds.objects(FileKind::DataControl) {
        if o.head_tag.as_deref() == Some("DEFAULTVARIABLEVALUE") {
            if let Some((f, v)) = o.name.split_once('|') {
                out.insert(f.to_ascii_uppercase(), v.to_string());
            }
        }
    }
    if out.is_empty() {
        if let Some(path) = gm
            .map(|g| {
                g.dir
                    .join("..")
                    .join("default")
                    .join("compatibilityDataControl.lst")
            })
            .filter(|p| p.is_file())
        {
            if let Ok(tf) = crate::text::TextFile::read(&path) {
                for l in &tf.lines {
                    if let Some(rest) = l.text.trim().strip_prefix("DEFAULTVARIABLEVALUE:") {
                        if let Some((f, v)) = rest.split('\t').next().unwrap_or("").split_once('|')
                        {
                            out.insert(f.to_ascii_uppercase(), v.to_string());
                        }
                    }
                }
            }
        }
    }
    out
}

/// The variable system for one character.
pub struct VarSystem<'a> {
    pub ds: &'a Dataset,
    pub decls: HashMap<(String, String), Decl>,
    pub controls: Controls,
    /// modifiers per instance and variable (lower case)
    pub modifiers: HashMap<(Owner, String), Vec<Modifier>>,
    /// all instances of each local scope
    pub instances: BTreeMap<String, Vec<Owner>>,
    /// keys of the objects behind instances, by owner (for this())
    objects: HashMap<Owner, &'a Object>,
    defaults: HashMap<String, String>,
    functions: HashMap<String, UserFunction>,
    channels: HashMap<(Owner, String), Value>,
    cache: RefCell<HashMap<(Owner, String), Value>>,
    busy: RefCell<HashSet<(Owner, String)>>,
    pub diags: RefCell<Diagnostics>,
    unknown: RefCell<BTreeMap<String, usize>>,
}

/// The scope objects of a kind live in.
pub fn scope_of(kind: FileKind, obj: &Object) -> String {
    match kind {
        FileKind::Stat => "PC.STAT".into(),
        FileKind::Save => "PC.SAVE".into(),
        FileKind::Skill => "PC.SKILL".into(),
        FileKind::Size => "PC.SIZE".into(),
        FileKind::Equipment => "PC.EQUIPMENT".into(),
        FileKind::EquipMod => "PC.EQUIPMENT.PART".into(),
        FileKind::Dynamic => format!(
            "PC.{}",
            obj.head_tag
                .clone()
                .unwrap_or_default()
                .to_ascii_uppercase()
        ),
        _ => PC.into(),
    }
}

impl<'a> VarSystem<'a> {
    /// Build the system from the dataset, game mode and an evaluated
    /// calculation (which knows the active objects).
    pub fn new(calc: &Calc<'a>) -> VarSystem<'a> {
        let ds = calc.ds;
        let gm = calc.gm;
        let mut sys = VarSystem {
            ds,
            decls: HashMap::new(),
            controls: Controls::from_gamemode(gm),
            modifiers: HashMap::new(),
            instances: BTreeMap::new(),
            objects: HashMap::new(),
            defaults: HashMap::new(),
            functions: HashMap::new(),
            channels: HashMap::new(),
            cache: RefCell::new(HashMap::new()),
            busy: RefCell::new(HashSet::new()),
            diags: RefCell::new(Diagnostics::default()),
            unknown: RefCell::new(BTreeMap::new()),
        };
        sys.load_declarations(gm);
        sys.load_data_controls(gm);
        sys.load_modifiers(calc);
        sys.load_channels(calc);
        sys
    }

    fn diag(&self, d: Diagnostic) {
        self.diags.borrow_mut().push(d);
    }

    fn declare(&mut self, d: Decl) {
        let key = (lower(&d.scope), lower(&d.name));
        if let Some(old) = self.decls.get(&key) {
            if old.format != d.format {
                self.diag(Diagnostic::error(format!(
                    "variable {} in {} declared as {} and {}",
                    d.name, d.scope, old.format, d.format
                )));
            }
            return;
        }
        // the same name in a scope and the scope it draws on conflicts
        let mut s = parent_scope(&d.scope);
        while let Some(p) = s {
            if self.decls.contains_key(&(lower(p), lower(&d.name))) {
                self.diag(Diagnostic::error(format!(
                    "variable {} in {} hides the one in {}",
                    d.name, d.scope, p
                )));
            }
            s = parent_scope(p);
        }
        self.decls.insert(key, d);
    }

    fn load_declarations(&mut self, gm: Option<&GameMode>) {
        for o in self.ds.objects(FileKind::Variable) {
            match parse_decl(o.head_tag.as_deref(), &o.name) {
                Ok(d) => self.declare(d),
                Err(e) => self.diag(Diagnostic::error(e)),
            }
        }
        // built-in variables behind code controls
        if let Some(face) = self.controls.get("FACE").map(str::to_string) {
            self.declare(Decl {
                scope: PC.into(),
                name: face,
                format: Format::OrderedPair,
                channel: false,
            });
        }
        let _ = gm;
    }

    fn load_data_controls(&mut self, gm: Option<&GameMode>) {
        let mut seen_default = false;
        for o in self.ds.objects(FileKind::DataControl) {
            match o.head_tag.as_deref() {
                Some("DEFAULTVARIABLEVALUE") => {
                    if let Some((f, v)) = o.name.split_once('|') {
                        self.defaults.insert(f.to_ascii_uppercase(), v.to_string());
                        seen_default = true;
                    }
                }
                Some("FUNCTION") => {
                    let Some(v) = o.get("VALUE") else { continue };
                    match Expr::parse(v) {
                        Ok(body) => {
                            self.functions.insert(lower(&o.name), UserFunction { body });
                        }
                        Err(e) => {
                            self.diag(Diagnostic::error(format!("function {}: {}", o.name, e)))
                        }
                    }
                }
                _ => {}
            }
        }
        // without data controls, the game mode's compatibility defaults
        if !seen_default {
            if let Some(path) = gm
                .map(|g| {
                    g.dir
                        .join("..")
                        .join("default")
                        .join("compatibilityDataControl.lst")
                })
                .filter(|p| p.is_file())
            {
                if let Ok(tf) = crate::text::TextFile::read(&path) {
                    for l in &tf.lines {
                        if let Some(rest) = l.text.trim().strip_prefix("DEFAULTVARIABLEVALUE:") {
                            if let Some((f, v)) =
                                rest.split('\t').next().unwrap_or("").split_once('|')
                            {
                                self.defaults.insert(f.to_ascii_uppercase(), v.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    /// Register an active object as an instance of its scope.
    fn add_instance(&mut self, scope: &str, key: &str, obj: &'a Object) -> Owner {
        let o = if scope == PC {
            Owner::global()
        } else {
            Owner::new(scope, key)
        };
        if scope != PC {
            let list = self.instances.entry(scope.to_string()).or_default();
            if !list.contains(&o) {
                list.push(o.clone());
            }
        }
        self.objects.entry(o.clone()).or_insert(obj);
        o
    }

    fn load_modifiers(&mut self, calc: &Calc<'a>) {
        let mut order = 0usize;
        // (source owner, object, tokens)
        let mut carriers: Vec<(Owner, &'a Object, String)> = Vec::new();

        // all stats and saves exist on every character
        for kind in [FileKind::Stat, FileKind::Save] {
            for o in self.ds.objects(kind) {
                let own = self.add_instance(&scope_of(kind, o), &o.key, o);
                carriers.push((own, o, format!("{} {}", kind, o.key)));
            }
        }
        for s in &calc.sources {
            if matches!(s.kind, SrcKind::Stat | SrcKind::Save) {
                continue;
            }
            let kind = s.obj.kind;
            let key = match s.item {
                Some(i) => calc.items()[i].name.clone(),
                None => s.obj.key.clone(),
            };
            let own = self.add_instance(&scope_of(kind, s.obj), &key, s.obj);
            carriers.push((own, s.obj, format!("{:?} {}", s.kind, s.obj.key)));
        }
        // dynamic objects granted by active objects
        let mut grants = Vec::new();
        for (_, o, _) in &carriers {
            for v in o.values("GRANT") {
                if let Some((cat, keys)) = v.split_once('|') {
                    for k in keys.split('|') {
                        grants.push((cat.to_ascii_uppercase(), k.to_string()));
                    }
                }
            }
        }
        for (cat, k) in grants {
            if let Some(d) = self.ds.objects(FileKind::Dynamic).iter().find(|d| {
                d.head_tag
                    .as_deref()
                    .is_some_and(|t| t.eq_ignore_ascii_case(&cat))
                    && d.name.eq_ignore_ascii_case(&k)
            }) {
                let own = self.add_instance(&format!("PC.{}", cat), &d.name, d);
                carriers.push((own, d, format!("Dynamic {}", d.name)));
            }
        }

        // global modifier files
        let globals: Vec<&'a Object> = self.ds.objects(FileKind::GlobalModifier).iter().collect();
        for g in globals {
            let text = format!("{}:{}", g.head_tag.clone().unwrap_or_default(), g.name);
            self.add_modifier_token(&Owner::global(), &text, "GlobalModifier", &mut order);
        }

        for (own, o, origin) in carriers {
            for t in &o.tokens {
                if t.tag == "MODIFY" || t.tag == "MODIFYOTHER" {
                    self.add_modifier_token(&own, &t.text(), &origin, &mut order);
                }
            }
        }
    }

    /// Resolve the instance a variable named from `from` lives in.
    fn resolve_owner(&self, from: &Owner, var: &str) -> Option<(Owner, Decl)> {
        let mut cur = Some(from.clone());
        while let Some(o) = cur {
            if let Some(d) = self.decls.get(&(lower(&o.scope), lower(var))) {
                return Some((o, d.clone()));
            }
            cur = o.parent();
        }
        None
    }

    fn add_modifier_token(&mut self, src: &Owner, token: &str, origin: &str, order: &mut usize) {
        let (tag, value) = token.split_once(':').unwrap_or((token, ""));
        let parts: Vec<&str> = split_top(value, '|');
        let (targets, rest): (Vec<Owner>, &[&str]) = match tag {
            "MODIFY" => (vec![src.clone()], &parts[..]),
            "MODIFYOTHER" => {
                if parts.len() < 2 {
                    self.diag(Diagnostic::error(format!("bad MODIFYOTHER: {}", value)));
                    return;
                }
                let scope = parts[0].trim();
                let scope = if scope.starts_with("PC.") || scope == PC {
                    scope.to_string()
                } else {
                    format!("PC.{}", scope.to_ascii_uppercase())
                };
                let group = parts[1];
                let insts = self.instances.get(&scope).cloned().unwrap_or_default();
                let hit: Vec<Owner> = insts
                    .into_iter()
                    .filter(|o| self.grouping_matches(group, o))
                    .collect();
                (hit, &parts[2..])
            }
            _ => return,
        };
        if rest.len() < 3 {
            self.diag(Diagnostic::error(format!("bad {}: {}", tag, value)));
            return;
        }
        let var = rest[0].trim();
        let Some(op) = Op::parse(rest[1].trim()) else {
            self.diag(Diagnostic::error(format!(
                "unknown modifier operation '{}'",
                rest[1]
            )));
            return;
        };
        let mut priority = 0i64;
        for extra in &rest[3..] {
            if let Some(p) = extra.trim().strip_prefix("PRIORITY=") {
                priority = p.trim().parse().unwrap_or(0);
            }
        }
        for t in targets {
            let Some((owner, _)) = self.resolve_owner(&t, var) else {
                self.diag(Diagnostic::warning(format!(
                    "{}: variable '{}' is not declared for {}",
                    origin, var, t.scope
                )));
                continue;
            };
            *order += 1;
            self.modifiers
                .entry((owner, lower(var)))
                .or_default()
                .push(Modifier {
                    op,
                    value: rest[2].to_string(),
                    priority,
                    order: *order,
                    source: if tag == "MODIFY" {
                        src.clone()
                    } else {
                        t.clone()
                    },
                    origin: origin.to_string(),
                });
        }
    }

    /// Does an instance match a MODIFYOTHER grouping (`ALL`, a key,
    /// `GROUP=x`, possibly with a nested `[...]` part selector)?
    fn grouping_matches(&self, group: &str, o: &Owner) -> bool {
        let g = group.split('[').next().unwrap_or(group).trim();
        if g.eq_ignore_ascii_case("ALL") {
            return true;
        }
        if let Some(name) = g.strip_prefix("GROUP=") {
            return self.objects.get(o).is_some_and(|obj| {
                obj.values("GROUP")
                    .flat_map(|v| v.split('|'))
                    .any(|x| x.eq_ignore_ascii_case(name))
            });
        }
        o.key.eq_ignore_ascii_case(g)
    }

    fn load_channels(&mut self, calc: &Calc<'a>) {
        let ch = calc.ch;
        if let Some(input) = self.controls.get("STATINPUT").map(str::to_string) {
            for (abb, score) in ch.stats() {
                let own = Owner::new("PC.STAT", &abb);
                self.channels.insert(
                    (own, lower(&format!("CHANNEL*{}", input))),
                    Value::Int(score),
                );
            }
        }
        let g = Owner::global();
        let num = |s: Option<String>| s.and_then(|v| v.trim().parse::<f64>().ok());
        let fields: [(&str, Option<Value>); 8] = [
            ("CHANNEL*AGE", num(ch.pcg.value("AGE")).map(Value::number)),
            (
                "CHANNEL*GOLD",
                num(ch.pcg.value("MONEY")).map(Value::number),
            ),
            (
                "CHANNEL*HEIGHT",
                num(ch.pcg.value("HEIGHT")).map(Value::number),
            ),
            (
                "CHANNEL*HAIRCOLOR",
                ch.pcg.value("HAIRCOLOR").map(Value::Str),
            ),
            (
                "CHANNEL*HAIRSTYLE",
                ch.pcg.value("HAIRSTYLE").map(Value::Str),
            ),
            (
                "CHANNEL*SKINCOLOR",
                ch.pcg.value("SKINCOLOR").map(Value::Str),
            ),
            ("CHANNEL*HANDED", ch.pcg.value("HANDED").map(Value::Str)),
            (
                "CHANNEL*ALIGNMENT",
                ch.alignment().map(|a| Value::Obj("ALIGNMENT".into(), a)),
            ),
        ];
        for (name, v) in fields {
            if let Some(v) = v {
                self.channels.insert((g.clone(), lower(name)), v);
            }
        }
        if let Some(d) = ch.deity() {
            self.channels
                .insert((g, lower("CHANNEL*DEITY")), Value::Obj("DEITY".into(), d));
        }
    }

    fn default_value(&self, f: &Format) -> Value {
        match self.defaults.get(&f.to_string()) {
            Some(v) => Value::parse_literal(f, v).unwrap_or_else(|| Value::default_for(f)),
            None => Value::default_for(f),
        }
    }

    /// Value of a variable as seen from an instance (looking through
    /// the scopes it draws on).
    pub fn get(&self, from: &Owner, var: &str) -> Option<Value> {
        let (owner, decl) = self.resolve_owner(from, var)?;
        Some(self.solve(&owner, &decl))
    }

    fn solve(&self, owner: &Owner, decl: &Decl) -> Value {
        let key = (owner.clone(), lower(&decl.name));
        if let Some(v) = self.cache.borrow().get(&key) {
            return v.clone();
        }
        if let Some(v) = self.channels.get(&key) {
            return v.clone();
        }
        if !self.busy.borrow_mut().insert(key.clone()) {
            self.diag(Diagnostic::warning(format!(
                "variable {} depends on itself",
                decl.name
            )));
            return self.default_value(&decl.format);
        }
        let mut v = self.default_value(&decl.format);
        if let Some(mods) = self.modifiers.get(&key) {
            let mut mods: Vec<&Modifier> = mods.iter().collect();
            mods.sort_by_key(|m| m.sort_key());
            for m in mods {
                v = self.apply(m, &decl.format, v);
            }
        }
        self.busy.borrow_mut().remove(&key);
        self.cache.borrow_mut().insert(key, v.clone());
        v
    }

    fn apply(&self, m: &Modifier, fmt: &Format, prev: Value) -> Value {
        let arg = if fmt.is_fixed() && !matches!(fmt, Format::Array(_)) {
            match Value::parse_literal(fmt, &m.value) {
                Some(v) => v,
                None => {
                    self.diag(Diagnostic::error(format!(
                        "{}: '{}' is not a {} value",
                        m.origin, m.value, fmt
                    )));
                    return prev;
                }
            }
        } else {
            match Expr::parse(&m.value) {
                Ok(e) => {
                    let env = VEnv {
                        sys: self,
                        owner: m.source.clone(),
                        input: Some(prev.clone()),
                        format: Some(fmt.clone()),
                        args: Vec::new(),
                    };
                    env.eval(&e)
                }
                Err(_) => match Value::parse_literal(fmt, &m.value) {
                    Some(v) => v,
                    None => {
                        self.diag(Diagnostic::error(format!(
                            "{}: cannot read '{}'",
                            m.origin, m.value
                        )));
                        return prev;
                    }
                },
            }
        };
        match (m.op, fmt) {
            (Op::Set, _) => arg,
            (Op::Add, Format::Array(_)) => {
                let mut a = match prev {
                    Value::Array(a) => a,
                    _ => Vec::new(),
                };
                if let Value::Array(b) = arg {
                    for x in b {
                        if !a.contains(&x) {
                            a.push(x);
                        }
                    }
                }
                Value::Array(a)
            }
            (Op::Add, _) => arith('+', &prev, &arg),
            (Op::Multiply, _) => arith('*', &prev, &arg),
            (Op::Divide, _) => arith('/', &prev, &arg),
            (Op::Max, _) => {
                if arg.as_f64() >= prev.as_f64() {
                    arg
                } else {
                    prev
                }
            }
            (Op::Min, _) => {
                if arg.as_f64() < prev.as_f64() {
                    arg
                } else {
                    prev
                }
            }
        }
    }

    /// Evaluate a formula in an instance's context.
    pub fn eval_in(&self, owner: &Owner, formula: &str) -> Option<Value> {
        let e = Expr::parse(formula).ok()?;
        let env = VEnv {
            sys: self,
            owner: owner.clone(),
            input: None,
            format: None,
            args: Vec::new(),
        };
        Some(env.eval(&e))
    }

    /// All declared variables with their values, per instance.
    pub fn results(&self) -> BTreeMap<String, BTreeMap<String, String>> {
        let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
        let mut decls: Vec<&Decl> = self.decls.values().collect();
        decls.sort_by(|a, b| (&a.scope, &a.name).cmp(&(&b.scope, &b.name)));
        for d in decls {
            let owners: Vec<Owner> = if d.scope == PC {
                vec![Owner::global()]
            } else {
                self.instances.get(&d.scope).cloned().unwrap_or_default()
            };
            for o in owners {
                let label = if o.key.is_empty() {
                    o.scope.clone()
                } else {
                    format!("{}:{}", o.scope, o.key)
                };
                let v = self.solve(&o, d);
                out.entry(label)
                    .or_default()
                    .insert(d.name.clone(), v.to_string());
            }
        }
        out
    }

    /// Names that resolved to nothing during evaluation.
    pub fn unknown(&self) -> BTreeMap<String, usize> {
        self.unknown.borrow().clone()
    }

    /// The object behind an instance.
    pub fn object(&self, o: &Owner) -> Option<&'a Object> {
        self.objects.get(o).copied()
    }
}

/// Formula evaluation context for declared variables.
struct VEnv<'s, 'a> {
    sys: &'s VarSystem<'a>,
    owner: Owner,
    /// value() inside a modifier
    input: Option<Value>,
    /// format the result should have (for string-to-object literals)
    format: Option<Format>,
    /// arguments of a user function (call by name)
    args: Vec<(Expr, Owner)>,
}

impl VEnv<'_, '_> {
    fn sub(&self, format: Option<Format>) -> VEnv<'_, '_> {
        VEnv {
            sys: self.sys,
            owner: self.owner.clone(),
            input: self.input.clone(),
            format,
            args: self.args.clone(),
        }
    }

    fn eval(&self, e: &Expr) -> Value {
        match e {
            Expr::Num(n) => Value::number(*n),
            Expr::Str(s) => match &self.format {
                Some(Format::Object(k)) => Value::Obj(k.clone(), s.clone()),
                _ => Value::Str(s.clone()),
            },
            Expr::Var(v) => match self.sys.get(&self.owner, v) {
                Some(x) => x,
                None => {
                    *self.sys.unknown.borrow_mut().entry(v.clone()).or_default() += 1;
                    Value::Int(0)
                }
            },
            Expr::Tuple(items) => items.last().map(|x| self.eval(x)).unwrap_or(Value::Int(0)),
            Expr::Unary(op, x) => {
                let v = self.sub(None).eval(x);
                match op {
                    UnOp::Neg => arith('-', &Value::Int(0), &v),
                    UnOp::Plus => v,
                    UnOp::Not => Value::Bool(!v.truthy()),
                }
            }
            Expr::Binary(op, l, r) => {
                let plain = self.sub(None);
                let (a, b) = (plain.eval(l), plain.eval(r));
                let cmp = |f: fn(f64, f64) -> bool| Value::Bool(f(a.as_f64(), b.as_f64()));
                match op {
                    BinOp::Add => arith('+', &a, &b),
                    BinOp::Sub => arith('-', &a, &b),
                    BinOp::Mul => arith('*', &a, &b),
                    BinOp::Div => arith('/', &a, &b),
                    BinOp::Rem => arith('%', &a, &b),
                    BinOp::Pow => arith('^', &a, &b),
                    BinOp::Eq => Value::Bool(values_equal(&a, &b)),
                    BinOp::Ne => Value::Bool(!values_equal(&a, &b)),
                    BinOp::Lt => cmp(|x, y| x < y),
                    BinOp::Gt => cmp(|x, y| x > y),
                    BinOp::Le => cmp(|x, y| x <= y),
                    BinOp::Ge => cmp(|x, y| x >= y),
                    BinOp::And => Value::Bool(a.truthy() && b.truthy()),
                    BinOp::Or => Value::Bool(a.truthy() || b.truthy()),
                }
            }
            Expr::Call(name, args) => self.call(name, args),
        }
    }

    fn str_arg(&self, a: Option<&Expr>) -> Option<String> {
        match a? {
            Expr::Str(s) => Some(s.clone()),
            Expr::Var(v) => Some(v.clone()),
            other => Some(self.sub(None).eval(other).to_string()),
        }
    }

    fn call(&self, name: &str, args: &[Expr]) -> Value {
        let n = name.to_ascii_lowercase();
        let num = |i: usize| {
            args.get(i)
                .map(|a| self.sub(None).eval(a))
                .unwrap_or(Value::Int(0))
        };
        match n.as_str() {
            "abs" => match num(0) {
                Value::Int(i) => Value::Int(i.abs()),
                v => Value::Float(v.as_f64().abs()),
            },
            "floor" => Value::Int(num(0).as_f64().floor() as i64),
            "ceil" => Value::Int(num(0).as_f64().ceil() as i64),
            "round" => Value::Int((num(0).as_f64() + 0.5).floor() as i64),
            "max" | "min" => {
                let mut best: Option<Value> = None;
                for a in args {
                    let v = self.sub(None).eval(a);
                    best = Some(match best {
                        None => v,
                        Some(b) => {
                            // ties: max keeps the later, min the earlier
                            let take = if n == "max" {
                                v.as_f64() >= b.as_f64()
                            } else {
                                v.as_f64() < b.as_f64()
                            };
                            if take {
                                v
                            } else {
                                b
                            }
                        }
                    });
                }
                best.unwrap_or(Value::Int(0))
            }
            "if" => {
                if num(0).truthy() {
                    args.get(1).map(|a| self.eval(a)).unwrap_or(Value::Int(0))
                } else {
                    args.get(2).map(|a| self.eval(a)).unwrap_or(Value::Int(0))
                }
            }
            "value" => self.input.clone().unwrap_or(Value::Int(0)),
            "arg" => {
                let i = num(0).as_f64() as usize;
                match self.args.get(i) {
                    Some((e, own)) => VEnv {
                        sys: self.sys,
                        owner: own.clone(),
                        input: None,
                        format: self.format.clone(),
                        args: Vec::new(),
                    }
                    .eval(e),
                    None => Value::Int(0),
                }
            }
            "input" => {
                let ch = self.str_arg(args.first()).unwrap_or_default();
                let key = format!("CHANNEL*{}", ch);
                let mut cur = Some(self.owner.clone());
                while let Some(o) = cur {
                    if let Some(v) = self.sys.channels.get(&(o.clone(), lower(&key))) {
                        return v.clone();
                    }
                    cur = o.parent();
                }
                self.sys.get(&self.owner, &key).unwrap_or(Value::Int(0))
            }
            "isempty" => Value::Bool(matches!(num(0), Value::Array(a) if a.is_empty())),
            "length" => match num(0) {
                Value::Array(a) => Value::Int(a.len() as i64),
                _ => Value::Int(0),
            },
            "slice" => match num(0) {
                Value::Array(a) => {
                    let s = num(1).as_f64().max(0.0) as usize;
                    let e = if args.len() > 2 {
                        num(2).as_f64().max(0.0) as usize
                    } else {
                        a.len()
                    };
                    Value::Array(
                        a.get(s.min(a.len())..e.min(a.len()))
                            .unwrap_or(&[])
                            .to_vec(),
                    )
                }
                _ => Value::Array(Vec::new()),
            },
            "ispresent" => Value::Bool(matches!(num(0), Value::Opt(Some(_)))),
            "getoptional" => match num(0) {
                Value::Opt(Some(v)) => *v,
                _ => Value::Int(0),
            },
            "key" => match num(0) {
                Value::Obj(_, k) => Value::Str(k),
                v => Value::Str(v.to_string()),
            },
            "get" => {
                let fmt = self.str_arg(args.first()).unwrap_or_default();
                let s = self.str_arg(args.get(1)).unwrap_or_default();
                Value::Obj(fmt.to_ascii_uppercase(), s)
            }
            "this" => match self.sys.object(&self.owner) {
                Some(o) => Value::Obj(self.owner.scope.clone(), o.key.clone()),
                None => Value::Int(0),
            },
            "getother" => {
                let scope = self.str_arg(args.first()).unwrap_or_default();
                let key = match args.get(1).map(|a| self.sub(None).eval(a)) {
                    Some(Value::Obj(_, k)) | Some(Value::Str(k)) => k,
                    Some(v) => v.to_string(),
                    None => String::new(),
                };
                let target = Owner::new(&scope.to_ascii_uppercase(), &key);
                let target = self
                    .sys
                    .instances
                    .get(&target.scope)
                    .and_then(|l| l.iter().find(|o| o.key.eq_ignore_ascii_case(&key)).cloned())
                    .unwrap_or(target);
                match args.get(2) {
                    Some(e) => VEnv {
                        sys: self.sys,
                        owner: target,
                        input: None,
                        format: None,
                        args: Vec::new(),
                    }
                    .eval(e),
                    None => Value::Int(0),
                }
            }
            "getfact" => {
                // getFact("FORMAT", obj, "Fact")
                let kind = self.str_arg(args.first()).unwrap_or_default();
                let key = match args.get(1).map(|a| self.sub(None).eval(a)) {
                    Some(Value::Obj(_, k)) | Some(Value::Str(k)) => k,
                    Some(v) => v.to_string(),
                    None => String::new(),
                };
                let fact = self.str_arg(args.get(2)).unwrap_or_default();
                let kind = FileKind::from_name(&kind);
                let obj = kind.and_then(|k| self.sys.ds.get(k, &key));
                obj.and_then(|o| {
                    o.values("FACT").find_map(|v| {
                        let (f, val) = v.split_once('|')?;
                        f.eq_ignore_ascii_case(&fact)
                            .then(|| Value::Str(val.to_string()))
                    })
                })
                .unwrap_or(Value::Str(String::new()))
            }
            "lookup" => {
                // lookup(table, key, column)
                let table = self.str_arg(args.first()).unwrap_or_default();
                let key = num(1);
                let col = self.str_arg(args.get(2)).unwrap_or_default();
                lookup_table(self.sys.ds, &table, &key.to_string(), &col)
                    .map(|v| match v.parse::<f64>() {
                        Ok(n) => Value::number(n),
                        Err(_) => Value::Str(v),
                    })
                    .unwrap_or(Value::Int(0))
            }
            "listall" | "group" => Value::Array(Vec::new()),
            other => match self.sys.functions.get(other) {
                Some(f) => VEnv {
                    sys: self.sys,
                    owner: self.owner.clone(),
                    input: self.input.clone(),
                    format: self.format.clone(),
                    args: args
                        .iter()
                        .map(|a| (a.clone(), self.owner.clone()))
                        .collect(),
                }
                .eval(&f.body),
                None => {
                    *self
                        .sys
                        .unknown
                        .borrow_mut()
                        .entry(format!("{}()", name))
                        .or_default() += 1;
                    Value::Int(0)
                }
            },
        }
    }
}

fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(_) | Value::Float(_), Value::Int(_) | Value::Float(_)) => {
            (a.as_f64() - b.as_f64()).abs() < 1e-9
        }
        _ => a.to_string().eq_ignore_ascii_case(&b.to_string()),
    }
}

/// Look up a value in a DATATABLE: the row whose first column equals
/// `key`, in the named column.
pub fn lookup_table(ds: &Dataset, table: &str, key: &str, column: &str) -> Option<String> {
    let t = ds
        .tables
        .iter()
        .find(|t| t.name.eq_ignore_ascii_case(table))?;
    let ci = t
        .columns
        .iter()
        .position(|c| c.eq_ignore_ascii_case(column))?;
    let row = t.rows.iter().find(|r| {
        r.first().is_some_and(|k| {
            k.eq_ignore_ascii_case(key)
                || matches!((k.parse::<f64>(), key.parse::<f64>()), (Ok(a), Ok(b)) if a == b)
        })
    })?;
    row.get(ci).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lst::line::LstFile;

    /// A system over a dataset built from one VARIABLE file and one
    /// DATACONTROL file, with no active objects.
    fn bare<'a>(ds: &'a Dataset) -> VarSystem<'a> {
        let mut sys = VarSystem {
            ds,
            decls: HashMap::new(),
            controls: Controls::default(),
            modifiers: HashMap::new(),
            instances: BTreeMap::new(),
            objects: HashMap::new(),
            defaults: HashMap::new(),
            functions: HashMap::new(),
            channels: HashMap::new(),
            cache: RefCell::new(HashMap::new()),
            busy: RefCell::new(HashSet::new()),
            diags: RefCell::new(Diagnostics::default()),
            unknown: RefCell::new(BTreeMap::new()),
        };
        sys.load_declarations(None);
        sys.load_data_controls(None);
        sys
    }

    fn dataset(files: &[(FileKind, &str)]) -> Dataset {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("pcgtools-vars-{}-{}", std::process::id(), n));
        std::fs::create_dir_all(&dir).unwrap();
        let mut plan = crate::campaign::LoadPlan::default();
        for (i, (k, text)) in files.iter().enumerate() {
            let p = dir.join(format!("f{}.lst", i));
            LstFile::parse(text).write(&p).unwrap();
            plan.files
                .entry(*k)
                .or_default()
                .push(crate::campaign::PlannedFile {
                    kind: *k,
                    path: p,
                    filter: None,
                    campaign: dir.clone(),
                    line: 1,
                });
        }
        Dataset::load(&plan)
    }

    fn eval(sys: &VarSystem, f: &str) -> String {
        sys.eval_in(&Owner::global(), f).unwrap().to_string()
    }

    #[test]
    fn functions_table() {
        let ds = dataset(&[
            (
                FileKind::Variable,
                "GLOBAL:NUMBER=Five\nGLOBAL:ARRAY[NUMBER]=Arr\n",
            ),
            (
                FileKind::DataControl,
                "FUNCTION:double\tVALUE:arg(0)*2\nFUNCTION:sum2\tVALUE:arg(0)+arg(1)\n",
            ),
            (
                FileKind::DataTable,
                "STARTTABLE:T\nK,V\nNUMBER,STRING\n1,one\n2,two\nENDTABLE:\n",
            ),
        ]);
        let sys = bare(&ds);
        let cases: &[(&str, &str)] = &[
            ("1+2*3", "7"),
            ("7/2", "3.5"),
            ("6/3", "2"),
            ("abs(-3)", "3"),
            ("abs(-2.5)", "2.5"),
            ("floor(2.7)", "2"),
            ("floor(-2.5)", "-3"),
            ("ceil(2.1)", "3"),
            ("round(2.5)", "3"),
            ("round(-2.5)", "-2"),
            ("max(1,5,3)", "5"),
            ("min(4,2,9)", "2"),
            ("if(1<2,10,20)", "10"),
            ("if(3<2,10,20)", "20"),
            ("double(4)", "8"),
            ("sum2(3,double(2))", "7"),
            ("Five", "0"),
            ("Five+1", "1"),
            ("lookup(\"T\",2,\"V\")", "two"),
            ("lookup(\"T\",9,\"V\")", "0"),
            ("key(get(\"STAT\",\"STR\"))", "STR"),
            ("length(Arr)", "0"),
            ("isEmpty(Arr)", "true"),
            ("1==1", "true"),
            ("2!=2", "false"),
            ("!(1<2)", "false"),
            ("1<2&&2<3", "true"),
            ("1>2||2>3", "false"),
            ("2^3", "8"),
            ("-(3)", "-3"),
        ];
        for (f, want) in cases {
            assert_eq!(&eval(&sys, f), want, "{}", f);
        }
        assert_eq!(eval(&sys, "nosuch(1)"), "0");
        assert!(sys.unknown().contains_key("nosuch()"));
    }

    #[test]
    fn declaration_conflicts() {
        let ds = dataset(&[(
            FileKind::Variable,
            "GLOBAL:NUMBER=X\nGLOBAL:STRING=X\nLOCAL:PC.STAT|NUMBER=Y\nGLOBAL:NUMBER=Y\nLOCAL:PC.STAT|NUMBER=Z\nLOCAL:PC.SKILL|NUMBER=Z\n",
        )]);
        let sys = bare(&ds);
        let msgs: Vec<String> = sys
            .diags
            .borrow()
            .items
            .iter()
            .map(|d| d.message.clone())
            .collect();
        assert!(
            msgs.iter()
                .any(|m| m.contains("declared as NUMBER and STRING")),
            "{:?}",
            msgs
        );
        // Y declared globally after a local Y: the local one is hidden
        // only when the parent comes first; sibling scopes may share Z
        assert!(!msgs.iter().any(|m| m.contains("variable Z")), "{:?}", msgs);
    }

    #[test]
    fn modifier_application() {
        let ds = dataset(&[(
            FileKind::Variable,
            "GLOBAL:NUMBER=N\nGLOBAL:ARRAY[NUMBER]=A\n",
        )]);
        let mut sys = bare(&ds);
        let mut order = 0;
        for t in [
            "MODIFY:N|SET|4",
            "MODIFY:N|MAX|3",
            "MODIFY:N|MIN|10",
            "MODIFY:N|DIVIDE|2|PRIORITY=2",
            "MODIFY:N|ADD|value()*10|PRIORITY=3",
            "MODIFY:A|SET|1,2",
            "MODIFY:A|ADD|2,3",
            "MODIFY:N|SOLVE|3",
        ] {
            sys.add_modifier_token(&Owner::global(), t, "test", &mut order);
        }
        // 4 (max 3, min 10) -> /2 = 2 -> +20 = 22
        assert_eq!(eval(&sys, "N"), "22");
        assert_eq!(eval(&sys, "A"), "1,2,3");
        assert!(sys
            .diags
            .borrow()
            .items
            .iter()
            .any(|d| d.message.contains("unknown modifier operation")));
    }

    #[test]
    fn declarations() {
        let d = parse_decl(Some("GLOBAL"), "NUMBER=Speed").unwrap();
        assert_eq!(
            (d.scope.as_str(), d.name.as_str(), &d.format),
            ("PC", "Speed", &Format::Number)
        );
        let d = parse_decl(None, "Bonus").unwrap();
        assert_eq!(d.name, "Bonus");
        let d = parse_decl(Some("LOCAL"), "PC.STAT|Score").unwrap();
        assert_eq!(d.scope, "PC.STAT");
        assert_eq!(d.format, Format::Number);
        let d = parse_decl(Some("CHANNEL"), "PC.STAT|NUMBER=StatScore").unwrap();
        assert_eq!(d.name, "CHANNEL*StatScore");
        assert!(d.channel);
        let d = parse_decl(Some("GLOBAL"), "ORDEREDPAIR=Face").unwrap();
        assert_eq!(d.format, Format::OrderedPair);
        for (t, v) in [
            (Some("LOCAL"), "NoScope"),
            (Some("GLOBAL"), "1Bad"),
            (Some("GLOBAL"), "BAD-NAME"),
            (Some("GLOBAL"), "NOPE[=X"),
            (Some("OTHER"), "X"),
        ] {
            assert!(parse_decl(t, v).is_err(), "{:?} {}", t, v);
        }
    }

    #[test]
    fn ops_and_scopes() {
        assert_eq!(Op::parse("set"), Some(Op::Set));
        assert_eq!(Op::parse("SOLVE"), None);
        let order: Vec<i64> = [Op::Set, Op::Multiply, Op::Divide, Op::Add, Op::Max, Op::Min]
            .iter()
            .map(|o| o.inherent())
            .collect();
        assert_eq!(order, vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(parent_scope("PC.EQUIPMENT.PART"), Some("PC.EQUIPMENT"));
        assert_eq!(parent_scope("PC.STAT"), Some("PC"));
        assert_eq!(parent_scope("PC"), None);
        let part = Owner::new("PC.EQUIPMENT.PART", "Sword");
        assert_eq!(part.parent(), Some(Owner::new("PC.EQUIPMENT", "Sword")));
        assert_eq!(Owner::new("PC.STAT", "STR").parent(), Some(Owner::global()));
        assert!(legal_name("Abc_1"));
        assert!(!legal_name("_x"));
        assert!(!legal_name(""));
    }
}
