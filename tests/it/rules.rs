// Derived statistics for the synthetic fixture character.

use crate::common;
use pcgtools::context::Context;
use pcgtools::pcc::resolve::DataDirs;
use pcgtools::pcg::character::Character;
use pcgtools::rules::calc::Calc;

fn dirs() -> DataDirs {
    DataDirs {
        data: Some(common::fixture("mini/data")),
        system: Some(common::fixture("mini/system")),
        ..Default::default()
    }
}

#[test]
fn fixture_character_sheet() {
    let ch = Character::read(&common::fixture("mini/characters/tess.pcg")).unwrap();
    let ctx = Context::for_character(&ch, dirs());
    assert!(ctx.gamemode.is_some());
    let mut calc = Calc::new(&ctx.dataset, ctx.gamemode.as_ref(), &ch);
    let s = calc.run();

    assert_eq!(s.level, 2);
    assert_eq!(s.stats["MGT"], (14, 2));
    assert_eq!(s.stats["GRC"], (12, 1));
    // classlevel()*3/4 = 1.5, truncated
    assert_eq!(s.bab, 1);
    // (6 + 2) + (4 + 2)
    assert_eq!(s.hp, 14);
    // base CL/2+2 = 3, plus MGT
    assert_eq!(s.saves["Toughness"], 5);
    assert_eq!(s.saves["Quickness"], 1);
    assert_eq!(s.ac["Total"], 12);
    assert_eq!(s.ac["Flatfooted"], 11);
    assert_eq!(s.ac["Touch"], 11);
    // 2 ranks + GRC 1 + class skill 3
    assert_eq!(s.skills["Tinkering"], 6);
    // MGT 2 + shield check penalty -1
    assert_eq!(s.skills["Climbing"], 1);
    assert_eq!(s.armor_check, -1);
    assert!(s.unknown.is_empty(), "{:?}", s.unknown);

    let why = calc.explain("SKILL", "Tinkering");
    assert_eq!(why, vec!["+3 [ClassSkill] from Skill Tinkering"]);
}

#[test]
fn check_penalty_bonus_offsets_armor_on_skills() {
    let path = common::fixture("mini/characters/tess.pcg");
    let mut s = pcgtools::repl::Session::open(&path, dirs()).unwrap();
    // MGT 2, shield -1
    assert_eq!(s.sheet().skills["Climbing"], 1);
    s.exec("feat add \"Armor Ease\"").unwrap();
    // the +1 to the check penalty cancels the shield's
    assert_eq!(s.sheet().skills["Climbing"], 2);
}

#[test]
fn edits_change_the_sheet() {
    let mut ch = Character::read(&common::fixture("mini/characters/tess.pcg")).unwrap();
    ch.set_stat("GRC", 16);
    ch.add_level("Tinker", 5, 4.0);
    ch.unequip("Tin Shield");
    let ctx = Context::for_character(&ch, dirs());
    let s = Calc::new(&ctx.dataset, ctx.gamemode.as_ref(), &ch).run();
    assert_eq!(s.level, 3);
    assert_eq!(s.bab, 2);
    assert_eq!(s.hp, 14 + 5 + 2);
    assert_eq!(s.ac["Total"], 13);
    assert_eq!(s.skills["Climbing"], 2);
}

#[test]
fn character_type_defaults_to_game_modes_first() {
    use pcgtools::token::pre::Prereq;
    let check = |ch: &Character, pre: &str| {
        let ctx = Context::for_character(ch, dirs());
        let mut calc = Calc::new(&ctx.dataset, ctx.gamemode.as_ref(), ch);
        calc.run();
        calc.passes(&Prereq::parse(pre).unwrap(), None)
    };
    // no CHARACTERTYPE line: the first type the game mode lists
    let mut ch = Character::read(&common::fixture("mini/characters/vera.pcg")).unwrap();
    assert!(check(&ch, "PRECHARACTERTYPE:1,Hero"));
    assert!(!check(&ch, "PRECHARACTERTYPE:1,PC"));
    ch.set_field("CHARACTERTYPE", "Villain");
    assert!(check(&ch, "PRECHARACTERTYPE:1,Villain"));
    // a type the game mode does not list falls back to the default
    ch.set_field("CHARACTERTYPE", "Bystander");
    assert!(check(&ch, "PRECHARACTERTYPE:1,Hero"));
}

#[test]
fn class_and_skill_type_prerequisites() {
    use pcgtools::token::pre::Prereq;
    let dirs = dirs();
    let s = pcgtools::repl::Session::create("Testmode", &["TB".into()], "Nova", dirs.clone());
    let check = |ch: &Character, pre: &str| {
        let ctx = Context::for_character(ch, dirs.clone());
        let mut calc = Calc::new(&ctx.dataset, ctx.gamemode.as_ref(), ch);
        calc.run();
        calc.passes(&Prereq::parse(pre).unwrap(), None)
    };
    // no classes at all: none of any type
    assert!(!check(&s.ch, "PRECLASS:1,TYPE=Base"));
    assert!(check(&s.ch, "!PRECLASS:1,TYPE=Base"));
    assert!(check(
        &s.ch,
        "PREMULT:1,[PRECLASS:1,Tinker=1],[!PRECLASS:1,TYPE=Base]"
    ));
    // a Tinker has a class of its type
    let tess = Character::read(&common::fixture("mini/characters/tess.pcg")).unwrap();
    assert!(check(&tess, "PRECLASS:1,TYPE=Base"));
    assert!(!check(&tess, "!PRECLASS:1,TYPE=Base"));
    assert!(check(&tess, "PRECLASS:1,TYPE=Base=2"));
    assert!(!check(&tess, "PRECLASS:1,TYPE=Base=3"));
    // skills by type, with and without ranks (Tess has 2 in Tinkering)
    assert!(check(&tess, "PRESKILL:1,TYPE=Craft=2"));
    assert!(check(&tess, "PRESKILL:1,TYPE.Craft=2"));
    assert!(!check(&tess, "PRESKILL:1,TYPE=Craft=3"));
    assert!(!check(&s.ch, "PRESKILL:1,TYPE=Craft"));
}

#[test]
fn carried_weight_without_saved_weights() {
    // the editor saves items without a WT; their data weight counts
    let mut s = pcgtools::repl::Session::create("Testmode", &["TB".into()], "Nova", dirs());
    s.exec("equip add \"Tin Shield\"").unwrap();
    s.exec("equip wear \"Tin Shield\"").unwrap();
    s.exec("equip add \"Wooden Spoon\" 3").unwrap();
    s.exec("equip wear \"Wooden Spoon\" Carried").unwrap();
    let mut calc = Calc::new(&s.ctx.dataset, s.ctx.gamemode.as_ref(), &s.ch);
    calc.run();
    let spoon: f64 = s
        .ctx
        .dataset
        .get(pcgtools::lst::kind::FileKind::Equipment, "Wooden Spoon")
        .and_then(|o| o.get("WT"))
        .and_then(|w| w.parse().ok())
        .unwrap();
    // wearing places one of the three spoons
    assert_eq!(calc.carried_weight(), 5.0 + spoon);
}
