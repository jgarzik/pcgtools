// Characters built by the editor, checked against PCGen itself.
//
// Each scenario in tests/scenarios.txt is a new character and a list
// of editor commands.  The character is built and saved with the
// editor, its sheet is exported by a PCGen build, and every value of
// that sheet must match the sheet pcgtools builds from the saved file.
// PCGen complaints about loading the saved file also fail the test,
// apart from those it also makes about a bare new character.
//
// A scenario with a `full` line is also exported with PCGen's fuller
// sheet template, and what that sheet adds which pcgtools builds with
// its fuller sheet (situational skill rows, spell concentration) must
// match too.  REF_FULL=1 does this for every scenario.
//
// Runs only when PCGEN_ROOT names the data checkouts and PCGEN_REF a
// built PCGen tree (with build/libs, and JavaFX jars in mods/lib);
// PCGEN_REQUIRE=root,ref makes their absence a failure.
// The JVM is $PCGEN_JAVA, else $JAVA_HOME/bin/java, else java.
// REF_ONLY=name runs one scenario; REF_VERBOSE=1 prints differences.

use crate::common;
use pcgtools::context::Context;
use pcgtools::pcc::resolve::DataDirs;
use pcgtools::pcg::character::Character;
use pcgtools::repl::Session;
use pcgtools::sheet::{self, Element};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

struct Scenario {
    name: String,
    gamemode: String,
    campaigns: Vec<String>,
    commands: Vec<String>,
    /// sheet paths where PCGen is known to differ, with the reason
    differs: Vec<(String, String)>,
    /// sheet paths where PCGen's own value changes from run to run
    varies: Vec<String>,
    /// also compare the fuller sheet's additions
    full: bool,
}

/// Parse the scenario file: `== name` starts a scenario, `gamemode`
/// and `campaign` lines set it up, other lines are editor commands.
fn scenarios(text: &str) -> Vec<Scenario> {
    let mut out: Vec<Scenario> = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix("== ") {
            out.push(Scenario {
                name: name.trim().to_string(),
                gamemode: String::new(),
                campaigns: Vec::new(),
                commands: Vec::new(),
                differs: Vec::new(),
                varies: Vec::new(),
                full: false,
            });
            continue;
        }
        let s = out.last_mut().expect("scenario file starts with '== name'");
        if let Some(g) = line.strip_prefix("gamemode ") {
            s.gamemode = g.trim().to_string();
        } else if let Some(c) = line.strip_prefix("campaign ") {
            s.campaigns.push(c.trim().to_string());
        } else if line == "full" {
            s.full = true;
        } else if let Some(d) = line.strip_prefix("differs ") {
            let (path, why) = d.split_once(" -- ").unwrap_or((d, ""));
            s.differs
                .push((path.trim().to_string(), why.trim().to_string()));
        } else if let Some(v) = line.strip_prefix("varies ") {
            let path = v.split_once(" -- ").map_or(v, |(p, _)| p);
            s.varies.push(path.trim().to_string());
        } else {
            s.commands.push(line.to_string());
        }
    }
    out
}

fn java() -> PathBuf {
    if let Some(j) = std::env::var_os("PCGEN_JAVA") {
        return j.into();
    }
    if let Some(h) = std::env::var_os("JAVA_HOME") {
        return Path::new(&h).join("bin/java");
    }
    "java".into()
}

/// PCGen's sheet template for its tests, and its fuller sheet template.
const BASE_TEMPLATE: &str = "code/testsuite/base-xml.ftl";
const FULL_TEMPLATE: &str = "outputsheets/base.xml.ftl";

