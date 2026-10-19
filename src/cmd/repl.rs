//
// cmd/repl.rs -- interactive and scripted character editing
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use super::{report, CmdResult};
use crate::GlobalOpts;
use pcgtools::repl::{Outcome, Session};
use pcgtools::Error;
use std::io::{BufRead, IsTerminal};
use std::path::{Path, PathBuf};

pub struct NewChar<'a> {
    pub gamemode: &'a str,
    pub campaigns: &'a [String],
    pub name: &'a str,
}

fn open(
    o: &GlobalOpts,
    file: Option<&Path>,
    new: Option<NewChar<'_>>,
) -> pcgtools::Result<Session> {
    let s = match (file, new) {
        (_, Some(n)) => {
            let mut s = Session::create(n.gamemode, n.campaigns, n.name, o.dirs(None));
            s.path = file.map(Path::to_path_buf);
            s
        }
        (Some(f), None) => Session::open(f, o.dirs(Some(f)))?,
        (None, None) => {
            return Err(Error::Other(
                "give a character file, or --new with --gamemode and --campaign".into(),
            ))
        }
    };
    report(&s.ctx.diags);
    Ok(s)
}

/// Run commands until quit or end of input.  Returns false if any
/// command failed when reading from a script.
fn run_lines(s: &mut Session, lines: impl Iterator<Item = String>, echo: bool) -> bool {
    let mut ok = true;
    for line in lines {
        if echo {
            println!("> {}", line);
        }
        match s.exec(&line) {
            Ok(Outcome::Quit) => break,
            Ok(Outcome::Text(t)) | Ok(Outcome::Changed(t)) => {
                if !t.is_empty() {
                    println!("{}", t);
                }
            }
            Err(e) => {
                eprintln!("error: {}", e);
                ok = false;
            }
        }
    }
    ok
}

pub fn repl(o: &GlobalOpts, file: Option<&Path>, new: Option<NewChar<'_>>) -> CmdResult {
    let mut s = open(o, file, new)?;
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        let lines = stdin.lock().lines().map_while(|l| l.ok());
        let ok = run_lines(&mut s, lines, false);
        if s.dirty && s.path.is_some() {
            eprintln!("note: unsaved changes (use 'save')");
        }
        return Ok(ok);
    }

    println!(
        "{}",
        s.exec("info").map(|o| match o {
            Outcome::Text(t) | Outcome::Changed(t) => t,
            Outcome::Quit => String::new(),
        })?
    );
    println!("type 'help' for commands");
    let mut rl = rustyline::DefaultEditor::new()
        .map_err(|e| Error::Other(format!("cannot start line editor: {}", e)))?;
    loop {
        let prompt = format!(
            "{}{}> ",
            s.ch.name().unwrap_or_default(),
            if s.dirty { "*" } else { "" }
        );
        let line = match rl.readline(&prompt) {
            Ok(l) => l,
            Err(rustyline::error::ReadlineError::Interrupted) => continue,
            Err(_) => break,
        };
        let _ = rl.add_history_entry(line.as_str());
        match s.exec(&line) {
            Ok(Outcome::Quit) => {
                if s.dirty {
                    println!("unsaved changes; 'save' first, or 'quit' again to discard");
                    s.dirty = false;
                    continue;
                }
                break;
            }
            Ok(Outcome::Text(t)) | Ok(Outcome::Changed(t)) => println!("{}", t),
            Err(e) => println!("error: {}", e),
        }
    }
    Ok(true)
}

/// Run one command against a character file, saving if it changed.
pub fn one_shot(
    o: &GlobalOpts,
    file: &Path,
    words: &[String],
    output: Option<&PathBuf>,
) -> CmdResult {
    let mut s = Session::open(file, o.dirs(Some(file)))?;
    report(&s.ctx.diags);
    let line = words
        .iter()
        .map(|w| {
            if w.contains(' ') || w.is_empty() {
                format!("\"{}\"", w)
            } else {
                w.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    match s.exec(&line)? {
        Outcome::Changed(t) => {
            let dest = output.cloned().unwrap_or_else(|| file.to_path_buf());
            s.ch.write(&dest)?;
            println!("{}", t);
            eprintln!("saved {}", dest.display());
        }
        Outcome::Text(t) => println!("{}", t),
        Outcome::Quit => {}
    }
    Ok(true)
}
