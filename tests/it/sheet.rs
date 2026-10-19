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
fn skill_level_maximums() {
    let m = tess();
    // the level 2 row's formula, which reads a variable
    assert_eq!(at(&m, "skills[0]/max_class_skill_level[0]"), "6");
    // the row has no cross-class maximum: zero, not the LEVEL row's
    assert_eq!(at(&m, "skills[0]/max_cross_class_skill_level[0]"), "0.0");
}

#[test]
fn skills_shown_by_misc_bonus() {
    let m = tess();
    // shown only while Tinkering's bonuses beyond ranks and stat
    // (3) pass the skill's prerequisite
    assert_eq!(at(&m, "skills[0]/skill[Tinkering Flair]/skill_mod[0]"), "1");
    assert!(!m.contains_key("character/skills[0]/skill[Gadget Flair]/name[0]"));
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

/// A new character's session after some commands.
fn new_session(cmds: &[&str]) -> pcgtools::repl::Session {
    let dirs = DataDirs {
        data: Some(common::fixture("mini/data")),
        system: Some(common::fixture("mini/system")),
        ..Default::default()
    };
    let mut s = pcgtools::repl::Session::create("Testmode", &["TB".into()], "Nova", dirs);
    for c in cmds {
        s.exec(c).unwrap();
    }
    s
}

fn session_sheet(s: &pcgtools::repl::Session) -> HashMap<String, String> {
    sheet::build(&s.ch, &s.ctx).flatten().into_iter().collect()
}

fn new_sheet(cmds: &[&str]) -> HashMap<String, String> {
    session_sheet(&new_session(cmds))
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

#[test]
fn class_type_fact_without_a_row_falls_back_to_types() {
    // the class's ClassType fact names no CLASSTYPE row, so its types
    // pick the row: PC, whose CR is the level less one
    let dirs = DataDirs {
        data: Some(common::fixture("mini/data")),
        system: Some(common::fixture("mini/system")),
        ..Default::default()
    };
    let mut s = pcgtools::repl::Session::create("Plainmode", &["PL".into()], "Pat", dirs);
    for _ in 0..3 {
        s.exec("class add Wanderer").unwrap();
    }
    assert_eq!(at(&sheet_of(&s), "basics[Pat]/cr[0]"), "2");
}

/// The experience factor and penalty of a new Plainmode character
/// after some commands.
fn xp_factor(cmds: &[&str]) -> (String, String) {
    let dirs = DataDirs {
        data: Some(common::fixture("mini/data")),
        system: Some(common::fixture("mini/system")),
        ..Default::default()
    };
    let mut s = pcgtools::repl::Session::create("Plainmode", &["PL".into()], "Pat", dirs);
    for c in cmds {
        s.exec(c).unwrap();
    }
    let m = sheet_of(&s);
    let x = "basics[Pat]/experience[0]";
    (
        at(&m, &format!("{}/factor[0]", x)).to_string(),
        at(&m, &format!("{}/penalty[0]", x)).to_string(),
    )
}

fn levels(class: &str, n: usize) -> Vec<String> {
    vec![format!("class add {}", class); n]
}

#[test]
fn multiclass_experience_penalty() {
    let pair = |f: &str, p: &str| (f.to_string(), p.to_string());
    let build = |race: &str, classes: &[(&str, usize)], extra: &[&str]| {
        let mut cmds: Vec<String> = Vec::new();
        if !race.is_empty() {
            cmds.push(format!("race {}", race));
        }
        for (c, n) in classes {
            cmds.extend(levels(c, *n));
        }
        cmds.extend(extra.iter().map(|s| s.to_string()));
        let refs: Vec<&str> = cmds.iter().map(String::as_str).collect();
        xp_factor(&refs)
    };
    // a single class has nothing to be penalized against
    assert_eq!(build("", &[("Wanderer", 3)], &[]), pair("100%", "0%"));
    // without a favored class, every class more than one level below
    // the highest costs a fifth; the prestige class is exempt by its
    // type, though its ClassType fact names a row with a penalty
    let mix = [
        ("Wanderer", 4),
        ("Drifter", 1),
        ("Seer", 1),
        ("Scout", 2),
        ("Warden", 1),
    ];
    assert_eq!(build("", &mix, &[]), pair("39%", "61%"));
    // one level apart is no penalty
    assert_eq!(
        build("", &[("Wanderer", 2), ("Scout", 1)], &[]),
        pair("100%", "0%")
    );
    // the favored class does not count, so the highest other class
    // sets the mark
    assert_eq!(build("Plainsfolk", &mix, &[]), pair("100%", "0%"));
    assert_eq!(
        build(
            "Plainsfolk",
            &[("Wanderer", 4), ("Scout", 3), ("Drifter", 1), ("Seer", 1)],
            &[]
        ),
        pair("60%", "40%")
    );
    // any class favored: the highest class is passed over
    assert_eq!(
        build(
            "Openfolk",
            &[("Wanderer", 4), ("Scout", 3), ("Drifter", 1)],
            &[]
        ),
        pair("80%", "20%")
    );
    // a favored subclass counts only once chosen
    let seer = [("Seer", 4), ("Scout", 1)];
    assert_eq!(build("Seerfolk", &seer, &[]), pair("80%", "20%"));
    assert_eq!(
        build("Seerfolk", &seer, &["class subclass Seer Oracle"]),
        pair("100%", "0%")
    );
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

#[test]
fn first_clothing_worn_weighs_nothing_for_skills() {
    // the weight penalty counts what is carried, but not the first
    // clothing worn
    let mut s = nova();
    let misc = |s: &pcgtools::repl::Session| {
        at(&sheet_of(s), "skills[0]/skill[Swimming]/misc_mod[0]").to_string()
    };
    s.exec("class add Tinker").unwrap();
    s.exec("skill Swimming 1 --force").unwrap();
    s.exec("equip add \"Wool Cloak\"").unwrap();
    s.exec("equip wear \"Wool Cloak\"").unwrap();
    assert_eq!(misc(&s), "0");
    s.exec("equip add \"Linen Shirt\"").unwrap();
    s.exec("equip wear \"Linen Shirt\"").unwrap();
    assert_eq!(misc(&s), "-1");
}

#[test]
fn built_in_modifier_choice_sets_its_variable() {
    // an item's own modifier, chosen in its data (EQMOD:KEY|choice),
    // sets the variable its bonuses read
    let mut s = nova();
    let total = |s: &pcgtools::repl::Session| {
        at(&sheet_of(s), "armor_class[0]/total[0]")
            .parse::<i64>()
            .unwrap()
    };
    let before = total(&s);
    s.exec("equip add \"Dented Vest\"").unwrap();
    s.exec("equip wear \"Dented Vest\"").unwrap();
    let m = sheet_of(&s);
    assert_eq!(total(&s), before - 1);
    assert_eq!(at(&m, "protection[0]/armor[*Dented Vest]/acbonus[0]"), "-1");
    assert_eq!(at(&m, "protection[0]/armor[*Dented Vest]/totalac[0]"), "-1");
}

#[test]
fn ranged_weapon_without_wield_categories() {
    // a purely ranged weapon never needs to be sized up for its
    // attacks, so they are computed; its damage still is not
    let root = fixture_copy("nowield-ranged", &|l| l.starts_with("WIELDCATEGORY"));
    let dirs = DataDirs {
        data: Some(root.join("data")),
        system: Some(root.join("system")),
        ..Default::default()
    };
    let mut s = pcgtools::repl::Session::create("Testmode", &["TB".into()], "Nova", dirs);
    s.exec("equip add \"Tin Pistol\"").unwrap();
    s.exec("equip wear \"Tin Pistol\"").unwrap();
    let m = sheet_of(&s);
    let _ = std::fs::remove_dir_all(&root);
    let w = "weapons[0]/weapon[0]";
    let hit = at(
        &m,
        &format!("{}/common[*Tin Pistol]/to_hit[0]/total_hit[0]", w),
    )
    .to_string();
    assert!(hit.starts_with(['+', '-']), "to-hit {:?}", hit);
    assert_eq!(
        at(&m, &format!("{}/ranges[0]/range[0]/to_hit[0]", w)),
        hit.as_str()
    );
    assert_ne!(at(&m, &format!("{}/ranges[0]/range[1]/to_hit[0]", w)), "");
    assert_eq!(at(&m, &format!("{}/ranges[0]/range[0]/damage[0]", w)), "");
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
fn unarmed_damage_bonus_levels() {
    let mut s = nova();
    let damage = |s: &pcgtools::repl::Session| {
        at(&sheet_of(s), "weapons[0]/unarmed[0]/damage[0]").to_string()
    };
    s.exec("class add Brawler").unwrap();
    assert_eq!(damage(&s), "1d4");
    // two more levels reach the level 3 line
    s.exec("template add \"Fist Wraps\"").unwrap();
    assert_eq!(damage(&s), "1d10");
    // CLASS= names the class too; one level less is level 2
    s.exec("template add \"Soft Fists\"").unwrap();
    assert_eq!(damage(&s), "1d4");
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

#[test]
fn stat_columns_past_the_game_modes_stats_are_empty() {
    // the fixture mode has two stats; saves and initiative take theirs
    // by position, so positions past the second show nothing
    let m = tess();
    assert_eq!(
        at(&m, "saving_throws[0]/saving_throw[fortitude]/abil_mod[0]"),
        ""
    );
    assert_eq!(
        at(&m, "saving_throws[0]/saving_throw[will]/abil_mod[0]"),
        ""
    );
    assert_ne!(
        at(&m, "saving_throws[0]/saving_throw[reflex]/abil_mod[0]"),
        ""
    );
    assert_ne!(at(&m, "initiative[0]/dex_mod[0]"), "");
}

#[test]
fn thrown_range_bonuses() {
    // the distances of the range rows (a short range row of 30 is
    // listed before the first increment beyond it)
    let rows = |feat: bool| {
        let mut cmds = vec!["equip add \"Throwing Cog\"", "equip wear \"Throwing Cog\""];
        if feat {
            cmds.push("feat add \"Long Arm\" --force");
        }
        let m = new_sheet(&cmds);
        (0..5)
            .filter_map(|i| {
                m.iter()
                    .find(|(k, _)| k.ends_with(&format!("/ranges[0]/range[{}]/distance[0]", i)))
                    .map(|(_, v)| v.clone())
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(rows(false), ["10'", "20'", "30'", "40'", "50'"]);
    // (10 + 10) doubled, then 5 more
    assert_eq!(rows(true), ["30'", "45'", "90'", "135'", "180'"]);
}

#[test]
fn spells_granted_as_known_count_toward_known() {
    let m = new_sheet(&["class add Chanter"]);
    let known = m
        .iter()
        .find(|(k, _)| k.contains("known_spells[0]/class[0]/level[0]/@known"))
        .map(|(_, v)| v.clone());
    // SPELLKNOWN grants Spark and Glow to the class's list at level 0
    assert_eq!(known.as_deref(), Some("2"));
}

#[test]
fn size_stat_bonuses_count_only_in_full_scores() {
    // three monster levels make the Cogbeast large; Large adds 4 Might
    let m = new_sheet(&[
        "race Cogbeast",
        "class add Beastling --force",
        "class add Beastling --force",
        "class add Beastling --force",
    ]);
    let a = "abilities[0]/ability[Might]";
    assert_eq!(at(&m, &format!("{}/score[0]", a)), "14");
    assert_eq!(at(&m, &format!("{}/noequip[0]", a)), "10");
    assert_eq!(at(&m, &format!("{}/no_temp_score[0]", a)), "10");
}

#[test]
fn natural_attacks_outside_the_equipment_set_give_no_proficiency() {
    // the race's natural attack is in no equipment set of the file
    let m = new_sheet(&["race Cogbeast"]);
    assert_eq!(at(&m, "weapon_proficiencies[0]"), "");
}

#[test]
fn region_from_templates() {
    let m = new_sheet(&[]);
    assert_eq!(at(&m, "basics[Nova]/region[0]"), "None");
    // the last template naming a region gives it
    let m = new_sheet(&["template add \"Hill Born\"", "template add Roamer"]);
    assert_eq!(at(&m, "basics[Nova]/region[0]"), "Far Hills");
}

#[test]
fn spellbook_items_are_books() {
    let m = new_sheet(&["class add Sage", "equip add Notebook 2"]);
    let name = |i: usize| {
        m.get(&format!(
            "character/spells[0]/memorized_spells[0]/spellbook[{}]/@name",
            i
        ))
        .cloned()
    };
    // one book per item, after the books the file names
    assert_eq!(name(0).as_deref(), Some("Notebook"));
    assert_eq!(name(1).as_deref(), Some("Notebook #2"));
    assert_eq!(name(2), None);
}

/// Prepared spells of the nth book's first class at a level, sorted:
/// (name, times, dc).
fn prepared(
    m: &HashMap<String, String>,
    book: usize,
    level: usize,
) -> Vec<(String, String, String)> {
    let prefix = format!(
        "character/spells[0]/memorized_spells[0]/spellbook[{}]/class[0]/level[{}]/spell[",
        book, level
    );
    let mut out: Vec<(String, String, String)> = m
        .iter()
        .filter(|(k, _)| k.starts_with(&prefix) && k.ends_with("]/name[0]"))
        .map(|(k, v)| {
            let base = k.strip_suffix("/name[0]").unwrap();
            let get = |f: &str| {
                m.get(&format!("{}/{}[0]", base, f))
                    .cloned()
                    .unwrap_or_default()
            };
            (v.clone(), get("times_memorized"), get("dc"))
        })
        .collect();
    out.sort();
    out
}

fn row(name: &str, times: &str, dc: &str) -> (String, String, String) {
    (name.to_string(), times.to_string(), dc.to_string())
}

#[test]
fn spells_prepared_with_metamagic() {
    let mut s = new_session(&[
        "set stat MGT 14",
        "class add Sage",
        "class add Sage",
        "spell add Sage Ember",
        "ability add \"Wide Spell\" --force",
        "ability add \"Sharp Spell\" --force",
    ]);
    // the level saved is that of the slot the feats raise it to
    let lines = pcgtools::pcg::PcgFile::parse(
        "SPELLBOOK:Prepared Spells|TYPE:2\n\
         SPELLNAME:Ember|TIMES:1|CLASS:Sage|BOOK:Prepared Spells|SPELLLEVEL:1|SOURCE:[TYPE:CLASS|NAME:Sage]\n\
         SPELLNAME:Ember|TIMES:3|CLASS:Sage|BOOK:Prepared Spells|SPELLLEVEL:1|SOURCE:[TYPE:CLASS|NAME:Sage]\n\
         SPELLNAME:Ember|TIMES:2|CLASS:Sage|BOOK:Prepared Spells|SPELLLEVEL:2|FEATLIST:[FEAT:Wide Spell]|SOURCE:[TYPE:CLASS|NAME:Sage]\n\
         SPELLNAME:Ember|TIMES:1|CLASS:Sage|BOOK:Prepared Spells|SPELLLEVEL:3|FEATLIST:[FEAT:Wide Spell|FEAT:Sharp Spell]|SOURCE:[TYPE:CLASS|NAME:Sage]\n",
    )
    .lines;
    s.ch.pcg.lines.extend(lines);
    let m = session_sheet(&s);
    // a second line for the same book and level is not counted
    assert_eq!(prepared(&m, 0, 1), [row("Ember", "1", "13")]);
    // names list the feats; the DC is from the spell's own level, plus
    // the feats' DC bonuses
    assert_eq!(prepared(&m, 0, 2), [row("Ember [Wide Spell]", "2", "13")]);
    assert_eq!(
        prepared(&m, 0, 3),
        [row("Ember [Wide Spell, Sharp Spell]", "1", "14")]
    );
}

#[test]
fn spell_ranges_with_metamagic() {
    let mut s = new_session(&[
        "set stat MGT 14",
        "class add Sage",
        "class add Sage",
        "spell add Sage Ember",
        "spell add Sage Mend",
        "ability add \"Wide Spell\" --force",
        "ability add \"Sharp Spell\" --force",
    ]);
    let lines = pcgtools::pcg::PcgFile::parse(
        "SPELLBOOK:Prepared Spells|TYPE:2\n\
         SPELLNAME:Ember|TIMES:1|CLASS:Sage|BOOK:Prepared Spells|SPELLLEVEL:1|SOURCE:[TYPE:CLASS|NAME:Sage]\n\
         SPELLNAME:Mend|TIMES:1|CLASS:Sage|BOOK:Prepared Spells|SPELLLEVEL:0|SOURCE:[TYPE:CLASS|NAME:Sage]\n\
         SPELLNAME:Ember|TIMES:1|CLASS:Sage|BOOK:Prepared Spells|SPELLLEVEL:2|FEATLIST:[FEAT:Wide Spell]|SOURCE:[TYPE:CLASS|NAME:Sage]\n\
         SPELLNAME:Mend|TIMES:1|CLASS:Sage|BOOK:Prepared Spells|SPELLLEVEL:1|FEATLIST:[FEAT:Sharp Spell]|SOURCE:[TYPE:CLASS|NAME:Sage]\n\
         SPELLNAME:Ember|TIMES:1|CLASS:Sage|BOOK:Prepared Spells|SPELLLEVEL:3|FEATLIST:[FEAT:Wide Spell|FEAT:Sharp Spell]|SOURCE:[TYPE:CLASS|NAME:Sage]\n\
         SPELLNAME:Mend|TIMES:1|CLASS:Sage|BOOK:Prepared Spells|SPELLLEVEL:2|FEATLIST:[FEAT:Sharp Spell|FEAT:Wide Spell]|SOURCE:[TYPE:CLASS|NAME:Sage]\n",
    )
    .lines;
    s.ch.pcg.lines.extend(lines);
    let m = session_sheet(&s);
    let ranges = |level: usize| -> Vec<(String, String)> {
        let prefix = format!(
            "character/spells[0]/memorized_spells[0]/spellbook[0]/class[0]/level[{}]/spell[",
            level
        );
        let mut out: Vec<(String, String)> = m
            .iter()
            .filter(|(k, _)| k.starts_with(&prefix) && k.ends_with("]/name[0]"))
            .map(|(k, v)| {
                let base = k.strip_suffix("/name[0]").unwrap();
                (v.clone(), m[&format!("{}/range[0]", base)].clone())
            })
            .collect();
        out.sort();
        out
    };
    let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
    // without game mode formulas: close is 25 ft. + 5 per two caster
    // levels, medium 100 ft. + 10 per level
    assert_eq!(ranges(0), [pair("Mend", "Medium (120')")]);
    assert_eq!(
        ranges(1),
        [
            pair("Ember", "Close (30')"),
            pair("Mend [Sharp Spell]", "Medium (130')")
        ]
    );
    // each feat in turn adds its range, then multiplies
    assert_eq!(
        ranges(2),
        [
            pair("Ember [Wide Spell]", "Close (60')"),
            pair("Mend [Sharp Spell, Wide Spell]", "Medium (260')")
        ]
    );
    assert_eq!(
        ranges(3),
        [pair("Ember [Wide Spell, Sharp Spell]", "Close (70')")]
    );
}

#[test]
fn one_equipment_set_element_per_set() {
    let mut s = new_session(&[
        "equip add \"Tin Shield\"",
        "equip add \"Wooden Spoon\"",
        "equip wear \"Wooden Spoon\" Carried",
        "eqset add Battle",
        "equip wear \"Tin Shield\" Arm",
        "eqset add Empty",
        "eqset use Battle",
    ]);
    let m = session_sheet(&s);
    let set = |i: usize, p: &str| {
        m.get(&format!(
            "character/equipment[0]/equipmentsets[0]/equipmentset[{}]/{}",
            i, p
        ))
        .cloned()
        .unwrap_or_default()
    };
    // the items of a set, by name
    let items = |i: usize| {
        let pre = format!(
            "character/equipment[0]/equipmentsets[0]/equipmentset[{}]/item[",
            i
        );
        let mut v: Vec<String> = m
            .keys()
            .filter_map(|k| {
                k.strip_prefix(&pre)?
                    .split_once(']')
                    .map(|x| x.0.to_string())
            })
            .collect();
        v.sort();
        v.dedup();
        v
    };
    // every set, in order, each with its own items
    assert_eq!(set(0, "@name"), "Default Set");
    assert_eq!(items(0), ["Wooden Spoon"]);
    assert_eq!(set(0, "item[Wooden Spoon]/equipped[0]"), "N");
    assert_eq!(set(1, "@name"), "Battle");
    assert_eq!(items(1), ["Tin Shield"]);
    assert_eq!(set(1, "item[Tin Shield]/equipped[0]"), "Y");
    assert_eq!(set(2, "@name"), "Empty");
    assert!(items(2).is_empty());
    // the rest of the sheet is the set used
    assert!(m.contains_key("character/equipment[0]/item[Tin Shield]/name[0]"));
    assert!(!m.contains_key("character/equipment[0]/item[Wooden Spoon]/name[0]"));
    assert_eq!(at(&m, "equipment[0]/total[0]/weight[0]"), "5 lbs.");
    assert_eq!(at(&m, "armor_class[0]/total[0]"), "11");
    s.exec("eqset use \"Default Set\"").unwrap();
    let m = session_sheet(&s);
    assert!(m.contains_key("character/equipment[0]/item[Wooden Spoon]/name[0]"));
    assert_eq!(at(&m, "armor_class[0]/total[0]"), "10");
}

/// A new character with saved lines added after the editor commands.
fn with_lines(cmds: &[&str], text: &str) -> HashMap<String, String> {
    let mut s = new_session(cmds);
    let lines = pcgtools::pcg::PcgFile::parse(text).lines;
    s.ch.pcg.lines.extend(lines);
    session_sheet(&s)
}

fn ac_total(m: &HashMap<String, String>) -> i64 {
    at(m, "armor_class[0]/total[0]").parse().unwrap()
}

#[test]
fn temporary_bonus_switches() {
    let base = ac_total(&new_sheet(&[]));
    let on = "TEMPBONUS:SPELL=Spark|TBTARGET:PC|TBBONUS:COMBAT&pipe;AC&pipe;2\n";
    assert_eq!(ac_total(&with_lines(&[], on)), base + 2);
    // a bonus switched off does not count
    let off = "TEMPBONUS:SPELL=Spark|TBTARGET:PC|TBACTIVE:N|TBBONUS:COMBAT&pipe;AC&pipe;2\n";
    assert_eq!(ac_total(&with_lines(&[], off)), base);
    // nor does any, with temporary modifiers off
    let unused = format!("USETEMPMODS:N\n{}", on);
    assert_eq!(ac_total(&with_lines(&[], &unused)), base);
}

#[test]
fn temporary_bonus_on_armor() {
    let cmds = ["equip add \"Dented Vest\"", "equip wear \"Dented Vest\""];
    let base = ac_total(&new_sheet(&cmds));
    let m = with_lines(
        &cmds,
        "TEMPBONUS:SPELL=Spark|TBTARGET:Dented Vest|TBBONUS:COMBAT&pipe;AC&pipe;2\n",
    );
    // a bonus aimed at an item counts for the character too
    assert_eq!(ac_total(&m), base + 2);
    // the copy carrying it replaces the armor worn
    let a = "protection[0]/armor[*Dented Vest [Spark]]";
    assert_eq!(at(&m, &format!("{}/acbonus[0]", a)), "+1");
    assert_eq!(at(&m, &format!("{}/wt[0]", a)), "5");
    assert!(!m.contains_key("character/protection[0]/armor[*Dented Vest]/acbonus[0]"));
    assert!(!m
        .keys()
        .any(|k| k.starts_with("character/equipment[0]/item[Dented Vest#2]/")));
    assert_eq!(at(&m, "equipment[0]/item[Dented Vest]/weight[0]"), "5");
    assert_eq!(at(&m, "equipment[0]/item[Dented Vest]/equipped[0]"), "Y");
    // switched off, it still rides on the copy but not the character
    let m = with_lines(
        &cmds,
        "TEMPBONUS:SPELL=Spark|TBTARGET:Dented Vest|TBACTIVE:N|TBBONUS:COMBAT&pipe;AC&pipe;2\n",
    );
    assert_eq!(ac_total(&m), base);
    assert_eq!(at(&m, &format!("{}/acbonus[0]", a)), "+1");
}

#[test]
fn temporary_copy_of_armor_loses_its_check_penalty() {
    // PCGen leaves copies carrying temporary bonuses out of the check
    // penalty, and the copy has taken the armor's place
    let cmds = ["equip add \"Scale Coat\"", "equip wear \"Scale Coat\""];
    let penalty =
        |m: &HashMap<String, String>| at(m, "armor_class[0]/check_penalty[0]").to_string();
    assert_eq!(penalty(&new_sheet(&cmds)), "-4");
    let m = with_lines(
        &cmds,
        "TEMPBONUS:SPELL=Spark|TBTARGET:Scale Coat|TBBONUS:COMBAT&pipe;AC&pipe;1\n",
    );
    assert_eq!(penalty(&m), "+0");
    // its own entry still shows it
    assert_eq!(
        at(&m, "protection[0]/armor[*Scale Coat [Spark]]/accheck[0]"),
        "-4"
    );
}

#[test]
fn temporary_bonus_on_unworn_weapon() {
    let m = with_lines(
        &[
            "equip add \"Throwing Cog\"",
            "equip wear \"Throwing Cog\" Carried",
        ],
        "TEMPBONUS:SPELL=Spark|TBTARGET:Throwing Cog|TBBONUS:WEAPON&pipe;TOHIT&pipe;1\n",
    );
    // the copy is listed apart, not worn, weighing nothing
    let mut both: Vec<(&str, &str)> = ["Throwing Cog", "Throwing Cog#2"]
        .iter()
        .map(|n| {
            let e = format!("equipment[0]/item[{}]", n);
            (
                at(&m, &format!("{}/equipped[0]", e)),
                at(&m, &format!("{}/weight[0]", e)),
            )
        })
        .collect();
    both.sort();
    assert_eq!(both, [("N", "0"), ("N", "1")]);
    // and held nowhere, but as a temporary bonus
    let hand: Vec<&String> = m
        .iter()
        .filter(|(k, _)| k.ends_with("/common[Throwing Cog [Spark]]/hand[0]"))
        .map(|(_, v)| v)
        .collect();
    assert_eq!(hand, ["Temp Bonus"]);
}

#[test]
fn temporary_copies_in_sets_follow_the_set() {
    let mut s = new_session(&[
        "equip add \"Throwing Cog\"",
        "equip wear \"Throwing Cog\" Carried",
    ]);
    s.ch.pcg.lines.extend(
        pcgtools::pcg::PcgFile::parse(
            "TEMPBONUS:SPELL=Spark|TBTARGET:Throwing Cog|TBBONUS:WEAPON&pipe;TOHIT&pipe;1\n",
        )
        .lines,
    );
    let copy =
        "character/equipment[0]/equipmentsets[0]/equipmentset[0]/item[Throwing Cog#2]/name[0]";
    let mark = |s: &mut pcgtools::repl::Session, flag: &str| {
        let root =
            s.ch.pcg
                .lines
                .iter_mut()
                .find(|l| l.tag() == Some("EQUIPSET") && l.text("ID").as_deref() == Some("0.1"))
                .unwrap();
        let mut elems: Vec<_> = root
            .elems
            .clone()
            .unwrap()
            .into_iter()
            .filter(|e| e.tag != "USETEMPMODS")
            .collect();
        elems.push(pcgtools::pcg::Elem::raw("USETEMPMODS", flag));
        root.set_elems(elems);
    };
    // a set not marked to use temporary modifiers lists no copies
    mark(&mut s, "N");
    assert!(!session_sheet(&s).contains_key(copy));
    mark(&mut s, "Y");
    assert!(session_sheet(&s).contains_key(copy));
}