/// Export a character's sheet with PCGen and a sheet template: the
/// sheet, and the problems PCGen reported after loading the character.
fn pcgen_sheet(
    reference: &Path,
    data: &Path,
    pcg: &Path,
    work: &Path,
    template: &str,
) -> Result<(Element, Vec<String>), String> {
    let settings = work.join("settings");
    std::fs::create_dir_all(&settings).map_err(|e| e.to_string())?;
    std::fs::write(
        settings.join("config.ini.ref"),
        format!(
            "settingsPath={}\r\npccFilesPath={}\r\n",
            settings.display(),
            data.display()
        ),
    )
    .map_err(|e| e.to_string())?;
    let out = work.join("pcgen.xml");
    let _ = std::fs::remove_file(&out);
    let sep = if cfg!(windows) { ";" } else { ":" };
    let run = Command::new(java())
        .current_dir(reference)
        .args(["-Xmx2g", "-Djava.awt.headless=true", "-cp"])
        .arg(format!("build/libs/*{}mods/lib/*", sep))
        .arg("pcgen.system.Main")
        .arg("--character")
        .arg(pcg)
        .args(["--exportsheet", template, "--outputfile"])
        .arg(&out)
        .arg("--settingsdir")
        .arg(&settings)
        .args(["--configfilename", "config.ini.ref"])
        .output()
        .map_err(|e| format!("running PCGen: {}", e))?;
    let log =
        String::from_utf8_lossy(&run.stdout).into_owned() + &String::from_utf8_lossy(&run.stderr);
    // problems reading the character, and after it is loaded, are
    // about the character
    let complaints = log
        .lines()
        .skip_while(|l| {
            !l.contains("Loaded character") && !l.contains("were encountered while loading")
        })
        .filter(|l| l.contains(" SEVERE ") || l.contains(" WARNING "))
        // reports on the data's own syntax, not on the character
        .filter(|l| {
            [
                " PreTypeTester:",
                " LoggingProviderImpl:",
                " GameMode:",
                " PrerequisiteUtilities:",
                // the sheet template's own errors
                " ExportHandler:",
            ]
            .iter()
            .all(|c| !l.contains(c))
        })
        .map(|l| l.split_once(' ').map_or(l, |(_, r)| r).to_string())
        .collect();
    let xml = std::fs::read_to_string(&out).map_err(|_| {
        format!(
            "PCGen wrote no sheet:\n{}",
            log.lines().rev().take(15).collect::<Vec<_>>().join("\n")
        )
    })?;
    Ok((
        Element::parse_xml(&xml).map_err(|e| e.to_string())?,
        complaints,
    ))
}

/// Names of the books of spell-like abilities (SPELLS on the race,
/// items and so on) on a sheet.
fn innate_books(sheet: &Element) -> Vec<String> {
    let path = ["spells", "spells_innate", "class_innate"];
    let mut at = Some(sheet);
    for p in path {
        at = at.and_then(|e| e.child(p));
    }
    at.map(|ci| {
        ci.children
            .iter()
            .filter(|c| c.tag == "spellbook")
            .filter_map(|c| book_name(c).map(str::to_string))
            .collect()
    })
    .unwrap_or_default()
}

fn book_name(e: &Element) -> Option<&str> {
    e.attrs
        .iter()
        .find(|(k, _)| k == "name")
        .map(|(_, v)| v.as_str())
}

/// PCGen numbers the books of spell-like abilities in the order of a
/// set hashed on object identity, which changes with unrelated details
/// of the character, so no rule can reproduce it.  On both sheets, put
/// those books in name order (keeping the numbers where they stand).
fn sort_books(e: &mut Element, innate: &[String]) {
    let slots: Vec<usize> = (0..e.children.len())
        .filter(|&i| {
            let c = &e.children[i];
            c.tag == "spellbook" && book_name(c).is_some_and(|n| innate.iter().any(|b| b == n))
        })
        .collect();
    if slots.len() > 1 {
        let numbers: Vec<Option<String>> = slots
            .iter()
            .map(|&i| {
                e.children[i]
                    .attrs
                    .iter()
                    .find(|(k, _)| k == "number")
                    .map(|(_, v)| v.clone())
            })
            .collect();
        let mut books: Vec<Element> = slots.iter().map(|&i| e.children[i].clone()).collect();
        books.sort_by(|a, b| book_name(a).cmp(&book_name(b)));
        for ((&i, mut book), number) in slots.iter().zip(books).zip(numbers) {
            if let Some(n) = number {
                for (k, v) in &mut book.attrs {
                    if k == "number" {
                        *v = n.clone();
                    }
                }
            }
            e.children[i] = book;
        }
    }
    for c in &mut e.children {
        sort_books(c, innate);
    }
}

