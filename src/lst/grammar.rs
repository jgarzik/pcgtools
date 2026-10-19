//
// lst/grammar.rs -- per-kind meaning of record heads
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! Most LST kinds hold one object per record, named by the head.  A
//! few kinds have richer structure: class files mix class, subclass,
//! and level lines; kit files group sub-lines under a STARTPACK line;
//! data control and variable files hold one declaration per line.
//! [`classify`] decides what role a record plays.

use super::kind::FileKind;
use super::line::{is_tag_name, Head};
use serde::Serialize;

/// The role of a record within its file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Role {
    /// Starts an object, or a .MOD/.COPY/.FORGET of one.
    /// `head_tag` is set for kinds whose heads are `TAG:name`.
    Object {
        head_tag: Option<String>,
        head: Head,
    },
    /// Starts a child object of the current object
    /// (class SUBCLASS and SUBSTITUTIONCLASS lines).
    Child { head_tag: String, name: String },
    /// Belongs to the current object (class level lines, kit lines,
    /// bioset race lines).  `tag` is the head tag, or `LEVEL` for a
    /// numeric class level line; `value` is the rest of the head.
    Sub { tag: String, value: String },
    /// Belongs to the current child object (SUBCLASSLEVEL lines).
    ChildSub { tag: String, value: String },
}

/// Split `TAG:rest` when TAG looks like a tag name.
fn tagged(head: &str) -> Option<(&str, &str)> {
    head.split_once(':').filter(|(t, _)| is_tag_name(t))
}

/// Tags that start objects in a kind whose heads are all `TAG:value`.
fn object_tags(kind: FileKind) -> Option<&'static [&'static str]> {
    Some(match kind {
        FileKind::Class => &["CLASS"],
        FileKind::Kit => &["STARTPACK"],
        FileKind::BioSet => &["AGESET"],
        FileKind::AbilityCategory => &["ABILITYCATEGORY"],
        FileKind::CompanionMod => &["FOLLOWER", "MASTERBONUSRACE"],
        _ => return None,
    })
}

/// Is every `TAG:` head a separate declaration in this kind?
fn declaration_kind(kind: FileKind) -> bool {
    matches!(
        kind,
        FileKind::DataControl
            | FileKind::Variable
            | FileKind::Dynamic
            | FileKind::GlobalModifier
            | FileKind::DataTable
    )
}

