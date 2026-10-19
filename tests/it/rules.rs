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
