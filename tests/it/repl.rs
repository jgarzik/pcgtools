// Character editing sessions on the synthetic fixture.

use crate::common;
use pcgtools::pcc::resolve::DataDirs;
use pcgtools::repl::{Outcome, Session};

fn dirs() -> DataDirs {
    DataDirs {
        data: Some(common::fixture("mini/data")),
        system: Some(common::fixture("mini/system")),
        ..Default::default()
    }
}

fn session() -> Session {
    Session::open(&common::fixture("mini/characters/tess.pcg"), dirs()).unwrap()
}

fn ok(s: &mut Session, line: &str) -> String {
    match s.exec(line) {
        Ok(Outcome::Text(t)) | Ok(Outcome::Changed(t)) => t,
        Ok(Outcome::Quit) => "QUIT".into(),
        Err(e) => panic!("{}: {}", line, e),
    }
}

fn err(s: &mut Session, line: &str) -> String {
    match s.exec(line) {
        Err(e) => e.to_string(),
        Ok(o) => panic!("{} unexpectedly succeeded: {:?}", line, o),
    }
}

#[test]
fn info_and_queries() {
    let mut s = session();
    let info = ok(&mut s, "info");
    assert!(info.starts_with("Tess Tinkerton\n"), "{}", info);
    assert!(info.contains("level 2  BAB +1  HP 14  AC 12"), "{}", info);
    let st = ok(&mut s, "stats");
    assert!(st.contains("Tinkering +6"), "{}", st);
    assert_eq!(
        ok(&mut s, "explain SKILL.Tinkering"),
        "+3 [ClassSkill] from Skill Tinkering"
    );
    // copies are made after the definitions of their kind
    assert_eq!(
        ok(&mut s, "ls feat"),
        "Skill Knack (FEAT)\nSteady Hands (FEAT)\nQuickStep (FEAT)\nIron Fist (FEAT)\nAxe Familiarity (FEAT)\nAxe Grip (FEAT)\nArmor Ease (FEAT)\nLong Arm (FEAT)\nSpell Spring (FEAT)\nKeen Nose (FEAT)\nQuicker Step (FEAT)"
    );
    assert_eq!(
        ok(&mut s, "ls equipment \"*spoon\" TYPE=Kitchen"),
        "Wooden Spoon\nSilver Spoon"
    );
    assert!(ok(&mut s, "show spell spark").contains("SCHOOL:Evocation"));
    assert_eq!(
        ok(&mut s, "prereq feat \"Quick Step\""),
        "qualifies for QuickStep"
    );
    assert_eq!(
        ok(&mut s, "prereq race tallfolk"),
        "does not qualify for Tallfolk: PREALIGN:LG"
    );
    assert!(ok(&mut s, "help").contains("class add"));
    assert_eq!(ok(&mut s, "quit"), "QUIT");
    assert!(!s.dirty);
}

#[test]
fn edits_and_undo() {
    let mut s = session();
    assert!(ok(&mut s, "set stat grc 16").contains("AC 14"));
    assert!(s.dirty);
    let out = ok(&mut s, "class add Tinker 5");
    assert!(out.starts_with("Tinker is now level 3"), "{}", out);
    assert!(out.contains("BAB +2"), "{}", out);
    ok(&mut s, "race Gearling");
    assert_eq!(s.ch.race().as_deref(), Some("Gearling"));
    // small size: +1 AC
    assert!(s.summary().contains("AC 16"), "{}", s.summary());
    ok(&mut s, "feat add \"Quick Step\"");
    assert!(err(&mut s, "feat add \"Quick Step\"").contains("already has"));
    ok(&mut s, "feat rm QuickStep");
    ok(&mut s, "skill Tinkering 5");
    assert!(err(&mut s, "skill Tinkering 40").contains("exceeds the maximum"));
    ok(&mut s, "skill Tinkering 40 --force");
    ok(&mut s, "equip add \"Wooden Spoon\" 3");
    assert!(ok(&mut s, "equip unwear \"Tin Shield\"").contains("AC 15"));
    ok(&mut s, "equip wear \"Tin Shield\"");
    ok(&mut s, "lang add \"Old Script\"");
    ok(&mut s, "set city \"New Gearton\"");
    ok(&mut s, "set align TN");
    assert_eq!(s.ch.pcg.value("CITY").as_deref(), Some("New Gearton"));

    // undo walks back one change at a time
    ok(&mut s, "undo");
    assert_eq!(s.ch.alignment(), None);
    ok(&mut s, "undo");
    assert_eq!(s.ch.pcg.value("CITY").as_deref(), Some("Gear:town"));
    assert!(ok(&mut s, "class rm").contains("removed a level of Tinker"));
}

