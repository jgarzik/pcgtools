#!/usr/bin/env python3
#
# gen_catalog.py -- build src/catalog/tags.tsv from PCGen sources
#
# Copyright (c) 2024 Jeff Garzik
#
# This file is part of the pcgtools software project covered under
# the MIT License.  For the full license text, please see the LICENSE
# file in the root directory of this project.
# SPDX-License-Identifier: MIT
#
# Usage: python3 gen_catalog.py PCGEN_ROOT > src/catalog/tags.tsv
#
# PCGEN_ROOT holds checkouts named pcgen/ (program source) and
# pcgen-docs/ (documentation).  Only facts are extracted: tag names,
# the scope (file kind or token family) each tag is valid in, and a
# status (active or deprecated).  No descriptive text is copied.

import os
import re
import sys

# token target class -> scope
CLASS_SCOPE = {
    "CDOMObject": "GLOBAL",
    "ConcretePrereqObject": "GLOBAL",
    "Campaign": "PCC",
    "PCClass": "CLASS",
    "PCClassLevel": "CLASSLEVEL",
    "SubClass": "SUBCLASS",
    "Ability": "ABILITY",
    "AbilityCategory": "ABILITYCATEGORY",
    "Race": "RACE",
    "Equipment": "EQUIPMENT",
    "EquipmentModifier": "EQUIPMOD",
    "PCTemplate": "TEMPLATE",
    "Spell": "SPELL",
    "Skill": "SKILL",
    "Deity": "DEITY",
    "Domain": "DOMAIN",
    "WeaponProf": "WEAPONPROF",
    "CompanionMod": "COMPANIONMOD",
    "PCStat": "STAT",
    "PCAlignment": "ALIGNMENT",
    "SizeAdjustment": "SIZE",
    "DatasetVariable": "VARIABLE",
    "ContentDefinition": "DATACONTROL",
    "FactDefinition": "DATACONTROL",
    "FactSetDefinition": "DATACONTROL",
    "DefaultVarValue": "DATACONTROL",
    "UserFunction": "DATACONTROL",
    "DynamicCategory": "DATACONTROL",
    "CodeControl": "GAMEMODE.CODECONTROL",
    "UserContent": "DATACONTROL",
    "VarHolder": "GLOBAL",
    "VarContainer": "GLOBAL",
    "LimitedVarHolder": "GLOBAL",
    "LimitedVarContainer": "GLOBAL",
    "PCCheck": "SAVE",
    "ArmorProf": "ARMORPROF",
    "ShieldProf": "SHIELDPROF",
    "Language": "LANGUAGE",
}

# documentation category -> scope
DOC_SCOPE = {
    "pcc-tag": "PCC",
    "pre-tag": "PRE",
    "bonus-tag": "BONUS",
    "choose-tag": "CHOOSE",
    "add-tag": "ADD",
    "other-tag": "GLOBAL",
    "classes-tag": "CLASS",
    "startingkits-tag": "KIT",
    "equipmentmodifiers-tag": "EQUIPMOD",
    "equipment-tag": "EQUIPMENT",
    "templates-tag": "TEMPLATE",
    "races-tag": "RACE",
    "spells-tag": "SPELL",
    "feats-tag": "FEAT",
    "ability-tag": "ABILITY",
    "deities-tag": "DEITY",
    "companionmodifiers-tag": "COMPANIONMOD",
    "skills-tag": "SKILL",
    "domains-tag": "DOMAIN",
    "weaponproficiencies-tag": "WEAPONPROF",
    "abilitycategory-tag": "ABILITYCATEGORY",
    "sizeadjustment-tag": "SIZE",
    "biosettings-tag": "BIOSET",
    "install-tag": "INSTALL",
    "gamemode-miscinfo-tag": "GAMEMODE.MISCINFO",
    "gamemode-statsandchecks-tag": "GAMEMODE.STATSANDCHECKS",
    "gamemode-migration-tag": "GAMEMODE.MIGRATION",
    "gamemode-load-tag": "GAMEMODE.LOAD",
    "gamemode-level-tag": "GAMEMODE.LEVEL",
    "gamemode-equipmentslots-tag": "GAMEMODE.EQUIPMENTSLOTS",
    "gamemode-equipicons-tag": "GAMEMODE.EQUIPICONS",
}

# token directory -> scope, for tokens without a target class
DIR_SCOPE = {
    "gamemode": "GAMEMODE.MISCINFO",
    "level": "GAMEMODE.LEVEL",
    "load": "GAMEMODE.LOAD",
    "eqslot": "GAMEMODE.EQUIPMENTSLOTS",
    "rules": "GAMEMODE.RULES",
    "paper": "GAMEMODE.PAPERINFO",
    "pointbuy": "GAMEMODE.POINTBUY",
    "statsandchecks": "GAMEMODE.STATSANDCHECKS",
    "campaign": "PCC",
}

