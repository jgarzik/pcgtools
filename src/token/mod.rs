//
// token/mod.rs -- token value syntax
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

pub mod bonus;
pub mod formula;
pub mod pre;
pub mod split;

/// Tags whose value starts with a sub-token name: `ADD:FEAT|...`.
pub const SUBTOKEN_TAGS: [&str; 5] = ["ADD", "AUTO", "CHOOSE", "REMOVE", "TEMPBONUS"];

/// Split a sub-token value into its sub-token name and the rest.
/// CHOOSE values may start with `NUMCHOICES=n|`, which is skipped
/// and returned separately.
pub fn split_subtoken(value: &str) -> (Option<&str>, &str, &str) {
    let (numchoices, v) = match value.strip_prefix("NUMCHOICES=") {
        Some(rest) => match rest.split_once('|') {
            Some((n, r)) => (Some(n), r),
            None => (Some(rest), ""),
        },
        None => (None, value),
    };
    let (sub, rest) = v.split_once('|').unwrap_or((v, ""));
    (numchoices, sub, rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtokens() {
        assert_eq!(split_subtoken("FEAT|Dodge"), (None, "FEAT", "Dodge"));
        assert_eq!(
            split_subtoken("NUMCHOICES=2|SKILL|ALL"),
            (Some("2"), "SKILL", "ALL")
        );
        assert_eq!(split_subtoken("NOCHOICE"), (None, "NOCHOICE", ""));
    }
}
