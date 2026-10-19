//
// dataset.rs -- the loaded, merged game data
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! Loads the LST files named by a [`LoadPlan`] into objects, applying
//! source headers, INCLUDE/EXCLUDE filters and the deferred
//! `.COPY`, `.MOD`, `.FORGET` operations.

use crate::campaign::{LoadPlan, PlannedFile};
use crate::error::{Diagnostic, Diagnostics};
use crate::lst::grammar::{classify, Role};
use crate::lst::kind::FileKind;
use crate::lst::line::{HeadOp, LineKind, LstFile};
use crate::pcc::fileref::Filter;
use crate::text::TextFile;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Where something came from: index into [`Dataset::files`] and a
/// 1-based line number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct Origin {
    pub file: u32,
    pub line: u32,
}

/// One `TAG:value` token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Tok {
    pub tag: String,
    pub value: String,
    pub origin: Origin,
}

impl Tok {
    /// The token as written in LST syntax.
    pub fn text(&self) -> String {
        if self.value.is_empty() {
            self.tag.clone()
        } else {
            format!("{}:{}", self.tag, self.value)
        }
    }
}

/// A sub-line attached to an object: a class level line, a kit line,
/// a bioset race line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SubLine {
    pub tag: String,
    pub value: String,
    pub tokens: Vec<Tok>,
    pub origin: Origin,
}

/// One modification applied to an object after its definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    /// "MOD" or "COPY" (for an object created by a copy)
    pub op: String,
    pub origin: Origin,
}

/// A game object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Object {
    pub kind: FileKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_tag: Option<String>,
    pub name: String,
    pub key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub tokens: Vec<Tok>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sublines: Vec<SubLine>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Object>,
    /// defaults from the file's SOURCE header line
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub source: Vec<(String, String)>,
    pub origin: Origin,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<Change>,
}

impl Object {
    fn new(kind: FileKind, head_tag: Option<String>, name: &str, origin: Origin) -> Object {
        Object {
            kind,
            head_tag,
            name: name.to_string(),
            key: name.to_string(),
            category: None,
            tokens: Vec::new(),
            sublines: Vec::new(),
            children: Vec::new(),
            source: Vec::new(),
            origin,
            history: Vec::new(),
        }
    }

    /// All values of a tag, in order.
    pub fn values<'a, 'b>(&'a self, tag: &'b str) -> impl Iterator<Item = &'a str> + use<'a, 'b> {
        self.tokens
            .iter()
            .filter(move |t| t.tag == tag)
            .map(|t| t.value.as_str())
    }

    /// The last value of a tag (later values override earlier ones for
    /// single-valued tags).
    pub fn get(&self, tag: &str) -> Option<&str> {
        self.values(tag).last()
    }

    pub fn has(&self, tag: &str) -> bool {
        self.tokens.iter().any(|t| t.tag == tag)
    }

    /// The object's TYPE values (TYPE tokens hold `.`-separated lists).
    pub fn types(&self) -> Vec<&str> {
        self.values("TYPE")
            .flat_map(|v| v.split('.'))
            .filter(|s| !s.is_empty())
            .collect()
    }

    pub fn is_type(&self, t: &str) -> bool {
        self.types().iter().any(|x| x.eq_ignore_ascii_case(t))
    }

    /// Source value: from the object's own tokens, else the file's
    /// SOURCE header.
    pub fn source_value(&self, tag: &str) -> Option<&str> {
        self.get(tag).or_else(|| {
            self.source
                .iter()
                .find(|(t, _)| t == tag)
                .map(|(_, v)| v.as_str())
        })
    }

    /// Append tokens.  Clearing forms drop earlier values:
    /// `TAG:.CLEAR` and `TAG:.CLEARALL` all of them, `TAG:.CLEAR.x` those
    /// matching x, `TAG:.CLEAR|v` all before adding v, `PRE:.CLEAR` every
    /// prerequisite.  A single-valued tag replaces its earlier value.
    /// KEY and CATEGORY tokens update identity.
    fn apply_tokens(&mut self, toks: Vec<Tok>) {
        for mut t in toks {
            if t.tag == "PRE" && t.value == ".CLEAR" {
                self.tokens
                    .retain(|x| !crate::token::pre::Prereq::is_pre_tag(&x.tag));
                continue;
            }
            if t.value == ".CLEAR" || t.value == ".CLEARALL" {
                self.tokens.retain(|x| x.tag != t.tag);
                continue;
            }
            if let Some(what) = t.value.strip_prefix(".CLEAR.") {
                let matches = clear_matcher(what);
                self.tokens.retain(|x| x.tag != t.tag || !matches(&x.value));
                continue;
            }
            if let Some(rest) = t.value.strip_prefix(".CLEAR|") {
                self.tokens.retain(|x| x.tag != t.tag);
                t.value = rest.to_string();
            }
            if single_valued(&t.tag) {
                self.tokens.retain(|x| x.tag != t.tag);
            }
            match t.tag.as_str() {
                "KEY" => self.key = t.value.clone(),
                // stats, sizes and alignments are keyed by abbreviation
                "ABB"
                    if matches!(
                        self.kind,
                        FileKind::Size | FileKind::Stat | FileKind::Alignment
                    ) =>
                {
                    self.key = t.value.clone()
                }
                "CATEGORY" if self.kind == FileKind::Ability => {
                    self.category = Some(t.value.clone())
                }
                _ => {}
            }
            self.tokens.push(t);
        }
    }

    /// Lookup key: category-qualified for abilities, lower case.
    pub fn index_key(&self) -> String {
        index_key(self.category.as_deref(), &self.key)
    }
}

