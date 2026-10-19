// Corpus tests: run against real PCGen data when PCGEN_ROOT is set.

use crate::common;

use pcgtools::text::TextFile;

#[test]
fn corpus_text_roundtrip() {
    let mut n = 0;
    for dir in common::data_dirs() {
        for f in common::files_with_ext(&dir, &["pcc", "lst", "pcg"]) {
            let bytes = std::fs::read(&f).unwrap();
            assert_eq!(TextFile::decode(&bytes).encode(), bytes, "{}", f.display());
            n += 1;
        }
    }
    eprintln!("text roundtrip: {} files", n);
}

#[test]
fn corpus_lst_roundtrip() {
    use pcgtools::lst::LstFile;
    let mut n = 0;
    let mut cont = 0;
    for dir in common::data_dirs() {
        for f in common::files_with_ext(&dir, &["pcc", "lst"]) {
            let bytes = std::fs::read(&f).unwrap();
            let lf = LstFile::decode(&bytes);
            assert_eq!(lf.encode(), bytes, "{}", f.display());
            cont += lf
                .lines
                .iter()
                .filter(|l| l.kind == pcgtools::lst::LineKind::Continuation)
                .count();
            n += 1;
        }
    }
    eprintln!("lst roundtrip: {} files, {} continuation lines", n, cont);
}

#[test]
fn corpus_campaigns() {
    use pcgtools::campaign::{LoadPlan, Registry};
    use pcgtools::pcc::resolve::DataDirs;
    use pcgtools::{Diagnostics, Severity};

    for rel in ["pcgen", "pcgen-newsources"] {
        let Some(root) = common::corpus_path(rel) else {
            continue;
        };
        let mut dirs = DataDirs::from_install(&root);
        if rel == "pcgen-newsources" {
            // newsources references both its own tree and the main data
            dirs.data = common::corpus_path("pcgen/data");
            dirs.vendor = Some(root.clone());
        }
        let mut reg = Registry::new(dirs);
        let mut diags = Diagnostics::default();
        let scan_root = if rel == "pcgen" {
            root.join("data")
        } else {
            root
        };
        let all = reg.scan(&scan_root, &mut diags);
        assert!(diags.is_empty(), "{:?}", diags.items.first());

        let mut missing = 0;
        let mut errors = 0;
        let mut files = 0;
        for p in &all {
            let plan = LoadPlan::build(&mut reg, std::slice::from_ref(p));
            files += plan.ordered().count();
            missing += plan
                .diags
                .items
                .iter()
                .filter(|d| d.message.starts_with("referenced file not found"))
                .count();
            errors += plan.diags.count(Severity::Error);
        }
        eprintln!(
            "{}: {} campaigns, {} planned files, {} missing refs, {} errors",
            rel,
            all.len(),
            files,
            missing,
            errors
        );
    }
}

#[test]
fn corpus_datasets() {
    use pcgtools::campaign::{LoadPlan, Registry};
    use pcgtools::dataset::Dataset;
    use pcgtools::pcc::resolve::DataDirs;
    use pcgtools::Severity;

    let Some(root) = common::corpus_path("pcgen") else {
        return;
    };
    let dirs = DataDirs::from_install(&root);
    let data = root.join("data");

    // big selections must load with sane counts
    for (rel, min_objects) in [
        ("pathfinder/paizo/pathfinder_rpg_core.pcc", 40000),
        (
            "35e/wizards_of_the_coast/rsrd/basics/_rsrd_basics.pcc",
            3000,
        ),
        (
            "5e/wizards_of_the_coast/srd5/_system_reference_document_5.0.pcc",
            1000,
        ),
    ] {
        let mut reg = Registry::new(dirs.clone());
        let plan = LoadPlan::build(&mut reg, &[data.join(rel)]);
        let ds = Dataset::load(&plan);
        eprintln!(
            "{}: {} objects, {} errors, {} warnings",
            rel,
            ds.len(),
            ds.diags.count(Severity::Error),
            ds.diags.count(Severity::Warning)
        );
        assert!(ds.len() >= min_objects, "{} loaded {}", rel, ds.len());
    }

    // every campaign individually: no panics
    let mut reg = Registry::new(dirs.clone());
    let mut diags = pcgtools::Diagnostics::default();
    let all = reg.scan(&data, &mut diags);
    let (mut objects, mut errors) = (0, 0);
    for p in &all {
        let plan = LoadPlan::build(&mut reg, std::slice::from_ref(p));
        let ds = Dataset::load(&plan);
        objects += ds.len();
        errors += ds.diags.count(Severity::Error);
    }
    eprintln!(
        "all {} campaigns: {} objects, {} errors",
        all.len(),
        objects,
        errors
    );
}

