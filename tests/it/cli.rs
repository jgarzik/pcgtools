// CLI integration tests: run the built binary against the synthetic
// fixture data set.

use crate::common;
use std::path::PathBuf;
use std::process::{Command, Output};

fn pcgtools(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pcgtools"))
        .args(args)
        .output()
        .expect("run pcgtools")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn pcc() -> String {
    common::fixture("mini/data/testbook/testbook.pcc")
        .to_string_lossy()
        .into_owned()
}

fn data_dir() -> String {
    common::fixture("mini/data").to_string_lossy().into_owned()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pcgtools-it-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn deps_lists_files() {
    let o = pcgtools(&["--data-dir", &data_dir(), "deps", &pcc()]);
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    assert!(out.contains("TB [1]"), "{}", out);
    assert!(out.contains("tb_spells_mod.lst"));
    assert!(!out.contains("tb_spells_never.lst"));
    assert!(out.contains("common_languages.lst"));
}

#[test]
fn load_json_counts() {
    let o = pcgtools(&["--data-dir", &data_dir(), "--json", "load", "-q", &pcc()]);
    assert!(o.status.success(), "{}", stderr(&o));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["campaigns"], 2);
    assert_eq!(v["objects"]["SPELL"], 2);
    assert_eq!(v["objects"]["CLASS"], 5);
    assert_eq!(v["tables"], 1);
    assert_eq!(v["errors"], 0);
}

#[test]
fn show_object() {
    let o = pcgtools(&[
        "--data-dir",
        &data_dir(),
        "show",
        "-p",
        &pcc(),
        "spell",
        "spark",
    ]);
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    assert!(out.starts_with("Spark\t# "), "{}", out);
    assert!(out.contains("\tDESC:A tiny spark.\n"));
    assert!(out.contains("# MOD at "));

    let o = pcgtools(&[
        "--data-dir",
        &data_dir(),
        "show",
        "-p",
        &pcc(),
        "spell",
        "nope",
    ]);
    assert!(!o.status.success());
}

#[test]
fn query_matches() {
    let o = pcgtools(&[
        "--data-dir",
        &data_dir(),
        "query",
        "-p",
        &pcc(),
        "equipment",
        "TYPE=Kitchen",
        "-f",
        "COST",
    ]);
    assert!(o.status.success(), "{}", stderr(&o));
    // a .COPY that sets a single-valued tag replaces the copied value
    assert_eq!(stdout(&o), "Wooden Spoon\t1\nSilver Spoon\t25\n");

    let o = pcgtools(&[
        "--data-dir",
        &data_dir(),
        "query",
        "-p",
        &pcc(),
        "ability",
        "-n",
        "*step",
        "-c",
    ]);
    assert_eq!(stdout(&o).trim(), "2");
}

#[test]
fn dump_is_json() {
    let o = pcgtools(&["--data-dir", &data_dir(), "dump", &pcc()]);
    assert!(o.status.success());
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert!(v["kinds"]["SPELL"]["objects"].is_array());
}

#[test]
fn edit_to_output_file() {
    let src = common::fixture("mini/data/testbook/tb_equip.lst");
    let out = scratch("edited.lst");
    let o = pcgtools(&[
        "edit",
        src.to_str().unwrap(),
        "Cursed Spoon",
        "--set",
        "COST:2",
        "--add",
        "DESC:Do not use",
        "-o",
        out.to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", stderr(&o));
    let before = std::fs::read_to_string(&src).unwrap();
    let after = std::fs::read_to_string(&out).unwrap();
    assert_eq!(
        after,
        before.replace(
            "Cursed Spoon\tTYPE:Goods.Kitchen\tCOST:1\n",
            "Cursed Spoon\tTYPE:Goods.Kitchen\tCOST:2\tDESC:Do not use\n"
        )
    );

    let o = pcgtools(&["edit", src.to_str().unwrap(), "Missing", "--remove", "X"]);
    assert!(!o.status.success());
}

#[test]
fn fmt_check_and_write() {
    let f = scratch("messy.lst");
    std::fs::write(&f, "A\t\t B:1 \r\n\r\n").unwrap();
    let o = pcgtools(&["fmt", "--check", f.to_str().unwrap()]);
    assert!(!o.status.success());
    let o = pcgtools(&["fmt", "-w", f.to_str().unwrap()]);
    assert!(o.status.success());
    assert_eq!(std::fs::read_to_string(&f).unwrap(), "A\tB:1\n");
    let o = pcgtools(&["fmt", "--check", f.to_str().unwrap()]);
    assert!(o.status.success());
}

#[test]
fn lint_fixture() {
    let o = pcgtools(&[
        "--data-dir",
        &data_dir(),
        "lint",
        "--min",
        "warning",
        &pcc(),
    ]);
    // the fixture contains one deliberate duplicate
    assert!(o.status.success(), "{}", stdout(&o));
    let out = stdout(&o);
    assert!(out.contains("duplicate object 'Wooden Spoon'"), "{}", out);

    let f = scratch("bad.lst");
    std::fs::write(&f, "SOURCELONG:X\nThing\tBOGUSTAG:1\tBONUS:VAR|X\n").unwrap();
    let o = pcgtools(&["lint", "-k", "spell", f.to_str().unwrap()]);
    assert!(!o.status.success());
    let out = stdout(&o);
    assert!(out.contains("unknown tag 'BOGUSTAG' for SPELL"), "{}", out);
    assert!(out.contains("BONUS needs"), "{}", out);
}

#[test]
fn tags_lists_scope() {
    let o = pcgtools(&["tags", "pcc"]);
    assert!(o.status.success());
    assert!(stdout(&o).lines().any(|l| l == "PCC\tGAMEMODE"));
}

#[test]
fn parse_and_roundtrip() {
    let o = pcgtools(&["parse", &pcc()]);
    assert!(o.status.success());
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["entries"][0]["tag"], "CAMPAIGN");

    let o = pcgtools(&["roundtrip", &data_dir()]);
    assert!(o.status.success(), "{}", stdout(&o));
}

#[test]
fn campaigns_lists_fixture() {
    let o = pcgtools(&["campaigns", &data_dir()]);
    assert!(o.status.success());
    let out = stdout(&o);
    assert!(out.contains("TB\tTestmode\t1\tCore Rules"), "{}", out);
    assert!(out.contains("Common Stuff"));
}

#[test]
fn gamemode_listing() {
    let sys = common::fixture("mini/system")
        .to_string_lossy()
        .into_owned();
    let o = pcgtools(&["--system-dir", &sys, "gamemode"]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).contains("Testmode\tTestmode|TM"));
    let o = pcgtools(&["--system-dir", &sys, "gamemode", "TM", "-f", "level.lst"]);
    assert!(o.status.success());
    assert!(stdout(&o).contains("LEVEL:2\tMINXP:3000"), "{}", stdout(&o));
    let o = pcgtools(&["--system-dir", &sys, "gamemode", "Nope"]);
    assert!(!o.status.success());
}

#[test]
fn lint_reports_unknown_gamemode() {
    let sys = common::fixture("mini/system")
        .to_string_lossy()
        .into_owned();
    let f = scratch("gm.pcc");
    std::fs::write(&f, "CAMPAIGN:X\nGAMEMODE:Nowhere\n").unwrap();
    let o = pcgtools(&["--system-dir", &sys, "lint", f.to_str().unwrap()]);
    assert!(
        stdout(&o).contains("unknown game mode 'Nowhere'"),
        "{}",
        stdout(&o)
    );
}

fn tess() -> String {
    common::fixture("mini/characters/tess.pcg")
        .to_string_lossy()
        .into_owned()
}

#[test]
fn pcg_show_and_lint() {
    let o = pcgtools(&["pcg", "show", &tess()]);
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    assert!(out.starts_with("Tess Tinkerton\n"), "{}", out);
    assert!(out.contains("classes:    Tinker 2"));
    assert!(out.contains("stats:      MGT 14, GRC 12"));

    let o = pcgtools(&["--json", "pcg", "show", &tess()]);
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["levels"][1]["hp"], 4);

    let sys = common::fixture("mini/system")
        .to_string_lossy()
        .into_owned();
    let o = pcgtools(&[
        "--data-dir",
        &data_dir(),
        "--system-dir",
        &sys,
        "pcg",
        "lint",
        &tess(),
    ]);
    assert!(o.status.success(), "{}", stdout(&o));
    let out = stdout(&o);
    assert!(
        out.contains("ability 'Vanished Feat' (FEAT) not found"),
        "{}",
        out
    );
    assert!(!out.contains("Steady Hands"), "{}", out);
    assert!(!out.contains("Spoon"), "{}", out);
}