#[test]
fn rejects_bad_input() {
    let mut s = session();
    assert!(err(&mut s, "race Dragonfolk").contains("no RACE named"));
    assert!(err(&mut s, "race tallfolk").contains("does not qualify"));
    ok(&mut s, "race tallfolk --force");
    assert!(err(&mut s, "class add Wizard").contains("no CLASS named"));
    assert!(err(&mut s, "feat add Nonexistent").contains("no ABILITY named"));
    assert!(err(&mut s, "set stat XYZ 3").contains("no STAT named"));
    assert!(err(&mut s, "set stat MGT lots").contains("must be a number"));
    assert!(err(&mut s, "set colour blue").contains("cannot set"));
    assert!(err(&mut s, "equip wear Lantern").contains("has no 'Lantern'"));
    assert!(err(&mut s, "frobnicate").contains("unknown command"));
    assert!(err(&mut s, "ls").contains("missing argument"));
    assert_eq!(
        ok(&mut s, "undo"),
        "undone\nlevel 2  BAB +1  HP 14  AC 12  Quickness +1 Toughness +5"
    );
    assert_eq!(ok(&mut s, "undo"), "nothing to undo");
}

#[test]
fn save_and_reload() {
    let mut s = session();
    ok(&mut s, "class add Tinker");
    ok(&mut s, "feat add \"Quick Step\"");
    let dir = std::env::temp_dir().join(format!("pcgtools-repl-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("tess2.pcg");
    ok(&mut s, &format!("save \"{}\"", out.display()));
    assert!(!s.dirty);
    let s2 = Session::open(&out, dirs()).unwrap();
    assert_eq!(s2.summary(), s.summary());
    assert_eq!(s2.ch.total_level(), 3);
    assert!(s2.ch.abilities().iter().any(|a| a.key == "QuickStep"));
}

#[test]
fn new_character() {
    let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
    assert!(
        s.ctx.diags.count(pcgtools::Severity::Error) == 0,
        "{:?}",
        s.ctx.diags
    );
    assert_eq!(s.ch.stats().len(), 2);
    ok(&mut s, "race Gearling");
    ok(&mut s, "class add Tinker");
    ok(&mut s, "skill Tinkering 1");
    let sheet = s.sheet();
    assert_eq!(sheet.level, 1);
    // hit die 8 (raised by a .MOD) plus MGT modifier 0
    assert_eq!(sheet.hp, 8);
    assert_eq!(sheet.skills["Tinkering"], 1 + 1 + 3);
    assert!(err(&mut s, "save").contains("no file name"));
}

#[test]
fn corpus_new_fighter() {
    let Some(root) = common::corpus_path("pcgen") else {
        return;
    };
    let dirs = DataDirs::from_install(&root);
    let mut s = Session::create("Pathfinder", &["Core Rulebook".into()], "Brakka", dirs);
    for cmd in [
        "set stat STR 16",
        "set stat DEX 14",
        "set stat CON 14",
        "race Human",
        "set align LN",
        "class add Fighter",
        "feat add \"Power Attack\"",
        "feat add Cleave",
        "skill Climb 1",
        "equip add \"Chain Shirt\"",
        "equip wear \"Chain Shirt\"",
        "class add Fighter 7",
    ] {
        ok(&mut s, cmd);
    }
    let sheet = s.sheet();
    assert_eq!(sheet.level, 2);
    assert_eq!(sheet.bab, 2);
    assert_eq!(sheet.hp, 10 + 2 + 7 + 2);
    assert_eq!(sheet.saves["Fortitude"], 3 + 2);
    assert_eq!(sheet.ac["Total"], 10 + 4 + 2);
    // 1 rank + STR 3 + class skill 3 + armor check -2
    assert_eq!(sheet.skills["Climb"], 5);
    assert!(ok(&mut s, "prereq feat \"Great Cleave\"").contains("PRETOTALAB:4"));
}

#[test]
fn vars_command() {
    let mut s = Session::open(&common::fixture("mini/characters/vera.pcg"), dirs()).unwrap();
    let out = ok(&mut s, "vars PC.MOVEMENT");
    assert_eq!(out, "PC.MOVEMENT:Walk\n  Speed = 40");
    let mut t = session();
    assert_eq!(ok(&mut t, "vars PC.MOVEMENT"), "no declared variables");
}

#[test]
fn skill_points_are_spent_and_checked() {
    let mut s = session();
    // the saved levels have no points left
    assert!(err(&mut s, "skill Climbing 1").contains("skill points needed"));
    // a new level brings the class's points
    ok(&mut s, "class add Tinker 5");
    assert_eq!(s.ch.skill_points_left("Tinker"), 4.0);
    assert!(ok(&mut s, "info").contains("Tinker skill points 4"));
    // a class skill costs one point a rank
    ok(&mut s, "skill Tinkering 4");
    assert_eq!(s.ch.skill_points_left("Tinker"), 2.0);
    // lowering ranks refunds points
    ok(&mut s, "skill Tinkering 3");
    assert_eq!(s.ch.skill_points_left("Tinker"), 3.0);
    // too many for what is left
    assert!(err(&mut s, "skill Climbing 3").contains("needed"));
    ok(&mut s, "skill Climbing 3 --force");
}

#[test]
fn ability_pools_and_choices() {
    let mut s = session();
    // FEAT pool: class level + 1 = 3; two chosen already
    let info = ok(&mut s, "info");
    assert!(info.contains("Feat 2/3"), "{}", info);
    // choices must be what CHOOSE offers
    assert!(err(&mut s, "feat add \"Skill Knack\"")
        .contains("needs a choice; choose from: Climbing, Tinkering"));
    assert!(err(&mut s, "feat add \"Skill Knack\" Swimming").contains("is not a choice"));
    ok(&mut s, "feat add \"Skill Knack\" Climbing");
    assert!(ok(&mut s, "info").contains("Feat 3/3"));
    // the pool is spent
    assert!(err(&mut s, "feat add \"Quick Step\"").contains("no Feat points left"));
    ok(&mut s, "feat add \"Quick Step\" --force");
}

#[test]
fn subclass_spells_followers_and_kits() {
    let mut s = session();
    assert!(err(&mut s, "class subclass Tinker Nope").contains("no subclass"));
    ok(&mut s, "class subclass Tinker Gearwright");
    let text = String::from_utf8(s.ch.pcg.encode()).unwrap();
    assert!(
        text.contains("CLASS:Tinker|SUBCLASS:Gearwright"),
        "{}",
        text
    );
    ok(&mut s, "class add Tinker --subclass Gearwright");
    assert_eq!(s.ch.total_level(), 3);

    // spells: level from the class list, or given
    assert!(ok(&mut s, "spell add Tinker Spark").contains("level 0"));
    ok(&mut s, "spell add Tinker Glow 1 Gadgets");
    assert_eq!(s.ch.spells().len(), 2);
    assert!(err(&mut s, "spell rm Tinker Fireball").contains("has no spell"));
    ok(&mut s, "spell rm Tinker Spark");
    assert_eq!(s.ch.spells().len(), 1);

    // followers
    ok(&mut s, "follower add Whiskers Familiar Gearling");
    assert_eq!(
        ok(&mut s, "follower ls"),
        "Whiskers\tFamiliar\tGearling\tWhiskers.pcg"
    );
    ok(&mut s, "follower rm Whiskers");
    assert!(err(&mut s, "follower rm Whiskers").contains("no follower"));

    // kits add their gear (Tess already carries two spoons) and stats
    ok(&mut s, "kit apply \"Cook Kit\"");
    assert_eq!(s.ch.stat("MGT"), Some(12));
    let gear: Vec<_> =
        s.ch.equipment()
            .iter()
            .map(|e| (e.name.clone(), e.quantity))
            .collect();
    assert!(
        gear.contains(&("Wooden Spoon".to_string(), 4.0)),
        "{:?}",
        gear
    );
}

#[test]
fn equipment_customization() {
    let mut s = session();
    ok(&mut s, "equip add \"Wooden Spoon\"");
    ok(&mut s, "equip customize \"Wooden Spoon\" +SHARP");
    ok(&mut s, "equip customize \"Wooden Spoon\" +ENGRAVED|Tess");
    ok(&mut s, "equip customize \"Wooden Spoon\" size S");
    let (base, data) = s.ch.customization("Wooden Spoon").unwrap();
    assert_eq!(base, "Wooden Spoon");
    assert_eq!(data, "NAME=Wooden Spoon$SIZE=S$EQMOD=SHARP.ENGRAVED|Tess");
    ok(&mut s, "equip customize \"Wooden Spoon\" -SHARP");
    assert_eq!(
        s.ch.customization("Wooden Spoon").unwrap().1,
        "NAME=Wooden Spoon$SIZE=S$EQMOD=ENGRAVED|Tess"
    );
    assert!(err(&mut s, "equip customize \"Wooden Spoon\" -SHARP").contains("no modifier"));
    assert!(err(&mut s, "equip customize Lantern +SHARP").contains("has no 'Lantern'"));
    assert!(err(&mut s, "equip customize \"Wooden Spoon\" +NOPE").contains("no EQUIPMOD named"));
}

#[test]
fn new_character_records_game_mode_key() {
    // a mode may be named by an alias; the file must name its key
    let s = Session::create("TM", &["TB".into()], "Nova", dirs());
    assert_eq!(s.ch.gamemode().as_deref(), Some("Testmode"));
}

#[test]
fn worn_items_go_to_game_mode_slots() {
    let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
    ok(&mut s, "equip add \"Tin Shield\"");
    ok(&mut s, "equip wear \"Tin Shield\"");
    // the slot whose CONTAINS lists the item's type, even for a shield
    // that is also a weapon (for bashing)
    let placed = s.ch.equipped();
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].location, "Arm");
    // an explicit location still wins
    ok(&mut s, "equip unwear \"Tin Shield\"");
    ok(&mut s, "equip wear \"Tin Shield\" \"Off Hand\"");
    assert_eq!(s.ch.equipped()[0].location, "Off Hand");
}