RE_NAME = re.compile(r'getTokenName\(\)\s*\{\s*return\s+"([^"]+)"')
RE_CLASS = re.compile(r'getTokenClass\(\)\s*\{\s*return\s+([A-Za-z]+)\.class')
RE_PARENT = re.compile(r'getParentToken\(\)\s*\{\s*return\s+"([^"]+)"')
RE_KINDS = re.compile(r'kindsHandled\(\)\s*\{\s*return\s+new\s+String\[\]\s*\{([^}]*)\}')
RE_BONUS = re.compile(r'getBonusHandled\(\)\s*\{\s*return\s+"([^"]+)"')


def add(cat, scope, tag, status, source):
    # a tag is deprecated only if no source knows an active form of it
    key = (scope, tag)
    old = cat.get(key)
    if old is None:
        cat[key] = [status, {source}]
    else:
        if status == "active":
            old[0] = "active"
        old[1].add(source)


def scan_code(root, cat):
    base = os.path.join(root, "pcgen/code/src/java/plugin")
    for d, _, files in os.walk(os.path.join(base, "lsttokens")):
        rel = os.path.relpath(d, os.path.join(base, "lsttokens"))
        top = rel.split(os.sep)[0]
        for f in files:
            if not f.endswith(".java"):
                continue
            src = open(os.path.join(d, f), encoding="utf-8", errors="replace").read()
            m = RE_NAME.search(src)
            if not m:
                continue
            tag = m.group(1)
            status = "deprecated" if top == "deprecated" else "active"
            p = RE_PARENT.search(src)
            c = RE_CLASS.search(src)
            if p:
                scope = p.group(1).lstrip("*")
                if c and c.group(1).startswith("Kit"):
                    scope = "KIT." + scope
            elif c:
                cls = c.group(1)
                if cls.startswith("Kit") or cls == "BaseKit":
                    scope = "KIT"
                else:
                    scope = CLASS_SCOPE.get(cls, "GAMEMODE." + cls.upper())
            else:
                scope = DIR_SCOPE.get(top, top.upper())
            add(cat, scope, tag, status, "code")

    for d, _, files in os.walk(os.path.join(base, "pretokens")):
        for f in files:
            if not f.endswith(".java"):
                continue
            src = open(os.path.join(d, f), encoding="utf-8", errors="replace").read()
            m = RE_KINDS.search(src)
            if m:
                for k in re.findall(r'"([^"]+)"', m.group(1)):
                    add(cat, "PRE", "PRE" + k.upper(), "active", "code")

    for d, _, files in os.walk(os.path.join(base, "bonustokens")):
        for f in files:
            if not f.endswith(".java"):
                continue
            src = open(os.path.join(d, f), encoding="utf-8", errors="replace").read()
            m = RE_BONUS.search(src)
            if m:
                add(cat, "BONUS", m.group(1), "active", "code")


def front_matter(text):
    if not text.startswith("+++"):
        return None
    end = text.find("+++", 3)
    return text[3:end] if end > 0 else None


def scan_docs(root, cat):
    base = os.path.join(root, "pcgen-docs/content/list")
    for d, _, files in os.walk(base):
        for f in files:
            if not f.endswith(".md"):
                continue
            text = open(os.path.join(d, f), encoding="utf-8", errors="replace").read()
            fm = front_matter(text)
            if not fm:
                continue
            cm = re.search(r'categories\s*=\s*\[\s*"all-tag"\s*,\s*"([^"]+)"', fm)
            nm = re.search(r'name\s*=\s*"([^"]+)"', fm)
            if not cm or not nm:
                continue
            scope = DOC_SCOPE.get(cm.group(1))
            if not scope:
                continue
            tag = nm.group(1).strip()
            # the documented name may carry syntax decoration
            tag = re.split(r"[:|( ]", tag)[0]
            if not re.fullmatch(r"!?[A-Z][A-Z0-9_.]*", tag):
                continue
            status = "active"
            sm = re.search(r"## Status\s+(.*?)\n\s*\n", text[len(fm) + 6 :], re.S)
            if sm and re.search(r"deprecat", sm.group(1), re.I):
                status = "deprecated"
            if scope == "BONUS" and tag.startswith("BONUS"):
                tag = tag[5:].lstrip(":")
            if not tag:
                continue
            add(cat, scope, tag, status, "docs")


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: gen_catalog.py PCGEN_ROOT")
    root = sys.argv[1]
    cat = {}
    scan_code(root, cat)
    scan_docs(root, cat)
    print("# scope\ttag\tstatus\tevidence")
    print("# generated by gen_catalog.py; facts only")
    for (scope, tag), (status, sources) in sorted(cat.items()):
        print(f"{scope}\t{tag}\t{status}\t{','.join(sorted(sources))}")


if __name__ == "__main__":
    main()
