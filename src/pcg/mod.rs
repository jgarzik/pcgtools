//
// pcg/mod.rs -- PCG character files
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! A PCG file stores one character.  Each data line is a list of
//! `TAG:value` elements separated by `|`; a value in `[...]` is itself
//! a nested element list.  Text values escape structural characters
//! as named entities (`&pipe;`, `&colon;`, ...).
//!
//! ```text
//! line  = elem { "|" elem } [ "|" ] ;
//! elem  = tag ":" ( text | "[" [ line ] "]" ) ;
//! ```
//!
//! Lines are kept losslessly: an unmodified line is written back
//! exactly as read.

pub mod character;

use crate::error::{Diagnostic, Diagnostics, Result};
use crate::text::{Eol, TextFile, TextLine};
use serde::Serialize;
use std::fmt;
use std::path::Path;

/// Characters replaced by entities in text values.
const ENTITIES: [(char, &str); 8] = [
    ('&', "&amp;"),
    ('\n', "&nl;"),
    ('\r', "&cr;"),
    ('\u{c}', "&lf;"),
    (':', "&colon;"),
    ('|', "&pipe;"),
    ('[', "&lbracket;"),
    (']', "&rbracket;"),
];

/// Escape text for use as a PCG value.  With `light`, colons and
/// brackets are left alone (used for free-text fields stored whole).
pub fn encode(s: &str, light: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match ENTITIES.iter().find(|(ch, _)| *ch == c) {
            Some((ch, _)) if light && matches!(ch, ':' | '[' | ']') => out.push(c),
            Some((_, ent)) => out.push_str(ent),
            None => out.push(c),
        }
    }
    out
}

