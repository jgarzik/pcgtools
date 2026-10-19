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

#[test]
fn domain_spell_levels_need_a_granting_class() {
    let dirs = DataDirs {
        data: Some(common::fixture("mini/data")),
        system: Some(common::fixture("mini/system")),
        ..Default::default()
    };
    let spark = |s: &pcgtools::repl::Session| {
        let m: HashMap<String, String> =
            sheet::build(&s.ch, &s.ctx).flatten().into_iter().collect();
        m.iter()
            .find(|(k, _)| k.contains("spell[Spark]") && k.ends_with("sourcelevel[0]"))
            .map(|(_, v)| v.clone())
            .unwrap()
    };
    let mut s = pcgtools::repl::Session::create("Testmode", &["TB".into()], "Nova", dirs);
    s.exec("class add Tinker").unwrap();
    s.exec("spell add Tinker Spark").unwrap();
    // a domain held without a class to grant it adds no spell levels
    s.ch.add_domain("Cogs");
    assert_eq!(spark(&s), "Tnk0");
    s.ch.pcg.lines.retain(|l| l.tag() != Some("DOMAIN"));
    s.exec("domain add Cogs").unwrap();
    assert_eq!(spark(&s), "Cogs1, Tnk0");
}

fn new_sheet(cmds: &[&str]) -> HashMap<String, String> {
    let dirs = DataDirs {
        data: Some(common::fixture("mini/data")),
        system: Some(common::fixture("mini/system")),
        ..Default::default()
    };
    let mut s = pcgtools::repl::Session::create("Testmode", &["TB".into()], "Nova", dirs);
    for c in cmds {
        s.exec(c).unwrap();
    }
    sheet::build(&s.ch, &s.ctx).flatten().into_iter().collect()
}

#[test]
fn locked_stats_ignore_saved_scores() {
    let m = new_sheet(&["set stat MGT 16", "race Stonekin"]);
    // the race locks Might at 12; the saved score and STAT bonuses
    // no longer count
    let a = "abilities[0]/ability[Might]";
    for (k, v) in [
        ("score", "12"),
        ("modifier", "+1"),
        ("base", "12"),
        ("basemod", "+1"),
        ("noequip", "12"),
        ("no_temp_score", "12"),
    ] {
        assert_eq!(at(&m, &format!("{}/{}[0]", a, k)), v, "{}", k);
    }
    assert_eq!(at(&m, "abilities[0]/ability[Grace]/score[0]"), "10");
}

#[test]
fn level_adjustment_counts_as_experience() {
    let m = new_sheet(&["race Stonekin"]);
    // one level of adjustment is worth the experience of level 2
    assert_eq!(at(&m, "basics[Nova]/experience[0]/current[0]"), "3000");
}

#[test]
fn class_without_hit_die_rolls_nothing() {
    let m = new_sheet(&["class add Drifter"]);
    assert_eq!(at(&m, "hit_points[0]/history[0]/roll[0]/roll[0]"), "0");
    assert_eq!(at(&m, "hit_points[0]/points[0]"), "5");
}

/// A mode with few code controls, an untitled experience table and a
/// default deity that no data defines.
#[test]
fn sparse_game_mode_basics() {
    let dirs = DataDirs {
        data: Some(common::fixture("mini/data")),
        system: Some(common::fixture("mini/system")),
        ..Default::default()
    };
    let mut s = pcgtools::repl::Session::create("Plainmode", &["PL".into()], "Pat", dirs);
    s.exec("class add Drifter").unwrap();
    let m: HashMap<String, String> = sheet::build(&s.ch, &s.ctx).flatten().into_iter().collect();
    // alignment is shown only when the mode turns the feature on
    assert_eq!(at(&m, "basics[Pat]/alignment[0]/long[0]"), "");
    assert_eq!(at(&m, "basics[Pat]/alignment[0]/short[0]"), "");
    // rows before any XPTABLE are a table of their own, used first
    assert_eq!(at(&m, "basics[Pat]/experience[0]/next_level[0]"), "1000");
    // without a race, the data's unselected race
    assert_eq!(at(&m, "basics[Pat]/race[0]"), "<none selected>");
    // the Face variable, declared by the data with no FACE control
    assert_eq!(at(&m, "basics[Pat]/face[0]/face[0]"), "5' by 5'");
    assert_eq!(at(&m, "basics[Pat]/face[0]/short[0]"), "5' x 5'");
    assert_eq!(at(&m, "basics[Pat]/face[0]/squares[0]"), "1 x 1");
    // the default deity names nothing loaded: shown by name only
    assert_eq!(at(&m, "basics[Pat]/deity[Nobody]/name[0]"), "Nobody");
    assert_eq!(at(&m, "basics[Pat]/deity[Nobody]/domainlist[0]"), "");
    assert_eq!(at(&m, "basics[Pat]/deity[Nobody]/holyitem[0]"), "");
    assert_eq!(at(&m, "basics[Pat]/deity[Nobody]/source[0]"), "");
}

