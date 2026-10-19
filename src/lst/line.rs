//
// lst/line.rs -- lossless LST line model
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! An LST file is a sequence of lines, each a TAB-separated list of
//! cells.  This module keeps every cell exactly as found (including
//! empty cells and stray whitespace) so that a file can be edited at
//! the cell level and written back with all other bytes unchanged.

use crate::error::Result;
use crate::text::{Eol, TextFile, TextLine};
use serde::Serialize;
use std::path::Path;

/// Classification of a physical line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum LineKind {
    /// empty or whitespace only
    Blank,
    /// `#` in column one
    Comment,
    /// file-level source header (`SOURCELONG:...<TAB>SOURCESHORT:...`)
    Source,
    /// begins with a TAB: continues the previous record
    Continuation,
    /// an object definition, modification, or sub-line
    Record,
}

/// One physical line of an LST file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LstLine {
    pub kind: LineKind,
    /// the raw line split on TAB; joining with TAB restores the line
    pub cells: Vec<String>,
    pub eol: Eol,
    pub latin1: bool,
}

/// A `TAG:value` token located within a line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Token<'a> {
    pub cell: usize,
    pub tag: &'a str,
    /// None if the cell holds no `:` at all
    pub value: Option<&'a str>,
}

/// Split a token into tag and value at the first colon.  Surrounding
/// whitespace is ignored, as PCGen does.
pub fn split_token(cell: &str) -> (&str, Option<&str>) {
    let cell = cell.trim();
    match cell.split_once(':') {
        Some((t, v)) => (t, Some(v)),
        None => (cell, None),
    }
}

impl LstLine {
    pub fn classify(text: &str) -> LineKind {
        if text.trim().is_empty() {
            LineKind::Blank
        } else if text.starts_with('#') {
            LineKind::Comment
        } else if text.starts_with('\t') || text.starts_with(' ') {
            LineKind::Continuation
        } else if text.starts_with("SOURCE") {
            LineKind::Source
        } else {
            LineKind::Record
        }
    }

    pub fn from_text_line(tl: &TextLine) -> LstLine {
        LstLine {
            kind: LstLine::classify(&tl.text),
            cells: tl.text.split('\t').map(str::to_string).collect(),
            eol: tl.eol,
            latin1: tl.latin1,
        }
    }

    /// Build a new record line from cells.
    pub fn record(cells: Vec<String>) -> LstLine {
        let mut l = LstLine {
            kind: LineKind::Record,
            cells,
            eol: Eol::Lf,
            latin1: false,
        };
        l.reclassify();
        l
    }

    pub fn text(&self) -> String {
        self.cells.join("\t")
    }

    pub fn reclassify(&mut self) {
        self.kind = LstLine::classify(&self.text());
    }

    /// Index of the first non-blank cell (the head of a record).
    pub fn head_cell(&self) -> Option<usize> {
        self.cells.iter().position(|c| !c.trim().is_empty())
    }

    /// The head (first non-blank cell), trimmed.
    pub fn head(&self) -> Option<&str> {
        self.head_cell().map(|i| self.cells[i].trim())
    }

    /// Tokens of this line.  For records, the head cell is skipped;
    /// for sources and continuations, every non-blank cell is a token.
    pub fn tokens(&self) -> Vec<Token<'_>> {
        let skip = match self.kind {
            LineKind::Record => self.head_cell(),
            LineKind::Source | LineKind::Continuation => None,
            LineKind::Blank | LineKind::Comment => return Vec::new(),
        };
        self.cells
            .iter()
            .enumerate()
            .filter(|(i, c)| Some(*i) != skip && !c.trim().is_empty())
            .map(|(i, c)| {
                let (tag, value) = split_token(c);
                Token {
                    cell: i,
                    tag,
                    value,
                }
            })
            .collect()
    }

    pub fn is_data(&self) -> bool {
        !matches!(self.kind, LineKind::Blank | LineKind::Comment)
    }
}

/// A whole LST file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LstFile {
    pub bom: bool,
    pub lines: Vec<LstLine>,
}

impl LstFile {
    pub fn from_text(tf: &TextFile) -> LstFile {
        LstFile {
            bom: tf.bom,
            lines: tf.lines.iter().map(LstLine::from_text_line).collect(),
        }
    }

    pub fn parse(s: &str) -> LstFile {
        LstFile::from_text(&TextFile::from_text(s))
    }

    pub fn decode(bytes: &[u8]) -> LstFile {
        LstFile::from_text(&TextFile::decode(bytes))
    }

    pub fn read(path: &Path) -> Result<LstFile> {
        Ok(LstFile::from_text(&TextFile::read(path)?))
    }

