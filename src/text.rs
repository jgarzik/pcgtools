//
// text.rs -- lossless text file decoding and encoding
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! PCGen data files are nominally UTF-8, but real-world data contains
//! byte-order marks, CRLF and CR-only line endings, missing final
//! newlines, and the occasional line in Latin-1.  [`TextFile`] decodes
//! all of that into Rust strings while remembering enough detail to
//! write the file back byte-for-byte.

use crate::error::{Error, Result};
use serde::Serialize;
use std::path::Path;

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// Line terminator following a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Eol {
    Lf,
    CrLf,
    Cr,
    /// last line of a file with no trailing newline
    None,
}

impl Eol {
    pub fn as_str(&self) -> &'static str {
        match self {
            Eol::Lf => "\n",
            Eol::CrLf => "\r\n",
            Eol::Cr => "\r",
            Eol::None => "",
        }
    }
}

/// One physical line of text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TextLine {
    pub text: String,
    pub eol: Eol,
    /// line was not valid UTF-8 and was decoded as Latin-1
    pub latin1: bool,
}

impl TextLine {
    pub fn new(text: impl Into<String>) -> TextLine {
        TextLine {
            text: text.into(),
            eol: Eol::Lf,
            latin1: false,
        }
    }
}

/// A decoded text file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TextFile {
    pub bom: bool,
    pub lines: Vec<TextLine>,
}

impl TextFile {
    /// Decode raw bytes.  Never fails: invalid UTF-8 lines fall back
    /// to Latin-1, which maps every byte to a char.
    pub fn decode(bytes: &[u8]) -> TextFile {
        let (bom, mut rest) = match bytes.strip_prefix(BOM) {
            Some(r) => (true, r),
            None => (false, bytes),
        };

        let mut lines = Vec::new();
        while !rest.is_empty() {
            let pos = rest.iter().position(|&b| b == b'\n' || b == b'\r');
            let (body, eol, adv) = match pos {
                None => (rest, Eol::None, rest.len()),
                Some(i) if rest[i] == b'\n' => (&rest[..i], Eol::Lf, i + 1),
                Some(i) if rest.get(i + 1) == Some(&b'\n') => (&rest[..i], Eol::CrLf, i + 2),
                Some(i) => (&rest[..i], Eol::Cr, i + 1),
            };
            let (text, latin1) = match std::str::from_utf8(body) {
                Ok(s) => (s.to_string(), false),
                Err(_) => (body.iter().map(|&b| b as char).collect(), true),
            };
            lines.push(TextLine { text, eol, latin1 });
            rest = &rest[adv..];
        }

        TextFile { bom, lines }
    }

    pub fn from_text(s: &str) -> TextFile {
        TextFile::decode(s.as_bytes())
    }

    pub fn read(path: &Path) -> Result<TextFile> {
        let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
        Ok(TextFile::decode(&bytes))
    }

    /// Encode back to bytes, reproducing the original exactly when
    /// nothing was modified.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if self.bom {
            out.extend_from_slice(BOM);
        }
        for line in &self.lines {
            encode_line(&mut out, &line.text, line.latin1);
            out.extend_from_slice(line.eol.as_str().as_bytes());
        }
        out
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        std::fs::write(path, self.encode()).map_err(|e| Error::io(path, e))
    }

    /// The most common line terminator, used for newly added lines.
    pub fn dominant_eol(&self) -> Eol {
        let mut counts = [0usize; 3];
        for l in &self.lines {
            match l.eol {
                Eol::Lf => counts[0] += 1,
                Eol::CrLf => counts[1] += 1,
                Eol::Cr => counts[2] += 1,
                Eol::None => {}
            }
        }
        if counts[1] > counts[0] && counts[1] >= counts[2] {
            Eol::CrLf
        } else if counts[2] > counts[0] {
            Eol::Cr
        } else {
            Eol::Lf
        }
    }

    /// True if any line needed the Latin-1 fallback.
    pub fn has_latin1(&self) -> bool {
        self.lines.iter().any(|l| l.latin1)
    }
}

fn encode_line(out: &mut Vec<u8>, text: &str, latin1: bool) {
    if latin1 && text.chars().all(|c| (c as u32) < 0x100) {
        out.extend(text.chars().map(|c| c as u8));
    } else {
        out.extend_from_slice(text.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(bytes: &[u8]) {
        let tf = TextFile::decode(bytes);
        assert_eq!(tf.encode(), bytes);
    }

    #[test]
    fn roundtrip_variants() {
        rt(b"");
        rt(b"a\nb\n");
        rt(b"a\r\nb\r\n");
        rt(b"a\rb\r");
        rt(b"a\nb");
        rt(b"\n\n\n");
        rt(b"\xEF\xBB\xBFhello\r\nworld");
        rt(b"mixed\r\nline\nends\rhere");
        rt(b"latin \xe9 here\nutf8 \xc3\xa9 here\n");
        rt(b"truncated \xe2\x80\"\n");
    }

    #[test]
    fn decode_details() {
        let tf = TextFile::decode(b"\xEF\xBB\xBFone\r\ntwo \xe9\nthree");
        assert!(tf.bom);
        assert_eq!(tf.lines.len(), 3);
        assert_eq!(tf.lines[0].text, "one");
        assert_eq!(tf.lines[0].eol, Eol::CrLf);
        assert_eq!(tf.lines[1].text, "two é");
        assert!(tf.lines[1].latin1);
        assert_eq!(tf.lines[2].eol, Eol::None);
    }

    #[test]
    fn dominant() {
        assert_eq!(TextFile::decode(b"a\r\nb\r\nc\n").dominant_eol(), Eol::CrLf);
        assert_eq!(TextFile::decode(b"a\nb").dominant_eol(), Eol::Lf);
    }

    #[test]
    fn eol_table() {
        let cases: &[(&[u8], &[Eol])] = &[
            (b"a", &[Eol::None]),
            (b"a\n", &[Eol::Lf]),
            (b"a\r\n", &[Eol::CrLf]),
            (b"a\r", &[Eol::Cr]),
            (b"\r\n\r\n", &[Eol::CrLf, Eol::CrLf]),
            (b"\r\r\n", &[Eol::Cr, Eol::CrLf]),
            (b"\n\r", &[Eol::Lf, Eol::Cr]),
        ];
        for (bytes, eols) in cases {
            let tf = TextFile::decode(bytes);
            let got: Vec<Eol> = tf.lines.iter().map(|l| l.eol).collect();
            assert_eq!(&got, eols, "{:?}", bytes);
        }
    }

    #[test]
    fn latin1_edit_reencodes() {
        let mut tf = TextFile::decode(b"caf\xe9\n");
        assert!(tf.has_latin1());
        tf.lines[0].text.push('!');
        assert_eq!(tf.encode(), b"caf\xe9!\n");
        // a char outside Latin-1 forces UTF-8 for that line
        tf.lines[0].text.push('\u{2014}');
        assert_eq!(tf.encode(), "café!\u{2014}\n".as_bytes());
    }
}