/// The source date of an object: its own SOURCEDATE, its file's, or
/// its campaign's.
fn object_date(
    o: &Object,
    campaigns: &BTreeMap<PathBuf, String>,
    campaign: &Path,
) -> Option<String> {
    o.source_value("SOURCEDATE")
        .map(str::to_string)
        .or_else(|| campaigns.get(campaign).cloned())
}

/// Tags that hold one value: a later one replaces an earlier one.
const SINGLE_VALUED: &[&str] = &[
    "ABB",
    "ACCHECK",
    "ALTCRITMULT",
    "ALTCRITRANGE",
    "ALTDAMAGE",
    "BASEITEM",
    "BASEQTY",
    "CHOOSE",
    "CONTAINS",
    "COST",
    "CR",
    "CRITMULT",
    "CRITRANGE",
    "DAMAGE",
    "EDR",
    "EXCLUSIVE",
    "FACE",
    "HANDS",
    "HD",
    "KEYSTAT",
    "LEGS",
    "LEVELADJUSTMENT",
    "MAXDEX",
    "MAXLEVEL",
    "MEMORIZE",
    "MULT",
    "OUTPUTNAME",
    "PROFICIENCY",
    "RACETYPE",
    "RANGE",
    "RATEOFFIRE",
    "REACH",
    "REACHMULT",
    "SELECT",
    "SIZE",
    "SLOTS",
    "SORTKEY",
    "SOURCEDATE",
    "SOURCELINK",
    "SOURCELONG",
    "SOURCEPAGE",
    "SOURCESHORT",
    "SOURCEWEB",
    "SPELLFAILURE",
    "SPELLSTAT",
    "STACK",
    "STARTSKILLPTS",
    "STATMOD",
    "USEUNTRAINED",
    "VISIBLE",
    "WIELD",
    "WT",
    "XTRASKILLPTSPERLVL",
];

/// Does a tag hold a single value?
pub fn single_valued(tag: &str) -> bool {
    SINGLE_VALUED.contains(&tag)
}

/// What `TAG:.CLEAR.x` removes: values equal to x, values listing x
/// among `|`-separated parts, or values x matches as a pattern.
fn clear_matcher(what: &str) -> impl Fn(&str) -> bool {
    let what = what.to_string();
    let re = regex::Regex::new(&what).ok();
    move |v: &str| {
        v.eq_ignore_ascii_case(&what)
            || v.split('|').any(|p| p.eq_ignore_ascii_case(&what))
            || re.as_ref().is_some_and(|r| r.is_match(v))
    }
}

fn index_key(category: Option<&str>, key: &str) -> String {
    match category {
        Some(c) => format!("{}|{}", c.to_lowercase(), key.to_lowercase()),
        None => key.to_lowercase(),
    }
}

/// A DATATABLE table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Table {
    pub name: String,
    pub columns: Vec<String>,
    pub formats: Vec<String>,
    pub rows: Vec<Vec<String>>,
    pub origin: Origin,
}

