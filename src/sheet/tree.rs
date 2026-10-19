//
// sheet/tree.rs -- character sheet element trees
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! A small XML-like element tree used for character sheet output, with
//! reading, writing, and comparison by flattened paths.

use crate::error::{Error, Result};
use quick_xml::escape::{resolve_predefined_entity, unescape};
use quick_xml::events::Event;
use quick_xml::Reader;
use serde::Serialize;

/// An element: a tag, optional text, and children.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Element {
    pub tag: String,
    /// attributes, in order
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attrs: Vec<(String, String)>,
    pub text: String,
    pub children: Vec<Element>,
}

impl Element {
    pub fn new(tag: &str) -> Element {
        Element {
            tag: tag.to_string(),
            ..Default::default()
        }
    }

    /// A leaf element with text.
    pub fn leaf(tag: &str, text: impl ToString) -> Element {
        Element {
            tag: tag.to_string(),
            attrs: Vec::new(),
            text: text.to_string(),
            children: Vec::new(),
        }
    }

    /// Add an attribute and return self (builder style).
    pub fn attr(mut self, name: &str, value: impl ToString) -> Element {
        self.attrs.push((name.to_string(), value.to_string()));
        self
    }

    /// Append a child and return self (builder style).
    pub fn with(mut self, child: Element) -> Element {
        self.children.push(child);
        self
    }

    pub fn push(&mut self, child: Element) -> &mut Element {
        self.children.push(child);
        self.children.last_mut().unwrap()
    }

    /// Append a leaf child.
    pub fn add(&mut self, tag: &str, text: impl ToString) {
        self.children.push(Element::leaf(tag, text));
    }

