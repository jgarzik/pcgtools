// Corpus tests: run against real PCGen data when PCGEN_ROOT is set.

mod common;

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