/// All objects of one kind.
#[derive(Debug, Clone, Default, Serialize)]
pub struct KindTable {
    pub objects: Vec<Object>,
    #[serde(skip)]
    index: HashMap<String, usize>,
}

impl KindTable {
    pub fn get(&self, category: Option<&str>, key: &str) -> Option<&Object> {
        self.index
            .get(&index_key(category, key))
            .map(|&i| &self.objects[i])
    }

    fn get_mut(&mut self, ikey: &str) -> Option<&mut Object> {
        self.index.get(ikey).map(|&i| &mut self.objects[i])
    }

    fn insert(&mut self, obj: Object) {
        self.index.insert(obj.index_key(), self.objects.len());
        self.objects.push(obj);
    }

    fn reindex(&mut self) {
        // with duplicates allowed, the first object of a key is found
        let mut index = HashMap::new();
        for (i, o) in self.objects.iter().enumerate() {
            index.entry(o.index_key()).or_insert(i);
        }
        self.index = index;
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}

/// Everything loaded from a campaign selection.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Dataset {
    /// every LST file read, indexed by [`Origin::file`]
    pub files: Vec<PathBuf>,
    pub kinds: BTreeMap<FileKind, KindTable>,
    pub tables: Vec<Table>,
    pub diags: Diagnostics,
    /// kinds whose objects may share keys
    #[serde(skip)]
    allow_dupes: BTreeSet<FileKind>,
    /// source date of each campaign
    #[serde(skip)]
    source_dates: BTreeMap<PathBuf, String>,
    /// source date each object was defined with, by kind and index key
    #[serde(skip)]
    dates: HashMap<(FileKind, String), String>,
    /// the campaigns loaded, for PRECAMPAIGN on objects
    #[serde(skip)]
    pub campaigns: crate::campaign::PreCampaignContext,
}

/// Deferred operations collected while loading the files of a kind.
struct Ops<'a> {
    copies: &'a mut Vec<Deferred>,
    mods: &'a mut Vec<Deferred>,
    forgets: &'a mut Vec<Deferred>,
    /// index keys of objects dropped by INCLUDE/EXCLUDE filters
    excluded: &'a mut HashSet<String>,
}

/// A deferred operation.
struct Deferred {
    op: HeadOp,
    category: Option<String>,
    key: String,
    tokens: Vec<Tok>,
    sublines: Vec<SubLine>,
    children: Vec<Object>,
    origin: Origin,
    /// objects excluded by the file filter; a missing target in this
    /// set is not an error
    filter: Option<Filter>,
}

impl Dataset {
    pub fn load(plan: &LoadPlan) -> Dataset {
        let mut ds = Dataset {
            allow_dupes: plan.allow_dupes.clone(),
            source_dates: plan.source_dates.clone(),
            campaigns: plan.campaigns_in_use.clone(),
            ..Default::default()
        };
        for (kind, files) in &plan.files {
            ds.load_kind(*kind, files);
        }
        ds
    }

    pub fn table(&self, kind: FileKind) -> Option<&KindTable> {
        self.kinds.get(&kind)
    }

    pub fn objects(&self, kind: FileKind) -> &[Object] {
        self.kinds
            .get(&kind)
            .map(|t| t.objects.as_slice())
            .unwrap_or(&[])
    }

    /// Find an object by key (case-insensitive).  For abilities, a
    /// key of the form `CATEGORY|Key` is accepted, as is an
    /// unqualified key (first match in any category).
    pub fn get(&self, kind: FileKind, key: &str) -> Option<&Object> {
        let t = self.kinds.get(&kind)?;
        if let Some(o) = t.get(None, key) {
            return Some(o);
        }
        if kind == FileKind::Ability {
            if let Some((c, k)) = key.split_once('|') {
                if let Some(o) = t.get(Some(c), k) {
                    return Some(o);
                }
            }
            let lk = key.to_lowercase();
            return t.objects.iter().find(|o| o.key.to_lowercase() == lk);
        }
        None
    }

    pub fn get_in(&self, kind: FileKind, category: &str, key: &str) -> Option<&Object> {
        self.kinds.get(&kind)?.get(Some(category), key)
    }

    pub fn file_path(&self, o: Origin) -> &Path {
        &self.files[o.file as usize]
    }

