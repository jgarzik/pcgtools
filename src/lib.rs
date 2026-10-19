//
// lib.rs -- pcgtools library root
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! Library for reading, querying, editing and writing PCGen data
//! files (PCC campaign files, LST data files, game mode system files)
//! and PCG character files, and for computing the derived statistics
//! and full character sheet of a character.
//!
//! Files are read and written losslessly: unmodified bytes survive a
//! round trip.  The formats themselves are described in
//! [FORMAT.md](https://github.com/jgarzik/pcgtools/blob/main/FORMAT.md).
//!
//! # Characters
//!
//! [`context::Context::for_character`] loads the game mode and the
//! campaigns a character uses; [`rules::calc::Calc`] computes its
//! statistics and [`sheet::build`] its full character sheet.
//!
//! ```no_run
//! use pcgtools::context::Context;
//! use pcgtools::pcc::resolve::DataDirs;
//! use pcgtools::pcg::character::Character;
//! use pcgtools::rules::calc::Calc;
//! use std::path::Path;
//!
//! let ch = Character::read(Path::new("hero.pcg"))?;
//! let ctx = Context::for_character(&ch, DataDirs::from_install(Path::new("/opt/pcgen")));
//! let mut calc = Calc::new(&ctx.dataset, ctx.gamemode.as_ref(), &ch);
//! let stats = calc.run();
//! println!("level {} BAB {:+} HP {}", stats.level, stats.bab, stats.hp);
//!
//! let sheet = pcgtools::sheet::build(&ch, &ctx);
//! print!("{}", sheet.to_xml());
//! # Ok::<(), pcgtools::Error>(())
//! ```
//!
//! # Data
//!
//! A [`campaign::Registry`] finds campaign files; a
//! [`campaign::LoadPlan`] orders the files a selection of campaigns
//! loads, and [`dataset::Dataset::load`] merges them into objects.
//!
//! ```no_run
//! use pcgtools::campaign::{LoadPlan, Registry};
//! use pcgtools::dataset::Dataset;
//! use pcgtools::lst::kind::FileKind;
//! use pcgtools::pcc::resolve::DataDirs;
//! use std::path::{Path, PathBuf};
//!
//! let mut reg = Registry::new(DataDirs::from_install(Path::new("/opt/pcgen")));
//! let plan = LoadPlan::build(&mut reg, &[PathBuf::from("/opt/pcgen/data/book/book.pcc")]);
//! let ds = Dataset::load(&plan);
//! if let Some(spell) = ds.get(FileKind::Spell, "Magic Missile") {
//!     println!("{:?}", spell.get("SCHOOL"));
//! }
//! ```
//!
//! # Modules
//!
//! | Module | Purpose |
//! |---|---|
//! | [`text`] | lossless text decoding (encodings, line endings) |
//! | [`lst`] | LST lines, record heads, per-kind grammar, edits, formatting |
//! | [`pcc`] | PCC files, file references, path resolution |
//! | [`campaign`] | campaign registry and load planning |
//! | [`dataset`] | loading a selection into merged objects |
//! | [`token`] | prerequisite, bonus, choice and formula syntax |
//! | [`gamemode`] | game mode files and key migration |
//! | [`catalog`], [`lint`], [`query`] | tag catalog, checks, object filters |
//! | [`pcg`] | PCG files and the typed [`pcg::character::Character`] view |
//! | [`context`], [`rules`] | a character's data; derived statistics, variables, equipment, spells |
//! | [`sheet`] | the full character sheet |
//! | [`repl`] | the character editing session and its command language |

pub mod campaign;
pub mod catalog;
pub mod context;
pub mod dataset;
pub mod error;
pub mod gamemode;
pub mod lint;
pub mod lst;
pub mod pcc;
pub mod pcg;
pub mod query;
pub mod repl;
pub mod rules;
pub mod sheet;
pub mod text;
pub mod token;

pub use error::{Diagnostic, Diagnostics, Error, Result, Severity};
