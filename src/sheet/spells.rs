//
// sheet/spells.rs -- the spells section: innate, known and prepared
// spells
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

use super::units::Units;
use super::{describe, output_name, source_text, Element, SheetCtx};
use crate::dataset::Object;
use crate::rules::calc::Calc;
use crate::rules::spells::{
    cast_per_day, caster_level, is_specialty, known_at, owner_class, save_dc, spell_type, Caster,
    CharSpell, SpellInfo, Spells, INNATE_BOOK, KNOWN_BOOK,
};

/// Evaluate parenthesized parts of spell text: `(CASTERLEVEL)` becomes
/// a number, text that is not a formula `[like this]`.
pub fn spell_text(text: &str, eval: &dyn Fn(&str) -> Option<f64>) -> String {
    let mut s = text.to_string();
    // each pass replaces the first parenthesized group
    for _ in 0..64 {
        let Some(start) = s.find('(') else { break };
        let mut depth = 0;
        let mut end = None;
        for (i, ch) in s[start..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(start + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else { break };
        let inner = &s[start + 1..end];
        let v = eval(inner);
        let forced = inner.contains("MIN")
            || inner.contains("MAX")
            || inner.to_uppercase().contains("MIN(")
            || inner.to_uppercase().contains("MAX(");
        let rep = match v {
            Some(x) if x != 0.0 || forced => (x.trunc() as i64).to_string(),
            None if forced => "0".to_string(),
            _ => format!("[{}]", inner),
        };
        s = format!("{}{}{}", &s[..start], rep, &s[end + 1..]);
    }
    s
}

/// Does text read as a formula the calculator can evaluate?
fn formula_like(t: &str) -> bool {
    let t = t.trim();
    !t.is_empty()
        && !t.contains(' ')
        && t.chars()
            .all(|c| c.is_alphanumeric() || "+-*/()._,=\"".contains(c))
}

struct Ctx<'s, 'a> {
    s: &'s SheetCtx<'a>,
    sp: Spells<'a>,
    units: Units,
}

fn joined(o: &Object, tag: &str) -> String {
    o.values(tag).collect::<Vec<_>>().join(", ")
}

impl Ctx<'_, '_> {
    fn calc(&self) -> &Calc<'_> {
        &self.s.calc
    }

    fn eval_spell(&self, f: &str, cl: i64) -> Option<f64> {
        if !formula_like(f) {
            return None;
        }
        Some(self.calc().eval_with(f, &|n| {
            (n.eq_ignore_ascii_case("CASTERLEVEL")).then_some(cl as f64)
        }))
    }

    fn sourcelevel(&self, spell: &Object) -> String {
        let c = self.calc();
        let mut out: Vec<String> = Vec::new();
        for (cls, _) in c.classes() {
            for list in self.sp.class_lists(c, cls) {
                let abbr =
                    c.ds.get(crate::lst::kind::FileKind::Class, &list)
                        .and_then(|o| {
                            o.values("FACT").find_map(|v| {
                                let (k, val) = v.split_once('|')?;
                                k.eq_ignore_ascii_case("Abb").then(|| val.to_string())
                            })
                        })
                        .unwrap_or_else(|| list.clone());
                for l in self.sp.levels_on('C', &list, spell) {
                    out.push(format!("{}{}", abbr, l));
                }
            }
        }
        for d in self.s.ch.domains() {
            for l in self.sp.levels_on('D', &d, spell) {
                out.push(format!("{}{}", d, l));
            }
        }
        out.sort();
        out.dedup();
        out.join(", ")
    }

    fn spell(&self, caster: usize, cs: &CharSpell, info: &SpellInfo) -> Element {
        let c = self.calc();
        let sp = cs.spell;
        let cl = caster_level(c, &self.sp, cs);
        let name = output_name(sp);
        let mut e = Element::new("spell");
        e.add("name", &name);
        e.add("outputname", &name);
        e.add(
            "times_memorized",
            if info.times < 0 {
                "At Will".to_string()
            } else {
                info.times.to_string()
            },
        );
        let range = joined(sp, "RANGE");
        let range_formula = c.gm.and_then(|g| {
            g.lines("miscinfo.lst", "SPELLRANGE").find_map(|l| {
                let (k, f) = l.value.split_once('|')?;
                k.eq_ignore_ascii_case(range.trim()).then(|| f.to_string())
            })
        });
        let range_text = match range_formula {
            Some(f) => {
                let feet = c.eval_with(&f, &|n| {
                    n.eq_ignore_ascii_case("CASTERLEVEL").then_some(cl as f64)
                }) as i64;
                format!("{} ({})", range, self.units.distance(feet as f64))
            }
            None => spell_text(&range, &|f| self.eval_spell(f, cl)),
        };
        e.add("range", range_text);
        e.add("components", joined(sp, "COMPS"));
        e.add("castingtime", joined(sp, "CASTTIME"));
        e.add("casterlevel", cl);
        let save = joined(sp, "SAVEINFO");
        let dc = if save.is_empty() || save == "None" || save == "No" {
            String::new()
        } else {
            save_dc(c, cs, info).to_string()
        };
        e.add("dc", dc);
        e.add(
            "duration",
            spell_text(&joined(sp, "DURATION"), &|f| self.eval_spell(f, cl)),
        );
        let desc = describe(self.s, sp);
        e.add("effect", spell_text(&desc, &|f| self.eval_spell(f, cl)));
        e.add(
            "target",
            spell_text(&joined(sp, "TARGETAREA"), &|f| self.eval_spell(f, cl)),
        );
        e.add("saveinfo", save);
        let school = joined(sp, "SCHOOL");
        let sub = joined(sp, "SUBSCHOOL");
        let descr: Vec<&str> = sp
            .values("DESCRIPTOR")
            .flat_map(|v| v.split(','))
            .map(str::trim)
            .collect();
        let mut full = school.clone();
        if !sub.trim().is_empty() && !sub.trim().eq_ignore_ascii_case("NONE") {
            full.push_str(&format!(" ({})", sub));
        }
        if !descr.is_empty() {
            full.push_str(&format!(" [{}]", descr.join(", ")));
        }
        e.push(
            Element::new("school")
                .with(Element::leaf("school", &school))
                .with(Element::leaf("subschool", &sub))
                .with(Element::leaf("descriptor", descr.join(", ")))
                .with(Element::leaf("fullschool", full)),
        );
        let page = sp.get("SOURCEPAGE").map(str::to_string);
        let short: String = sp
            .get("SOURCESHORT")
            .map(str::to_string)
            .or_else(|| {
                sp.source
                    .iter()
                    .find(|(t, _)| t == "SOURCESHORT")
                    .map(|(_, v)| v.clone())
            })
            .unwrap_or_default()
            .chars()
            .take(8)
            .collect();
        e.push(
            Element::new("source")
                .with(Element::leaf("sourcelevel", self.sourcelevel(sp)))
                .with(Element::leaf("source", source_text(self.s, sp)))
                .with(Element::leaf(
                    "sourcepage",
                    page.unwrap_or_else(|| "null".into()),
                ))
                .with(Element::leaf("sourceshort", short)),
        );
        e.add("spell_resistance", joined(sp, "SPELLRES"));
        e.add("description", desc);
        let bonus = if is_specialty(cs) {
            let same: Vec<_> = self
                .sp
                .spells_in(caster, &info.book, Some(info.level))
                .into_iter()
                .filter(|(x, _)| std::ptr::eq(x.spell, sp))
                .collect();
            if same.iter().all(|(x, _)| is_specialty(x)) {
                "*"
            } else {
                "**"
            }
        } else {
            ""
        };
        e.add("bonusspell", bonus);
        e
    }

    fn spells_of(&self, caster: usize, book: &str, level: i64) -> Vec<Element> {
        self.sp
            .spells_in(caster, book, Some(level))
            .into_iter()
            .map(|(cs, i)| self.spell(caster, cs, i))
            .collect()
    }

    fn class_name(&self, i: usize) -> String {
        match self.sp.casters.get(i) {
            Some(Caster::Class(c)) => output_name(c),
            _ => String::new(),
        }
    }
}

pub fn build(s: &SheetCtx) -> Element {
    let cx = Ctx {
        s,
        sp: Spells::new(&s.calc),
        units: Units::from_gamemode(s.ctx.gamemode.as_ref()),
    };
    let c = &s.calc;
    let race = matches!(cx.sp.casters.first(), Some(Caster::Race));
    let first = usize::from(race);
    let class_count = c.classes().len();
    let books = cx.sp.books.clone();
    let mut out = Element::new("spells");

    // innate
    if !race {
        out.push(Element::new("spells_innate").attr("number", "none"));
    } else {
        let mut inn = Element::new("spells_innate");
        let racial = cx.spells_of(0, INNATE_BOOK, 0);
        if !racial.is_empty() {
            let mut r = Element::new("racial_innate");
            for e in racial {
                r.push(e);
            }
            inn.push(r);
        }
        let mut ci = Element::new("class_innate");
        for (b, name) in books.iter().enumerate().skip(2) {
            let list = cx.spells_of(0, name, 0);
            if list.is_empty() {
                continue;
            }
            let mut bk = Element::new("spellbook")
                .attr("number", b)
                .attr("name", name);
            for e in list {
                bk.push(e);
            }
            ci.push(bk);
        }
        inn.push(ci);
        out.push(inn);
    }

    // known
    let mut known = Element::new("known_spells");
    for ci in first..first + class_count {
        let Some(Caster::Class(cls)) = cx.sp.casters.get(ci) else {
            continue;
        };
        let mut ce = Element::new("class")
            .attr("number", ci)
            .attr("spelllistclass", cx.class_name(ci))
            .attr("spellcastertype", spell_type(cls).unwrap_or(""));
        let domains_here = s.ch.pcg.lines_tagged("DOMAIN").any(|l| {
            l.get("SOURCE")
                .and_then(|e| e.children().iter().find(|x| x.tag == "NAME"))
                .is_some_and(|n| n.value_text().eq_ignore_ascii_case(&cls.key))
        });
        for lvl in 0..=9 {
            let list = cx.spells_of(ci, KNOWN_BOOK, lvl);
            if list.is_empty() {
                ce.push(
                    Element::new("level")
                        .attr("number", lvl)
                        .attr("known", 0)
                        .attr("cast", 0),
                );
                continue;
            }
            let granted = 0;
            let kn = known_at(c, cls, lvl, granted);
            let cast = cast_per_day(c, cls, lvl, false);
            let mut bonus = String::new();
            if cast_per_day(c, cls, lvl, true) > 0
                && lvl > 0
                && spell_type(cls).is_some_and(|t| t.eq_ignore_ascii_case("Divine"))
                && domains_here
            {
                bonus = "+1".into();
            }
            let mut le = Element::new("level")
                .attr("number", lvl)
                .attr("known", kn)
                .attr("cast", format!("{}{}", cast, bonus));
            for e in list {
                le.push(e);
            }
            ce.push(le);
        }
        known.push(ce);
    }
    out.push(known);

    // prepared, by book
    if usize::from(race) + books.len() - 2 == 0 {
        out.push(Element::new("memorized_spells"));
        return out;
    }
    let mut mem = Element::new("memorized_spells");
    if race {
        let racial = cx.spells_of(0, INNATE_BOOK, 0);
        if !racial.is_empty() {
            let mut r = Element::new("racial_innate_memorized");
            for e in racial {
                r.push(e);
            }
            mem.push(r);
        }
        let mut cim = Element::new("class_innate_memorized");
        for (b, name) in books.iter().enumerate().skip(2) {
            let list = cx.spells_of(0, name, 0);
            if list.is_empty() {
                continue;
            }
            let mut bk = Element::new("spellbook")
                .attr("number", b)
                .attr("name", name);
            for e in list {
                bk.push(e);
            }
            cim.push(bk);
        }
        mem.push(cim);
    }
    for (b, name) in books.iter().enumerate().skip(2) {
        let mut bk = Element::new("spellbook")
            .attr("number", b)
            .attr("name", name);
        for ci in first..first + class_count {
            let mut ce = Element::new("class")
                .attr("number", ci)
                .attr("spelllistclass", cx.class_name(ci));
            for lvl in 0..=9 {
                let list = if cx.sp.casters.get(ci).is_some() {
                    cx.spells_of(ci, name, lvl)
                } else {
                    Vec::new()
                };
                let mut le = Element::new("level")
                    .attr("number", lvl)
                    .attr("spellcount", list.len());
                for e in list {
                    le.push(e);
                }
                ce.push(le);
            }
            bk.push(ce);
        }
        mem.push(bk);
    }
    out.push(mem);
    let _ = owner_class;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spell_text_parts() {
        let cl = |f: &str| -> Option<f64> {
            match f {
                "CASTERLEVEL" => Some(3.0),
                "CASTERLEVEL/4+1" => Some(1.75),
                "min(6,(CASTERLEVEL/3))" => Some(0.0),
                _ => None,
            }
        };
        assert_eq!(spell_text("(CASTERLEVEL) minutes", &cl), "3 minutes");
        assert_eq!(spell_text("1 round (D)", &cl), "1 round [D]");
        assert_eq!(spell_text("x (CASTERLEVEL/4+1)", &cl), "x 1");
        assert_eq!(spell_text("+(min(6,(CASTERLEVEL/3)))", &cl), "+0");
        assert_eq!(spell_text("(up to (CASTERLEVEL))", &cl), "[up to 3]");
        assert_eq!(spell_text("no parens", &cl), "no parens");
        assert_eq!(spell_text("unclosed (oops", &cl), "unclosed (oops");
    }
}