    pub fn child(&self, tag: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.tag == tag)
    }

    /// Parse XML text (comments and declarations are ignored).
    pub fn parse_xml(xml: &str) -> Result<Element> {
        let mut reader = Reader::from_str(xml);
        let mut stack: Vec<Element> = vec![Element::new("#document")];
        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) => {
                    let tag = e.name().as_ref().to_string();
                    let mut el = Element::new(&tag);
                    el.attrs = attributes(&e)?;
                    stack.push(el);
                }
                Ok(Event::Empty(e)) => {
                    let tag = e.name().as_ref().to_string();
                    let mut el = Element::new(&tag);
                    el.attrs = attributes(&e)?;
                    stack.last_mut().unwrap().children.push(el);
                }
                Ok(Event::End(_)) => {
                    let el = stack.pop().unwrap();
                    match stack.last_mut() {
                        Some(parent) => parent.children.push(el),
                        None => return Err(Error::Other("unbalanced XML".into())),
                    }
                }
                Ok(Event::Text(t)) => {
                    stack.last_mut().unwrap().text.push_str(&t.xml10_content());
                }
                // character and entity references come between text runs
                Ok(Event::GeneralRef(r)) => {
                    let text = match r.resolve_char_ref() {
                        Ok(Some(c)) => c.to_string(),
                        Ok(None) => resolve_predefined_entity(&r)
                            .ok_or_else(|| {
                                Error::Other(format!("XML text: unknown entity &{};", &*r))
                            })?
                            .to_string(),
                        Err(e) => return Err(Error::Other(format!("XML text: {}", e))),
                    };
                    stack.last_mut().unwrap().text.push_str(&text);
                }
                Ok(Event::CData(t)) => {
                    stack.last_mut().unwrap().text.push_str(&t.xml10_content());
                }
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(e) => {
                    return Err(Error::Other(format!(
                        "XML error at {}: {}",
                        reader.buffer_position(),
                        e
                    )))
                }
            }
        }
        let doc = stack.pop().unwrap();
        doc.children
            .into_iter()
            .next()
            .ok_or_else(|| Error::Other("empty XML document".into()))
    }

    /// Write as indented XML.
    pub fn to_xml(&self) -> String {
        let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        self.write(&mut s, 0);
        s
    }

    fn write(&self, out: &mut String, depth: usize) {
        let pad = "\t".repeat(depth);
        out.push_str(&pad);
        out.push('<');
        out.push_str(&self.tag);
        for (k, v) in &self.attrs {
            out.push_str(&format!(" {}=\"{}\"", k, escape(v)));
        }
        if self.children.is_empty() && self.text.is_empty() {
            out.push_str("/>\n");
            return;
        }
        out.push('>');
        out.push_str(&escape(&self.text));
        if self.children.is_empty() {
            out.push_str(&format!("</{}>\n", self.tag));
            return;
        }
        out.push('\n');
        for c in &self.children {
            c.write(out, depth + 1);
        }
        out.push_str(&format!("{}</{}>\n", pad, self.tag));
    }

    /// The key that identifies this element among same-tag siblings:
    /// its `name` (or `name/long`) child's text if present.
    fn name_key(&self) -> Option<String> {
        let n = self.child("name")?;
        let t = if n.children.is_empty() {
            n.text.trim().to_string()
        } else {
            n.child("long").map(|l| l.text.trim().to_string())?
        };
        (!t.is_empty()).then_some(t)
    }

    /// Flatten into (path, value) pairs.  Repeated siblings are
    /// distinguished by their name (`skill[Climb]`) or position
    /// (`para[2]`); text of elements with children appears as `#text`.
    pub fn flatten(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        self.flatten_into(&self.tag, &mut out);
        out
    }

    fn flatten_into(&self, path: &str, out: &mut Vec<(String, String)>) {
        for (k, v) in &self.attrs {
            out.push((format!("{}/@{}", path, k), v.clone()));
        }
        let text = self.text.trim();
        if self.children.is_empty() {
            out.push((path.to_string(), text.to_string()));
            return;
        }
        if !text.is_empty() {
            out.push((format!("{}/#text", path), text.to_string()));
        }
        let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        let mut name_counts: std::collections::HashMap<(String, String), usize> =
            std::collections::HashMap::new();
        for c in &self.children {
            let i = counts.entry(c.tag.as_str()).or_default();
            let seg = match c.name_key() {
                Some(n) => {
                    let k = name_counts.entry((c.tag.clone(), n.clone())).or_default();
                    *k += 1;
                    if *k > 1 {
                        format!("{}[{}#{}]", c.tag, n, k)
                    } else {
                        format!("{}[{}]", c.tag, n)
                    }
                }
                None => format!("{}[{}]", c.tag, i),
            };
            *i += 1;
            c.flatten_into(&format!("{}/{}", path, seg), out);
        }
    }
}

/// Attributes of a start or empty tag, unescaped.
fn attributes(e: &quick_xml::events::BytesStart) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for a in e.attributes() {
        let a = a.map_err(|x| Error::Other(format!("XML attribute: {}", x)))?;
        let k = a.key.as_ref().to_string();
        let v = unescape(&a.value)
            .map_err(|x| Error::Other(format!("XML attribute: {}", x)))?
            .into_owned();
        out.push((k, v));
    }
    Ok(out)
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Differences between an expected and an actual tree.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Diff {
    /// (path, expected, actual or None if missing)
    pub mismatches: Vec<(String, String, Option<String>)>,
    pub total: usize,
}

impl Diff {
    pub fn matched(&self) -> usize {
        self.total - self.mismatches.len()
    }
}

/// Compare every expected leaf with the actual tree.
pub fn compare(expected: &Element, actual: &Element) -> Diff {
    let act: std::collections::HashMap<String, String> = actual.flatten().into_iter().collect();
    let mut d = Diff::default();
    for (path, want) in expected.flatten() {
        d.total += 1;
        match act.get(&path) {
            Some(got) if *got == want => {}
            got => d.mismatches.push((path, want, got.cloned())),
        }
    }
    d
}

