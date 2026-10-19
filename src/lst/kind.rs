//
// lst/kind.rs -- data file kinds and their load order
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use serde::{Deserialize, Serialize};
use std::fmt;

/// The kind of an LST data file, named by the PCC tag that loads it.
/// Variants are declared in load order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FileKind {
    DataControl,
    DataTable,
    Variable,
    Dynamic,
    GlobalModifier,
    AbilityCategory,
    Size,
    Stat,
    Save,
    Alignment,
    WeaponProf,
    ArmorProf,
    ShieldProf,
    Skill,
    Language,
    Feat,
    Ability,
    Race,
    Domain,
    Spell,
    Deity,
    Class,
    Template,
    EquipMod,
    Equipment,
    CompanionMod,
    Kit,
    BioSet,
}

impl FileKind {
    /// All kinds, in load order.
    pub const ALL: [FileKind; 28] = [
        FileKind::DataControl,
        FileKind::DataTable,
        FileKind::Variable,
        FileKind::Dynamic,
        FileKind::GlobalModifier,
        FileKind::AbilityCategory,
        FileKind::Size,
        FileKind::Stat,
        FileKind::Save,
        FileKind::Alignment,
        FileKind::WeaponProf,
        FileKind::ArmorProf,
        FileKind::ShieldProf,
        FileKind::Skill,
        FileKind::Language,
        FileKind::Feat,
        FileKind::Ability,
        FileKind::Race,
        FileKind::Domain,
        FileKind::Spell,
        FileKind::Deity,
        FileKind::Class,
        FileKind::Template,
        FileKind::EquipMod,
        FileKind::Equipment,
        FileKind::CompanionMod,
        FileKind::Kit,
        FileKind::BioSet,
    ];

    /// The PCC tag that names files of this kind.
    pub fn tag(&self) -> &'static str {
        match self {
            FileKind::DataControl => "DATACONTROL",
            FileKind::DataTable => "DATATABLE",
            FileKind::Variable => "VARIABLE",
            FileKind::Dynamic => "DYNAMIC",
            FileKind::GlobalModifier => "GLOBALMODIFIER",
            FileKind::AbilityCategory => "ABILITYCATEGORY",
            FileKind::Size => "SIZE",
            FileKind::Stat => "STAT",
            FileKind::Save => "SAVE",
            FileKind::Alignment => "ALIGNMENT",
            FileKind::WeaponProf => "WEAPONPROF",
            FileKind::ArmorProf => "ARMORPROF",
            FileKind::ShieldProf => "SHIELDPROF",
            FileKind::Skill => "SKILL",
            FileKind::Language => "LANGUAGE",
            FileKind::Feat => "FEAT",
            FileKind::Ability => "ABILITY",
            FileKind::Race => "RACE",
            FileKind::Domain => "DOMAIN",
            FileKind::Spell => "SPELL",
            FileKind::Deity => "DEITY",
            FileKind::Class => "CLASS",
            FileKind::Template => "TEMPLATE",
            FileKind::EquipMod => "EQUIPMOD",
            FileKind::Equipment => "EQUIPMENT",
            FileKind::CompanionMod => "COMPANIONMOD",
            FileKind::Kit => "KIT",
            FileKind::BioSet => "BIOSET",
        }
    }

    pub fn from_tag(tag: &str) -> Option<FileKind> {
        FileKind::ALL.iter().copied().find(|k| k.tag() == tag)
    }

    /// Parse a user-supplied kind name, case-insensitively, accepting
    /// a few common aliases.
    pub fn from_name(name: &str) -> Option<FileKind> {
        let up = name.to_ascii_uppercase();
        let up = match up.as_str() {
            "EQUIP" | "EQ" => "EQUIPMENT",
            "EQMOD" => "EQUIPMOD",
            "DATACTRL" => "DATACONTROL",
            "GLOBALMOD" => "GLOBALMODIFIER",
            "COMPANION" => "COMPANIONMOD",
            "CLASSES" => "CLASS",
            "SPELLS" => "SPELL",
            "ABILITIES" => "ABILITY",
            "FEATS" => "FEAT",
            "RACES" => "RACE",
            "SKILLS" => "SKILL",
            other => other,
        };
        FileKind::from_tag(up)
    }

    /// Does this kind hold named objects (as opposed to tables,
    /// variable declarations, or global modifiers)?
    pub fn has_objects(&self) -> bool {
        !matches!(
            self,
            FileKind::DataTable | FileKind::Variable | FileKind::GlobalModifier
        )
    }

    /// PCC file entries of this kind may not carry INCLUDE, EXCLUDE,
    /// or prerequisite suffixes.
    pub fn allows_filters(&self) -> bool {
        !matches!(
            self,
            FileKind::CompanionMod
                | FileKind::BioSet
                | FileKind::Dynamic
                | FileKind::Variable
                | FileKind::DataControl
                | FileKind::DataTable
                | FileKind::GlobalModifier
        )
    }
}

impl fmt::Display for FileKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.tag())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_and_tags() {
        for w in FileKind::ALL.windows(2) {
            assert!(w[0] < w[1]);
        }
        for k in FileKind::ALL {
            assert_eq!(FileKind::from_tag(k.tag()), Some(k));
        }
        assert_eq!(FileKind::from_name("eqmod"), Some(FileKind::EquipMod));
        assert_eq!(FileKind::from_name("spell"), Some(FileKind::Spell));
        assert_eq!(FileKind::from_tag("PCC"), None);
    }
}
