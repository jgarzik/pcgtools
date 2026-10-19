// Dataset loading against the synthetic fixture under tests/fixtures/mini.

use crate::common;

use pcgtools::campaign::{LoadPlan, Registry};
use pcgtools::dataset::Dataset;
use pcgtools::lst::FileKind;
use pcgtools::pcc::resolve::DataDirs;

fn load() -> (LoadPlan, Dataset) {
    let data = common::fixture("mini/data");
    let mut reg = Registry::new(DataDirs {
        data: Some(data.clone()),
        ..Default::default()
    });
    let plan = LoadPlan::build(&mut reg, &[data.join("testbook/testbook.pcc")]);
    let ds = Dataset::load(&plan);
    (plan, ds)
}

#[test]
fn plan() {
    let (plan, _) = load();
    assert_eq!(plan.campaigns.len(), 2, "testbook plus included common");
    let spells: Vec<String> = plan.files[&FileKind::Spell]
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(spells, vec!["tb_spells.lst", "tb_spells_mod.lst"]);
    assert!(plan.files.contains_key(&FileKind::Language));
}

#[test]
fn objects_and_operations() {
    let (_, ds) = load();
    for d in &ds.diags.items {
        eprintln!("{}", d);
    }

    // stats and sizes are keyed by ABB
    assert_eq!(ds.get(FileKind::Stat, "MGT").unwrap().name, "Might");
    let small = ds.get(FileKind::Size, "s").unwrap();
    assert_eq!(small.get("BONUS"), Some("COMBAT|AC|1|TYPE=Size"));
    assert_eq!(small.history.len(), 1);

    // abilities: KEY, category-qualified MOD, COPY with .CLEAR
    let sh = ds
        .get_in(FileKind::Ability, "FEAT", "Steady Hands")
        .unwrap();
    assert_eq!(sh.get("DESC"), Some("Hands do not shake."));
    assert_eq!(sh.source_value("SOURCESHORT"), Some("TB"));
    assert!(ds.get(FileKind::Ability, "FEAT|QuickStep").is_some());
    let qs2 = ds.get(FileKind::Ability, "Quicker Step").unwrap();
    assert_eq!(qs2.types(), vec!["Advanced"]);
    // INCLUDE filter: only Keen Nose from the shared file
    assert!(ds.get(FileKind::Ability, "Keen Nose").is_some());
    assert!(ds.get(FileKind::Ability, "Loud Voice").is_none());

    // .MOD: single-valued tags replace, clearing forms drop values
    let glow = ds.get(FileKind::Spell, "Glow").unwrap();
    let vals = |tag: &str| glow.values(tag).collect::<Vec<_>>();
    assert_eq!(vals("SOURCEPAGE"), vec!["p.2"]);
    assert_eq!(vals("CLASSES"), vec!["Tinker=1"]);
    assert_eq!(vals("DESCRIPTOR"), vec!["Fire"]);
    assert!(vals("PRECLASS").is_empty());

    // spells: MOD applied, FORGET applied, PRECAMPAIGN-gated file skipped
    let spark = ds.get(FileKind::Spell, "Spark").unwrap();
    assert_eq!(spark.get("DESC"), Some("A tiny spark."));
    assert!(ds.get(FileKind::Spell, "Doomed Spell").is_none());
    assert_eq!(ds.objects(FileKind::Spell).len(), 2);

    // class: merged CLASS lines, level lines, subclass, .MOD with level line
    let t = ds.get(FileKind::Class, "Tinker").unwrap();
    assert_eq!(t.get("HD"), Some("8"));
    assert!(t.values("FACT").any(|v| v == "Abb|Tnk"));
    let levels: Vec<&str> = t.sublines.iter().map(|s| s.value.as_str()).collect();
    assert_eq!(levels, vec!["1:REPEATLEVEL:1", "1", "3", "5"]);
    assert_eq!(t.children.len(), 1);
    assert_eq!(t.children[0].sublines.len(), 1);

    // equipment: EXCLUDE filter, COPY, duplicate ignored
    assert!(ds.get(FileKind::Equipment, "Cursed Spoon").is_none());
    let silver = ds.get(FileKind::Equipment, "Silver Spoon").unwrap();
    assert_eq!(silver.get("COST"), Some("25"));
    let wooden = ds.get(FileKind::Equipment, "Wooden Spoon").unwrap();
    assert_eq!(wooden.get("WT"), Some("0.5"));
    assert!(ds
        .diags
        .items
        .iter()
        .any(|d| d.message.contains("duplicate object 'Wooden Spoon'")));

    // kits group their lines
    let kit = ds.get(FileKind::Kit, "Cook Kit").unwrap();
    let tags: Vec<&str> = kit.sublines.iter().map(|s| s.tag.as_str()).collect();
    assert_eq!(tags, vec!["REGION", "GEAR", "STAT"]);

    // included campaign's languages
    assert_eq!(ds.objects(FileKind::Language).len(), 2);

    // tables
    assert_eq!(ds.tables.len(), 1);
    assert_eq!(ds.tables[0].name, "Spoon Prices");
    assert_eq!(ds.tables[0].rows, vec![vec!["1", "10"], vec!["2", "20"]]);

    // no errors: the MOD of a filtered-out ability is silent
    let errs: Vec<String> = ds
        .diags
        .items
        .iter()
        .filter(|d| d.severity == pcgtools::Severity::Error)
        .map(|d| d.message.clone())
        .collect();
    assert!(errs.is_empty(), "{:?}", errs);
}

#[test]
fn shared_include_diagnostics_reported_once() {
    // two selected campaigns both include common.pcc, which names a
    // missing file; its warning must be reported once, not per include
    let data = common::fixture("mini/data");
    let mut reg = Registry::new(DataDirs {
        data: Some(data.clone()),
        ..Default::default()
    });
    let plan = LoadPlan::build(
        &mut reg,
        &[
            data.join("testbook/testbook.pcc"),
            data.join("other/other.pcc"),
        ],
    );
    let n = plan
        .diags
        .items
        .iter()
        .filter(|d| d.message.contains("missing_languages.lst"))
        .count();
    assert_eq!(n, 1, "{:?}", plan.diags.items);
}

/// Two campaigns defining the same objects: the newer source date
/// wins, an object's own SOURCEDATE counts, and ALLOWDUPES keeps both.
#[test]
fn duplicates_by_source_date_and_allowed() {
    let data = common::fixture("mini/data");
    let mut reg = Registry::new(DataDirs {
        data: Some(data.clone()),
        ..Default::default()
    });
    let plan = LoadPlan::build(
        &mut reg,
        &[data.join("dated/old.pcc"), data.join("dated/new.pcc")],
    );
    assert!(plan.allow_dupes.contains(&FileKind::Language));
    assert!(plan
        .forward_refs
        .contains(&("SPELL".to_string(), "Promised Spell".to_string())));
    let ds = Dataset::load(&plan);
    // the newer campaign's Flicker replaces the older one
    let f = ds.get(FileKind::Spell, "Flicker").unwrap();
    assert_eq!(f.get("SCHOOL"), Some("Illusion"));
    // Steady carries its own later date, so the first definition stays
    let st = ds.get(FileKind::Spell, "Steady").unwrap();
    assert_eq!(st.get("SCHOOL"), Some("Abjuration"));
    // both languages are kept; lookups find the first
    assert_eq!(
        ds.objects(FileKind::Language)
            .iter()
            .filter(|o| o.key == "Hush")
            .count(),
        2
    );
    assert_eq!(
        ds.get(FileKind::Language, "Hush").unwrap().get("TYPE"),
        Some("Spoken")
    );
}
