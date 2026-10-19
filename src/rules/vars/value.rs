//
// rules/vars/value.rs -- typed values of declared variables
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! Declared variables have a *format*: numbers, booleans, strings,
//! ordered pairs, dice, handedness, references to data objects, and
//! arrays or optionals of those.

use serde::Serialize;
use std::fmt;

/// The format (type) of a variable.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub enum Format {
    Number,
    Boolean,
    String,
    OrderedPair,
    Dice,
    Handed,
    /// a data object of a kind: STAT, SKILL, ALIGNMENT, a dynamic
    /// category, ...
    Object(String),
    Array(Box<Format>),
    Optional(Box<Format>),
}

impl Format {
    /// Parse a format name as written in data.
    pub fn parse(s: &str) -> Option<Format> {
        let u = s.trim().to_ascii_uppercase();
        for (prefix, wrap) in [("ARRAY[", 0), ("OPTIONAL[", 1)] {
            if let Some(inner) = u.strip_prefix(prefix).and_then(|x| x.strip_suffix(']')) {
                let f = Box::new(Format::parse(inner)?);
                return Some(if wrap == 0 {
                    Format::Array(f)
                } else {
                    Format::Optional(f)
                });
            }
        }
        Some(match u.as_str() {
            "NUMBER" => Format::Number,
            "BOOLEAN" => Format::Boolean,
            "STRING" => Format::String,
            "ORDEREDPAIR" => Format::OrderedPair,
            "DICE" => Format::Dice,
            "HANDED" => Format::Handed,
            "" => return None,
            other if other.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {
                Format::Object(other.to_string())
            }
            _ => return None,
        })
    }

    /// Fixed formats take literal values only (no formulas).
    pub fn is_fixed(&self) -> bool {
        matches!(
            self,
            Format::Boolean
                | Format::String
                | Format::OrderedPair
                | Format::Dice
                | Format::Array(_)
        )
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Format::Number => f.write_str("NUMBER"),
            Format::Boolean => f.write_str("BOOLEAN"),
            Format::String => f.write_str("STRING"),
            Format::OrderedPair => f.write_str("ORDEREDPAIR"),
            Format::Dice => f.write_str("DICE"),
            Format::Handed => f.write_str("HANDED"),
            Format::Object(k) => f.write_str(k),
            Format::Array(x) => write!(f, "ARRAY[{}]", x),
            Format::Optional(x) => write!(f, "OPTIONAL[{}]", x),
        }
    }
}

/// A value.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Pair(f64, f64),
    Dice(String),
    /// an object reference: (format, key)
    Obj(String, String),
    Array(Vec<Value>),
    Opt(Option<Box<Value>>),
}

fn num_text(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(i) => write!(f, "{}", i),
            Value::Float(x) => f.write_str(&num_text(*x)),
            Value::Bool(b) => f.write_str(if *b { "true" } else { "false" }),
            Value::Str(s) | Value::Dice(s) => f.write_str(s),
            Value::Pair(a, b) => write!(f, "{},{}", num_text(*a), num_text(*b)),
            Value::Obj(_, k) => f.write_str(k),
            Value::Array(v) => {
                let parts: Vec<String> = v.iter().map(|x| x.to_string()).collect();
                f.write_str(&parts.join(","))
            }
            Value::Opt(None) => Ok(()),
            Value::Opt(Some(v)) => write!(f, "{}", v),
        }
    }
}

impl Value {
    /// Numeric view (booleans are 1/0; others 0).
    pub fn as_f64(&self) -> f64 {
        match self {
            Value::Int(i) => *i as f64,
            Value::Float(x) => *x,
            Value::Bool(b) => f64::from(u8::from(*b)),
            Value::Str(s) => s.trim().parse().unwrap_or(0.0),
            Value::Opt(Some(v)) => v.as_f64(),
            _ => 0.0,
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Str(s) => s.eq_ignore_ascii_case("true"),
            Value::Opt(o) => o.is_some(),
            Value::Array(a) => !a.is_empty(),
            other => other.as_f64() != 0.0,
        }
    }

