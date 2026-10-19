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