#[test]
fn corpus_token_syntax() {
    use pcgtools::lst::{LineKind, LstFile};
    use pcgtools::token::bonus::Bonus;
    use pcgtools::token::pre::Prereq;

    let mut pre = (0, 0);
    let mut bonus = (0, 0);
    let mut formula = (0, 0);
    let mut fails: Vec<String> = Vec::new();
    let mut ffails: Vec<String> = Vec::new();
    for dir in common::data_dirs() {
        for f in common::files_with_ext(&dir, &["lst"]) {
            let lf = LstFile::read(&f).unwrap();
            for l in &lf.lines {
                if !matches!(l.kind, LineKind::Record | LineKind::Continuation) {
                    continue;
                }
                for t in l.tokens() {
                    let v = t.value.unwrap_or("");
                    if Prereq::is_pre_tag(t.tag) {
                        pre.0 += 1;
                        match Prereq::from_token(t.tag, v) {
                            Ok(p) if p.to_string() == format!("{}:{}", t.tag, v) => pre.1 += 1,
                            Ok(p) => fails.push(format!("PRE unparse {} => {}", v, p)),
                            Err(e) => fails.push(format!("PRE {}:{} ({})", t.tag, v, e)),
                        }
                    } else if t.tag == "BONUS" {
                        bonus.0 += 1;
                        match Bonus::parse(v) {
                            Ok(b) if b.to_string() == v => {
                                bonus.1 += 1;
                                formula.0 += 1;
                                match b.expr() {
                                    Ok(_) => formula.1 += 1,
                                    Err(e) => ffails.push(format!("{} ({})", b.formula, e)),
                                }
                            }
                            Ok(b) => fails.push(format!("BONUS unparse {} => {}", v, b)),
                            Err(e) => fails.push(format!("BONUS {} ({})", v, e)),
                        }
                    }
                }
            }
        }
    }
    eprintln!("PRE: {}/{} round-trip", pre.1, pre.0);
    eprintln!("BONUS: {}/{} round-trip", bonus.1, bonus.0);
    eprintln!("BONUS formulas: {}/{} parse", formula.1, formula.0);
    for f in fails.iter().take(15) {
        eprintln!("  {}", f);
    }
    ffails.sort();
    ffails.dedup();
    for f in ffails.iter().take(400) {
        eprintln!("  formula: {}", f);
    }
    assert!(pre.1 * 1000 >= pre.0 * 999, "PRE round-trip below 99.9%");
    assert!(
        bonus.1 * 1000 >= bonus.0 * 999,
        "BONUS round-trip below 99.9%"
    );
}

#[test]
fn corpus_lint_unknown_tags() {
    use pcgtools::campaign::{LoadPlan, Registry};
    use pcgtools::lint::{lint_lst, LintOptions};
    use pcgtools::lst::LstFile;
    use pcgtools::pcc::resolve::DataDirs;

    let Some(root) = common::corpus_path("pcgen") else {
        return;
    };
    let mut reg = Registry::new(DataDirs::from_install(&root));
    let plan = LoadPlan::build(
        &mut reg,
        &[root.join("data/pathfinder/paizo/pathfinder_rpg_core.pcc")],
    );
    let opts = LintOptions {
        unknown_tags: true,
        deprecated: false,
        formulas: false,
    };
    let (mut tokens, mut unknown) = (0usize, 0usize);
    for pf in plan.ordered() {
        let lf = LstFile::read(&pf.path).unwrap();
        tokens += lf.lines.iter().map(|l| l.tokens().len()).sum::<usize>();
        unknown += lint_lst(&pf.path, pf.kind, &lf, &opts)
            .items
            .iter()
            .filter(|d| d.message.starts_with("unknown tag"))
            .count();
    }
    eprintln!("lint: {} unknown tags in {} tokens", unknown, tokens);
    // the catalog should recognize essentially every tag in shipped data
    assert!(
        unknown * 10000 < tokens,
        "{} unknown of {}",
        unknown,
        tokens
    );
}

/// Every CHOOSE, ADD and AUTO value in the data prints back exactly.
#[test]
fn corpus_choice_syntax() {
    use pcgtools::lst::{LineKind, LstFile};
    use pcgtools::token::choice::SubToken;

    let mut n = 0;
    let mut fails: Vec<String> = Vec::new();
    for dir in common::data_dirs() {
        for f in common::files_with_ext(&dir, &["lst"]) {
            let lf = LstFile::read(&f).unwrap();
            for l in &lf.lines {
                if !matches!(l.kind, LineKind::Record | LineKind::Continuation) {
                    continue;
                }
                for t in l.tokens() {
                    if !matches!(t.tag, "CHOOSE" | "ADD" | "AUTO") {
                        continue;
                    }
                    let v = t.value.unwrap_or("");
                    n += 1;
                    let back = SubToken::parse(v).to_string();
                    if back != v {
                        fails.push(format!("{}:{} => {}", t.tag, v, back));
                    }
                }
            }
        }
    }
    eprintln!("CHOOSE/ADD/AUTO: {}/{} round-trip", n - fails.len(), n);
    for f in fails.iter().take(20) {
        eprintln!("  {}", f);
    }
    assert!(fails.is_empty(), "{} values do not round-trip", fails.len());
}