    pub fn to_text(&self) -> TextFile {
        TextFile {
            bom: self.bom,
            lines: self
                .lines
                .iter()
                .map(|l| TextLine {
                    text: l.text(),
                    eol: l.eol,
                    latin1: l.latin1,
                })
                .collect(),
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        self.to_text().encode()
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        self.to_text().write(path)
    }

    /// Append a line, fixing up the terminator of the previous last line.
    pub fn push_line(&mut self, mut line: LstLine) {
        let eol = self.to_text().dominant_eol();
        if let Some(last) = self.lines.last_mut() {
            if last.eol == Eol::None {
                last.eol = eol;
            }
        }
        line.eol = eol;
        self.lines.push(line);
    }

    /// Group physical lines into logical records: each record line
    /// absorbs any continuation lines that follow it.
    pub fn records(&self) -> Vec<Record<'_>> {
        let mut out: Vec<Record<'_>> = Vec::new();
        for (i, line) in self.lines.iter().enumerate() {
            match line.kind {
                LineKind::Record | LineKind::Source => out.push(Record {
                    lines: vec![i],
                    file: self,
                }),
                LineKind::Continuation => match out.last_mut() {
                    Some(r) if self.lines[r.lines[0]].kind == LineKind::Record => r.lines.push(i),
                    _ => out.push(Record {
                        lines: vec![i],
                        file: self,
                    }),
                },
                LineKind::Blank | LineKind::Comment => {}
            }
        }
        out
    }
}

/// A logical record: one record line plus its continuation lines.
#[derive(Debug, Clone)]
pub struct Record<'a> {
    /// indices into `LstFile::lines`; the first is the record line
    pub lines: Vec<usize>,
    file: &'a LstFile,
}

/// A token with its physical location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocatedToken<'a> {
    pub line: usize,
    pub cell: usize,
    pub tag: &'a str,
    pub value: Option<&'a str>,
}

impl<'a> Record<'a> {
    pub fn first(&self) -> &'a LstLine {
        &self.file.lines[self.lines[0]]
    }

    pub fn kind(&self) -> LineKind {
        self.first().kind
    }

    /// The head cell of the record line (None for sources and
    /// orphan continuations).
    pub fn head(&self) -> Option<&'a str> {
        match self.kind() {
            LineKind::Record => self.first().head(),
            _ => None,
        }
    }

    /// 1-based line number of the record line.
    pub fn line_no(&self) -> usize {
        self.lines[0] + 1
    }

    pub fn tokens(&self) -> Vec<LocatedToken<'a>> {
        let mut out = Vec::new();
        for &li in &self.lines {
            for t in self.file.lines[li].tokens() {
                out.push(LocatedToken {
                    line: li,
                    cell: t.cell,
                    tag: t.tag,
                    value: t.value,
                });
            }
        }
        out
    }
}

/// What a record head asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum HeadOp {
    /// define a new object
    Define,
    /// `Name.MOD`: modify an existing object
    Mod,
    /// `Name.COPY=NewName`: copy an existing object under a new name
    Copy(String),
    /// `Name.FORGET`: remove an existing object
    Forget,
}

/// Parsed form of a record head cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Head {
    /// leading `TAG:` for sub-line style heads (`CLASS:Fighter`)
    pub prefix: Option<String>,
    /// `CATEGORY=cat|` qualifier, used by abilities
    pub category: Option<String>,
    pub name: String,
    pub op: HeadOp,
}

const COPY: &str = ".COPY";
const MOD: &str = ".MOD";
const FORGET: &str = ".FORGET";

/// True if `s` looks like an all-caps tag name.
pub fn is_tag_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && s.starts_with(|c: char| c.is_ascii_uppercase())
}

impl Head {
    pub fn parse(cell: &str) -> Head {
        let cell = cell.trim();

        // operation suffix; PCGen matches these anywhere after
        // position zero, in the order COPY, MOD, FORGET
        let (body, op) = if let Some(i) = cell.find(COPY).filter(|&i| i > 0) {
            let rest = &cell[i + COPY.len()..];
            let newname = rest.strip_prefix('=').unwrap_or(rest);
            (&cell[..i], HeadOp::Copy(newname.to_string()))
        } else if let Some(i) = cell.find(MOD).filter(|&i| i > 0) {
            (&cell[..i], HeadOp::Mod)
        } else if let Some(i) = cell.find(FORGET).filter(|&i| i > 0) {
            (&cell[..i], HeadOp::Forget)
        } else {
            (cell, HeadOp::Define)
        };

        let (prefix, body) = match body.split_once(':') {
            Some((t, rest)) if is_tag_name(t) => (Some(t.to_string()), rest),
            _ => (None, body),
        };

        let (category, name) = match body.strip_prefix("CATEGORY=") {
            Some(rest) => match rest.split_once('|') {
                Some((c, n)) => (Some(c.to_string()), n),
                None => (None, body),
            },
            None => (None, body),
        };

        Head {
            prefix,
            category,
            name: name.to_string(),
            op,
        }
    }