#[test]
fn domains_record_the_class_granting_them() {
    let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
    // no class grants domains yet
    ok(&mut s, "domain add Cogs");
    let line = |s: &Session| {
        String::from_utf8(s.ch.pcg.encode())
            .unwrap()
            .lines()
            .find(|l| l.starts_with("DOMAIN:"))
            .unwrap()
            .to_string()
    };
    assert_eq!(line(&s), "DOMAIN:Cogs");
    ok(&mut s, "undo");
    // a Tinker gains a domain at level 1
    ok(&mut s, "class add Tinker");
    ok(&mut s, "domain add Cogs");
    assert_eq!(
        line(&s),
        "DOMAIN:Cogs|SOURCE:[TYPE:PCClass|NAME:Tinker|LEVEL:1]"
    );
}

#[test]
fn equipment_commands_accept_display_names() {
    // "Copper Pot" is the display name of the item keyed "Pot (Copper)"
    let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
    ok(&mut s, "equip add \"Copper Pot\"");
    ok(&mut s, "equip wear \"Copper Pot\" Carried");
    assert_eq!(s.ch.equipped()[0].item.as_deref(), Some("Pot (Copper)"));
    ok(&mut s, "equip unwear \"Copper Pot\"");
    ok(&mut s, "equip rm \"Copper Pot\"");
    assert!(s.ch.equipment().is_empty());
}

