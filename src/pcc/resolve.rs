//
// pcc/resolve.rs -- map PCC path specs to filesystem paths
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use super::fileref::{PathBase, PathSpec};
use serde::Serialize;
use std::path::{Component, Path, PathBuf};

/// The directories against which PCC paths are resolved.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DataDirs {
    pub data: Option<PathBuf>,
    pub vendor: Option<PathBuf>,
    pub homebrew: Option<PathBuf>,
    /// the `system` directory (holds `gameModes/`)
    pub system: Option<PathBuf>,
}

impl DataDirs {
    /// Directories of a PCGen installation rooted at `root`
    /// (`root/data`, `root/vendordata`, `root/homebrewdata`).
    pub fn from_install(root: &Path) -> DataDirs {
        let opt = |n: &str| {
            let p = root.join(n);
            p.is_dir().then_some(p)
        };
        DataDirs {
            data: opt("data"),
            vendor: opt("vendordata"),
            homebrew: opt("homebrewdata"),
            system: opt("system"),
        }
    }

    /// Guess the data directory from a PCC file location: the nearest
    /// ancestor directory named `data`.  Siblings `vendordata` and
    /// `homebrewdata` are picked up if present.
    pub fn guess_from(file: &Path) -> DataDirs {
        let abs = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
        for anc in abs.ancestors().skip(1) {
            if anc.file_name().is_some_and(|n| n == "data") {
                if let Some(root) = anc.parent() {
                    return DataDirs::from_install(root);
                }
            }
        }
        DataDirs::default()
    }

    /// Fill in any directory not already set from `other`.
    pub fn or(mut self, other: DataDirs) -> DataDirs {
        self.data = self.data.or(other.data);
        self.vendor = self.vendor.or(other.vendor);
        self.homebrew = self.homebrew.or(other.homebrew);
        self.system = self.system.or(other.system);
        self
    }

    /// The `system/gameModes` directory, if known.
    pub fn gamemodes(&self) -> Option<PathBuf> {
        self.system
            .as_ref()
            .map(|s| s.join("gameModes"))
            .filter(|p| p.is_dir())
    }

    /// Resolve a path spec written in the PCC file `pcc`.  Returns None
    /// for URIs and for bases whose directory is unknown.  The result
    /// may not exist; see [`find_existing`].
    pub fn resolve(&self, spec: &PathSpec, pcc: &Path) -> Option<PathBuf> {
        let join = |d: &Option<PathBuf>| d.as_ref().map(|d| normalize(&d.join(&spec.rel)));
        match spec.base {
            PathBase::Data | PathBase::LegacyData => join(&self.data),
            PathBase::Vendor => join(&self.vendor),
            PathBase::Homebrew => join(&self.homebrew),
            PathBase::Any => [&self.homebrew, &self.vendor]
                .into_iter()
                .filter_map(join)
                .find(|p| find_existing(p).is_some())
                .or_else(|| join(&self.data)),
            PathBase::Uri => spec
                .rel
                .strip_prefix("file:")
                .map(|p| PathBuf::from(p.trim_start_matches("//"))),
            PathBase::Local => {
                let dir = pcc.parent().unwrap_or(Path::new("."));
                Some(normalize(&dir.join(&spec.rel)))
            }
        }
    }
}

/// Lexically normalize `.` and `..` components.
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Return `p` if it exists; otherwise try a case-insensitive match of
/// each path component (data written on case-insensitive filesystems
/// often gets the case of file names wrong).
pub fn find_existing(p: &Path) -> Option<PathBuf> {
    if p.exists() {
        return Some(p.to_path_buf());
    }
    let mut cur = PathBuf::new();
    for c in p.components() {
        let next = cur.join(c.as_os_str());
        if next.exists() {
            cur = next;
            continue;
        }
        let want = c.as_os_str().to_string_lossy().to_lowercase();
        let dir = if cur.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            cur.clone()
        };
        let found = std::fs::read_dir(&dir)
            .ok()?
            .flatten()
            .find(|e| e.file_name().to_string_lossy().to_lowercase() == want)?;
        cur = cur.join(found.file_name());
    }
    Some(cur)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm() {
        assert_eq!(
            normalize(Path::new("/a/b/../c/./d.lst")),
            PathBuf::from("/a/c/d.lst")
        );
    }

    #[test]
    fn resolve_local_and_data() {
        let dirs = DataDirs {
            data: Some(PathBuf::from("/pcgen/data")),
            ..Default::default()
        };
        let pcc = Path::new("/pcgen/data/sys/book/book.pcc");
        let r = dirs.resolve(&PathSpec::parse("../common/x.lst"), pcc);
        assert_eq!(r, Some(PathBuf::from("/pcgen/data/sys/common/x.lst")));
        let r = dirs.resolve(&PathSpec::parse("@/other/y.lst"), pcc);
        assert_eq!(r, Some(PathBuf::from("/pcgen/data/other/y.lst")));
        let r = dirs.resolve(&PathSpec::parse("*/other/y.lst"), pcc);
        assert_eq!(r, Some(PathBuf::from("/pcgen/data/other/y.lst")));
        assert_eq!(dirs.resolve(&PathSpec::parse("&/v.lst"), pcc), None);
    }
}
