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

    /// Load a campaign selection and summarize the result
    Load {
        /// Selected PCC files
        #[arg(required = true)]
        pcc: Vec<PathBuf>,

        /// Do not print diagnostics
        #[arg(short, long)]
        quiet: bool,
    },

    /// Load a campaign selection and print the whole dataset as JSON
    Dump {
        /// Selected PCC files
        #[arg(required = true)]
        pcc: Vec<PathBuf>,
    },

    /// Show one object, merged, with provenance
    Show {
        /// Selected PCC files
        #[arg(short, long = "pcc", required = true)]
        pcc: Vec<PathBuf>,

        /// Object kind (SPELL, CLASS, ABILITY, ...)
        kind: String,

        /// Object key (abilities: CATEGORY|Key or Key)
        key: String,
    },

    /// Find objects matching conditions
    Query {
        /// Selected PCC files
        #[arg(short, long = "pcc", required = true)]
        pcc: Vec<PathBuf>,

        /// Object kind (SPELL, CLASS, ABILITY, ...)
        kind: String,

        /// Conditions: TAG, !TAG, TAG=value, TAG~regex, TYPE=type
        conds: Vec<String>,

        /// Glob on object key or name (case-insensitive)
        #[arg(short, long)]
        name: Option<String>,

        /// Comma-separated tags to print for each match
        #[arg(short, long)]
        fields: Option<String>,

        /// Print only the number of matches
        #[arg(short, long)]
        count: bool,
    },

    /// Edit one object in an LST file, preserving all other bytes
    Edit {
        /// LST file
        file: PathBuf,

        /// Object name (record head)
        object: String,

        /// Replace all TAG tokens with TAG:value
        #[arg(long, value_name = "TAG:VALUE")]
        set: Vec<String>,

        /// Append TAG:value
        #[arg(long, value_name = "TAG:VALUE")]
        add: Vec<String>,

        /// Remove all tokens with this tag
        #[arg(long, value_name = "TAG")]
        remove: Vec<String>,

        /// Rename the object (keeps .MOD/.COPY suffixes)
        #[arg(long)]
        rename: Option<String>,

        /// Delete the record
        #[arg(long)]
        delete: bool,

        /// Also match .MOD/.COPY/.FORGET records
        #[arg(long)]
        any_op: bool,

        /// Write the result here instead of stdout
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Modify the file in place
        #[arg(short, long)]
        in_place: bool,
    },

    /// Rewrite files in canonical layout
    Fmt {
        /// LST or PCC files
        #[arg(required = true)]
        files: Vec<PathBuf>,

        /// Report files that are not canonical; exit 1 if any
        #[arg(long)]
        check: bool,

        /// Rewrite files in place
        #[arg(short, long)]
        write: bool,
    },

    /// Check data files for problems
    Lint {
        /// PCC files (or LST files with --kind)
        #[arg(required = true)]
        files: Vec<PathBuf>,

        /// Treat the arguments as LST files of this kind
        #[arg(short, long)]
        kind: Option<String>,

        /// Print counts per problem type instead of each problem
        #[arg(short, long)]
        summary: bool,

        /// Do not report unknown tags
        #[arg(long)]
        no_unknown: bool,

        /// Do not check formulas
        #[arg(long)]
        no_formulas: bool,

        /// Minimum severity to report: info, warning, error
        #[arg(long, default_value = "info")]
        min: String,
    },

    /// List known tags from the built-in catalog
    Tags {
        /// Only this scope (a file kind, GLOBAL, PRE, BONUS, CLASSLEVEL, ...)
        scope: Option<String>,
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
        Command::Load { pcc, quiet } => cmd::data::load(o, pcc, *quiet),
        Command::Dump { pcc } => cmd::data::dump(o, pcc),
        Command::Show { pcc, kind, key } => cmd::data::show(o, pcc, kind, key),
        Command::Lint {
            files,
            kind,
            summary,
            no_unknown,
            no_formulas,
            min,
        } => {
            let min = match min.as_str() {
                "error" => pcgtools::Severity::Error,
                "warning" => pcgtools::Severity::Warning,
                _ => pcgtools::Severity::Info,
            };
            cmd::check::lint(
                o,
                cmd::check::LintArgs {
                    files,
                    kind: kind.as_deref(),
                    summary: *summary,
                    opts: pcgtools::lint::LintOptions {
                        unknown_tags: !no_unknown,
                        deprecated: true,
                        formulas: !no_formulas,
                    },
                    min,
                },
            )
        }
        Command::Query {
            pcc,
            kind,
            conds,
            name,
            fields,
            count,
        } => cmd::data::query(
            o,
            cmd::data::QueryArgs {
                pcc,
                kind,
                name: name.as_deref(),
                conds,
                fields: fields.as_deref(),
                count: *count,
            },
        ),
        Command::Edit {
            file,
            object,
            set,
            add,
            remove,
            rename,
            delete,
            any_op,
            output,
            in_place,
        } => cmd::edit::edit(cmd::edit::EditArgs {
            file,
            object,
            set,
            add,
            remove,
            rename: rename.as_deref(),
            delete: *delete,
            any_op: *any_op,
            output: output.as_deref(),
            in_place: *in_place,
        }),
        Command::Fmt {
            files,
            check,
            write,
        } => cmd::edit::fmt(files, *check, *write),
        Command::Tags { scope } => cmd::check::tags(o, scope.as_deref()),
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
