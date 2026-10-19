// PCG character files: corpus round-trip and campaign resolution.

use crate::common;
use pcgtools::pcg::character::Character;
use pcgtools::pcg::{join, PcgFile};

fn corpus_pcgs() -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    for rel in ["pcgen/characters", "pcgen/code/testsuite/PCGfiles"] {
        if let Some(d) = common::corpus_path(rel) {
            v.extend(common::files_with_ext(&d, &["pcg"]));
        }
    }
    v
}

#[test]
fn corpus_pcg_parse_and_roundtrip() {
    let files = corpus_pcgs();
    let mut lines = 0;
    let mut regenerated_same = 0;
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        let pcg = PcgFile::from_text(&pcgtools::text::TextFile::decode(&bytes), Some(f));
        assert!(pcg.diags.is_empty(), "{:?}", pcg.diags.items.first());
        assert_eq!(pcg.encode(), bytes, "{}", f.display());
        for l in &pcg.lines {
            if let Some(e) = &l.elems {
                lines += 1;
                let raw = l.raw.trim().trim_end_matches('|');
                if join(e) == raw {
                    regenerated_same += 1;
                }
            }
        }
        let c = Character::from_pcg(pcg);
        // two test files predate the version 2 format
        let v1 = c
            .pcg
            .lines
            .first()
            .is_some_and(|l| l.raw.starts_with("CAMPAIGNS:"));
        assert!(c.version().is_some() != v1, "{}", f.display());
    }
    eprintln!(
        "pcg: {} files, {} data lines, {} regenerate identically",
        files.len(),
        lines,
        regenerated_same
    );
    assert!(regenerated_same * 1000 >= lines * 995);
}

#[test]
fn corpus_pcg_campaigns_resolve() {
    use pcgtools::campaign::Registry;
    use pcgtools::pcc::resolve::DataDirs;
    let Some(root) = common::corpus_path("pcgen") else {
        return;
    };
    let mut reg = Registry::new(DataDirs::from_install(&root));
    let mut diags = pcgtools::Diagnostics::default();
    reg.scan(&root.join("data"), &mut diags);
    let modes = pcgtools::gamemode::GameModes::scan(&root.join("system/gameModes"));
    let mut missing = Vec::new();
    let mut total = 0;
    for f in corpus_pcgs() {
        let c = Character::read(&f).unwrap();
        let gm = c.gamemode().and_then(|g| modes.find(&g));
        let ver = c.pcg.value("VERSION").unwrap_or_default();
        for k in c.campaigns() {
            total += 1;
            let key = gm.and_then(|m| m.migrate("SOURCE", &k, &ver)).unwrap_or(k);
            if reg.by_key(&key).is_none() {
                missing.push(key);
            }
        }
    }
    missing.sort();
    missing.dedup();
    eprintln!(
        "pcg campaigns: {} refs, unresolved keys: {:?}",
        total, missing
    );
}