/// Undo [`encode`].  Unknown `&...;` sequences are kept as written.
pub fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        match ENTITIES.iter().find(|(_, ent)| tail.starts_with(ent)) {
            Some((c, ent)) => {
                out.push(*c);
                rest = &tail[ent.len()..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// An element value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum Value {
    /// encoded text, exactly as in the file
    Text(String),
    List(Vec<Elem>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Elem {
    pub tag: String,
    pub value: Value,
}

impl Elem {
    /// A text element; `text` is encoded here.
    pub fn text(tag: &str, text: &str) -> Elem {
        Elem {
            tag: tag.to_string(),
            value: Value::Text(encode(text, false)),
        }
    }

    /// A text element whose value is already in file form.
    pub fn raw(tag: &str, raw: &str) -> Elem {
        Elem {
            tag: tag.to_string(),
            value: Value::Text(raw.to_string()),
        }
    }

    pub fn list(tag: &str, elems: Vec<Elem>) -> Elem {
        Elem {
            tag: tag.to_string(),
            value: Value::List(elems),
        }
    }

    /// Decoded text value (empty for lists).
    pub fn value_text(&self) -> String {
        match &self.value {
            Value::Text(t) => decode(t),
            Value::List(_) => String::new(),
        }
    }

    pub fn children(&self) -> &[Elem] {
        match &self.value {
            Value::List(l) => l,
            Value::Text(_) => &[],
        }
    }
}

impl fmt::Display for Elem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:", self.tag)?;
        match &self.value {
            Value::Text(t) => f.write_str(t),
            Value::List(l) => write!(f, "[{}]", join(l)),
        }
    }
}

/// Serialize an element list.
pub fn join(elems: &[Elem]) -> String {
    elems
        .iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("|")
}

/// Split a line into elements.
pub fn tokenize(line: &str) -> std::result::Result<Vec<Elem>, String> {
    let mut depth: i32 = 0;
    for c in line.chars() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth < 0 {
                    return Err("missing '['".into());
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err("missing ']'".into());
    }
    tokenize_inner(line)
}

fn tokenize_inner(s: &str) -> std::result::Result<Vec<Elem>, String> {
    // Elements end at a top-level '|', and also right after a nested
    // list closes ("A:[x]B:[y]" is accepted, as PCGen does).
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    // is the outermost open bracket a list value (directly after ':')?
    let mut list_open = false;
    let mut prev = '\0';
    for (i, c) in s.char_indices() {
        match c {
            '[' => {
                if depth == 0 {
                    list_open = prev == ':';
                }
                depth += 1;
            }
            ']' => {
                depth -= 1;
                if depth == 0 && list_open {
                    parts.push(&s[start..=i]);
                    start = i + 1;
                }
            }
            '|' if depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        prev = c;
    }
    parts.push(&s[start..]);

    let mut out = Vec::new();
    for p in parts {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        let (tag, val) = p
            .split_once(':')
            .ok_or_else(|| format!("element has no ':': {}", p))?;
        let value = match val.strip_prefix('[') {
            Some(inner) if val.ends_with(']') => {
                Value::List(tokenize_inner(&inner[..inner.len() - 1])?)
            }
            // brackets inside text (e.g. a dice expression) are kept as text
            _ => Value::Text(val.to_string()),
        };
        out.push(Elem {
            tag: tag.to_string(),
            value,
        });
    }
    Ok(out)
}

/// Tags whose whole line after `TAG:` is a single text value; these
/// are not split into elements.
pub const WHOLE_LINE_TAGS: [&str; 47] = [
    "AGE",
    "ALIGNMENT",
    "BIRTHDAY",
    "BIRTHPLACE",
    "CATCHPHRASE",
    "CHARACTERASSET",
    "CHARACTERBIO",
    "CHARACTERCOMP",
    "CHARACTERDESC",
    "CHARACTERDMNOTES",
    "CHARACTERMAGIC",
    "CHARACTERNAME",
    "CHARACTERTYPE",
    "CITY",
    "EXPERIENCE",
    "EXPERIENCETABLE",
    "EYECOLOR",
    "FAVOREDCLASS",
    "FEATPOOL",
    "GAMEMODE",
    "GENDER",
    "HAIRCOLOR",
    "HAIRSTYLE",
    "HANDED",
    "HEIGHT",
    "HTMLOUTPUTSHEET",
    "INTERESTS",
    "LOCATION",
    "MONEY",
    "PDFOUTPUTSHEET",
    "PERSONALITYTRAIT1",
    "PERSONALITYTRAIT2",
    "PHOBIAS",
    "PLAYERNAME",
    "POOLPOINTS",
    "POOLPOINTSAVAIL",
    "PORTRAIT",
    "PORTRAIT_THUMBNAIL_RECT",
    "PREVIEWSHEET",
    "PREVIEWSHEETVAR",
    "RESIDENCE",
    "SKILLFILTER",
    "SKILLSOUTPUTORDER",
    "SKINCOLOR",
    "SPEECHPATTERN",
    "SPELLLIST",
    "TABNAME",
];

/// One physical line of a PCG file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PcgLine {
    /// exact text as read (or as last written)
    pub raw: String,
    #[serde(skip)]
    pub eol: Eol,
    #[serde(skip)]
    pub latin1: bool,
    /// parsed elements; None for blank and comment lines, and for
    /// lines that failed to parse
    pub elems: Option<Vec<Elem>>,
}

impl PcgLine {
    fn from_text(tl: &TextLine) -> (PcgLine, Option<String>) {
        let t = tl.text.trim();
        let whole = t
            .split_once(':')
            .filter(|(tag, _)| WHOLE_LINE_TAGS.contains(tag));
        let (elems, err) = if t.is_empty() || t.starts_with('#') {
            (None, None)
        } else if let Some((tag, rest)) = whole {
            (Some(vec![Elem::raw(tag, rest)]), None)
        } else {
            match tokenize(t) {
                Ok(e) => (Some(e), None),
                Err(e) => (None, Some(e)),
            }
        };
        (
            PcgLine {
                raw: tl.text.clone(),
                eol: tl.eol,
                latin1: tl.latin1,
                elems,
            },
            err,
        )
    }

    /// A data line built from elements.
    pub fn new(elems: Vec<Elem>) -> PcgLine {
        PcgLine {
            raw: join(&elems),
            eol: Eol::Lf,
            latin1: false,
            elems: Some(elems),
        }
    }

    pub fn comment(text: &str) -> PcgLine {
        PcgLine {
            raw: format!("# {}", text),
            eol: Eol::Lf,
            latin1: false,
            elems: None,
        }
    }

    pub fn blank() -> PcgLine {
        PcgLine {
            raw: String::new(),
            eol: Eol::Lf,
            latin1: false,
            elems: None,
        }
    }

    /// The tag of the first element.
    pub fn tag(&self) -> Option<&str> {
        self.elems.as_ref()?.first().map(|e| e.tag.as_str())
    }

    /// First element (the line's subject).
    pub fn head(&self) -> Option<&Elem> {
        self.elems.as_ref()?.first()
    }

    /// Value of the first element with this tag.
    pub fn get(&self, tag: &str) -> Option<&Elem> {
        self.elems.as_ref()?.iter().find(|e| e.tag == tag)
    }

    /// Decoded text of the first element with this tag.
    pub fn text(&self, tag: &str) -> Option<String> {
        self.get(tag).map(Elem::value_text)
    }

    /// Replace the elements, regenerating the raw text.
    pub fn set_elems(&mut self, elems: Vec<Elem>) {
        self.raw = join(&elems);
        self.elems = Some(elems);
    }
}

/// A PCG file.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PcgFile {
    #[serde(skip)]
    pub bom: bool,
    pub lines: Vec<PcgLine>,
    pub diags: Diagnostics,
}

impl PcgFile {
    pub fn from_text(tf: &TextFile, path: Option<&Path>) -> PcgFile {
        let mut diags = Diagnostics::default();
        let mut lines = Vec::new();
        for (i, tl) in tf.lines.iter().enumerate() {
            let (l, err) = PcgLine::from_text(tl);
            if let Some(e) = err {
                let d = Diagnostic::error(e);
                diags.push(match path {
                    Some(p) => d.at(p, i + 1),
                    None => d,
                });
            }
            lines.push(l);
        }
        PcgFile {
            bom: tf.bom,
            lines,
            diags,
        }
    }

    pub fn parse(s: &str) -> PcgFile {
        PcgFile::from_text(&TextFile::from_text(s), None)
    }

    pub fn read(path: &Path) -> Result<PcgFile> {
        Ok(PcgFile::from_text(&TextFile::read(path)?, Some(path)))
    }

    pub fn to_text(&self) -> TextFile {
        TextFile {
            bom: self.bom,
            lines: self
                .lines
                .iter()
                .map(|l| TextLine {
                    text: l.raw.clone(),
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

    /// Data lines whose first element has this tag.
    pub fn lines_tagged<'a, 'b>(
        &'a self,
        tag: &'b str,
    ) -> impl Iterator<Item = &'a PcgLine> + use<'a, 'b> {
        self.lines.iter().filter(move |l| l.tag() == Some(tag))
    }

    /// Indices of data lines with this tag.
    pub fn indices(&self, tag: &str) -> Vec<usize> {
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.tag() == Some(tag))
            .map(|(i, _)| i)
            .collect()
    }

    /// Decoded value of the first line with this tag.
    pub fn value(&self, tag: &str) -> Option<String> {
        self.lines_tagged(tag)
            .next()
            .and_then(|l| l.head())
            .map(Elem::value_text)
    }

    /// Insert a line after the last line with `tag` (or after the last
    /// line with any of `after`, or at the end).  Returns its index.
    pub fn insert_near(&mut self, tag: &str, after: &[&str], line: PcgLine) -> usize {
        let pos = self
            .indices(tag)
            .last()
            .copied()
            .or_else(|| {
                after
                    .iter()
                    .filter_map(|t| self.indices(t).last().copied())
                    .max()
            })
            .map(|i| i + 1)
            .unwrap_or(self.lines.len());
        let mut line = line;
        line.eol = self.to_text().dominant_eol();
        if pos == self.lines.len() {
            if let Some(last) = self.lines.last_mut() {
                if last.eol == Eol::None {
                    last.eol = line.eol;
                }
            }
        }
        self.lines.insert(pos, line);
        pos
    }

    /// Set the single line `TAG:value` (replacing or inserting).
    pub fn set_value(&mut self, tag: &str, value: &str, after: &[&str]) {
        let elems = vec![Elem::text(tag, value)];
        match self.indices(tag).first() {
            Some(&i) => self.lines[i].set_elems(elems),
            None => {
                self.insert_near(tag, after, PcgLine::new(elems));
            }
        }
    }

    /// Remove all lines with this tag.
    pub fn remove_all(&mut self, tag: &str) {
        self.lines.retain(|l| l.tag() != Some(tag));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_table() {
        let cases: &[(&str, &str)] = &[
            ("", ""),
            ("plain", "plain"),
            ("a|b", "a&pipe;b"),
            ("a:b", "a&colon;b"),
            ("[x]", "&lbracket;x&rbracket;"),
            ("x & y", "x &amp; y"),
            ("l1\nl2", "l1&nl;l2"),
            ("cr\r", "cr&cr;"),
            ("ff\u{c}", "ff&lf;"),
            ("&pipe;", "&amp;pipe;"),
            ("ünïcode", "ünïcode"),
        ];
        for (plain, enc) in cases {
            assert_eq!(&encode(plain, false), enc, "{:?}", plain);
            assert_eq!(&decode(enc), plain, "{:?}", enc);
        }
        assert_eq!(encode("a:b[c]|d", true), "a:b[c]&pipe;d");
        assert_eq!(decode("a & b"), "a & b");
        assert_eq!(decode("&bogus;x"), "&bogus;x");
        assert_eq!(decode("trailing &"), "trailing &");
    }

    #[test]
    fn tokenize_table() {
        let cases: &[(&str, usize)] = &[
            ("A:1", 1),
            ("A:1|B:2", 2),
            ("A:1|B:2|", 2),
            ("A:", 1),
            ("A:[]", 1),
            ("A:[B:1|C:[D:2]]", 1),
            (
                "SKILL:X|CLASSBOUGHT:[CLASS:Y|RANKS:2.0]|ASSOCIATEDDATA:Z",
                3,
            ),
            ("A:x:y", 1),
            ("A:[B:1]C:[D:2]", 2),
            ("R:2|E:roll(4,6,[2,3,4])", 2),
            ("A:x[y]", 1),
            ("S:x|C:[K:1]C:[K:2]", 3),
            (" A:1 | B:2 ", 2),
        ];
        for (s, n) in cases {
            let e = tokenize(s).unwrap_or_else(|e| panic!("{:?}: {}", s, e));
            assert_eq!(e.len(), *n, "{:?}", s);
        }
        let e = tokenize("A:[B:1|C:[D:2]]").unwrap();
        assert_eq!(e[0].children().len(), 2);
        assert_eq!(e[0].children()[1].children()[0].value_text(), "2");
        assert_eq!(tokenize("A:x:y").unwrap()[0].value_text(), "x:y");
    }

    #[test]
    fn tokenize_errors() {
        for s in ["A", "A:[", "A:]", "A:[B:1", "A:1|B", "A:[B]"] {
            assert!(tokenize(s).is_err(), "accepted {:?}", s);
        }
    }

    #[test]
    fn join_roundtrip() {
        for s in [
            "A:1",
            "A:1|B:2",
            "A:[B:1|C:[D:2]]|E:",
            "CLASS:Mage|LEVEL:3|CANCASTPERDAY:4,2,1",
            "EQUIPNAME:Rope|CUSTOMIZATION:[BASEITEM:Rope|DATA:EQMOD=X]",
            "A:[]",
        ] {
            assert_eq!(join(&tokenize(s).unwrap()), s);
        }
    }

    #[test]
    fn file_roundtrip_and_access() {
        let s = "PCGVERSION:2.0\r\n# comment\r\n\r\nSTAT:STR|SCORE:12\r\nSTAT:DEX|SCORE:14\r\nSKILL:Ride|OUTPUTORDER:1|\r\nNAME:A&colon;B";
        let mut f = PcgFile::parse(s);
        assert!(f.diags.is_empty());
        assert_eq!(f.encode(), s.as_bytes());
        assert_eq!(f.value("PCGVERSION").as_deref(), Some("2.0"));
        assert_eq!(f.value("NAME").as_deref(), Some("A:B"));
        assert_eq!(f.indices("STAT"), vec![3, 4]);
        let l = &f.lines[3];
        assert_eq!(l.text("SCORE").as_deref(), Some("12"));

        f.set_value("NAME", "C|D", &[]);
        assert_eq!(
            PcgFile::parse("CHARACTERBIO:a|b[c]:d").lines[0]
                .text("CHARACTERBIO")
                .as_deref(),
            Some("a|b[c]:d")
        );
        f.set_value("EXPERIENCE", "100", &["STAT"]);
        f.remove_all("SKILL");
        let out = String::from_utf8(f.encode()).unwrap();
        assert_eq!(
            out,
            "PCGVERSION:2.0\r\n# comment\r\n\r\nSTAT:STR|SCORE:12\r\nSTAT:DEX|SCORE:14\r\nEXPERIENCE:100\r\nNAME:C&pipe;D"
        );
    }

    #[test]
    fn bad_line_kept() {
        let f = PcgFile::parse("GOOD:1\nBAD[\n");
        assert_eq!(f.diags.len(), 1);
        assert!(f.lines[1].elems.is_none());
        assert_eq!(f.encode(), b"GOOD:1\nBAD[\n");
    }
}