#[test]
fn bonus_skill_points_for_class_and_character_levels() {
    let gained = |race: bool| {
        let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
        if race {
            ok(&mut s, "set align LG");
            ok(&mut s, "race Tallfolk");
        }
        ok(&mut s, "class add Tinker");
        let first = s.ch.skill_points_left("Tinker");
        ok(&mut s, "class add Tinker");
        (first, s.ch.skill_points_left("Tinker") - first)
    };
    let (plain1, plain2) = gained(false);
    let (tall1, tall2) = gained(true);
    // the race adds 2 at Tinker level 1 and 3 on the class's first
    // level, and 1 at character level 2
    assert_eq!(tall1, plain1 + 5.0);
    assert_eq!(tall2, plain2 + 1.0);

    // the class itself adds one more at its level 4
    let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
    let mut gains = Vec::new();
    for _ in 0..4 {
        let before = s.ch.skill_points_left("Tinker");
        ok(&mut s, "class add Tinker");
        gains.push(s.ch.skill_points_left("Tinker") - before);
    }
    assert_eq!(gains[3], gains[2] + 1.0, "{:?}", gains);
}

#[test]
fn new_characters_record_an_age() {
    // without one PCGen rolls an age each time it loads the file
    let s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
    assert_eq!(s.ch.pcg.value("AGE").as_deref(), Some("0"));
}