/// Classify a record by its head cell.
pub fn classify(kind: FileKind, head: &str) -> Role {
    let head = head.trim();

    if kind == FileKind::Class {
        // numeric level lines: "3" or "1:REPEATLEVEL:4"
        let (num, rest) = head.split_once(':').unwrap_or((head, ""));
        if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
            return Role::Sub {
                tag: "LEVEL".into(),
                value: if rest.is_empty() {
                    num.to_string()
                } else {
                    format!("{}:{}", num, rest)
                },
            };
        }
        if let Some((t, v)) = tagged(head) {
            match t {
                "SUBCLASS" | "SUBSTITUTIONCLASS" => {
                    return Role::Child {
                        head_tag: t.into(),
                        name: v.into(),
                    }
                }
                "SUBCLASSLEVEL" | "SUBSTITUTIONLEVEL" => {
                    return Role::ChildSub {
                        tag: t.into(),
                        value: v.into(),
                    }
                }
                _ => {}
            }
        }
    }

    if let Some(tags) = object_tags(kind) {
        if let Some((t, _)) = tagged(head) {
            if tags.contains(&t) {
                return Role::Object {
                    head_tag: Some(t.into()),
                    head: Head::parse(head),
                };
            }
            if kind != FileKind::AbilityCategory && kind != FileKind::CompanionMod {
                let (_, v) = tagged(head).unwrap();
                return Role::Sub {
                    tag: t.into(),
                    value: v.into(),
                };
            }
        }
    }

    if declaration_kind(kind) {
        if let Some((t, _)) = tagged(head) {
            let mut h = Head::parse(head);
            // declarations are keyed by their whole value
            h.name = head[t.len() + 1..].to_string();
            h.op = super::line::HeadOp::Define;
            h.category = None;
            return Role::Object {
                head_tag: Some(t.into()),
                head: h,
            };
        }
    }

    let h = Head::parse(head);
    Role::Object {
        head_tag: h.prefix.clone(),
        head: h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lst::line::HeadOp;

    fn obj(r: &Role) -> (&Option<String>, &Head) {
        match r {
            Role::Object { head_tag, head } => (head_tag, head),
            other => panic!("not an object: {:?}", other),
        }
    }

    #[test]
    fn class_lines() {
        let r = classify(FileKind::Class, "CLASS:Fighter");
        let (t, h) = obj(&r);
        assert_eq!(t.as_deref(), Some("CLASS"));
        assert_eq!(h.name, "Fighter");

        let r = classify(FileKind::Class, "CLASS:Fighter.MOD");
        assert_eq!(obj(&r).1.op, HeadOp::Mod);

        assert_eq!(
            classify(FileKind::Class, "4"),
            Role::Sub {
                tag: "LEVEL".into(),
                value: "4".into()
            }
        );
        assert_eq!(
            classify(FileKind::Class, "1:REPEATLEVEL:2"),
            Role::Sub {
                tag: "LEVEL".into(),
                value: "1:REPEATLEVEL:2".into()
            }
        );
        assert_eq!(
            classify(FileKind::Class, "SUBCLASS:Fire"),
            Role::Child {
                head_tag: "SUBCLASS".into(),
                name: "Fire".into()
            }
        );
        assert_eq!(
            classify(FileKind::Class, "SUBCLASSLEVEL:1"),
            Role::ChildSub {
                tag: "SUBCLASSLEVEL".into(),
                value: "1".into()
            }
        );
    }

    #[test]
    fn kit_and_decl() {
        let r = classify(FileKind::Kit, "STARTPACK:Explorer");
        assert_eq!(obj(&r).1.name, "Explorer");
        assert_eq!(
            classify(FileKind::Kit, "GEAR:Rope"),
            Role::Sub {
                tag: "GEAR".into(),
                value: "Rope".into()
            }
        );
        let r = classify(FileKind::DataControl, "FACTDEF:CLASS|Abb");
        let (t, h) = obj(&r);
        assert_eq!(t.as_deref(), Some("FACTDEF"));
        assert_eq!(h.name, "CLASS|Abb");
        let r = classify(FileKind::Variable, "LOCAL:PC.STAT|NUMBER=Score");
        assert_eq!(obj(&r).1.name, "PC.STAT|NUMBER=Score");
    }

    #[test]
    fn generic() {
        let r = classify(FileKind::Spell, "Magic Missile.COPY=Magic Missile II");
        let (t, h) = obj(&r);
        assert_eq!(*t, None);
        assert_eq!(h.op, HeadOp::Copy("Magic Missile II".into()));
        let r = classify(FileKind::Ability, "CATEGORY=FEAT|Dodge.MOD");
        assert_eq!(obj(&r).1.category.as_deref(), Some("FEAT"));
    }

    #[test]
    fn role_table() {
        let sub = |t: &str, v: &str| Role::Sub {
            tag: t.into(),
            value: v.into(),
        };
        let cases: &[(FileKind, &str, Role)] = &[
            (FileKind::Class, "10", sub("LEVEL", "10")),
            (
                FileKind::Class,
                "2:REPEATLEVEL:4|SKIP=1|MAX=20",
                sub("LEVEL", "2:REPEATLEVEL:4|SKIP=1|MAX=20"),
            ),
            (
                FileKind::Class,
                "SUBSTITUTIONLEVEL:3",
                Role::ChildSub {
                    tag: "SUBSTITUTIONLEVEL".into(),
                    value: "3".into(),
                },
            ),
            (
                FileKind::Class,
                "SUBSTITUTIONCLASS:Alt",
                Role::Child {
                    head_tag: "SUBSTITUTIONCLASS".into(),
                    name: "Alt".into(),
                },
            ),
            (FileKind::Kit, "REGION:None", sub("REGION", "None")),
            (FileKind::Kit, "NAME:Bob", sub("NAME", "Bob")),
            (FileKind::BioSet, "RACENAME:Elf%", sub("RACENAME", "Elf%")),
        ];
        for (k, head, want) in cases {
            assert_eq!(&classify(*k, head), want, "{} {:?}", k, head);
        }
    }

    #[test]
    fn object_heads() {
        let cases: &[(FileKind, &str, Option<&str>, &str)] = &[
            (
                FileKind::BioSet,
                "AGESET:0|Young",
                Some("AGESET"),
                "0|Young",
            ),
            (
                FileKind::CompanionMod,
                "FOLLOWER:Pet=1",
                Some("FOLLOWER"),
                "Pet=1",
            ),
            (
                FileKind::CompanionMod,
                "MASTERBONUSRACE:Imp",
                Some("MASTERBONUSRACE"),
                "Imp",
            ),
            (
                FileKind::AbilityCategory,
                "ABILITYCATEGORY:Feat",
                Some("ABILITYCATEGORY"),
                "Feat",
            ),
            (
                FileKind::GlobalModifier,
                "MODIFY:X|ADD|1",
                Some("MODIFY"),
                "X|ADD|1",
            ),
            (FileKind::Dynamic, "MOVEMENT:Walk", Some("MOVEMENT"), "Walk"),
            (
                FileKind::Variable,
                "GLOBAL:NUMBER=X",
                Some("GLOBAL"),
                "NUMBER=X",
            ),
            (
                FileKind::DataControl,
                "FUNCTION:d20Mod",
                Some("FUNCTION"),
                "d20Mod",
            ),
            (FileKind::Deity, "DEITY:Sun God", Some("DEITY"), "Sun God"),
            (FileKind::Race, "Elf", None, "Elf"),
            (FileKind::Race, "Elf (Wood)", None, "Elf (Wood)"),
        ];
        for (k, head, tag, name) in cases {
            match classify(*k, head) {
                Role::Object { head_tag, head: h } => {
                    assert_eq!(head_tag.as_deref(), *tag, "{:?}", head);
                    assert_eq!(h.name, *name, "{:?}", head);
                }
                other => panic!("{:?}: {:?}", head, other),
            }
        }
    }
}