    fn add_file(&mut self, p: &Path) -> u32 {
        if let Some(i) = self.files.iter().position(|f| f == p) {
            return i as u32;
        }
        self.files.push(p.to_path_buf());
        (self.files.len() - 1) as u32
    }

    fn diag(&mut self, d: Diagnostic, o: Origin) {
        let f = self.files[o.file as usize].clone();
        self.diags.push(d.at(&f, o.line as usize));
    }

    fn load_kind(&mut self, kind: FileKind, files: &[PlannedFile]) {
        let mut copies = Vec::new();
        let mut mods = Vec::new();
        let mut forgets = Vec::new();
        let mut excluded = HashSet::new();

        for pf in files {
            if kind == FileKind::DataTable {
                self.load_tables(pf);
                continue;
            }
            let fidx = self.add_file(&pf.path);
            let lf = match LstFile::read(&pf.path) {
                Ok(f) => f,
                Err(e) => {
                    self.diags.push(Diagnostic::error(e.to_string()));
                    continue;
                }
            };
            // PCGen leaves such a file out entirely
            if lf.lines.iter().any(|l| l.latin1) {
                self.diags
                    .push(Diagnostic::error("file is not valid UTF-8; not loaded").at(&pf.path, 0));
                continue;
            }
            let mut ops = Ops {
                copies: &mut copies,
                mods: &mut mods,
                forgets: &mut forgets,
                excluded: &mut excluded,
            };
            self.load_file(kind, pf, fidx, &lf, &mut ops);
        }

        // a file listed twice (e.g. with different INCLUDE filters)
        // must not apply its operations twice
        let mut seen = HashSet::new();
        let mut dedup = |v: Vec<Deferred>| -> Vec<Deferred> {
            v.into_iter().filter(|d| seen.insert(d.origin)).collect()
        };
        let (copies, mods, forgets) = (dedup(copies), dedup(mods), dedup(forgets));

        let table = self.kinds.entry(kind).or_default();
        let mut diags: Vec<(Diagnostic, Origin)> = Vec::new();

        for d in copies {
            let HeadOp::Copy(newname) = &d.op else {
                unreachable!()
            };
            let Some(base) = table.get(d.category.as_deref(), &d.key).cloned() else {
                diags.push((
                    Diagnostic::error(format!("{}: .COPY of unknown object '{}'", kind, d.key)),
                    d.origin,
                ));
                continue;
            };
            let mut o = base;
            o.name = newname.clone();
            o.key = newname.clone();
            o.tokens.retain(|t| t.tag != "KEY");
            o.origin = d.origin;
            o.history.push(Change {
                op: "COPY".into(),
                origin: d.origin,
            });
            o.apply_tokens(d.tokens);
            o.sublines.extend(d.sublines);
            if table.index.contains_key(&o.index_key()) {
                diags.push((
                    Diagnostic::warning(format!(
                        "{}: .COPY target '{}' already exists",
                        kind, newname
                    )),
                    d.origin,
                ));
                continue;
            }
            table.insert(o);
        }

        for d in mods {
            let ik = index_key(d.category.as_deref(), &d.key);
            match table.get_mut(&ik) {
                Some(o) => {
                    o.history.push(Change {
                        op: "MOD".into(),
                        origin: d.origin,
                    });
                    o.apply_tokens(d.tokens);
                    o.sublines.extend(d.sublines);
                    merge_children(&mut o.children, d.children);
                }
                None => {
                    let filtered = excluded.contains(&ik)
                        || d.filter
                            .as_ref()
                            .is_some_and(|f| !f.admits(d.category.as_deref(), &d.key));
                    if !filtered {
                        diags.push((
                            Diagnostic::error(format!(
                                "{}: .MOD of unknown object '{}'",
                                kind, d.key
                            )),
                            d.origin,
                        ));
                    }
                }
            }
        }

        if !forgets.is_empty() {
            let drop: HashSet<String> = forgets
                .iter()
                .map(|d: &Deferred| index_key(d.category.as_deref(), &d.key))
                .collect();
            table.objects.retain(|o| !drop.contains(&o.index_key()));
            table.reindex();
        }

        for (d, o) in diags {
            self.diag(d, o);
        }
    }

