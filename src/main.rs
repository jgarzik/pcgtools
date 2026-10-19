//
// main.rs -- pcgtools command line interface
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

mod cmd;

use clap::{Args, Parser, Subcommand};
use pcgtools::pcc::resolve::DataDirs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(flatten)]
    opts: GlobalOpts,

    #[command(subcommand)]
    cmd: Command,
}

/// Options shared by all subcommands.
#[derive(Args, Debug, Clone)]
pub struct GlobalOpts {
    /// PCGen installation root (contains data/, vendordata/, homebrewdata/)
    #[arg(long, global = true, env = "PCGEN_INSTALL")]
    pub pcgen_root: Option<PathBuf>,

    /// Data directory (target of '@' paths); guessed from PCC paths if unset
    #[arg(long, global = true)]
    pub data_dir: Option<PathBuf>,

    /// Vendor data directory (target of '&' paths)
    #[arg(long, global = true)]
    pub vendor_dir: Option<PathBuf>,

    /// Homebrew data directory (target of '$' paths)
    #[arg(long, global = true)]
    pub homebrew_dir: Option<PathBuf>,

    /// Emit JSON instead of text
    #[arg(long, global = true)]
    pub json: bool,
}

impl GlobalOpts {
    /// Resolve data directories: explicit options first, then the
    /// installation root, then a guess based on `hint`.
    pub fn dirs(&self, hint: Option<&Path>) -> DataDirs {
        let explicit = DataDirs {
            data: self.data_dir.clone(),
            vendor: self.vendor_dir.clone(),
            homebrew: self.homebrew_dir.clone(),
        };
        let install = self
            .pcgen_root
            .as_deref()
            .map(DataDirs::from_install)
            .unwrap_or_default();
        let guess = hint.map(DataDirs::guess_from).unwrap_or_default();
        explicit.or(install).or(guess)
    }
}

#[derive(Subcommand, Debug)]
enum Command {
    /// List campaigns (PCC files) found below a directory
    Campaigns {
        /// Directory to scan (default: the data directory)
        dir: Option<PathBuf>,

        /// Only campaigns supporting this game mode
        #[arg(short, long)]
        gamemode: Option<String>,
    },

    /// Show the campaigns and LST files a selection of PCCs will load
    Deps {
        /// Selected PCC files
        #[arg(required = true)]
        pcc: Vec<PathBuf>,
    },

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

fn main() -> ExitCode {
    env_logger::builder().format_timestamp(None).init();

    let cli = Cli::parse();
    let o = &cli.opts;
    let res = match &cli.cmd {
        Command::Campaigns { dir, gamemode } => {
            cmd::campaign::campaigns(o, dir.as_deref(), gamemode.as_deref())
        }
        Command::Deps { pcc } => cmd::campaign::deps(o, pcc),
        Command::Dump { pccfile, datadir } => cmd::misc::dump(pccfile, datadir),
        Command::Parse { file } => cmd::misc::parse(file),
        Command::Roundtrip { paths } => cmd::misc::roundtrip(paths),
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
