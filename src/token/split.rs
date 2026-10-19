//
// token/split.rs -- separator splitting aware of bracket nesting
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

/// Split `s` on `sep`, ignoring separators nested inside `()` or `[]`.
/// Unbalanced closers are treated as ordinary characters, so this
/// never fails; use [`is_balanced`] to check input first if needed.
pub fn split_top(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth: i32 = 0;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' if depth > 0 => depth -= 1,
            _ if c == sep && depth == 0 => {
                out.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// Split on the first top-level occurrence of `sep`.
pub fn split_once_top(s: &str, sep: char) -> Option<(&str, &str)> {
    let mut depth: i32 = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' if depth > 0 => depth -= 1,
            _ if c == sep && depth == 0 => return Some((&s[..i], &s[i + c.len_utf8()..])),
            _ => {}
        }
    }
    None
}

/// True if every `(` and `[` is matched by its closer, properly nested.
pub fn is_balanced(s: &str) -> bool {
    let mut stack = Vec::new();
    for c in s.chars() {
        let want = match c {
            '(' | '[' => {
                stack.push(c);
                continue;
            }
            ')' => '(',
            ']' => '[',
            _ => continue,
        };
        if stack.pop() != Some(want) {
            return false;
        }
    }
    stack.is_empty()
}

/// If `s` is wholly wrapped in `open`...`close` (with the opener
/// matched by the final closer), return the inside.
pub fn unwrap_group(s: &str, open: char, close: char) -> Option<&str> {
    let inner = s.strip_prefix(open)?.strip_suffix(close)?;
    // the opening bracket must match the final closer
    let mut depth = 1;
    for c in inner.chars() {
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return None;
            }
        }
    }
    Some(inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits() {
        assert_eq!(split_top("a|b|c", '|'), vec!["a", "b", "c"]);
        assert_eq!(
            split_top("f.lst|(INCLUDE:x (y)|z)|PREX:1,a", '|'),
            vec!["f.lst", "(INCLUDE:x (y)|z)", "PREX:1,a"]
        );
        assert_eq!(
            split_top("2,[PREA:1,x],[PREB:1,y]", ','),
            vec!["2", "[PREA:1,x]", "[PREB:1,y]"]
        );
        assert_eq!(split_top("", '|'), vec![""]);
        assert_eq!(split_top("a)|b", '|'), vec!["a)", "b"]);
        assert_eq!(split_once_top("x(a|b)|c|d", '|'), Some(("x(a|b)", "c|d")));
    }

    #[test]
    fn balance() {
        assert!(is_balanced("a(b[c])d"));
        assert!(!is_balanced("a(b"));
        assert!(!is_balanced("a(b]"));
        assert_eq!(unwrap_group("(a(b)c)", '(', ')'), Some("a(b)c"));
        assert_eq!(unwrap_group("(a)(b)", '(', ')'), None);
    }

    #[test]
    fn split_table() {
        let cases: &[(&str, char, &[&str])] = &[
            ("a", '|', &["a"]),
            ("a|", '|', &["a", ""]),
            ("|a", '|', &["", "a"]),
            ("a||b", '|', &["a", "", "b"]),
            ("a(b|c)d|e", '|', &["a(b|c)d", "e"]),
            ("[a,b],c", ',', &["[a,b]", "c"]),
            ("((a,b)),c", ',', &["((a,b))", "c"]),
            ("a[b(c,d)e]f,g", ',', &["a[b(c,d)e]f", "g"]),
            ("a(b,c", ',', &["a(b,c"]),
            ("é|ü", '|', &["é", "ü"]),
        ];
        for (s, sep, want) in cases {
            assert_eq!(&split_top(s, *sep), want, "{:?}", s);
        }
    }

    #[test]
    fn split_once_table() {
        assert_eq!(split_once_top("a|b", '|'), Some(("a", "b")));
        assert_eq!(split_once_top("(a|b)", '|'), None);
        assert_eq!(split_once_top("|", '|'), Some(("", "")));
        assert_eq!(split_once_top("", '|'), None);
    }

    #[test]
    fn balance_table() {
        for s in ["", "()", "[]", "([])", "a(b)c[d]", "[(x)(y)]"] {
            assert!(is_balanced(s), "{}", s);
        }
        for s in ["(", ")", "[", "]", "(]", "[)", "([)]", "())"] {
            assert!(!is_balanced(s), "{}", s);
        }
    }

    #[test]
    fn unwrap_table() {
        assert_eq!(unwrap_group("[x]", '[', ']'), Some("x"));
        assert_eq!(unwrap_group("[]", '[', ']'), Some(""));
        assert_eq!(unwrap_group("[[x]]", '[', ']'), Some("[x]"));
        assert_eq!(unwrap_group("[x][y]", '[', ']'), None);
        assert_eq!(unwrap_group("x", '[', ']'), None);
        assert_eq!(unwrap_group("[x", '[', ']'), None);
    }
}