    fn load_file(
        &mut self,
        kind: FileKind,
        pf: &PlannedFile,
        fidx: u32,
        lf: &LstFile,
        ops: &mut Ops<'_>,
    ) {
        let Ops {
            copies,
            mods,
            forgets,
            excluded,
        } = ops;
        // kinds whose heads need not be unique: every line is kept
        let declarations = !kind.has_objects()
            || matches!(
                kind,
                FileKind::DataControl | FileKind::CompanionMod | FileKind::BioSet
            );
        let mut source: Vec<(String, String)> = Vec::new();

        // the object (or deferred op) receiving sub-lines
        enum Cur {
            None,
            Obj(usize),
            Def(usize),
            Skip,
        }
        let mut cur = Cur::None;
        // kit REGION lines apply to all following kits in the file
        let mut kit_region: Option<SubLine> = None;

        for rec in lf.records() {
            let origin = Origin {
                file: fidx,
                line: rec.line_no() as u32,
            };
            let toks: Vec<Tok> = rec
                .tokens()
                .into_iter()
                .map(|t| Tok {
                    tag: t.tag.to_string(),
                    value: t.value.unwrap_or("").to_string(),
                    origin: Origin {
                        file: fidx,
                        line: t.line as u32 + 1,
                    },
                })
                .collect();

            match rec.kind() {
                LineKind::Source => {
                    source = toks.into_iter().map(|t| (t.tag, t.value)).collect();
                    continue;
                }
                LineKind::Continuation => {
                    self.diag(
                        Diagnostic::error("continuation line with no preceding record"),
                        origin,
                    );
                    continue;
                }
                _ => {}
            }
            let head = rec.head().unwrap_or("");

            match classify(kind, head) {
                Role::Object { head_tag, head } => match head.op {
                    HeadOp::Define => {
                        let mut o = Object::new(kind, head_tag, &head.name, origin);
                        o.source = source.clone();
                        o.apply_tokens(toks);
                        if o.kind == FileKind::Ability && o.category.is_none() {
                            self.diag(
                                Diagnostic::warning(format!(
                                    "ability '{}' has no CATEGORY",
                                    o.name
                                )),
                                origin,
                            );
                        }
                        if let Some(f) = &pf.filter {
                            if !f.admits(o.category.as_deref(), &o.key) {
                                excluded.insert(o.index_key());
                                cur = Cur::Skip;
                                continue;
                            }
                        }
                        if let Some(r) = &kit_region {
                            o.sublines.push(r.clone());
                        }
                        let table = self.kinds.entry(kind).or_default();
                        let ik = o.index_key();
                        if declarations {
                            table.objects.push(o);
                            cur = Cur::Obj(table.objects.len() - 1);
                            continue;
                        }
                        let idx = table.index.get(&ik).copied();
                        match idx.map(|i| &mut table.objects[i]) {
                            // repeated CLASS:X lines in one file extend the class
                            Some(existing)
                                if kind == FileKind::Class && existing.origin.file == fidx =>
                            {
                                existing.apply_tokens(o.tokens);
                            }
                            // same definition reached through a second listing of the file
                            Some(existing) if existing.origin == origin => {
                                cur = Cur::Skip;
                                continue;
                            }
                            // the kind allows duplicates: keep both, the
                            // first found by key
                            Some(_) if self.allow_dupes.contains(&kind) => {
                                let i = table.objects.len();
                                table.objects.push(o);
                                cur = Cur::Obj(i);
                                continue;
                            }
                            Some(_) => {
                                // a definition from a newer source replaces
                                // an older one
                                let date = object_date(&o, &self.source_dates, &pf.campaign);
                                let old = self.dates.get(&(kind, ik.clone())).cloned();
                                if let (Some(new), Some(old)) = (&date, &old) {
                                    if new > old {
                                        let i = table.index[&ik];
                                        let key = o.key.clone();
                                        table.objects[i] = o;
                                        self.dates.insert((kind, ik.clone()), new.clone());
                                        self.diag(
                                            Diagnostic::info(format!(
                                                "{}: newer definition of '{}' replaces the older",
                                                kind, key
                                            )),
                                            origin,
                                        );
                                        cur = Cur::Obj(i);
                                        continue;
                                    }
                                }
                                self.diag(
                                    Diagnostic::warning(format!(
                                        "{}: duplicate object '{}' ignored",
                                        kind, o.key
                                    )),
                                    origin,
                                );
                                cur = Cur::Skip;
                                continue;
                            }
                            None => {
                                if let Some(d) = object_date(&o, &self.source_dates, &pf.campaign) {
                                    self.dates.insert((kind, ik.clone()), d);
                                }
                                table.insert(o)
                            }
                        }
                        cur = Cur::Obj(table.index[&ik]);
                    }
                    op => {
                        let d = Deferred {
                            op: op.clone(),
                            category: head.category.clone(),
                            key: head.name.clone(),
                            tokens: toks,
                            sublines: Vec::new(),
                            children: Vec::new(),
                            origin,
                            filter: pf.filter.clone(),
                        };
                        let list = match op {
                            HeadOp::Copy(_) => &mut **copies,
                            HeadOp::Mod => &mut **mods,
                            _ => &mut **forgets,
                        };
                        list.push(d);
                        // only class .MOD lines collect following level lines
                        cur = if matches!(op, HeadOp::Mod) && kind == FileKind::Class {
                            Cur::Def(mods.len() - 1)
                        } else {
                            Cur::None
                        };
                    }
                },
                Role::Child { head_tag, name } => {
                    let mut c = Object::new(kind, Some(head_tag.clone()), &name, origin);
                    c.apply_tokens(toks);
                    match &cur {
                        Cur::Obj(i) => {
                            let table = self.kinds.get_mut(&kind).expect("current object");
                            merge_children(&mut table.objects[*i].children, vec![c]);
                        }
                        Cur::Def(i) => merge_children(&mut mods[*i].children, vec![c]),
                        Cur::Skip => {}
                        Cur::None => self.diag(
                            Diagnostic::error(format!("{} with no class", head_tag)),
                            origin,
                        ),
                    }
                }
                Role::Sub { tag, value } | Role::ChildSub { tag, value } => {
                    let child = matches!(classify(kind, head), Role::ChildSub { .. });
                    let sl = SubLine {
                        tag,
                        value,
                        tokens: toks,
                        origin,
                    };
                    if kind == FileKind::Kit && sl.tag == "REGION" {
                        kit_region = Some(sl);
                        continue;
                    }
                    match &cur {
                        Cur::Obj(i) => {
                            let table = self.kinds.get_mut(&kind).expect("current object");
                            let o = &mut table.objects[*i];
                            match o.children.last_mut() {
                                Some(c) if child => c.sublines.push(sl),
                                _ => o.sublines.push(sl),
                            }
                        }
                        Cur::Def(i) => {
                            let d = &mut mods[*i];
                            match d.children.last_mut() {
                                Some(c) if child => c.sublines.push(sl),
                                _ => d.sublines.push(sl),
                            }
                        }
                        Cur::Skip => {}
                        Cur::None => self.diag(
                            Diagnostic::error(format!("'{}' line with no object", head)),
                            origin,
                        ),
                    }
                }
            }
        }
    }

