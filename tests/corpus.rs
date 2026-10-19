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