    /// Reconstruct the canonical head text.
    pub fn unparse(&self) -> String {
        let mut s = String::new();
        if let Some(p) = &self.prefix {
            s.push_str(p);
            s.push(':');
        }
        if let Some(c) = &self.category {
            s.push_str("CATEGORY=");
            s.push_str(c);
            s.push('|');
        }
        s.push_str(&self.name);
        match &self.op {
            HeadOp::Define => {}
            HeadOp::Mod => s.push_str(MOD),
            HeadOp::Copy(n) => {
                s.push_str(COPY);
                s.push('=');
                s.push_str(n);
            }
            HeadOp::Forget => s.push_str(FORGET),
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# comment\n\
SOURCELONG:Book of Tests\tSOURCESHORT:BoT\tSOURCEWEB:http://example.com\n\
\n\
Widget\tTYPE:Gadget.Small\t\tCOST:5\tDESC:A small: widget\n\
\tWT:1\n\
Widget.MOD\tCOST:6\n\
Widget.COPY=Gizmo\tCOST:7  \n\
CATEGORY=FEAT|Toughness.MOD\tDESC:x\n\
Gizmo.FORGET\n";

    #[test]
    fn roundtrip() {
        let f = LstFile::parse(SAMPLE);
        assert_eq!(f.encode(), SAMPLE.as_bytes());
    }

    #[test]
    fn kinds() {
        let f = LstFile::parse(SAMPLE);
        let k: Vec<LineKind> = f.lines.iter().map(|l| l.kind).collect();
        use LineKind::*;
        assert_eq!(
            k,
            vec![
                Comment,
                Source,
                Blank,
                Record,
                Continuation,
                Record,
                Record,
                Record,
                Record
            ]
        );
    }

    #[test]
    fn tokens_and_records() {
        let f = LstFile::parse(SAMPLE);
        let recs = f.records();
        assert_eq!(recs.len(), 6);
        let w = &recs[1];
        assert_eq!(w.head(), Some("Widget"));
        let toks = w.tokens();
        let tags: Vec<&str> = toks.iter().map(|t| t.tag).collect();
        assert_eq!(tags, vec!["TYPE", "COST", "DESC", "WT"]);
        assert_eq!(toks[2].value, Some("A small: widget"));
        assert_eq!(toks[1].cell, 3);
    }

    #[test]
    fn heads() {
        let h = Head::parse("Widget.COPY=Gizmo");
        assert_eq!(h.name, "Widget");
        assert_eq!(h.op, HeadOp::Copy("Gizmo".into()));

        let h = Head::parse("CATEGORY=FEAT|Power Attack.MOD");
        assert_eq!(h.category.as_deref(), Some("FEAT"));
        assert_eq!(h.name, "Power Attack");
        assert_eq!(h.op, HeadOp::Mod);

        let h = Head::parse("CLASS:Fighter.MOD");
        assert_eq!(h.prefix.as_deref(), Some("CLASS"));
        assert_eq!(h.name, "Fighter");

        let h = Head::parse("Thing.FORGET");
        assert_eq!(h.op, HeadOp::Forget);

        let h = Head::parse("Plain Name");
        assert_eq!(h.op, HeadOp::Define);
        assert_eq!(h.prefix, None);

        for s in [
            "Widget.COPY=Gizmo",
            "CATEGORY=FEAT|Power Attack.MOD",
            "CLASS:Fighter.MOD",
            "X.FORGET",
            "Y",
        ] {
            assert_eq!(Head::parse(s).unparse(), s);
        }
    }

    #[test]
    fn push_line_fixes_eol() {
        let mut f = LstFile::parse("A\tB:1\r\nC\tD:2");
        f.push_line(LstLine::record(vec!["E".into(), "F:3".into()]));
        assert_eq!(f.encode(), b"A\tB:1\r\nC\tD:2\r\nE\tF:3\r\n");
    }

    #[test]
    fn classify_table() {
        use LineKind::*;
        let cases: &[(&str, LineKind)] = &[
            ("", Blank),
            ("   ", Blank),
            ("\t\t", Blank),
            ("#", Comment),
            ("# x\ty", Comment),
            (" # indented", Continuation),
            ("\tTAG:x", Continuation),
            ("SOURCELONG:x", Source),
            ("SOURCEPAGE:p.1", Source),
            ("Name", Record),
            ("Name\tTAG:x", Record),
            ("CLASS:Mage", Record),
            ("1\tTAG:x", Record),
        ];
        for (s, k) in cases {
            assert_eq!(LstLine::classify(s), *k, "{:?}", s);
        }
    }

    /// (cell, prefix, category, name, op)
    type HeadCase<'a> = (&'a str, Option<&'a str>, Option<&'a str>, &'a str, HeadOp);

    #[test]
    fn head_table() {
        let cases: &[HeadCase] = &[
            ("Name", None, None, "Name", HeadOp::Define),
            (" Name ", None, None, "Name", HeadOp::Define),
            ("Name (Long)", None, None, "Name (Long)", HeadOp::Define),
            ("Name.MOD", None, None, "Name", HeadOp::Mod),
            ("Name.FORGET", None, None, "Name", HeadOp::Forget),
            (
                "Name.COPY=Other",
                None,
                None,
                "Name",
                HeadOp::Copy("Other".into()),
            ),
            (
                "Name.COPY=Other (Two)",
                None,
                None,
                "Name",
                HeadOp::Copy("Other (Two)".into()),
            ),
            ("CLASS:Mage", Some("CLASS"), None, "Mage", HeadOp::Define),
            ("CLASS:Mage.MOD", Some("CLASS"), None, "Mage", HeadOp::Mod),
            (
                "CATEGORY=FEAT|Dodge",
                None,
                Some("FEAT"),
                "Dodge",
                HeadOp::Define,
            ),
            (
                "CATEGORY=Special Ability|Evasion.FORGET",
                None,
                Some("Special Ability"),
                "Evasion",
                HeadOp::Forget,
            ),
            (
                "CATEGORY=FEAT|A.COPY=B",
                None,
                Some("FEAT"),
                "A",
                HeadOp::Copy("B".into()),
            ),
            ("Ratio 2:1", None, None, "Ratio 2:1", HeadOp::Define),
            ("lower:case", None, None, "lower:case", HeadOp::Define),
            (".MOD", None, None, ".MOD", HeadOp::Define),
            ("CATEGORY=FEAT", None, None, "CATEGORY=FEAT", HeadOp::Define),
        ];
        for (cell, prefix, cat, name, op) in cases {
            let h = Head::parse(cell);
            assert_eq!(h.prefix.as_deref(), *prefix, "{:?}", cell);
            assert_eq!(h.category.as_deref(), *cat, "{:?}", cell);
            assert_eq!(h.name, *name, "{:?}", cell);
            assert_eq!(&h.op, op, "{:?}", cell);
        }
    }

    #[test]
    fn split_token_table() {
        let cases: &[(&str, &str, Option<&str>)] = &[
            ("TAG:v", "TAG", Some("v")),
            (" TAG:v ", "TAG", Some("v")),
            ("TAG:", "TAG", Some("")),
            ("TAG:a:b", "TAG", Some("a:b")),
            ("TAG", "TAG", None),
            (":v", "", Some("v")),
            ("!PREX:1", "!PREX", Some("1")),
        ];
        for (cell, tag, val) in cases {
            assert_eq!(split_token(cell), (*tag, *val), "{:?}", cell);
        }
    }

    #[test]
    fn tag_names() {
        for t in ["A", "TAG", "PRE_X", "A1"] {
            assert!(is_tag_name(t), "{}", t);
        }
        for t in ["", "a", "Tag", "1A", "_A", "A-B", "A B"] {
            assert!(!is_tag_name(t), "{}", t);
        }
    }

    #[test]
    fn records_and_orphans() {
        let f = LstFile::parse("\tORPHAN:1\nA\tX:1\n\tY:2\n# c\n\tZ:3\nSOURCELONG:S\n\tW:4\n");
        let recs = f.records();
        assert_eq!(recs.len(), 4);
        assert_eq!(recs[0].kind(), LineKind::Continuation);
        assert_eq!(recs[1].lines, vec![1, 2, 4]);
        assert_eq!(recs[2].kind(), LineKind::Source);
        // a continuation after a source line is an orphan
        assert_eq!(recs[3].kind(), LineKind::Continuation);
        assert_eq!(recs[1].line_no(), 2);
        let tags: Vec<&str> = recs[1].tokens().iter().map(|t| t.tag).collect();
        assert_eq!(tags, vec!["X", "Y", "Z"]);
    }

    #[test]
    fn roundtrip_odd_inputs() {
        for s in [
            "",
            "\n",
            "A",
            "A\t",
            "\tA",
            "A\t\t\t",
            "A\r\nB\rC\nD",
            "A\tB:1\t \tC:2 \n",
            "\u{feff}A\n",
        ] {
            let f = LstFile::parse(s);
            assert_eq!(f.encode(), s.as_bytes(), "{:?}", s);
        }
    }
}
