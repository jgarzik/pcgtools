//
// error.rs -- error and diagnostic types
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use serde::Serialize;
use std::fmt;
use std::path::{Path, PathBuf};

/// Fatal errors returned by library operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("{0}")]
    Parse(Diagnostic),

    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn io(path: &Path, source: std::io::Error) -> Error {
        Error::Io {
            path: path.to_path_buf(),
            source,
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        f.write_str(s)
    }
}

/// A non-fatal problem found while reading or checking data.
#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub file: Option<PathBuf>,
    /// 1-based line number, 0 if unknown
    pub line: usize,
    pub message: String,
}

impl Diagnostic {
    pub fn new(severity: Severity, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            severity,
            file: None,
            line: 0,
            message: message.into(),
        }
    }

    pub fn error(message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(Severity::Error, message)
    }

    pub fn warning(message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(Severity::Warning, message)
    }

    pub fn info(message: impl Into<String>) -> Diagnostic {
        Diagnostic::new(Severity::Info, message)
    }

    pub fn at(mut self, file: &Path, line: usize) -> Diagnostic {
        self.file = Some(file.to_path_buf());
        self.line = line;
        self
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(file) = &self.file {
            write!(f, "{}:", file.display())?;
            if self.line > 0 {
                write!(f, "{}:", self.line)?;
            }
            write!(f, " ")?;
        }
        write!(f, "{}: {}", self.severity, self.message)
    }
}

/// Collector for diagnostics produced during a multi-file operation.
#[derive(Debug, Default, Clone, Serialize)]
pub struct Diagnostics {
    pub items: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn push(&mut self, d: Diagnostic) {
        log::debug!("{}", d);
        self.items.push(d);
    }

    pub fn extend(&mut self, other: Diagnostics) {
        self.items.extend(other.items);
    }

    pub fn count(&self, sev: Severity) -> usize {
        self.items.iter().filter(|d| d.severity == sev).count()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}
