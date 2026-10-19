// Game mode loading against the synthetic fixture and the corpus.

use crate::common;
use pcgtools::gamemode::GameModes;

#[test]
fn fixture_modes() {
    let modes = GameModes::scan(&common::fixture("mini/system/gameModes"));
    assert!(modes.diags.is_empty(), "{:?}", modes.diags);
    assert_eq!(modes.modes.len(), 2);

    let tm = modes.find("tm").expect("alias lookup");
    assert_eq!(tm.key, "Testmode");
    assert_eq!(tm.misc("ACNAME"), Some("Defense"));
    assert_eq!(
        tm.stacking_bonus_types(),
        vec!["Dodge", "Circumstance", "Untyped"]
    );

    // inherited from default
    let rules = tm.file("rules.lst").unwrap();
    assert!(rules.inherited);
    assert_eq!(rules.lines[0].get("VAR"), Some("SYS_SAMPLE"));
    assert!(tm.file("bio/biosettings.lst").unwrap().inherited);
    assert!(!tm.file("level.lst").unwrap().inherited);

    // experience tables
    let slow = tm.level_table(None);
    assert_eq!(slow.len(), 3);
    assert_eq!(slow[0].class_skill_max, "LEVEL+3");
    let fast = tm.level_table(Some("Fast"));
    assert_eq!(fast.len(), 3);
    assert_eq!(fast[2].level, "LEVEL");
    assert_eq!(fast[2].min_xp, "LEVEL*1000");

    let sc: Vec<_> = tm.lines("statsandchecks.lst", "BONUSSPELLLEVEL").collect();
    assert_eq!(sc[0].get("BASESTATSCORE"), Some("12"));
}

#[test]
fn corpus_modes() {
    let Some(dir) = common::corpus_path("pcgen/system/gameModes") else {
        return;
    };
    let modes = GameModes::scan(&dir);
    assert!(modes.diags.is_empty(), "{:?}", modes.diags.items.first());
    assert!(modes.modes.len() >= 15);
    for m in &modes.modes {
        assert!(m.diags.is_empty(), "{}: {:?}", m.key, m.diags.items.first());
        assert!(m.file("miscinfo.lst").is_some());
    }
    let pf = modes.find("Pathfinder").unwrap();
    assert!(!pf.level_table(None).is_empty());
    assert!(!pf.stacking_bonus_types().is_empty());

    // every GAMEMODE named by a shipped campaign resolves
    use pcgtools::campaign::Registry;
    use pcgtools::pcc::resolve::DataDirs;
    let root = common::corpus_path("pcgen").unwrap();
    let mut reg = Registry::new(DataDirs::from_install(&root));
    let mut diags = pcgtools::Diagnostics::default();
    reg.scan(&root.join("data"), &mut diags);
    let mut unknown: Vec<String> = reg
        .campaigns()
        .flat_map(|c| c.gamemodes.iter())
        .filter(|g| modes.find(g).is_none())
        .cloned()
        .collect();
    unknown.sort();
    unknown.dedup();
    eprintln!("GAMEMODE values with no game mode: {:?}", unknown);
}
