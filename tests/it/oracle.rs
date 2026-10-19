// Derived statistics compared against PCGen's own expected character
// sheets (code/testsuite/csheets/*.xml), read in place from PCGEN_ROOT.
//
// The gate covers the pf and 35e characters on BAB, saves, hit points,
// armor class and skills: any mismatch there fails the test.  Other
// game modes are reported only.  ORACLE_ONLY=name limits the run to
// one character; ORACLE_VERBOSE=1 prints every mismatch.

use crate::common;
use pcgtools::context::Context;
use pcgtools::pcc::resolve::DataDirs;
use pcgtools::pcg::character::Character;
use pcgtools::rules::calc::{Calc, Sheet};
use regex::Regex;
use std::collections::BTreeMap;

/// Values read from an expected character sheet.
#[derive(Debug, Default)]
struct Expected {
    bab: Option<i64>,
    saves: BTreeMap<String, i64>,
    hp: Option<i64>,
    ac: BTreeMap<String, i64>,
    skills: BTreeMap<String, i64>,
}

fn int(s: &str) -> Option<i64> {
    s.trim().trim_start_matches('+').parse().ok()
}

fn section<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let start = xml.find(&format!("<{}>", tag))?;
    let end = xml[start..].find(&format!("</{}>", tag))? + start;
    Some(&xml[start..end])
}

fn expected(xml: &str) -> Expected {
    let mut e = Expected::default();
    let field = |sec: &str, tag: &str| -> Option<i64> {
        let re = Regex::new(&format!(r"<{}>\s*([^<]*)", tag)).unwrap();
        re.captures(sec).and_then(|c| int(&c[1]))
    };
    if let Some(b) = section(xml, "basics") {
        e.bab = field(b, "bab");
    }
    if let Some(hp) = section(xml, "hit_points") {
        e.hp = field(hp, "points");
    }
    if let Some(ac) = section(xml, "armor_class") {
        for (tag, name) in [
            ("total", "Total"),
            ("flat", "Flatfooted"),
            ("touch", "Touch"),
        ] {
            if let Some(v) = field(ac, tag) {
                e.ac.insert(name.to_string(), v);
            }
        }
    }
    if let Some(st) = section(xml, "saving_throws") {
        let re = Regex::new(r"(?s)<saving_throw>.*?<long>([^<]*)</long>.*?<total>([^<]*)</total>")
            .unwrap();
        for c in re.captures_iter(st) {
            if let Some(v) = int(&c[2]) {
                e.saves.insert(c[1].to_lowercase(), v);
            }
        }
    }
    if let Some(sk) = section(xml, "skills") {
        let re = Regex::new(r"(?s)<skill>\s*<name>([^<]*)</name>.*?<skill_mod>([^<]*)</skill_mod>")
            .unwrap();
        for c in re.captures_iter(sk) {
            if let Some(v) = int(&c[2]) {
                e.skills.insert(c[1].to_string(), v);
            }
        }
    }
    e
}

#[derive(Default)]
struct Score {
    total: usize,
    ok: usize,
}

impl Score {
    fn check(&mut self, ok: bool) {
        self.total += 1;
        if ok {
            self.ok += 1;
        }
    }
}

fn compare(
    name: &str,
    e: &Expected,
    s: &Sheet,
    scores: &mut BTreeMap<&'static str, Score>,
    out: &mut Vec<String>,
) {
    let mut note = |field: &'static str, ok: bool, msg: String| {
        scores.entry(field).or_default().check(ok);
        if !ok {
            out.push(format!("{}: {} {}", name, field, msg));
        }
    };
    if let Some(b) = e.bab {
        note("bab", b == s.bab, format!("want {} got {}", b, s.bab));
    }
    if let Some(h) = e.hp {
        note("hp", h == s.hp, format!("want {} got {}", h, s.hp));
    }
    for (k, v) in &e.ac {
        let got = s.ac.get(k).copied().unwrap_or(0);
        note("ac", *v == got, format!("{} want {} got {}", k, v, got));
    }
    for (k, v) in &e.saves {
        let got = s
            .saves
            .iter()
            .find(|(n, _)| n.to_lowercase() == *k)
            .map(|(_, v)| *v);
        note(
            "saves",
            got == Some(*v),
            format!("{} want {} got {:?}", k, v, got),
        );
    }
    for (k, v) in &e.skills {
        // "Linguistics(Elven)" names a skill with a sub-choice
        let base = match k.find('(') {
            Some(i) if i > 0 && !k[..i].ends_with(' ') => &k[..i],
            _ => k.as_str(),
        };
        let got = s.skills.get(k).or_else(|| s.skills.get(base)).copied();
        note(
            "skills",
            got == Some(*v),
            format!("{} want {} got {:?}", k, v, got),
        );
    }
}

#[test]
fn oracle_derived_stats() {
    let (Some(root), Some(csheets)) = (
        common::corpus_path("pcgen"),
        common::corpus_path("pcgen/code/testsuite/csheets"),
    ) else {
        return;
    };
    let pcgs = root.join("code/testsuite/PCGfiles");
    let only = std::env::var("ORACLE_ONLY").ok();
    let verbose = std::env::var("ORACLE_VERBOSE").is_ok();
    let dirs = DataDirs::from_install(&root);

    let mut per_mode: BTreeMap<String, BTreeMap<&'static str, Score>> = BTreeMap::new();
    let mut mismatches = Vec::new();
    for xml_path in common::files_with_ext(&csheets, &["xml"]) {
        let stem = xml_path.file_stem().unwrap().to_string_lossy().into_owned();
        if only.as_deref().is_some_and(|o| o != stem) {
            continue;
        }
        let pcg = pcgs.join(format!("{}.pcg", stem));
        if !pcg.exists() {
            continue;
        }
        let mode = stem.split('_').next().unwrap_or("").to_string();
        let ch = Character::read(&pcg).unwrap();
        if ch.version().is_none() {
            continue;
        }
        let ctx = Context::for_character(&ch, dirs.clone());
        let sheet = Calc::new(&ctx.dataset, ctx.gamemode.as_ref(), &ch).run();
        let exp = expected(&std::fs::read_to_string(&xml_path).unwrap());
        let scores = per_mode.entry(mode).or_default();
        compare(&stem, &exp, &sheet, scores, &mut mismatches);
    }

    let mut gate_ok = true;
    for (mode, scores) in &per_mode {
        let line: Vec<String> = scores
            .iter()
            .map(|(f, s)| format!("{} {}/{}", f, s.ok, s.total))
            .collect();
        eprintln!("oracle {}: {}", mode, line.join(", "));
        if mode == "pf" || mode == "35e" {
            gate_ok &= scores.values().all(|s| s.ok == s.total);
        }
    }
    if verbose {
        for m in &mismatches {
            eprintln!("  {}", m);
        }
    }
    if only.is_none() {
        assert!(gate_ok, "pf/35e derived statistics differ from the oracle");
    }
}