/// Build, export and compare one scenario: problems found, and
/// (values matched, values compared).
fn run(s: &Scenario, root: &Path, reference: &Path, work: &Path) -> (Vec<String>, usize, usize) {
    let mut problems = Vec::new();
    let dirs = DataDirs::from_install(&root.join("pcgen"));
    let mut session = Session::create(&s.gamemode, &s.campaigns, &s.name, dirs.clone());
    for c in &s.commands {
        if let Err(e) = session.exec(c) {
            problems.push(format!("command `{}`: {}", c, e));
        }
    }
    std::fs::create_dir_all(work).unwrap();
    let pcg = work.join(format!("{}.pcg", s.name));
    if let Err(e) = session.exec(&format!("save \"{}\"", pcg.display())) {
        problems.push(format!("save: {}", e));
        return (problems, 0, 0);
    }
    let data = root.join("pcgen/data");
    let (expected, complaints) = match pcgen_sheet(reference, &data, &pcg, work, BASE_TEMPLATE) {
        Ok(x) => x,
        Err(e) => {
            problems.push(e);
            return (problems, 0, 0);
        }
    };
    // PCGen also complains about its own data; only what it does not
    // say about a bare new character of the same setup is ours
    let bare = work.join("bare");
    std::fs::create_dir_all(&bare).unwrap();
    let bare_pcg = bare.join("bare.pcg");
    let mut b = Session::create(&s.gamemode, &s.campaigns, "bare", dirs.clone());
    let baseline = match b.exec(&format!("save \"{}\"", bare_pcg.display())) {
        Ok(_) => pcgen_sheet(reference, &data, &bare_pcg, &bare, BASE_TEMPLATE)
            .map(|(_, c)| c)
            .unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    problems.extend(
        complaints
            .into_iter()
            .filter(|c| !baseline.contains(c))
            .map(|c| format!("PCGen: {}", c)),
    );
    // our sheet is built from the saved file, as PCGen's is
    let ch = Character::read(&pcg).unwrap();
    let ctx = Context::for_character(&ch, dirs);
    let mut expected = expected;
    let mut ours = sheet::build(&ch, &ctx);
    let innate: Vec<String> = [&expected, &ours]
        .iter()
        .flat_map(|s| innate_books(s))
        .collect();
    sort_books(&mut expected, &innate);
    sort_books(&mut ours, &innate);
    let mut diff = sheet::compare(&expected, &ours);
    if s.full || std::env::var_os("REF_FULL").is_some() {
        let full = work.join("full");
        std::fs::create_dir_all(&full).unwrap();
        match pcgen_sheet(reference, &data, &pcg, &full, FULL_TEMPLATE) {
            Ok((fuller, _)) => {
                let ours_full = sheet::build_full(&ch, &ctx);
                let d = compare_additions(&expected, &fuller, &ours, &ours_full);
                diff.total += d.total;
                diff.mismatches.extend(d.mismatches);
            }
            Err(e) => problems.push(format!("fuller sheet: {}", e)),
        }
    }
    for (p, want, got) in &diff.mismatches {
        if !s.differs.iter().any(|(d, _)| d == p) && !s.varies.contains(p) {
            problems.push(format!("{} want {:?} got {:?}", p, want, got));
        }
    }
    // a known difference that no longer differs should be removed
    for (d, _) in &s.differs {
        if !diff.mismatches.iter().any(|(p, _, _)| p == d) {
            problems.push(format!(
                "{} no longer differs; remove its 'differs' line",
                d
            ));
        }
    }
    (problems, diff.matched(), diff.total)
}

/// The name of the skill row a sheet path is in.
fn skill_row(path: &str) -> Option<&str> {
    path.strip_prefix("character/skills[0]/skill[")?
        .split_once("]/")
        .map(|(n, _)| n)
}

/// The skill rows of a sheet.
fn skill_rows(sheet: &Element) -> HashSet<String> {
    sheet
        .flatten()
        .iter()
        .filter_map(|(p, _)| skill_row(p).map(str::to_string))
        .collect()
}

/// Compare what PCGen's fuller sheet adds to its base sheet with what
/// ours adds: skill rows for situations, and spell concentration.
fn compare_additions(
    base: &Element,
    fuller: &Element,
    ours_base: &Element,
    ours: &Element,
) -> sheet::Diff {
    let base_rows = skill_rows(base);
    // the values of the added skill rows we also show for a skill's
    // own row, and concentration wherever it is
    let added = |p: &str| match skill_row(p) {
        Some(n) => {
            !base_rows.contains(n) && !p.ends_with("/classes[0]") && !p.ends_with("/type[0]")
        }
        None => {
            p.starts_with("character/spells[0]/")
                && (p.ends_with("/concentration[0]") || p.ends_with("/@concentration"))
        }
    };
    let ours = ours.flatten();
    let got: HashMap<&str, &str> = ours.iter().map(|(p, v)| (p.as_str(), v.as_str())).collect();
    let mut d = sheet::Diff::default();
    let mut rows: HashSet<String> = HashSet::new();
    for (p, want) in fuller.flatten().into_iter().filter(|(p, _)| added(p)) {
        if let Some(n) = skill_row(&p) {
            rows.insert(n.to_string());
        }
        d.total += 1;
        let g = got.get(p.as_str()).copied();
        if g != Some(want.as_str()) {
            d.mismatches.push((p, want, g.map(str::to_string)));
        }
    }
    // a situation row of ours that PCGen does not show
    let ours_rows = skill_rows(ours_base);
    let extra: BTreeSet<&str> = ours
        .iter()
        .filter_map(|(p, _)| skill_row(p))
        .filter(|n| !ours_rows.contains(*n) && !rows.contains(*n))
        .collect();
    for n in extra {
        d.total += 1;
        d.mismatches.push((
            format!("character/skills[0]/skill[{}]", n),
            String::new(),
            Some("a row PCGen does not show".into()),
        ));
    }
    d
}

#[test]
fn reference_sheets() {
    let Some(root) = common::pcgen_root() else {
        return;
    };
    let Some(reference) = std::env::var_os("PCGEN_REF")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
    else {
        common::skip("ref", "PCGEN_REF");
        return;
    };
    let text =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scenarios.txt"))
            .unwrap();
    let only = std::env::var("REF_ONLY").ok();
    let verbose = std::env::var("REF_VERBOSE").is_ok();
    let list: Vec<Scenario> = scenarios(&text)
        .into_iter()
        .filter(|s| only.as_deref().is_none_or(|o| o == s.name))
        .collect();
    let base = std::env::temp_dir().join(format!("pcgtools-ref-{}", std::process::id()));
    let results = Mutex::new(Vec::new());
    let next = Mutex::new(0usize);
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| loop {
                let i = {
                    let mut n = next.lock().unwrap();
                    *n += 1;
                    *n - 1
                };
                let Some(s) = list.get(i) else { break };
                let r = run(s, &root, &reference, &base.join(&s.name));
                results.lock().unwrap().push((s.name.clone(), r));
            });
        }
    });
    let _ = std::fs::remove_dir_all(&base);
    let mut results = results.into_inner().unwrap();
    results.sort_by(|a, b| a.0.cmp(&b.0));
    let mut failed = Vec::new();
    for (name, (problems, ok, total)) in &results {
        eprintln!(
            "reference {}: {}/{} values, {} problems",
            name,
            ok,
            total,
            problems.len()
        );
        if verbose {
            for p in problems {
                eprintln!("  {}", p);
            }
        }
        if !problems.is_empty() {
            failed.push(format!("{}: {}", name, problems.first().unwrap()));
        }
    }
    assert!(
        failed.is_empty(),
        "scenarios differ from PCGen:\n{}",
        failed.join("\n")
    );
}

