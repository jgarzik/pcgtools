//
// sheet/basics.rs -- the basics section: identity, biography, classes
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use super::{signed, Element, SheetCtx};

pub fn build(s: &SheetCtx) -> Element {
    let mut b = Element::new("basics");
    b.add("name", s.ch.name().unwrap_or_default());
    b.add("bab", signed(s.sheet.bab));
    b
}
