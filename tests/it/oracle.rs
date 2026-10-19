// Character sheets compared against PCGen's expected sheets
// (code/testsuite/csheets/*.xml), read in place from PCGEN_ROOT.
//
// Every leaf value of every expected sheet is compared with the sheet
// pcgtools builds, and every one must match; results are reported per
// game mode and section.
//
// ORACLE_ONLY=name limits the run to one character, ORACLE_SECTION to
// one section; ORACLE_VERBOSE=1 prints mismatches (at most
// ORACLE_LIMIT, default 400).

use crate::common;
use pcgtools::context::Context;
use pcgtools::pcc::resolve::DataDirs;
use pcgtools::pcg::character::Character;
use pcgtools::sheet::{self, tree::section, Element};
use std::collections::{BTreeMap, HashSet};

#[derive(Default)]
struct Score {
    total: usize,
    ok: usize,
}

#[test]
fn oracle_character_sheets() {
    let (Some(root), Some(csheets)) = (
        common::corpus_path("pcgen"),
        common::corpus_path("pcgen/code/testsuite/csheets"),
    ) else {
        return;
    };
    let pcgs = root.join("code/testsuite/PCGfiles");
    let only = std::env::var("ORACLE_ONLY").ok();
    let only_section = std::env::var("ORACLE_SECTION").ok();
    let verbose = std::env::var("ORACLE_VERBOSE").is_ok();
    let dirs = DataDirs::from_install(&root);

    // mode -> section -> score
    let mut scores: BTreeMap<String, BTreeMap<String, Score>> = BTreeMap::new();
    let mut shown = Vec::new();
    for xml_path in common::files_with_ext(&csheets, &["xml"]) {
        let stem = xml_path.file_stem().unwrap().to_string_lossy().into_owned();
        if only.as_deref().is_some_and(|o| o != stem) {
            continue;
        }
        let pcg = pcgs.join(format!("{}.pcg", stem));
        if !pcg.exists() {
            continue;
        }
        let ch = Character::read(&pcg).unwrap();
        if ch.version().is_none() {
            continue;
        }
        let mode = stem.split('_').next().unwrap_or("").to_string();
        let expected = Element::parse_xml(&std::fs::read_to_string(&xml_path).unwrap()).unwrap();
        let ctx = Context::for_character(&ch, dirs.clone());
        let actual = sheet::build(&ch, &ctx);
        let diff = sheet::compare(&expected, &actual);

        let ms = scores.entry(mode.clone()).or_default();
        let bad: HashSet<&str> = diff.mismatches.iter().map(|(p, _, _)| p.as_str()).collect();
        for (path, _) in expected.flatten() {
            let s = ms.entry(section(&path).to_string()).or_default();
            s.total += 1;
            let ok = !bad.contains(path.as_str());
            if ok {
                s.ok += 1;
            }
        }
        for (p, want, got) in &diff.mismatches {
            if only_section.as_deref().is_none_or(|s| section(p) == s) {
                shown.push(format!("{}: {} want {:?} got {:?}", stem, p, want, got));
            }
        }
    }

    for (mode, secs) in &scores {
        let (t, o) = secs
            .iter()
            .fold((0, 0), |a, (_, s)| (a.0 + s.total, a.1 + s.ok));
        let line: Vec<String> = secs
            .iter()
            .filter(|(k, _)| only_section.as_deref().is_none_or(|s| s == k.as_str()))
            .map(|(k, s)| format!("{} {}/{}", k, s.ok, s.total))
            .collect();
        eprintln!("oracle {}: {}/{} | {}", mode, o, t, line.join(", "));
    }
    if verbose {
        let limit = std::env::var("ORACLE_LIMIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(400);
        for m in shown.iter().take(limit) {
            eprintln!("  {}", m);
        }
    }
    assert!(!scores.is_empty(), "no expected sheets were compared");
    let failed: Vec<&String> = shown.iter().take(400).collect();
    assert!(
        shown.is_empty(),
        "{} sheet values differ from the oracle:\n{}",
        shown.len(),
        failed
            .iter()
            .map(|x| x.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