    fn load_tables(&mut self, pf: &PlannedFile) {
        let fidx = self.add_file(&pf.path);
        let tf = match TextFile::read(&pf.path) {
            Ok(t) => t,
            Err(e) => {
                self.diags.push(Diagnostic::error(e.to_string()));
                return;
            }
        };
        if tf.has_latin1() {
            self.diags
                .push(Diagnostic::error("file is not valid UTF-8; not loaded").at(&pf.path, 0));
            return;
        }
        let mut cur: Option<Table> = None;
        for (i, l) in tf.lines.iter().enumerate() {
            let origin = Origin {
                file: fidx,
                line: i as u32 + 1,
            };
            let s = l.text.trim();
            if s.is_empty() || s.starts_with('#') || s.starts_with("\"#") {
                continue;
            }
            let cells = split_csv(s);
            if cells.iter().all(|c| c.trim().is_empty()) {
                continue;
            }
            let first = cells[0].trim();
            if let Some(name) = first.strip_prefix("STARTTABLE:") {
                if cur.is_some() {
                    self.diag(Diagnostic::error("STARTTABLE inside a table"), origin);
                }
                cur = Some(Table {
                    name: name.to_string(),
                    columns: Vec::new(),
                    formats: Vec::new(),
                    rows: Vec::new(),
                    origin,
                });
            } else if first.starts_with("ENDTABLE") {
                match cur.take() {
                    Some(t) => self.tables.push(t),
                    None => self.diag(Diagnostic::error("ENDTABLE with no table"), origin),
                }
            } else if let Some(t) = cur.as_mut() {
                let row: Vec<String> = cells.into_iter().map(|c| c.trim().to_string()).collect();
                if t.columns.is_empty() {
                    t.columns = row;
                } else if t.formats.is_empty() {
                    t.formats = row;
                } else {
                    t.rows.push(row);
                }
            } else {
                self.diag(Diagnostic::error("data outside of a table"), origin);
            }
        }
        if let Some(t) = cur {
            let o = t.origin;
            self.tables.push(t);
            self.diag(Diagnostic::error("table has no ENDTABLE"), o);
        }
    }

