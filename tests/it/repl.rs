// Character editing sessions on the synthetic fixture.

use crate::common;
use pcgtools::pcc::resolve::DataDirs;
use pcgtools::pcg::character::Character;
use pcgtools::pcg::PcgFile;
use pcgtools::repl::{Outcome, Session};
use pcgtools::rules::calc::Calc;

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
        "Skill Knack (FEAT)\nSteady Hands (FEAT)\nQuickStep (FEAT)\nIron Fist (FEAT)\nAxe Familiarity (FEAT)\nAxe Grip (FEAT)\nArmor Ease (FEAT)\nOdd Knack (FEAT)\nEven Knack (FEAT)\nLong Arm (FEAT)\nSpell Spring (FEAT)\nFlicker (FEAT)\nWide Spell (FEAT)\nSharp Spell (FEAT)\nKeen Nose (FEAT)\nQuicker Step (FEAT)"
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
fn feat_pool_bonuses() {
    let mut s = session();
    // BONUS:FEAT POOL 1, PCPOOL 1.5 and MONSTERPOOL 0.5 on top of
    // ABILITYPOOL 3: the PC and monster parts round down on their own
    ok(&mut s, "template add Studious");
    let info = ok(&mut s, "info");
    assert!(info.contains("Feat 2/5"), "{}", info);
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
    // a book other than the known spells is a prepared list
    let text = String::from_utf8(s.ch.pcg.encode()).unwrap();
    assert!(text.contains("SPELLBOOK:Gadgets|TYPE:2\n"), "{}", text);
    // a second copy in a list is counted on the same line, and copies
    // are removed one at a time
    ok(&mut s, "spell add Tinker Glow 1 Gadgets");
    let glow = |s: &Session| -> Vec<i64> {
        s.ch.spells()
            .into_iter()
            .filter(|x| x.0 == "Glow")
            .map(|x| x.4)
            .collect()
    };
    assert_eq!(glow(&s), [2]);
    ok(&mut s, "spell rm Tinker Glow Gadgets");
    assert_eq!(glow(&s), [1]);
    ok(&mut s, "spell rm Tinker Glow Gadgets");
    assert!(glow(&s).is_empty());
    ok(&mut s, "spell add Tinker Glow 1 Gadgets");
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
fn skill_points_count_variables_the_class_defines() {
    // a class whose points come from a variable it defines is counted
    // with the new level held; this game mode never recounts them, so
    // the saved pool must be right as gained
    let gained = |class: &str| {
        let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
        ok(&mut s, &format!("class add {}", class));
        let saved = Character::from_pcg(PcgFile::parse(&pcg_text(&s)));
        saved.skill_points_left(class)
    };
    // Drillmaster's 3 points are one fewer than Tinker's STARTSKILLPTS:4
    let tinker = gained("Tinker");
    assert!(tinker >= 2.0, "{}", tinker);
    assert_eq!(gained("Drillmaster"), tinker - 1.0);
}

#[test]
fn monster_skill_points() {
    // skill points gained by each level of a class, after some commands
    let gains = |class: &str, setup: &[&str], n: usize| {
        let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
        for c in setup {
            ok(&mut s, c);
        }
        let mut gains = Vec::new();
        for _ in 0..n {
            let before = s.ch.skill_points_left(class);
            ok(&mut s, &format!("class add {} --force", class));
            gains.push(s.ch.skill_points_left(class) - before);
        }
        gains
    };
    // the first level is worth MONSKILL instead of the class's points;
    // levels up to MONNONSKILLHD (for a medium creature) are worth none
    assert_eq!(gains("Brute", &[], 3), vec![8.0, 0.0, 2.0]);
    // a locked number replaces the points of every level
    assert_eq!(
        gains("Brute", &["template add Dragonblood"], 3),
        vec![6.0, 0.0, 6.0]
    );
    // a template's bonus points come on top, even of a level worth none
    assert_eq!(
        gains("Brute", &["template add Learned"], 3),
        vec![9.0, 1.0, 3.0]
    );
    // classes that are not monsters keep their points, plus the
    // template's
    let tinker = gains("Tinker", &[], 2);
    assert_eq!(gains("Tinker", &["template add Dragonblood"], 2), tinker);
    let learned: Vec<f64> = tinker.iter().map(|g| g + 1.0).collect();
    assert_eq!(gains("Tinker", &["template add Learned"], 2), learned);
}

#[test]
fn new_characters_record_an_age() {
    // without one PCGen rolls an age each time it loads the file
    let s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
    assert_eq!(s.ch.pcg.value("AGE").as_deref(), Some("0"));
}

/// Lines of a session's character file starting with `prefix`.
fn lines(s: &Session, prefix: &str) -> Vec<String> {
    String::from_utf8(s.ch.pcg.encode())
        .unwrap()
        .lines()
        .filter(|l| l.starts_with(prefix))
        .map(str::to_string)
        .collect()
}

/// Feat points left.
fn feats_left(s: &Session) -> f64 {
    let mut c = Calc::new(&s.ctx.dataset, s.ctx.gamemode.as_ref(), &s.ch);
    c.run();
    pcgtools::rules::pools::pools(&c)
        .iter()
        .find(|p| p.category == "FEAT")
        .map_or(0.0, |p| p.left())
}

#[test]
fn level_choices_from_add_tokens() {
    let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
    ok(&mut s, "class add Tinker");
    ok(&mut s, "class add Chanter");

    // ADD:SPELLCASTER|ANY offers both spellcasting classes
    let e = err(&mut s, "class add Mystic");
    assert!(e.contains("Chanter, Tinker"), "{}", e);
    let e = err(&mut s, "class add Mystic --choose Drifter");
    assert!(e.contains("'Drifter' is not"), "{}", e);
    assert!(e.contains("Chanter, Tinker"), "{}", e);
    // the class line's own choice (of two class skills) is needed too
    let e = err(&mut s, "class add Mystic --choose Tinker");
    assert!(e.contains("Climbing, Tinkering"), "{}", e);
    let e = err(
        &mut s,
        "class add Mystic --choose Tinker --choose Climbing --choose x",
    );
    assert!(e.contains("unused"), "{}", e);
    assert!(s.ch.classes().iter().all(|c| c.name != "Mystic"));

    // Divine has one class to offer, which is taken without asking
    // (naming it is accepted too)
    ok(
        &mut s,
        "class add Mystic --choose tinker --choose Chanter --choose climbing",
    );
    assert_eq!(
        lines(&s, "CLASS:Mystic"),
        ["CLASS:Mystic|LEVEL:1|SKILLPOOL:0|ADD:[CLASSSKILLS:Climbing,Tinkering|CHOICE:Climbing]"]
    );
    let lvl = &lines(&s, "CLASSABILITIESLEVEL:Mystic=1")[0];
    assert!(
        lvl.starts_with(
            "CLASSABILITIESLEVEL:Mystic=1|HITPOINTS:4|SAVES:[SAVE:BONUS&pipe;PCLEVEL&pipe;TINKER&pipe;1|SAVE:BONUS&pipe;PCLEVEL&pipe;CHANTER&pipe;1]|ADD:[SPELLCASTER:ANY|CHOICE:Tinker]|ADD:[SPELLCASTER:Divine|CHOICE:Chanter]|SKILLSGAINED:"
        ),
        "{}",
        lvl
    );
    // the choices take effect
    assert!(ok(&mut s, "explain PCLEVEL.Tinker").contains("+1"));
    assert!(ok(&mut s, "explain PCLEVEL.Chanter").contains("+1"));
    {
        let mut c = Calc::new(&s.ctx.dataset, s.ctx.gamemode.as_ref(), &s.ch);
        c.run();
        assert!(c.is_class_skill_for("Climbing", "Mystic"));
        assert!(c.is_class_skill("Climbing"));
        assert!(!c.is_class_skill_for("Tinkering", "Mystic"));
    }

    // one ability qualifies (Steady Hands is had already); the
    // language needs a choice
    let e = err(&mut s, "class add Mystic");
    assert!(e.contains("Old Script, Trade Tongue"), "{}", e);
    let left = feats_left(&s);
    ok(&mut s, "class add Mystic --choose \"Trade Tongue\"");
    let lvl = &lines(&s, "CLASSABILITIESLEVEL:Mystic=2")[0];
    assert!(
        lvl.starts_with(
            "CLASSABILITIESLEVEL:Mystic=2|HITPOINTS:4|ADD:[ABILITY:Iron Fist,TYPE=Combat|CHOICE:CATEGORY=FEAT&pipe;NATURE=NORMAL&pipe;Iron Fist]|ADD:[LANGUAGE:Old Script,TYPE=Spoken|CHOICE:Trade Tongue]|SKILLSGAINED:"
        ),
        "{}",
        lvl
    );
    assert_eq!(lines(&s, "LANGUAGE"), ["LANGUAGE:Trade Tongue"]);
    assert!(lines(&s, "ABILITY:")
        .contains(&"ABILITY:FEAT|TYPE:NORMAL|CATEGORY:FEAT|KEY:Iron Fist".to_string()));
    // the granted feat costs no feat points
    assert_eq!(lines(&s, "USERPOOL"), ["USERPOOL:FEAT|POOLPOINTS:1.0"]);
    assert_eq!(feats_left(&s), left);

    // a token whose prerequisites fail offers nothing
    ok(&mut s, "class add Mystic");
    assert!(!lines(&s, "CLASSABILITIESLEVEL:Mystic=3")[0].contains("ADD:"));
    ok(&mut s, "undo");
    ok(&mut s, "undo");
    assert!(lines(&s, "USERPOOL").is_empty());
    assert!(lines(&s, "LANGUAGE").is_empty());

    // with --force, a choice not offered skips its token
    ok(&mut s, "class add Mystic --choose Nonsense --force");
    let lvl = &lines(&s, "CLASSABILITIESLEVEL:Mystic=2")[0];
    assert!(
        lvl.contains("ABILITY:Iron Fist") && !lvl.contains("LANGUAGE"),
        "{}",
        lvl
    );
}

#[test]
fn point_buy_stats() {
    let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
    // two methods: one must be named
    let e = err(&mut s, "stats buy");
    assert!(e.contains("Modest, Grand"), "{}", e);
    assert!(err(&mut s, "stats buy Lavish").contains("Modest, Grand"));
    ok(&mut s, "stats buy modest");
    assert_eq!(s.ch.stats(), [("MGT".into(), 10), ("GRC".into(), 10)]);
    assert_eq!(lines(&s, "ROLLMETHOD"), ["ROLLMETHOD:2|EXPRESSION:Modest"]);
    assert_eq!(
        lines(&s, "PURCHASEPOINTS"),
        ["PURCHASEPOINTS:Y|TYPE:Modest"]
    );
    assert_eq!(
        lines(&s, "POOLPOINTS"),
        ["POOLPOINTS:0", "POOLPOINTSAVAIL:6"]
    );

    // the points spent are kept as scores change
    ok(&mut s, "set stat MGT 14");
    assert_eq!(lines(&s, "POOLPOINTS:"), ["POOLPOINTS:5"]);
    // 13 costs what 12 does (no row of its own)
    let e = err(&mut s, "set stat GRC 13");
    assert!(e.contains("7 points") && e.contains("budget of 6"), "{}", e);
    ok(&mut s, "set stat GRC 11");
    assert_eq!(lines(&s, "POOLPOINTS:"), ["POOLPOINTS:6"]);
    let out = ok(&mut s, "stats method");
    assert!(
        out.contains("point buy (Modest): 6 of 6 points spent"),
        "{}",
        out
    );
    assert!(out.contains("scores 7 to 16"), "{}", out);
    // beyond what can be bought, or over budget, needs --force
    assert!(err(&mut s, "set stat GRC 17").contains("between 7 and 16"));
    assert!(err(&mut s, "set stat GRC 6").contains("between 7 and 16"));
    ok(&mut s, "set stat GRC 7");
    assert_eq!(lines(&s, "POOLPOINTS:"), ["POOLPOINTS:1"]);
    ok(&mut s, "set stat GRC 17 --force");
    assert_eq!(lines(&s, "POOLPOINTS:"), ["POOLPOINTS:15"]);

    // first level: still bought; second: no longer
    ok(&mut s, "set stat GRC 10");
    ok(&mut s, "class add Tinker");
    ok(&mut s, "set stat GRC 11");
    assert_eq!(lines(&s, "POOLPOINTS:"), ["POOLPOINTS:6"]);
    ok(&mut s, "class add Tinker");
    ok(&mut s, "set stat GRC 16");
    assert_eq!(lines(&s, "POOLPOINTS:"), ["POOLPOINTS:6"]);
    assert!(err(&mut s, "stats buy Grand").contains("--force"));
}

#[test]
fn stat_ranges_and_other_methods() {
    let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
    // the stat's own range (STATRANGE)
    assert!(err(&mut s, "set stat MGT 31").contains("between 1 and 30"));
    assert!(err(&mut s, "set stat MGT 0").contains("between 1 and 30"));
    ok(&mut s, "set stat MGT 31 --force");
    // no range: any score
    ok(&mut s, "set stat GRC 99");

    ok(&mut s, "stats same 12");
    assert_eq!(s.ch.stats(), [("MGT".into(), 12), ("GRC".into(), 12)]);
    assert_eq!(lines(&s, "ROLLMETHOD"), ["ROLLMETHOD:1|EXPRESSION:12"]);
    assert_eq!(lines(&s, "PURCHASEPOINTS"), ["PURCHASEPOINTS:N"]);

    ok(&mut s, "stats assign 15 9");
    assert_eq!(s.ch.stats(), [("MGT".into(), 15), ("GRC".into(), 9)]);
    assert_eq!(lines(&s, "ROLLMETHOD"), ["ROLLMETHOD:0|EXPRESSION:0"]);
    assert!(err(&mut s, "stats assign 15").contains("2 scores"));
    assert!(err(&mut s, "stats assign 40 9").contains("between 1 and 30"));

    // rolling: the first method by its sort key, unless one is named
    ok(&mut s, "stats roll --seed 3");
    assert_eq!(lines(&s, "ROLLMETHOD"), ["ROLLMETHOD:3|EXPRESSION:3d6"]);
    let first = s.ch.stats();
    assert!(
        first.iter().all(|(_, v)| (3..=18).contains(v)),
        "{:?}",
        first
    );
    ok(&mut s, "stats roll --seed 3");
    assert_eq!(s.ch.stats(), first);
    ok(&mut s, "stats roll \"4d6 drop lowest\" --seed 9");
    assert_eq!(
        lines(&s, "ROLLMETHOD"),
        ["ROLLMETHOD:3|EXPRESSION:roll(4,6,top(3))"]
    );
    assert!(err(&mut s, "stats roll 5d6").contains("3d6, 4d6 drop lowest"));

    // past the first level, scores are no longer generated
    let mut t = session();
    assert!(err(&mut t, "stats same 10").contains("--force"));
    ok(&mut t, "stats same 10 --force");
}

#[test]
fn level_stat_increases() {
    let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
    for _ in 0..5 {
        ok(&mut s, "class add Tinker");
    }
    // level 6 grants an increase (the mode's 6|3)
    let e = err(&mut s, "class add Tinker");
    assert!(
        e.contains("1 stat increase") && e.contains("--stat"),
        "{}",
        e
    );
    assert!(err(&mut s, "class add Tinker --stat Luck").contains("no STAT named"));
    assert!(err(&mut s, "class add Tinker --stat MGT --stat GRC").contains("1 stat increase"));
    assert_eq!(s.ch.total_level(), 5);
    ok(&mut s, "class add Tinker --stat MGT");
    assert_eq!(s.ch.stat("MGT"), Some(11));
    let lvl = &lines(&s, "CLASSABILITIESLEVEL:Tinker=6")[0];
    assert!(lvl.contains("|PRESTAT:MGT=1|SKILLSGAINED:"), "{}", lvl);
    // taking the level away takes its increase too
    ok(&mut s, "class rm");
    assert_eq!(s.ch.stat("MGT"), Some(10));
    // left pending with --force
    ok(&mut s, "class add Tinker --force");
    assert_eq!(lines(&s, "POOLPOINTS:"), ["POOLPOINTS:1"]);
    assert!(!lines(&s, "CLASSABILITIESLEVEL:Tinker=6")[0].contains("PRESTAT"));
    // a level that grants none takes no --stat
    assert!(err(&mut s, "class add Tinker --stat MGT").contains("0 stat increase"));
}

#[test]
fn stat_edits_after_levels_are_recorded() {
    let mut s = session();
    ok(&mut s, "set stat MGT 15");
    assert!(lines(&s, "CLASSABILITIESLEVEL:Tinker=2")[0].contains("|PRESTAT:MGT=1|SKILLSGAINED"));
    ok(&mut s, "set stat MGT 13");
    assert!(lines(&s, "CLASSABILITIESLEVEL:Tinker=2")[0]
        .contains("|PRESTAT:MGT=1|PRESTAT:MGT=-2|SKILLSGAINED"));
    ok(&mut s, "class rm");
    assert_eq!(s.ch.stat("MGT"), Some(14));
}

#[test]
fn gold_buying_and_selling() {
    let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
    ok(&mut s, "set gold 100");
    assert_eq!(lines(&s, "MONEY"), ["MONEY:100"]);
    ok(&mut s, "set gold +25.5");
    ok(&mut s, "set gold -0.5");
    assert_eq!(lines(&s, "MONEY"), ["MONEY:125"]);
    assert!(err(&mut s, "set gold lots").contains("amount"));

    // buying pays the full price
    ok(&mut s, "equip buy \"Tin Shield\" 2");
    assert_eq!(s.ch.gold(), 115.0);
    assert_eq!(s.ch.equipment()[0].quantity, 2.0);
    let e = err(&mut s, "equip buy \"Great Axe\" 10");
    assert!(e.contains("200") && e.contains("115"), "{}", e);
    // in debt only when allowed (or forced)
    ok(&mut s, "set debt on");
    assert_eq!(lines(&s, "ALLOWDEBT"), ["ALLOWDEBT:Y"]);
    ok(&mut s, "equip buy \"Great Axe\" 10");
    assert_eq!(s.ch.gold(), -85.0);
    ok(&mut s, "undo");
    ok(&mut s, "undo");
    ok(&mut s, "equip buy \"Great Axe\" 10 --force");
    assert_eq!(s.ch.gold(), -85.0);
    ok(&mut s, "undo");
    assert!(info_line(&mut s, "gold:").contains("115 gp"));

    // selling refunds half, a gem all of its price
    ok(&mut s, "equip sell \"Tin Shield\"");
    assert_eq!(s.ch.gold(), 117.5);
    assert_eq!(s.ch.equipment()[0].quantity, 1.0);
    ok(&mut s, "equip wear \"Tin Shield\"");
    ok(&mut s, "equip sell \"Tin Shield\" all");
    assert_eq!(s.ch.gold(), 120.0);
    assert!(s.ch.equipment().is_empty() && s.ch.equipped().is_empty());
    ok(&mut s, "equip buy Ruby");
    ok(&mut s, "equip sell Ruby");
    assert_eq!(s.ch.gold(), 120.0);
    assert!(err(&mut s, "equip sell Ruby").contains("has no"));
    // a free addition costs nothing
    ok(&mut s, "equip add Ruby");
    assert_eq!(s.ch.gold(), 120.0);
}

#[test]
fn kits_bring_funds_and_pay_for_gear() {
    let kit = |name: &str| {
        let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
        let out = ok(&mut s, &format!("kit apply \"{}\"", name));
        let gear: Vec<(String, f64)> =
            s.ch.equipment()
                .iter()
                .map(|e| (e.name.clone(), e.quantity))
                .collect();
        (s.ch.gold(), gear, lines(&s, "KIT"), out)
    };
    // funds first, then two spoons at 1 each
    let (gold, gear, kits, _) = kit("Cook Kit");
    assert_eq!(gold, 8.0);
    assert_eq!(gear, [("Wooden Spoon".to_string(), 2.0)]);
    assert_eq!(kits, ["KIT:Cook Kit"]);
    // a line's prerequisites decide whether it applies
    let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
    ok(&mut s, "class add Tinker");
    ok(&mut s, "kit apply \"Cook Kit\"");
    assert_eq!(s.ch.gold(), 108.0);
    // 25 funds buy one of two axes at 20
    let (gold, gear, _, out) = kit("Axe Kit");
    assert_eq!(gold, 5.0);
    assert_eq!(gear, [("Great Axe".to_string(), 1.0)]);
    assert!(out.contains("could not buy 1 Great Axe"), "{}", out);
    // free gear
    let (gold, gear, _, _) = kit("Free Kit");
    assert_eq!(gold, 0.0);
    assert_eq!(gear.len(), 1);
    // one price for the whole kit, even into debt; applied at once,
    // so not recorded
    let (gold, gear, kits, _) = kit("Pricey Kit");
    assert_eq!(gold, -30.0);
    assert_eq!(gear.len(), 1);
    assert!(kits.is_empty());
}

/// The line of `info` starting with `label`.
fn info_line(s: &mut Session, label: &str) -> String {
    ok(s, "info")
        .lines()
        .find(|l| l.trim_start().starts_with(label))
        .unwrap_or_default()
        .to_string()
}

#[test]
fn new_characters_carry_pcgen_defaults() {
    let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
    ok(&mut s, "class add Tinker");
    ok(&mut s, "equip add \"Wooden Spoon\"");
    let text = String::from_utf8(s.ch.pcg.encode()).unwrap();
    let wanted = [
        "VERSION",
        "ROLLMETHOD",
        "PURCHASEPOINTS",
        "POOLPOINTS",
        "POOLPOINTSAVAIL",
        "GAMEMODE",
        "IGNORECOST",
        "ALLOWDEBT",
        "AUTORESIZEGEAR",
        "CHARACTERNAME",
        "STAT",
        "MONEY",
        "EQUIPNAME",
    ];
    let tags: Vec<&str> = text
        .lines()
        .filter_map(|l| l.split(':').next())
        .filter(|t| wanted.contains(t))
        .collect();
    assert_eq!(
        tags,
        [
            "VERSION",
            "ROLLMETHOD",
            "PURCHASEPOINTS",
            "POOLPOINTS",
            "POOLPOINTSAVAIL",
            "GAMEMODE",
            "IGNORECOST",
            "ALLOWDEBT",
            "AUTORESIZEGEAR",
            "CHARACTERNAME",
            "STAT",
            "STAT",
            "MONEY",
            "EQUIPNAME"
        ],
        "{}",
        text
    );
    // every stat starts at 10, as the all-the-same method gives
    assert_eq!(lines(&s, "ROLLMETHOD"), ["ROLLMETHOD:1|EXPRESSION:10"]);
    assert_eq!(lines(&s, "PURCHASEPOINTS"), ["PURCHASEPOINTS:N"]);
    assert_eq!(
        lines(&s, "POOLPOINTS"),
        ["POOLPOINTS:0", "POOLPOINTSAVAIL:-1"]
    );
    assert_eq!(lines(&s, "MONEY"), ["MONEY:0"]);
    assert_eq!(lines(&s, "IGNORECOST"), ["IGNORECOST:N"]);
    assert_eq!(lines(&s, "ALLOWDEBT"), ["ALLOWDEBT:N"]);
    assert_eq!(lines(&s, "AUTORESIZEGEAR"), ["AUTORESIZEGEAR:Y"]);
}

#[test]
fn absurd_kit_quantities_are_refused() {
    // a data file asking for a vast number of items fails quickly
    let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
    let e = err(&mut s, "kit apply \"Hoard Kit\"");
    assert!(e.contains("quantity"), "{}", e);
}

/// A second-level Sage (3 level-0 casts, 2 level-1 plus one bonus
/// spell for Might 14) who knows Ember.
fn sage() -> Session {
    let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
    for c in [
        "set stat MGT 14",
        "class add Sage",
        "class add Sage",
        "spell add Sage Ember",
    ] {
        ok(&mut s, c);
    }
    s
}

fn pcg_text(s: &Session) -> String {
    String::from_utf8(s.ch.pcg.encode()).unwrap()
}

#[test]
fn spell_lists() {
    let mut s = sage();
    ok(&mut s, "spellbook add Morning");
    assert!(pcg_text(&s).contains("SPELLBOOK:Morning|TYPE:2\n"));
    assert!(err(&mut s, "spellbook add morning").contains("already"));
    // a list may not be named for a class, or hold the line's separators
    assert!(err(&mut s, "spellbook add Sage").contains("class"));
    assert!(err(&mut s, "spellbook add \"A|B\"").contains("may not contain"));
    assert!(err(&mut s, "spellbook rm \"Known Spells\"").contains("cannot"));
    ok(&mut s, "spell prep Sage Ember Morning");
    assert_eq!(
        ok(&mut s, "spellbook ls"),
        "Known Spells\tknown spells\t1\nInnate\tinnate\t0\nMorning\tprepared list\t1"
    );
    // removing a list removes what was prepared in it
    ok(&mut s, "spellbook rm Morning");
    assert!(!pcg_text(&s).contains("Morning"));
    assert!(err(&mut s, "spellbook rm Morning").contains("no spell book"));
}

#[test]
fn preparing_spells() {
    let mut s = sage();
    // into "Prepared Spells" unless a list is named
    assert!(ok(&mut s, "spell prep Sage Ember").contains("Prepared Spells"));
    let text = pcg_text(&s);
    assert!(
        text.contains("SPELLBOOK:Prepared Spells|TYPE:2\n"),
        "{}",
        text
    );
    assert!(
        text.contains(
            "SPELLNAME:Ember|TIMES:1|CLASS:Sage|BOOK:Prepared Spells|SPELLLEVEL:1|SOURCE:[TYPE:CLASS|NAME:Sage]\n"
        ),
        "{}",
        text
    );
    ok(&mut s, "spell prep Sage Ember");
    ok(&mut s, "spell prep Sage Ember");
    assert!(pcg_text(&s).contains("SPELLNAME:Ember|TIMES:3|"));
    // the level's three slots are full
    let e = err(&mut s, "spell prep Sage Ember");
    assert!(e.contains("only prepare 3 spells for level 1"), "{}", e);
    ok(&mut s, "spell prep Sage Ember --force");
    assert!(pcg_text(&s).contains("SPELLNAME:Ember|TIMES:4|"));
    // only spells known, of a class that prepares them
    assert!(err(&mut s, "spell prep Sage Mend").contains("does not know"));
    assert!(err(&mut s, "spell prep Sage Spark").contains("not on"));
    ok(&mut s, "class add Chanter");
    assert!(err(&mut s, "spell prep Chanter Spark").contains("only add to Known Spells"));
    // unpreparing takes one copy away
    ok(&mut s, "spell unprep Sage Ember");
    assert!(pcg_text(&s).contains("SPELLNAME:Ember|TIMES:3|"));
    assert!(err(&mut s, "spell unprep Sage Ember Morning").contains("not prepared"));
}

#[test]
fn prepared_spell_lines_keep_power_point_costs() {
    let mut s = sage();
    let lines = pcgtools::pcg::PcgFile::parse(
        "SPELLBOOK:Prepared Spells|TYPE:2\n\
         SPELLNAME:Ember|TIMES:1|CLASS:Sage|BOOK:Prepared Spells|SPELLLEVEL:1|SPELLPPCOST:3|SOURCE:[TYPE:CLASS|NAME:Sage]\n",
    )
    .lines;
    s.ch.pcg.lines.extend(lines);
    ok(&mut s, "spell prep Sage Ember");
    let want = "SPELLNAME:Ember|TIMES:2|CLASS:Sage|BOOK:Prepared Spells|SPELLLEVEL:1|SPELLPPCOST:3|SOURCE:[TYPE:CLASS|NAME:Sage]\n";
    assert!(pcg_text(&s).contains(want), "{}", pcg_text(&s));
    ok(&mut s, "spell unprep Sage Ember");
    assert!(pcg_text(&s)
        .contains("|TIMES:1|CLASS:Sage|BOOK:Prepared Spells|SPELLLEVEL:1|SPELLPPCOST:3|"));
}

#[test]
fn scribing_spells() {
    let mut s = sage();
    let e = err(&mut s, "spell scribe Sage Ember Notebook");
    assert!(e.contains("no spellbook named Notebook"), "{}", e);
    ok(&mut s, "equip add Notebook");
    // the item's pages: max(2*SPELLLEVEL,1) of its 10 for level 1
    ok(&mut s, "spell scribe Sage Ember Notebook");
    let text = pcg_text(&s);
    assert!(text.contains("SPELLBOOK:Notebook|TYPE:3\n"), "{}", text);
    assert!(
        text.contains(
            "SPELLNAME:Ember|TIMES:1|CLASS:Sage|BOOK:Notebook|SPELLLEVEL:1|SPELLNUMPAGES:2|SOURCE:[TYPE:CLASS|NAME:Sage]\n"
        ),
        "{}",
        text
    );
    let ls = ok(&mut s, "spellbook ls");
    assert!(
        ls.ends_with("\nNotebook\tspellbook\t1\t2/10 pages"),
        "{}",
        ls
    );
    for _ in 0..4 {
        ok(&mut s, "spell scribe Sage Ember Notebook");
    }
    assert!(
        pcg_text(&s).contains("|TIMES:5|CLASS:Sage|BOOK:Notebook|SPELLLEVEL:1|SPELLNUMPAGES:10|")
    );
    let e = err(&mut s, "spell scribe Sage Ember Notebook");
    assert!(e.contains("not enough pages"), "{}", e);
    ok(&mut s, "spell unscribe Sage Ember Notebook");
    assert!(
        pcg_text(&s).contains("|TIMES:4|CLASS:Sage|BOOK:Notebook|SPELLLEVEL:1|SPELLNUMPAGES:8|")
    );
    // spells are scribed only in spellbooks, and prepared only in lists
    let e = err(&mut s, "spell scribe Sage Ember \"Prepared Spells\"");
    assert!(e.contains("no spellbook named"), "{}", e);
    let e = err(&mut s, "spell prep Sage Ember Notebook");
    assert!(e.contains("is a spellbook"), "{}", e);
    assert!(err(&mut s, "spell scribe Sage Mend Notebook").contains("does not know"));
    // the item keeps its book
    assert!(err(&mut s, "spellbook rm Notebook").contains("item"));
}

#[test]
fn preparing_spells_with_metamagic() {
    let mut s = sage();
    // third level: one level-2 slot, none of level 3
    ok(&mut s, "class add Sage");
    let wide = "spell prep Sage Ember --meta \"Wide Spell\"";
    assert!(err(&mut s, wide).contains("does not have Wide Spell"));
    ok(&mut s, "ability add \"Wide Spell\" --force");
    ok(&mut s, "ability add \"Sharp Spell\" --force");
    ok(&mut s, "ability add \"Spell Spring\" --force");
    let e = err(&mut s, "spell prep Sage Ember --meta \"Spell Spring\"");
    assert!(e.contains("not a metamagic feat"), "{}", e);
    // a feat raises the level of the slot the spell takes
    assert!(ok(&mut s, wide).contains("(level 2)"));
    let text = pcg_text(&s);
    assert!(
        text.contains(
            "SPELLNAME:Ember|TIMES:1|CLASS:Sage|BOOK:Prepared Spells|SPELLLEVEL:2|FEATLIST:[FEAT:Wide Spell]|SOURCE:[TYPE:CLASS|NAME:Sage]\n"
        ),
        "{}",
        text
    );
    let both = "spell prep Sage Ember --meta \"Wide Spell,Sharp Spell\"";
    let e = err(&mut s, both);
    assert!(e.contains("only prepare 0 spells for level 3"), "{}", e);
    ok(&mut s, &format!("{} --force", both));
    assert!(pcg_text(&s).contains("|SPELLLEVEL:3|FEATLIST:[FEAT:Wide Spell|FEAT:Sharp Spell]|"));
    // copies with and without feats are counted apart
    ok(&mut s, "spell prep Sage Ember");
    ok(&mut s, wide.replace("prep", "unprep").as_str());
    let text = pcg_text(&s);
    assert!(!text.contains("|SPELLLEVEL:2|"), "{}", text);
    assert!(
        text.contains("|BOOK:Prepared Spells|SPELLLEVEL:1|"),
        "{}",
        text
    );
}

#[test]
fn a_known_spell_is_not_learned_twice() {
    let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
    ok(&mut s, "class add Tinker");
    ok(&mut s, "spell add Tinker Spark");
    assert!(err(&mut s, "spell add Tinker Spark").contains("already knows"));
    let known = String::from_utf8(s.ch.pcg.encode())
        .unwrap()
        .lines()
        .filter(|l| l.starts_with("SPELLNAME:Spark|"))
        .count();
    assert_eq!(known, 1);
}

fn discarding_a_spellbook_loses_its_spells() {
    let mut s = sage();
    ok(&mut s, "equip add Notebook 2");
    ok(&mut s, "spell scribe Sage Ember Notebook");
    ok(&mut s, "spell scribe Sage Ember \"Notebook #2\"");
    ok(&mut s, "spell prep Sage Ember");
    let books = |s: &Session| {
        lines(s, "SPELLBOOK:")
            .into_iter()
            .filter(|l| l.ends_with("|TYPE:3"))
            .collect::<Vec<_>>()
    };
    assert_eq!(books(&s).len(), 2);
    // as in PCGen, giving up a copy loses the last book
    ok(&mut s, "equip sell Notebook 1");
    assert_eq!(books(&s), ["SPELLBOOK:Notebook|TYPE:3"]);
    assert!(lines(&s, "SPELLNAME:")
        .iter()
        .all(|l| !l.contains("BOOK:Notebook #2|")));
    ok(&mut s, "equip rm Notebook");
    assert!(books(&s).is_empty());
    let left = lines(&s, "SPELLNAME:");
    assert!(
        left.iter().all(|l| !l.contains("BOOK:Notebook")),
        "{:?}",
        left
    );
    // spells known and prepared stay
    assert!(
        left.iter().any(|l| l.contains("BOOK:Prepared Spells|")),
        "{:?}",
        left
    );
    assert!(
        left.iter().any(|l| l.contains("BOOK:Known Spells|")),
        "{:?}",
        left
    );
}

#[test]
fn equipment_sets() {
    let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
    ok(&mut s, "equip add \"Tin Shield\"");
    ok(&mut s, "equip add \"Wooden Spoon\" 2");
    // with no sets there is PCGen's default one
    assert_eq!(ok(&mut s, "eqset ls"), "* 0.1\tDefault Set\t0 items");
    ok(&mut s, "equip wear \"Wooden Spoon\" Carried");
    // a new set is empty and becomes the one used, as in PCGen
    ok(&mut s, "eqset add Travel");
    assert_eq!(lines(&s, "CALCEQUIPSET"), ["CALCEQUIPSET:0.2"]);
    ok(&mut s, "equip wear \"Tin Shield\" Arm");
    ok(
        &mut s,
        "equip wear \"Wooden Spoon\" Carried --set \"default set\"",
    );
    // a set holds no more of an item than the character has
    let e = err(&mut s, "equip wear \"Wooden Spoon\" Carried --set 0.1");
    assert!(e.contains("2 of 2"), "{}", e);
    let e = err(&mut s, "equip wear \"Tin Shield\" --set Nowhere");
    assert!(e.contains("no equipment set"), "{}", e);
    assert_eq!(
        lines(&s, "EQUIPSET:"),
        [
            "EQUIPSET:Default Set|ID:0.1|USETEMPMODS:Y",
            "EQUIPSET:Carried|ID:0.1.01|VALUE:Wooden Spoon|QUANTITY:1.0|USETEMPMODS:Y",
            "EQUIPSET:Carried|ID:0.1.02|VALUE:Wooden Spoon|QUANTITY:1.0|USETEMPMODS:Y",
            "EQUIPSET:Travel|ID:0.2|USETEMPMODS:Y",
            "EQUIPSET:Arm|ID:0.2.01|VALUE:Tin Shield|QUANTITY:1.0|USETEMPMODS:Y",
        ]
    );
    // only the set used counts
    assert_eq!(s.ch.equipped().len(), 1);
    assert_eq!(
        ok(&mut s, "eqset ls"),
        "  0.1\tDefault Set\t2 items\n* 0.2\tTravel\t1 item"
    );
    assert_eq!(
        ok(&mut s, "eqset show \"Default Set\""),
        "0.1.01\tCarried\tWooden Spoon x1\n0.1.02\tCarried\tWooden Spoon x1"
    );
    // a copy has the same items under a new root
    ok(&mut s, "eqset copy Travel Battle");
    assert_eq!(lines(&s, "CALCEQUIPSET"), ["CALCEQUIPSET:0.3"]);
    assert_eq!(
        lines(&s, "EQUIPSET:")[5..],
        [
            "EQUIPSET:Battle|ID:0.3|USETEMPMODS:Y",
            "EQUIPSET:Arm|ID:0.3.01|VALUE:Tin Shield|QUANTITY:1.0|USETEMPMODS:Y",
        ]
    );
    assert!(err(&mut s, "eqset rename Battle travel").contains("already"));
    assert!(err(&mut s, "eqset add TRAVEL").contains("already"));
    ok(&mut s, "eqset rename Battle War");
    assert_eq!(lines(&s, "EQUIPSET:War|ID:0.3|").len(), 1);
    // taking an item off one set leaves it in the others
    ok(&mut s, "equip unwear \"Tin Shield\" --set Travel");
    assert_eq!(lines(&s, "EQUIPSET:Arm|").len(), 1);
    assert_eq!(s.ch.equipped().len(), 1);
    ok(&mut s, "eqset use \"Default Set\"");
    assert_eq!(lines(&s, "CALCEQUIPSET"), ["CALCEQUIPSET:0.1"]);
    assert_eq!(lines(&s, "USETEMPMODS"), ["USETEMPMODS:Y"]);
    assert_eq!(s.ch.equipped().len(), 2);
    // removing a set leaves the others numbered as they were
    ok(&mut s, "eqset rm Travel");
    let roots: Vec<String> = lines(&s, "EQUIPSET:")
        .into_iter()
        .filter(|l| !l.contains("VALUE:"))
        .collect();
    assert_eq!(
        roots,
        [
            "EQUIPSET:Default Set|ID:0.1|USETEMPMODS:Y",
            "EQUIPSET:War|ID:0.3|USETEMPMODS:Y",
        ]
    );
    // removing the set used falls back to the first one
    ok(&mut s, "eqset use War");
    ok(&mut s, "eqset rm War");
    assert_eq!(lines(&s, "CALCEQUIPSET"), ["CALCEQUIPSET:0.1"]);
    assert!(err(&mut s, "eqset rm \"Default Set\"").contains("last"));
    ok(&mut s, "undo");
    assert_eq!(lines(&s, "EQUIPSET:War").len(), 1);
}

#[test]
fn calculation_set_falls_back_to_the_first_set() {
    // PCGen uses the first set when CALCEQUIPSET names none
    let mut s = Session::create("Testmode", &["TB".into()], "Nova", dirs());
    ok(&mut s, "equip add \"Tin Shield\"");
    ok(&mut s, "eqset add Travel");
    ok(&mut s, "equip wear \"Tin Shield\" Arm");
    let text = pcg_text(&s)
        .replace("ID:0.2", "ID:0.02")
        .replace("CALCEQUIPSET:0.2\n", "CALCEQUIPSET:0.7\n");
    let ch = Character::from_pcg(PcgFile::parse(&text));
    assert_eq!(ch.calc_equip_set(), "0.1");
    assert!(ch.equipped().is_empty());
    // the first in the file, whatever its number
    let text = text.replace("ID:0.1|", "ID:0.9|");
    let ch = Character::from_pcg(PcgFile::parse(&text));
    assert_eq!(ch.calc_equip_set(), "0.9");
}

fn temp_lines(s: &Session) -> Vec<String> {
    s.ch.pcg
        .lines_tagged("TEMPBONUS")
        .map(|l| l.raw.clone())
        .collect()
}

fn ac(s: &Session) -> i64 {
    s.sheet().ac["Total"]
}

#[test]
fn temporary_bonuses_offered() {
    let mut s = session();
    let ls = ok(&mut s, "temp ls");
    // a spell any character may use, on itself or on a shield; a
    // spell for weapons that asks for a number; a feat the character
    // has, for itself only
    assert!(ls.contains("Ember (SPELL): PC, Tin Shield\n"), "{}", ls);
    assert!(
        ls.contains("Mend (SPELL): Tin Shield  value 1-3\n"),
        "{}",
        ls
    );
    assert!(ls.contains("Steady Hands (FEAT): PC"), "{}", ls);
    assert!(!ls.contains("applied"), "{}", ls);
    assert_eq!(
        ok(&mut s, "temp ls mend"),
        "Mend (SPELL): Tin Shield  value 1-3"
    );
}

#[test]
fn temporary_bonuses_applied() {
    let mut s = session();
    let base = ac(&s);
    ok(&mut s, "temp add Ember");
    assert_eq!(ac(&s), base + 2);
    ok(&mut s, "temp add spell=ember \"Tin Shield\"");
    assert_eq!(ac(&s), base + 3);
    assert!(err(&mut s, "temp add Mend \"Tin Shield\"").contains("from 1 to 3"));
    assert!(err(&mut s, "temp add Mend \"Tin Shield\" --value 4").contains("from 1 to 3"));
    assert!(err(&mut s, "temp add Mend").contains("only to items"));
    assert!(err(&mut s, "temp add Mend \"Silver Spoon\" --value 2").contains("does not suit"));
    assert!(
        err(&mut s, "temp add \"Steady Hands\" \"Tin Shield\"").contains("only to the character")
    );
    assert!(err(&mut s, "temp add Ember pc").contains("already applied"));
    assert!(err(&mut s, "temp add Glow").contains("offers"));
    ok(&mut s, "temp add Mend \"Tin Shield\" --value 2");
    // one line per source and target, in the order of their bonuses
    assert_eq!(
        temp_lines(&s),
        [
            "TEMPBONUS:SPELL=Ember|TBTARGET:Tin Shield|TBBONUS:COMBAT&pipe;AC&pipe;1&pipe;TYPE=ENHANCEMENT",
            "TEMPBONUS:SPELL=Ember|TBTARGET:PC|TBBONUS:COMBAT&pipe;AC&pipe;2&pipe;TYPE=DEFLECTION",
            "TEMPBONUS:SPELL=Mend|TBTARGET:Tin Shield|TBBONUS:WEAPON&pipe;TOHIT,DAMAGE&pipe;2&pipe;TYPE=ENHANCEMENT",
        ]
    );
    // after the equipment set used
    let i = s.ch.pcg.indices("TEMPBONUS")[0];
    assert_eq!(s.ch.pcg.lines[i - 1].tag(), Some("CALCEQUIPSET"));
    let ls = ok(&mut s, "temp ls");
    assert!(
        ls.contains("applied:\n  Ember (SPELL) on Tin Shield\n"),
        "{}",
        ls
    );
}

#[test]
fn temporary_bonuses_switched_and_removed() {
    let mut s = session();
    let base = ac(&s);
    ok(&mut s, "temp add Ember");
    ok(&mut s, "temp add Ember \"Tin Shield\"");
    assert!(err(&mut s, "temp off Ember").contains("more than one"));
    ok(&mut s, "temp off Ember pc");
    assert_eq!(ac(&s), base + 1);
    assert_eq!(
        temp_lines(&s)[1],
        "TEMPBONUS:SPELL=Ember|TBTARGET:PC|TBACTIVE:N|TBBONUS:COMBAT&pipe;AC&pipe;2&pipe;TYPE=DEFLECTION"
    );
    assert!(ok(&mut s, "temp ls").contains("Ember (SPELL) on PC (off)"));
    ok(&mut s, "temp on Ember pc");
    assert_eq!(ac(&s), base + 3);
    ok(&mut s, "temp rm Ember \"Tin Shield\"");
    assert_eq!(ac(&s), base + 2);
    assert_eq!(temp_lines(&s).len(), 1);
    assert!(err(&mut s, "temp rm Ember \"Tin Shield\"").contains("no temporary bonus"));
    ok(&mut s, "undo");
    assert_eq!(temp_lines(&s).len(), 2);
    // with temporary modifiers off, none counts
    ok(&mut s, "temp mods off");
    assert_eq!(ac(&s), base);
    assert_eq!(s.ch.pcg.value("USETEMPMODS").as_deref(), Some("N"));
    ok(&mut s, "temp mods on");
    assert_eq!(ac(&s), base + 3);
    // the set used, and all placed in it, are marked to use them
    assert!(s
        .ch
        .pcg
        .lines_tagged("EQUIPSET")
        .all(|l| l.raw.ends_with("|USETEMPMODS:Y")));
}

#[test]
fn followers_limited_per_companion_list() {
    let mut s = session();
    let max = |s: &Session, list: &str| {
        let mut c = Calc::new(&s.ctx.dataset, s.ctx.gamemode.as_ref(), &s.ch);
        c.run();
        c.max_followers(list)
    };
    // FOLLOWERS on the class allows one familiar
    assert_eq!(max(&s, "Familiar"), Some(1));
    ok(&mut s, "follower add Whiskers Familiar Gearling");
    let e = err(&mut s, "follower add Tibbles Familiar Gearling");
    assert!(e.contains("1 Familiar"), "{}", e);
    // and BONUS:FOLLOWERS one more
    ok(&mut s, "template add Beastfriend");
    assert_eq!(max(&s, "familiar"), Some(2));
    ok(&mut s, "follower add Tibbles Familiar Gearling");
    assert!(err(&mut s, "follower add Mittens Familiar Gearling").contains("2 Familiar"));
    ok(&mut s, "follower add Mittens Familiar Gearling --force");
    // without FOLLOWERS, the companion modifiers of the list decide:
    // a variable they name above zero allows any number, a class they
    // name the character has its count, and otherwise none
    assert_eq!(max(&s, "Gadget"), None);
    ok(&mut s, "follower add Cog Gadget Gearling");
    ok(&mut s, "follower add Sprocket Gadget Gearling");
    assert_eq!(max(&s, "Chorus"), Some(0));
    assert!(err(&mut s, "follower add Echo Chorus Gearling").contains("0 Chorus"));
    ok(&mut s, "class add Chanter");
    assert_eq!(max(&s, "Chorus"), Some(1));
    ok(&mut s, "follower add Echo Chorus Gearling");
    assert_eq!(max(&s, "Nothing"), Some(0));
}

#[test]
fn hit_points_rolled_within_bonus_bounds() {
    let mut s = Session::create("Testmode", &["TB".into()], "Hale", dirs());
    let hp = |s: &Session, cls: &str, n: u32| {
        let l = &lines(s, &format!("CLASSABILITIESLEVEL:{}={}|", cls, n))[0];
        l.split('|').nth(1).unwrap().to_string()
    };
    // the Warden's minimum is 1 + 3 with the new level's bonus; the
    // die's maximum would be 8 + 1 + 2, but a saved roll keeps only
    // the class's bonuses from below the level (here its class line);
    // the default is the maximum
    ok(&mut s, "class add Warden");
    assert_eq!(hp(&s, "Warden", 1), "HITPOINTS:9");
    let e = err(&mut s, "class add Warden 12");
    assert!(e.contains("4 to 11"), "{}", e);
    assert!(err(&mut s, "class add Warden 3").contains("4 to 11"));
    ok(&mut s, "class add Warden 4");
    assert_eq!(hp(&s, "Warden", 2), "HITPOINTS:4");
    ok(&mut s, "undo");
    ok(&mut s, "class add Warden");
    assert_eq!(hp(&s, "Warden", 2), "HITPOINTS:11");
    // the minimum bonus is the Warden's own, and another class's roll
    // keeps none of the Warden's maximum bonuses
    let e = err(&mut s, "class add Tinker 9");
    assert!(e.contains("1 to 8"), "{}", e);
    ok(&mut s, "class add Tinker 1");
    ok(&mut s, "undo");
    ok(&mut s, "class add Tinker");
    assert_eq!(hp(&s, "Tinker", 1), "HITPOINTS:8");
    ok(&mut s, "class add Tinker 12 --force");
    assert_eq!(hp(&s, "Tinker", 2), "HITPOINTS:12");
}
