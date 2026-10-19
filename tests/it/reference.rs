// Characters built by the editor, checked against PCGen itself.
//
// Each scenario in tests/scenarios.txt is a new character and a list
// of editor commands.  The character is built and saved with the
// editor, its sheet is exported by a PCGen build, and every value of
// that sheet must match the sheet pcgtools builds from the saved file.
// PCGen complaints about loading the saved file also fail the test,
// apart from those it also makes about a bare new character.
//
// Runs only when PCGEN_ROOT names the data checkouts and PCGEN_REF a
// built PCGen tree (with build/libs, and JavaFX jars in mods/lib).
// The JVM is $PCGEN_JAVA, else $JAVA_HOME/bin/java, else java.
// REF_ONLY=name runs one scenario; REF_VERBOSE=1 prints differences.

use crate::common;
use pcgtools::context::Context;
use pcgtools::pcc::resolve::DataDirs;
use pcgtools::pcg::character::Character;
use pcgtools::repl::Session;
use pcgtools::sheet::{self, Element};
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
            });
            continue;
        }
        let s = out.last_mut().expect("scenario file starts with '== name'");
        if let Some(g) = line.strip_prefix("gamemode ") {
            s.gamemode = g.trim().to_string();
        } else if let Some(c) = line.strip_prefix("campaign ") {
            s.campaigns.push(c.trim().to_string());
        } else if let Some(d) = line.strip_prefix("differs ") {
            let (path, why) = d.split_once(" -- ").unwrap_or((d, ""));
            s.differs
                .push((path.trim().to_string(), why.trim().to_string()));
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

/// Export a character's sheet with PCGen: the sheet, and the problems
/// PCGen reported after loading the character.
fn pcgen_sheet(
    reference: &Path,
    data: &Path,
    pcg: &Path,
    work: &Path,
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
        .args([
            "--exportsheet",
            "code/testsuite/base-xml.ftl",
            "--outputfile",
        ])
        .arg(&out)
        .arg("--settingsdir")
        .arg(&settings)
        .args(["--configfilename", "config.ini.ref"])
        .output()
        .map_err(|e| format!("running PCGen: {}", e))?;
    let log =
        String::from_utf8_lossy(&run.stdout).into_owned() + &String::from_utf8_lossy(&run.stderr);
    // problems after the character is loaded are about the character
    let complaints = log
        .lines()
        .skip_while(|l| !l.contains("Loaded character"))
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
    let (expected, complaints) = match pcgen_sheet(reference, &root.join("pcgen/data"), &pcg, work)
    {
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
        Ok(_) => pcgen_sheet(reference, &root.join("pcgen/data"), &bare_pcg, &bare)
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
    let diff = sheet::compare(&expected, &sheet::build(&ch, &ctx));
    for (p, want, got) in &diff.mismatches {
        if !s.differs.iter().any(|(d, _)| d == p) {
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

#[test]
fn reference_sheets() {
    let (Some(root), Some(reference)) = (
        common::pcgen_root(),
        std::env::var_os("PCGEN_REF").map(PathBuf::from),
    ) else {
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
