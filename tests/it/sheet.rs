// The full character sheet built for the synthetic fixture character.

use crate::common;
use pcgtools::context::Context;
use pcgtools::pcc::resolve::DataDirs;
use pcgtools::pcg::character::Character;
use pcgtools::sheet::{self, Element};
use std::collections::HashMap;

fn tess() -> HashMap<String, String> {
    let ch = Character::read(&common::fixture("mini/characters/tess.pcg")).unwrap();
    let dirs = DataDirs {
        data: Some(common::fixture("mini/data")),
        system: Some(common::fixture("mini/system")),
        ..Default::default()
    };
    let ctx = Context::for_character(&ch, dirs);
    sheet::build(&ch, &ctx).flatten().into_iter().collect()
}

fn at<'a>(m: &'a HashMap<String, String>, path: &str) -> &'a str {
    m.get(&format!("character/{}", path))
        .unwrap_or_else(|| panic!("no sheet value at {}", path))
}

#[test]
fn basics_and_classes() {
    let m = tess();
    assert_eq!(at(&m, "basics[Tess Tinkerton]/name[0]"), "Tess Tinkerton");
    assert_eq!(at(&m, "basics[Tess Tinkerton]/bab[0]"), "+1");
    assert_eq!(
        at(
            &m,
            "basics[Tess Tinkerton]/classes[0]/class[Tinker]/level[0]"
        ),
        "2"
    );
    assert_eq!(
        at(&m, "basics[Tess Tinkerton]/classes[0]/shortform[0]"),
        "Tnk2"
    );
    assert_eq!(
        at(&m, "basics[Tess Tinkerton]/classes[0]/levels_total[0]"),
        "2"
    );
    assert_eq!(
        at(&m, "basics[Tess Tinkerton]/experience[0]/current[0]"),
        "1200"
    );
    assert_eq!(
        at(&m, "basics[Tess Tinkerton]/languages[0]/all[0]"),
        "Trade Tongue"
    );
    assert_eq!(at(&m, "basics[Tess Tinkerton]/hitdice[0]"), "(2d8)+4");
}

#[test]
fn ability_scores() {
    let m = tess();
    let a = "abilities[0]/ability[Might]";
    assert_eq!(at(&m, &format!("{}/name[0]/short[0]", a)), "MGT");
    assert_eq!(at(&m, &format!("{}/score[0]", a)), "14");
    assert_eq!(at(&m, &format!("{}/modifier[0]", a)), "+2");
    assert_eq!(at(&m, &format!("{}/base[0]", a)), "14");
    assert_eq!(at(&m, "abilities[0]/ability[Grace]/modifier[0]"), "+1");
}

#[test]
fn hit_points_and_history() {
    let m = tess();
    assert_eq!(at(&m, "hit_points[0]/points[0]"), "14");
    assert_eq!(at(&m, "hit_points[0]/die[0]"), "(2d8)+4");
    // level 1: rolled 6, plus 2 from Might
    assert_eq!(at(&m, "hit_points[0]/history[0]/roll[0]/roll[0]"), "6");
    assert_eq!(at(&m, "hit_points[0]/history[0]/roll[0]/stat[0]"), "2");
    assert_eq!(at(&m, "hit_points[0]/history[0]/roll[0]/total[0]"), "8");
    assert_eq!(at(&m, "hit_points[0]/history[0]/roll[1]/total[0]"), "6");
}

#[test]
fn armor_class_breakdown() {
    let m = tess();
    assert_eq!(at(&m, "armor_class[0]/total[0]"), "12");
    assert_eq!(at(&m, "armor_class[0]/flat[0]"), "11");
    assert_eq!(at(&m, "armor_class[0]/touch[0]"), "11");
    assert_eq!(
        at(&m, "armor_class[0]/listing[0]"),
        "ABILITY +1, BASE +10, SHIELD +1"
    );
    // the shield's check penalty
    assert_eq!(at(&m, "armor_class[0]/check_penalty[0]"), "-1");
    // no armor limits dexterity
    assert_eq!(at(&m, "armor_class[0]/max_dex[0]"), "");
}

