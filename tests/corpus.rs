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
