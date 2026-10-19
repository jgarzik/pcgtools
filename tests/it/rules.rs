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
fn move_prerequisites_without_any_movement() {
    use pcgtools::token::pre::Prereq;
    let dirs = dirs();
    let mut s = pcgtools::repl::Session::create("Testmode", &["TB".into()], "Nova", dirs.clone());
    let check = |ch: &Character, pre: &str| {
        let ctx = Context::for_character(ch, dirs.clone());
        let mut calc = Calc::new(&ctx.dataset, ctx.gamemode.as_ref(), ch);
        calc.run();
        calc.passes(&Prereq::parse(pre).unwrap(), None)
    };
    // with no movement at all, every speed requirement is met
    assert!(check(&s.ch, "PREMOVE:1,Climb=1"));
    assert!(!check(&s.ch, "!PREMOVE:1,Climb=1"));
    // once the race gives a speed, only the speeds it has count
    s.exec("race Gearling").unwrap();
    assert!(!check(&s.ch, "PREMOVE:1,Climb=1"));
    assert!(check(&s.ch, "PREMOVE:1,Walk=20"));
    assert!(!check(&s.ch, "PREMOVE:1,Walk=30"));
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

#[test]
fn spellcast_bonus_targets_written_with_equals() {
    // data writes CLASS=x;LEVEL=n and TYPE=t;LEVEL=n for what PCGen
    // reads as CLASS.x;LEVEL.n
    let per_day = |class: &str, levels: usize, feat: bool| {
        let mut s = pcgtools::repl::Session::create("Testmode", &["TB".into()], "Nova", dirs());
        for _ in 0..levels {
            s.exec(&format!("class add {}", class)).unwrap();
        }
        if feat {
            s.exec("feat add \"Spell Spring\" --force").unwrap();
        }
        let mut calc = Calc::new(&s.ctx.dataset, s.ctx.gamemode.as_ref(), &s.ch);
        calc.run();
        let cls = s
            .ctx
            .dataset
            .get(pcgtools::lst::kind::FileKind::Class, class)
            .unwrap();
        (0..3)
            .map(|l| pcgtools::rules::spells::cast_per_day(&calc, cls, l, false))
            .collect::<Vec<_>>()
    };
    // a class without a table at a level gets only its class's bonus
    assert_eq!(per_day("Tinker", 1, false)[1], 0);
    assert_eq!(per_day("Tinker", 1, true)[1], 2);
    // with a table: the spell type's bonus adds, and a multiplier
    // multiplies the table's count only
    assert_eq!(per_day("Chanter", 2, false), [2, 2, 1]);
    assert_eq!(per_day("Chanter", 2, true), [2, 3, 2]);
}

/// Whether a new character, after editor commands, meets a
/// prerequisite.
fn meets(cmds: &[&str], pre: &str) -> bool {
    use pcgtools::token::pre::Prereq;
    let mut s = pcgtools::repl::Session::create("Testmode", &["TB".into()], "Nova", dirs());
    for c in cmds {
        s.exec(c).unwrap();
    }
    let mut calc = Calc::new(&s.ctx.dataset, s.ctx.gamemode.as_ref(), &s.ch);
    calc.run();
    calc.passes(&Prereq::parse(pre).unwrap(), None)
}

#[test]
fn spellcaster_kind_prerequisites() {
    let tinker = ["class add Tinker"];
    let chanter = ["class add Chanter"];
    // PRESPELLCAST: spell type, or whether a class memorizes
    // (default yes); several arguments are alternatives
    assert!(meets(&tinker, "PRESPELLCAST:TYPE=Arcane"));
    assert!(!meets(&tinker, "PRESPELLCAST:TYPE=Divine"));
    assert!(meets(&tinker, "PRESPELLCAST:TYPE=Divine,MEMORIZE=Y"));
    assert!(meets(&chanter, "PRESPELLCAST:MEMORIZE=N"));
    assert!(!meets(&chanter, "PRESPELLCAST:MEMORIZE=Y"));
    assert!(!meets(&[], "PRESPELLCAST:TYPE=Arcane"));
    // PRESPELLBOOK: exactly one class keeps (or does not keep) a book
    assert!(meets(&tinker, "PRESPELLBOOK:YES"));
    assert!(!meets(&chanter, "PRESPELLBOOK:YES"));
    assert!(meets(&chanter, "PRESPELLBOOK:NO"));
    assert!(meets(
        &["class add Tinker", "class add Chanter"],
        "PRESPELLBOOK:NO"
    ));
}

#[test]
fn body_and_defense_prerequisites() {
    let tall = ["set align LG", "race Tallfolk"];
    // bare HANDS/LEGS compare at least
    assert!(meets(&tall, "PREHANDS:4"));
    assert!(!meets(&[], "PREHANDS:4"));
    assert!(meets(&[], "PRELEGS:2"));
    // reach: the race's (5 if it gives none), and its comparisons
    assert!(meets(&tall, "PREREACH:10"));
    assert!(meets(&tall, "PREREACHEQ:10"));
    assert!(!meets(&["race Gearling"], "PREREACHGTEQ:10"));
    assert!(meets(&["race Gearling"], "PREREACHEQ:5"));
    // spell resistance
    assert!(meets(&tall, "PRESRGTEQ:5"));
    assert!(!meets(&tall, "PRESRGT:5"));
    assert!(meets(&[], "PRESRLT:1"));
    // hit points
    assert!(meets(&["class add Tinker 6"], "PREHP:6"));
    assert!(!meets(&["class add Tinker 6"], "PREHP:7"));
}

#[test]
fn class_level_max_prerequisites() {
    let two = ["class add Tinker", "class add Tinker"];
    assert!(meets(&two, "PRECLASSLEVELMAX:1,Tinker=2"));
    assert!(!meets(&two, "PRECLASSLEVELMAX:1,Tinker=1"));
    // a class the character lacks is at level 0
    assert!(meets(&two, "PRECLASSLEVELMAX:1,Chanter=1"));
    assert!(meets(&two, "!PRECLASSLEVELMAX:1,Tinker=1"));
    // with several items the negation has no effect (as in PCGen)
    assert!(meets(&two, "!PRECLASSLEVELMAX:1,Tinker=1,Chanter=1"));
}

#[test]
fn context_prerequisites() {
    // a wield category can only be judged on an item; on a character
    // it fails, negated or not
    assert!(!meets(&[], "PREWIELD:1,Light"));
    assert!(!meets(&[], "!PREWIELD:1,Light"));
    // campaigns in use
    assert!(meets(&[], "PRECAMPAIGN:1,TB"));
    assert!(!meets(&[], "PRECAMPAIGN:1,OLD"));
    assert!(meets(&[], "PRECAMPAIGN:1,BOOKTYPE=Core Rules"));
    assert!(meets(&[], "!PRECAMPAIGN:1,OLD"));
    // kits applied (recorded in the file); a trailing % is a prefix
    assert!(!meets(&[], "PREKIT:1,Cook Kit"));
    assert!(meets(&["kit apply \"Cook Kit\""], "PREKIT:1,Cook Kit"));
    assert!(meets(&["kit apply \"Cook Kit\""], "PREKIT:1,Cook%"));
}

#[test]
fn spell_type_prerequisites() {
    let tinker = ["class add Tinker"];
    let tinker_slot = ["class add Tinker", "feat add \"Spell Spring\" --force"];
    // a level of a spell type the character can cast (or knows)
    assert!(!meets(&tinker, "PRESPELLTYPE:1,Arcane=1"));
    assert!(meets(&tinker_slot, "PRESPELLTYPE:1,Arcane=1"));
    // the level is exact
    assert!(!meets(&tinker_slot, "PRESPELLTYPE:1,Arcane=2"));
    assert!(meets(&tinker_slot, "PRESPELLTYPE:1,ANY=1"));
    assert!(!meets(&tinker_slot, "PRESPELLTYPE:1,Divine=1"));
    // the count is how many spells of that level: two slots
    assert!(meets(&tinker_slot, "PRESPELLTYPE:2,Arcane=1"));
    assert!(!meets(&tinker_slot, "PRESPELLTYPE:3,Arcane=1"));
    // a class that neither memorizes nor has a known table, but can
    // cast, counts as unlimited
    assert!(meets(&["class add Chanter"], "PRESPELLTYPE:1,Divine=1"));
    assert!(meets(&["class add Chanter"], "PRESPELLTYPE:5,Divine=4"));
    assert!(!meets(&[], "PRESPELLTYPE:1,ANY=0"));
}

#[test]
fn spell_level_grants_check_their_prerequisites() {
    let mut s = pcgtools::repl::Session::create("Testmode", &["TB".into()], "Nova", dirs());
    s.exec("class add Tinker").unwrap();
    let mut calc = Calc::new(&s.ctx.dataset, s.ctx.gamemode.as_ref(), &s.ch);
    calc.run();
    let sp = pcgtools::rules::spells::Spells::new(&calc);
    let glow = s
        .ctx
        .dataset
        .get(pcgtools::lst::kind::FileKind::Spell, "Glow")
        .unwrap();
    // its own level, and only the grant whose prerequisite holds
    let mut levels = sp.levels_on('C', "Tinker", glow);
    levels.sort();
    assert_eq!(levels, vec![1, 3]);
}