#[test]
fn innate_books_compare_in_name_order() {
    let a = Element::parse_xml(
        "<character><spells><spells_innate><class_innate>\
         <spellbook number=\"2\" name=\"Ring\"><n>r</n></spellbook>\
         <spellbook number=\"3\" name=\"Hand\"><n>h</n></spellbook>\
         </class_innate></spells_innate><memorized_spells>\
         <spellbook number=\"2\" name=\"Ring\"/><spellbook number=\"3\" name=\"Hand\"/>\
         <spellbook number=\"4\" name=\"Tome\"/></memorized_spells></spells></character>",
    )
    .unwrap();
    let b = Element::parse_xml(
        "<character><spells><spells_innate><class_innate>\
         <spellbook number=\"2\" name=\"Hand\"><n>h</n></spellbook>\
         <spellbook number=\"3\" name=\"Ring\"><n>r</n></spellbook>\
         </class_innate></spells_innate><memorized_spells>\
         <spellbook number=\"2\" name=\"Hand\"/><spellbook number=\"3\" name=\"Ring\"/>\
         <spellbook number=\"4\" name=\"Tome\"/></memorized_spells></spells></character>",
    )
    .unwrap();
    assert!(!sheet::compare(&a, &b).mismatches.is_empty());
    let innate = innate_books(&a);
    assert_eq!(innate, ["Ring", "Hand"]);
    let (mut a, mut b) = (a, b);
    sort_books(&mut a, &innate);
    sort_books(&mut b, &innate);
    assert!(sheet::compare(&a, &b).mismatches.is_empty());
    assert!(a.to_xml().contains("number=\"2\" name=\"Hand\""));
    // a book that is not innate keeps its place
    assert!(a.to_xml().contains("number=\"4\" name=\"Tome\""));
}

#[test]
fn scenario_file_parses() {
    let text =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/scenarios.txt"))
            .unwrap();
    let list = scenarios(&text);
    assert!(!list.is_empty());
    for s in &list {
        assert!(!s.gamemode.is_empty(), "{} has no game mode", s.name);
        assert!(!s.campaigns.is_empty(), "{} has no campaign", s.name);
        assert!(
            list.iter().filter(|x| x.name == s.name).count() == 1,
            "{} is named twice",
            s.name
        );
    }
}
