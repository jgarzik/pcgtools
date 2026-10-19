// Declared variables, modifiers, channels and code controls on the
// synthetic fixture (tests/fixtures/mini/data/vars).

use crate::common;
use pcgtools::context::Context;
use pcgtools::pcc::resolve::DataDirs;
use pcgtools::pcg::character::Character;
use pcgtools::rules::calc::Calc;
use pcgtools::rules::vars::system::{Owner, VarSystem};
use pcgtools::rules::vars::value::Value;

fn with_system(f: impl FnOnce(&VarSystem)) {
    let dirs = DataDirs {
        data: Some(common::fixture("mini/data")),
        system: Some(common::fixture("mini/system")),
        ..Default::default()
    };
    let ch = Character::read(&common::fixture("mini/characters/vera.pcg")).unwrap();
    let ctx = Context::for_character(&ch, dirs);
    let mut calc = Calc::new(&ctx.dataset, ctx.gamemode.as_ref(), &ch);
    calc.run();
    let sys = VarSystem::new(&calc);
    f(&sys);
}

fn global(sys: &VarSystem, v: &str) -> String {
    sys.get(&Owner::global(), v).unwrap().to_string()
}

#[test]
fn channels_functions_and_scopes() {
    with_system(|sys| {
        let mgt = Owner::new("PC.STAT", "MGT");
        // the channel carries the base score from the character
        assert_eq!(sys.get(&mgt, "Score"), Some(Value::Int(15)));
        // data-defined function d20Mod
        assert_eq!(sys.get(&mgt, "Mod"), Some(Value::Int(2)));
        // a stat line may set a global
        assert_eq!(global(sys, "MGTMOD"), "2");
        // globals are visible from local scopes
        assert_eq!(sys.get(&mgt, "MGTMOD"), Some(Value::Int(2)));
    });
}

#[test]
fn modifier_order_and_formats() {
    with_system(|sys| {
        // SET 10, MULTIPLY 2, ADD 1 (inherent order), then PRIORITY=1 ADD 5
        assert_eq!(global(sys, "Order"), "26");
        assert_eq!(global(sys, "Face"), "5,5");
        assert_eq!(global(sys, "Motto"), "Onward");
        assert_eq!(global(sys, "Shiny"), "true");
        // never modified: the declared default for its format
        assert_eq!(global(sys, "Untouched"), "none");
        assert!(sys.get(&Owner::global(), "NoSuchVariable").is_none());
        let warn = sys.diags.borrow();
        assert!(warn
            .items
            .iter()
            .any(|d| d.message.contains("'NotDeclared' is not declared")));
    });
}

#[test]
fn dynamic_objects_and_remote_modifiers() {
    with_system(|sys| {
        // granted by the race, set by the race, raised by an ability
        let walk = Owner::new("PC.MOVEMENT", "Walk");
        assert_eq!(sys.get(&walk, "Speed"), Some(Value::Int(40)));
        // not granted: no instance
        assert!(!sys.instances["PC.MOVEMENT"].iter().any(|o| o.key == "Fly"));
    });
}

#[test]
fn equipment_and_parts() {
    with_system(|sys| {
        // the item sets 40; its modifier adds a table lookup (12)
        let pack = Owner::new("PC.EQUIPMENT", "Backpack");
        assert_eq!(sys.get(&pack, "Capacity"), Some(Value::Int(52)));
        let res = sys.results();
        assert_eq!(res["PC.EQUIPMENT:Backpack"]["Capacity"], "52");
        assert_eq!(res["PC"]["Face"], "5,5");
        assert!(sys.unknown().is_empty(), "{:?}", sys.unknown());
    });
}