    /// A number, kept integral when it is.
    pub fn number(x: f64) -> Value {
        if x.fract() == 0.0 && x.abs() < 9.0e15 {
            Value::Int(x as i64)
        } else {
            Value::Float(x)
        }
    }

    /// The default value of a format when no data sets one.
    pub fn default_for(f: &Format) -> Value {
        match f {
            Format::Number => Value::Int(0),
            Format::Boolean => Value::Bool(false),
            Format::String => Value::Str(String::new()),
            Format::OrderedPair => Value::Pair(0.0, 0.0),
            Format::Dice => Value::Dice("0".into()),
            Format::Handed => Value::Str("Right".into()),
            Format::Object(k) => Value::Obj(k.clone(), String::new()),
            Format::Array(_) => Value::Array(Vec::new()),
            Format::Optional(_) => Value::Opt(None),
        }
    }

    /// Parse a literal written in data for a format.
    pub fn parse_literal(f: &Format, s: &str) -> Option<Value> {
        let t = s.trim();
        Some(match f {
            Format::Number => {
                if let Ok(i) = t.parse::<i64>() {
                    Value::Int(i)
                } else {
                    Value::Float(t.parse().ok()?)
                }
            }
            Format::Boolean => match t.to_ascii_lowercase().as_str() {
                "true" | "yes" => Value::Bool(true),
                "false" | "no" => Value::Bool(false),
                _ => return None,
            },
            Format::String => Value::Str(t.to_string()),
            Format::OrderedPair => {
                let (a, b) = t.split_once(',')?;
                Value::Pair(a.trim().parse().ok()?, b.trim().parse().ok()?)
            }
            Format::Dice => Value::Dice(t.to_string()),
            Format::Handed => Value::Str(t.to_string()),
            Format::Object(k) => Value::Obj(k.clone(), t.to_string()),
            Format::Array(inner) => {
                if t.is_empty() {
                    Value::Array(Vec::new())
                } else {
                    Value::Array(
                        t.split(',')
                            .map(|x| Value::parse_literal(inner, x))
                            .collect::<Option<Vec<_>>>()?,
                    )
                }
            }
            Format::Optional(inner) => {
                if t.is_empty() {
                    Value::Opt(None)
                } else {
                    Value::Opt(Some(Box::new(Value::parse_literal(inner, t)?)))
                }
            }
        })
    }
}