    /// Total object count across all kinds.
    pub fn len(&self) -> usize {
        self.kinds.values().map(|t| t.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Add children, merging into an existing child with the same key.
fn merge_children(into: &mut Vec<Object>, children: Vec<Object>) {
    for c in children {
        match into.iter_mut().find(|x| x.key == c.key) {
            Some(existing) => {
                existing.apply_tokens(c.tokens);
                existing.sublines.extend(c.sublines);
            }
            None => into.push(c),
        }
    }
}

/// Split one CSV line, honoring double quotes (`""` is a literal quote).
pub fn split_csv(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(tag: &str, value: &str) -> Tok {
        Tok {
            tag: tag.into(),
            value: value.into(),
            origin: Origin { file: 0, line: 0 },
        }
    }

    fn obj(toks: &[(&str, &str)]) -> Object {
        let mut o = Object::new(FileKind::Spell, None, "Bless", Origin { file: 0, line: 0 });
        o.apply_tokens(toks.iter().map(|(t, v)| tok(t, v)).collect());
        o
    }

    fn vals<'a>(o: &'a Object, tag: &str) -> Vec<&'a str> {
        o.values(tag).collect()
    }

    #[test]
    fn clear_forms() {
        let o = obj(&[
            ("DESC", "a"),
            ("DESC", "b"),
            ("DESC", ".CLEAR"),
            ("DESC", "c"),
        ]);
        assert_eq!(vals(&o, "DESC"), vec!["c"]);
        let o = obj(&[
            ("CLASSES", "Wizard=1"),
            ("CLASSES", ".CLEARALL"),
            ("CLASSES", "Bard=2"),
        ]);
        assert_eq!(vals(&o, "CLASSES"), vec!["Bard=2"]);
        let o = obj(&[
            ("CSKILL", "Climb"),
            ("CSKILL", "Swim"),
            ("CSKILL", ".CLEAR.Climb"),
        ]);
        assert_eq!(vals(&o, "CSKILL"), vec!["Swim"]);
        let o = obj(&[("CSKILL", "Climb|Swim"), ("CSKILL", ".CLEAR.Swim")]);
        assert!(vals(&o, "CSKILL").is_empty());
        let o = obj(&[("DESCRIPTOR", "Fire"), ("DESCRIPTOR", ".CLEAR|Cold")]);
        assert_eq!(vals(&o, "DESCRIPTOR"), vec!["Cold"]);
        let o = obj(&[
            ("PRECLASS", "1,Wizard=1"),
            ("!PREFEAT", "1,Alertness"),
            ("PRE", ".CLEAR"),
            ("TYPE", "Arcane"),
        ]);
        assert!(vals(&o, "PRECLASS").is_empty() && vals(&o, "!PREFEAT").is_empty());
        assert_eq!(vals(&o, "TYPE"), vec!["Arcane"]);
    }

    #[test]
    fn single_valued_tags_replace() {
        let o = obj(&[
            ("SOURCEPAGE", "p.1"),
            ("SOURCEPAGE", "p.91"),
            ("TYPE", "A"),
            ("TYPE", "B"),
        ]);
        assert_eq!(vals(&o, "SOURCEPAGE"), vec!["p.91"]);
        assert_eq!(vals(&o, "TYPE"), vec!["A", "B"]);
        assert!(single_valued("COST") && !single_valued("BONUS"));
    }

    #[test]
    fn csv() {
        assert_eq!(split_csv("a,b,,c"), vec!["a", "b", "", "c"]);
        assert_eq!(split_csv("\"a,b\",\"x\"\"y\""), vec!["a,b", "x\"y"]);
    }
}
