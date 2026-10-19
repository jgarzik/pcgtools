//
// lib.rs -- pcgtools library root
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! Library for reading, querying, editing and writing PCGen data
//! files (PCC campaign files, LST data files, game mode system files)
//! and PCG character files.
//!
//! See `docs/FORMAT.md` for a description of the file formats.

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