/// The top-level section of a flattened path (`character/skills/...`
/// gives `skills`).
pub fn section(path: &str) -> &str {
    path.split('/')
        .nth(1)
        .map(|s| s.split('[').next().unwrap_or(s))
        .unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    const X: &str = "<?xml version=\"1.0\"?>\n<character>\n<!-- c -->\n<basics><name>Tess</name><bab>+1</bab>\n<bio><para>a &amp; b</para><para></para></bio>\n<face>\n<face>5 ft.</face><short>5</short></face></basics>\n<skills><skill><name>Climb</name><ranks>1.0</ranks></skill><skill><name>Swim</name><ranks>0.0</ranks></skill></skills><empty/></character>";

    #[test]
    fn parse_and_flatten() {
        let e = Element::parse_xml(X).unwrap();
        assert_eq!(e.tag, "character");
        let f = e.flatten();
        let get = |p: &str| f.iter().find(|(k, _)| k == p).map(|(_, v)| v.as_str());
        // an element with a name child is keyed by it
        assert_eq!(get("character/basics[Tess]/name[0]"), Some("Tess"));
        assert_eq!(get("character/basics[Tess]/bio[0]/para[0]"), Some("a & b"));
        assert_eq!(get("character/basics[Tess]/bio[0]/para[1]"), Some(""));
        assert_eq!(get("character/basics[Tess]/face[0]/face[0]"), Some("5 ft."));
        assert_eq!(get("character/skills[0]/skill[Swim]/ranks[0]"), Some("0.0"));
        assert_eq!(get("character/empty[0]"), Some(""));
    }

    #[test]
    fn entities_and_cdata() {
        let e = Element::parse_xml(
            "<r a=\"x &amp; &#x41;\"><t>Tom &amp; Jerry &lt;3 &#65;&#x42; &quot;q&quot;</t><c><![CDATA[a & <b>]]></c></r>",
        )
        .unwrap();
        assert_eq!(e.attrs, vec![("a".to_string(), "x & A".to_string())]);
        assert_eq!(e.child("t").unwrap().text, "Tom & Jerry <3 AB \"q\"");
        assert_eq!(e.child("c").unwrap().text, "a & <b>");
        assert!(Element::parse_xml("<r>&bogus;</r>").is_err());
        // escaped text survives writing and reading back
        assert_eq!(
            Element::parse_xml(&e.to_xml()).unwrap().children,
            e.children
        );
    }

    #[test]
    fn write_roundtrip() {
        let e = Element::parse_xml(X).unwrap();
        let again = Element::parse_xml(&e.to_xml()).unwrap();
        assert_eq!(e.flatten(), again.flatten());
    }

    #[test]
    fn diffs() {
        let e = Element::parse_xml(X).unwrap();
        let mut a = Element::new("character");
        let b = a.push(Element::new("basics"));
        b.add("name", "Tess");
        b.add("bab", "+2");
        let d = compare(&e, &a);
        assert_eq!(d.total, e.flatten().len());
        assert!(d
            .mismatches
            .iter()
            .any(|(p, w, g)| p.ends_with("bab[0]") && w == "+1" && g.as_deref() == Some("+2")));
        assert!(!d
            .mismatches
            .iter()
            .any(|(p, _, _)| p == "character/basics[Tess]/name[0]"));
        assert!(d
            .mismatches
            .iter()
            .any(|(p, _, g)| p == "character/skills[0]/skill[Climb]/ranks[0]" && g.is_none()));
        assert_eq!(
            section("character/skills[0]/skill[Swim]/ranks[0]"),
            "skills"
        );
    }

    #[test]
    fn duplicate_names() {
        let e =
            Element::parse_xml("<r><s><name>A</name><v>1</v></s><s><name>A</name><v>2</v></s></r>")
                .unwrap();
        let f = e.flatten();
        assert_eq!(f[3], ("r/s[A#2]/v[0]".to_string(), "2".to_string()));
    }
}