/// A copy of the mini fixture with lines of the game mode's
/// miscinfo.lst for which `drop` is true removed.
fn fixture_copy(name: &str, drop: &dyn Fn(&str) -> bool) -> std::path::PathBuf {
    let src = common::fixture("mini");
    let dst = std::env::temp_dir().join(format!("pcgtools-{}-{}", name, std::process::id()));
    for e in walkdir::WalkDir::new(&src).into_iter().flatten() {
        let to = dst.join(e.path().strip_prefix(&src).unwrap());
        if e.file_type().is_dir() {
            std::fs::create_dir_all(&to).unwrap();
        } else if e.file_name() == "miscinfo.lst" {
            let text = std::fs::read_to_string(e.path()).unwrap();
            let kept: Vec<&str> = text.lines().filter(|l| !drop(l)).collect();
            std::fs::write(&to, kept.join("\n")).unwrap();
        } else {
            std::fs::copy(e.path(), &to).unwrap();
        }
    }
    dst
}

#[test]
fn unarmed_strike_without_wield_categories() {
    // PCGen cannot size up the unarmed strike when the game mode
    // defines no wield categories, and leaves its attack empty
    let root = fixture_copy("nowield", &|l| l.starts_with("WIELDCATEGORY"));
    let dirs = DataDirs {
        data: Some(root.join("data")),
        system: Some(root.join("system")),
        ..Default::default()
    };
    let s = pcgtools::repl::Session::create("Testmode", &["TB".into()], "Nova", dirs);
    let m: HashMap<String, String> = sheet::build(&s.ch, &s.ctx).flatten().into_iter().collect();
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(at(&m, "weapons[0]/unarmed[0]/total[0]"), "");
    assert_eq!(at(&m, "weapons[0]/unarmed[0]/damage[0]"), "");
    assert_eq!(at(&m, "weapons[0]/unarmed[0]/critical[0]"), "20/x2");
}

/// A new character in the fixture game mode.
fn nova() -> pcgtools::repl::Session {
    let dirs = DataDirs {
        data: Some(common::fixture("mini/data")),
        system: Some(common::fixture("mini/system")),
        ..Default::default()
    };
    pcgtools::repl::Session::create("Testmode", &["TB".into()], "Nova", dirs)
}

fn sheet_of(s: &pcgtools::repl::Session) -> HashMap<String, String> {
    sheet::build(&s.ch, &s.ctx).flatten().into_iter().collect()
}

#[test]
fn unarmed_damage_from_a_single_die() {
    let mut s = nova();
    let unarmed = |s: &pcgtools::repl::Session| {
        let m = sheet_of(s);
        (
            at(&m, "weapons[0]/unarmed[0]/total[0]").to_string(),
            at(&m, "weapons[0]/unarmed[0]/damage[0]").to_string(),
        )
    };
    // not proficient with it in this game mode
    assert_eq!(unarmed(&s), ("-4".into(), "1d3".into()));
    // one UDAM value holds at every size
    s.exec("feat add \"Iron Fist\"").unwrap();
    assert_eq!(unarmed(&s).1, "1d8");
}

#[test]
fn wield_category_bonuses() {
    let w = "weapons[0]/weapon[0]";
    let one_handed = |s: &pcgtools::repl::Session| {
        let m = sheet_of(s);
        (
            at(&m, &format!("{}/melee[0]/w1_h1_p[0]/to_hit[0]", w)).to_string(),
            at(&m, &format!("{}/melee[0]/w1_h1_p[0]/damage[0]", w)).to_string(),
        )
    };
    let mut s = nova();
    s.exec("equip add \"Great Axe\"").unwrap();
    s.exec("equip wear \"Great Axe\"").unwrap();
    // two-handed: not usable in one hand
    assert_eq!(one_handed(&s), ("N/A".into(), "N/A".into()));
    // a bonus to the proficiency makes it one-handed
    s.exec("feat add \"Axe Familiarity\"").unwrap();
    assert_eq!(one_handed(&s), ("+0".into(), "1d10".into()));

    // so does the item's own bonus, when its prerequisites pass
    let mut s = nova();
    s.exec("equip add \"Great Axe\"").unwrap();
    s.exec("equip wear \"Great Axe\"").unwrap();
    s.exec("feat add \"Axe Grip\"").unwrap();
    assert_eq!(one_handed(&s), ("+0".into(), "1d10".into()));
}

#[test]
fn double_weapon_in_one_hand() {
    let mut s = nova();
    s.exec("equip add \"Twin Staff\"").unwrap();
    s.exec("equip wear \"Twin Staff\"").unwrap();
    let m = sheet_of(&s);
    let w = "weapons[0]/weapon[0]";
    assert_eq!(
        at(&m, &format!("{}/common[*Twin Staff]/hand[0]", w)),
        "Primary"
    );
    // both heads' damage types
    assert_eq!(at(&m, &format!("{}/common[*Twin Staff]/type[0]", w)), "B/S");
    // a two-handed weapon held in one hand cannot be used
    assert_eq!(at(&m, &format!("{}/simple[0]/to_hit[0]", w)), "N/A");
    assert_eq!(at(&m, &format!("{}/simple[0]/damage[0]", w)), "N/A");
}