#[test]
fn parse_pcg_json() {
    let o = pcgtools(&["parse", &tess()]);
    assert!(o.status.success());
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["lines"][0]["elems"][0]["tag"], "PCGVERSION");
}

#[test]
fn pcg_stats_json_and_explain() {
    let sys = common::fixture("mini/system")
        .to_string_lossy()
        .into_owned();
    let base = ["--data-dir", &data_dir(), "--system-dir", &sys];
    let mut args: Vec<&str> = base.to_vec();
    let t = tess();
    args.extend(["--json", "pcg", "stats", &t]);
    let o = pcgtools(&args);
    assert!(o.status.success(), "{}", stderr(&o));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["bab"], 1);
    assert_eq!(v["hp"], 14);
    assert_eq!(v["ac"]["Total"], 12);

    let mut args: Vec<&str> = base.to_vec();
    args.extend(["pcg", "stats", &t, "-e", "COMBAT.AC"]);
    let o = pcgtools(&args);
    let out = stdout(&o);
    assert!(
        out.contains("+1 [Shield] from Equipment Tin Shield"),
        "{}",
        out
    );
}

fn pcgtools_stdin(args: &[&str], input: &str) -> Output {
    use std::io::Write;
    let mut child = Command::new(env!("CARGO_BIN_EXE_pcgtools"))
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("run pcgtools");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn repl_script_from_stdin() {
    let sys = common::fixture("mini/system")
        .to_string_lossy()
        .into_owned();
    let out = scratch("scripted.pcg");
    let script = format!(
        "set stat MGT 16\nrace Gearling\nclass add Tinker\nfeat add \"Quick Step\"\nbogus\nsave \"{}\"\n",
        out.display()
    );
    let o = pcgtools_stdin(
        &[
            "--data-dir",
            &data_dir(),
            "--system-dir",
            &sys,
            "repl",
            "--new",
            "--gamemode",
            "Testmode",
            "--campaign",
            "TB",
            "--name",
            "Scripty",
        ],
        &script,
    );
    // the bad command makes the run fail, but the rest still happens
    assert!(!o.status.success());
    assert!(
        stderr(&o).contains("unknown command 'bogus'"),
        "{}",
        stderr(&o)
    );
    let text = stdout(&o);
    assert!(text.contains("Tinker is now level 1"), "{}", text);
    let saved = std::fs::read_to_string(&out).unwrap();
    assert!(saved.contains("CHARACTERNAME:Scripty"));
    assert!(saved.contains("RACE:Gearling"));
    assert!(saved.contains("KEY:QuickStep"));
}

#[test]
fn char_one_shot_writes_output() {
    let sys = common::fixture("mini/system")
        .to_string_lossy()
        .into_owned();
    let out = scratch("oneshot.pcg");
    let t = tess();
    let o = pcgtools(&[
        "--data-dir",
        &data_dir(),
        "--system-dir",
        &sys,
        "char",
        &t,
        "-o",
        out.to_str().unwrap(),
        "class",
        "add",
        "Tinker",
        "3",
    ]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).contains("Tinker is now level 3"));
    let saved = std::fs::read_to_string(&out).unwrap();
    assert!(saved.contains("CLASSABILITIESLEVEL:Tinker=3|HITPOINTS:3"));
    // the original is untouched
    let orig = std::fs::read_to_string(common::fixture("mini/characters/tess.pcg")).unwrap();
    assert!(!orig.contains("Tinker=3"));

    // read-only commands do not write
    let o = pcgtools(&[
        "--data-dir",
        &data_dir(),
        "--system-dir",
        &sys,
        "char",
        &t,
        "prereq",
        "feat",
        "Quick Step",
    ]);
    assert_eq!(stdout(&o).trim(), "qualifies for QuickStep");
}
