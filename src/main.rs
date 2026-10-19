//
// main.rs -- pcgtools command line interface
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use clap::{Parser, Subcommand};
use pcgtools::text::TextFile;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use walkdir::WalkDir;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Dump a PCC file and its LST data as JSON (prototype reader)
    Dump {
        /// Pathname of PCC file, relative to the data directory
        pccfile: String,

        /// Base directory where PCC and LST files are found
        #[arg(short, long, default_value = ".")]
        datadir: String,
    },

    /// Parse a data file and print its syntax tree as JSON
    Parse {
        /// File to parse
        file: PathBuf,
    },

    /// Verify that files survive a parse/write cycle byte-for-byte
    Roundtrip {
        /// Files or directories to check
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
}

fn is_data_file(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|e| e.to_str()),
        Some("pcc" | "lst" | "pcg" | "PCC" | "LST")
    )
}

fn data_files(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            for ent in WalkDir::new(p).sort_by_file_name().into_iter().flatten() {
                if ent.file_type().is_file() && is_data_file(ent.path()) {
                    out.push(ent.into_path());
                }
            }
        } else {
            out.push(p.clone());
        }
    }
    out
}

fn cmd_roundtrip(paths: &[PathBuf]) -> pcgtools::Result<bool> {
    let mut ok = true;
    let files = data_files(paths);
    for f in &files {
        let bytes = std::fs::read(f).map_err(|e| pcgtools::Error::io(f, e))?;
        if TextFile::decode(&bytes).encode() != bytes {
            println!("MISMATCH {}", f.display());
            ok = false;
        }
    }
    println!("{} files checked", files.len());
    Ok(ok)
}

fn cmd_parse(file: &Path) -> pcgtools::Result<bool> {
    use pcgtools::lst::{Head, LstFile};
    use serde_json::json;

    let lf = LstFile::read(file)?;
    let recs: Vec<serde_json::Value> = lf
        .records()
        .iter()
        .map(|r| {
            let toks: Vec<serde_json::Value> = r
                .tokens()
                .iter()
                .map(|t| json!({"tag": t.tag, "value": t.value}))
                .collect();
            json!({
                "line": r.line_no(),
                "kind": r.kind(),
                "head": r.head().map(Head::parse),
                "tokens": toks,
            })
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&recs).unwrap());
    Ok(true)
}

fn cmd_dump(pccfile: &str, datadir: &str) -> pcgtools::Result<bool> {
    let mut datadir = datadir.to_string();
    if !datadir.ends_with('/') {
        datadir.push('/');
    }
    let cfg = pcgtools::legacy::PccConfig { datadir };
    let mut pcc = pcgtools::legacy::Pcc::new(&cfg);
    pcc.read(pccfile, true)
        .map_err(|e| pcgtools::Error::io(Path::new(pccfile), e))?;
    pcc.display();
    Ok(true)
}

fn main() -> ExitCode {
    env_logger::builder().format_timestamp(None).init();

    let cli = Cli::parse();
    let res = match &cli.cmd {
        Command::Dump { pccfile, datadir } => cmd_dump(pccfile, datadir),
        Command::Parse { file } => cmd_parse(file),
        Command::Roundtrip { paths } => cmd_roundtrip(paths),
    };

    match res {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("pcgtools: {}", e);
            ExitCode::from(2)
        }
    }
}