/// Arithmetic that keeps integers integral where exact.
pub fn arith(op: char, a: &Value, b: &Value) -> Value {
    if let (Value::Int(x), Value::Int(y)) = (a, b) {
        match op {
            '+' => return Value::Int(x.wrapping_add(*y)),
            '-' => return Value::Int(x.wrapping_sub(*y)),
            '*' => return Value::Int(x.wrapping_mul(*y)),
            '/' if *y != 0 && x % y == 0 => return Value::Int(x / y),
            '%' if *y != 0 => return Value::Int(x % y),
            _ => {}
        }
    }
    let (x, y) = (a.as_f64(), b.as_f64());
    match op {
        '+' => {
            if let (Value::Str(p), Value::Str(q)) = (a, b) {
                return Value::Str(format!("{}{}", p, q));
            }
            Value::number(x + y)
        }
        '-' => Value::number(x - y),
        '*' => Value::number(x * y),
        '/' => {
            if y == 0.0 {
                Value::Int(0)
            } else {
                Value::number(x / y)
            }
        }
        '%' => {
            if y == 0.0 {
                Value::Int(0)
            } else {
                Value::number(x % y)
            }
        }
        '^' => Value::Float(x.powf(y)),
        _ => Value::Int(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        let cases: &[(&str, Format)] = &[
            ("NUMBER", Format::Number),
            ("number", Format::Number),
            ("BOOLEAN", Format::Boolean),
            ("STRING", Format::String),
            ("ORDEREDPAIR", Format::OrderedPair),
            ("DICE", Format::Dice),
            ("HANDED", Format::Handed),
            ("STAT", Format::Object("STAT".into())),
            ("MOVEMENT", Format::Object("MOVEMENT".into())),
            ("ARRAY[NUMBER]", Format::Array(Box::new(Format::Number))),
            (
                "OPTIONAL[STAT]",
                Format::Optional(Box::new(Format::Object("STAT".into()))),
            ),
            (
                "ARRAY[ARRAY[STRING]]",
                Format::Array(Box::new(Format::Array(Box::new(Format::String)))),
            ),
        ];
        for (s, f) in cases {
            assert_eq!(Format::parse(s).as_ref(), Some(f), "{}", s);
            assert_eq!(Format::parse(&f.to_string()).as_ref(), Some(f));
        }
        for bad in ["", "ARRAY[", "A-B", "ARRAY[]"] {
            assert_eq!(Format::parse(bad), None, "{}", bad);
        }
        assert!(Format::Boolean.is_fixed());
        assert!(!Format::Number.is_fixed());
        assert!(!Format::Object("STAT".into()).is_fixed());
    }

    #[test]
    fn literals() {
        let n = Format::Number;
        assert_eq!(Value::parse_literal(&n, "5"), Some(Value::Int(5)));
        assert_eq!(Value::parse_literal(&n, " -2 "), Some(Value::Int(-2)));
        assert_eq!(Value::parse_literal(&n, "2.5"), Some(Value::Float(2.5)));
        assert_eq!(Value::parse_literal(&n, "x"), None);
        assert_eq!(
            Value::parse_literal(&Format::Boolean, "True"),
            Some(Value::Bool(true))
        );
        assert_eq!(Value::parse_literal(&Format::Boolean, "maybe"), None);
        assert_eq!(
            Value::parse_literal(&Format::OrderedPair, "5,10"),
            Some(Value::Pair(5.0, 10.0))
        );
        assert_eq!(Value::parse_literal(&Format::OrderedPair, "5"), None);
        assert_eq!(
            Value::parse_literal(&Format::Array(Box::new(Format::Number)), "1,2"),
            Some(Value::Array(vec![Value::Int(1), Value::Int(2)]))
        );
        assert_eq!(
            Value::parse_literal(&Format::Optional(Box::new(Format::Number)), ""),
            Some(Value::Opt(None))
        );
        assert_eq!(
            Value::parse_literal(&Format::Dice, "2d6"),
            Some(Value::Dice("2d6".into()))
        );
    }

    #[test]
    fn defaults_and_display() {
        assert_eq!(Value::default_for(&Format::Number).to_string(), "0");
        assert_eq!(Value::default_for(&Format::Boolean).to_string(), "false");
        assert_eq!(Value::default_for(&Format::OrderedPair).to_string(), "0,0");
        assert_eq!(Value::Pair(5.0, 5.0).to_string(), "5,5");
        assert_eq!(Value::Float(2.5).to_string(), "2.5");
        assert_eq!(Value::Obj("STAT".into(), "STR".into()).to_string(), "STR");
    }

    #[test]
    fn arithmetic() {
        use Value::*;
        assert_eq!(arith('+', &Int(2), &Int(3)), Int(5));
        assert_eq!(arith('/', &Int(6), &Int(3)), Int(2));
        assert_eq!(arith('/', &Int(5), &Int(2)), Float(2.5));
        assert_eq!(arith('/', &Int(5), &Int(0)), Int(0));
        assert_eq!(arith('%', &Int(7), &Int(3)), Int(1));
        assert_eq!(arith('*', &Float(1.5), &Int(2)), Int(3));
        assert_eq!(arith('^', &Int(2), &Int(3)), Float(8.0));
        assert_eq!(
            arith('+', &Str("a".into()), &Str("b".into())),
            Str("ab".into())
        );
        assert!(Bool(true).truthy());
        assert!(!Int(0).truthy());
        assert!(Str("TRUE".into()).truthy());
    }
}