#[test]
fn skills_list() {
    let m = tess();
    let t = "skills[0]/skill[Tinkering]";
    assert_eq!(at(&m, &format!("{}/ranks[0]", t)), "2.0");
    assert_eq!(at(&m, &format!("{}/skill_mod[0]", t)), "6");
    assert_eq!(at(&m, &format!("{}/ability_mod[0]", t)), "1");
    assert_eq!(at(&m, &format!("{}/misc_mod[0]", t)), "3");
    assert_eq!(at(&m, &format!("{}/ability[0]", t)), "GRC");
    assert_eq!(at(&m, "skills[0]/list_mods[0]"), "Tinkering +6");
    // usable untrained: listed with no ranks
    assert_eq!(at(&m, "skills[0]/skill[Climbing]/ranks[0]"), "0.0");
}

#[test]
fn saves_and_attacks() {
    let m = tess();
    let f = "saving_throws[0]/saving_throw[fortitude]";
    assert_eq!(at(&m, &format!("{}/total[0]", f)), "+5");
    assert_eq!(at(&m, &format!("{}/base[0]", f)), "+3");
    assert_eq!(at(&m, "attack[0]/melee[0]/bab[0]"), "+1");
    assert_eq!(at(&m, "attack[0]/melee[0]/base_attack_bonus[0]"), "+1");
}

#[test]
fn feats_with_descriptions() {
    let m = tess();
    assert_eq!(
        at(&m, "feats[0]/feat[Steady Hands]/description[0]"),
        "Hands do not shake."
    );
    assert_eq!(
        at(&m, "feats[0]/feat[Steady Hands]/type[0]"),
        "GENERAL.COMBAT"
    );
}

#[test]
fn sheet_xml_round_trips() {
    let ch = Character::read(&common::fixture("mini/characters/tess.pcg")).unwrap();
    let dirs = DataDirs {
        data: Some(common::fixture("mini/data")),
        system: Some(common::fixture("mini/system")),
        ..Default::default()
    };
    let ctx = Context::for_character(&ch, dirs);
    let built = sheet::build(&ch, &ctx);
    let reparsed = Element::parse_xml(&built.to_xml()).unwrap();
    let diff = sheet::compare(&built, &reparsed);
    assert!(diff.mismatches.is_empty(), "{:?}", diff.mismatches);
}

#[test]
fn defaults_for_missing_values() {
    // a new character has no gender, handedness or equipment set lines
    let dirs = DataDirs {
        data: Some(common::fixture("mini/data")),
        system: Some(common::fixture("mini/system")),
        ..Default::default()
    };
    let s = pcgtools::repl::Session::create("Testmode", &["TB".into()], "Nova", dirs);
    let m: HashMap<String, String> = sheet::build(&s.ch, &s.ctx).flatten().into_iter().collect();
    assert_eq!(at(&m, "basics[Nova]/gender[0]/long[0]"), "Male");
    assert_eq!(at(&m, "basics[Nova]/gender[0]/short[0]"), "Male");
    assert_eq!(at(&m, "basics[Nova]/handed[0]"), "Right");
    assert_eq!(
        at(&m, "equipment[0]/equipmentsets[0]/equipmentset[0]/@name"),
        "Default Set"
    );
}

#[test]
fn automatic_languages() {
    let dirs = DataDirs {
        data: Some(common::fixture("mini/data")),
        system: Some(common::fixture("mini/system")),
        ..Default::default()
    };
    let mut s = pcgtools::repl::Session::create("Testmode", &["TB".into()], "Nova", dirs);
    s.exec("race Gearling").unwrap();
    let langs = |s: &pcgtools::repl::Session| {
        let m: HashMap<String, String> =
            sheet::build(&s.ch, &s.ctx).flatten().into_iter().collect();
        (
            at(&m, "basics[Nova]/languages[0]/all[0]").to_string(),
            at(&m, "languages[0]").to_string(),
        )
    };
    // granted by the race; the second only for neutral characters
    assert_eq!(langs(&s), ("Old Script".into(), "Old Script".into()));
    s.exec("set align TN").unwrap();
    let both = "Old Script, Trade Tongue".to_string();
    assert_eq!(langs(&s), (both.clone(), both.clone()));
    // a language also chosen is listed once
    s.exec("lang add \"Trade Tongue\"").unwrap();
    assert_eq!(langs(&s), (both.clone(), both));
}
