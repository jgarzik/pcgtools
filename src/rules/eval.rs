//
// rules/eval.rs -- formula evaluation
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtoolssoftware project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! Evaluates [`Expr`] trees.  Names and data-specific functions are
//! resolved through an [`Env`]; arithmetic, comparisons, and the
//! common math functions are built in.

use crate::token::formula::{BinOp, Expr, UnOp};

/// Supplies variable values and data-dependent functions.
pub trait Env {
    /// Value of a variable or term (`STR`, `CL=Mage`, `TL`, ...).
    fn var(&self, name: &str) -> Option<f64>;

    /// Value of a function the evaluator does not know.  `args` are
    /// the unevaluated arguments; string arguments are common.
    fn call(&self, name: &str, args: &[Expr]) -> Option<f64> {
        let _ = (name, args);
        None
    }

    /// Called for names that resolve to nothing (evaluate as zero).
    fn unknown(&self, name: &str) {
        let _ = name;
    }
}

fn truth(x: f64) -> bool {
    x != 0.0
}

fn b(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
    }
}

/// Evaluate an expression.  Unknown names count as zero.
pub fn eval(e: &Expr, env: &dyn Env) -> f64 {
    match e {
        Expr::Num(n) => *n,
        Expr::Str(s) => s.parse().unwrap_or(0.0),
        Expr::Var(v) => env.var(v).unwrap_or_else(|| {
            env.unknown(v);
            0.0
        }),
        Expr::Tuple(items) => items.iter().map(|x| eval(x, env)).fold(f64::MIN, f64::max),
        Expr::Unary(op, x) => {
            let v = eval(x, env);
            match op {
                UnOp::Neg => -v,
                UnOp::Plus => v,
                UnOp::Not => b(!truth(v)),
            }
        }
        Expr::Binary(op, l, r) => {
            // short-circuit logic
            if *op == BinOp::And {
                return b(truth(eval(l, env)) && truth(eval(r, env)));
            }
            if *op == BinOp::Or {
                return b(truth(eval(l, env)) || truth(eval(r, env)));
            }
            let (x, y) = (eval(l, env), eval(r, env));
            match op {
                BinOp::Add => x + y,
                BinOp::Sub => x - y,
                BinOp::Mul => x * y,
                BinOp::Div => {
                    if y == 0.0 {
                        0.0
                    } else {
                        x / y
                    }
                }
                BinOp::Rem => {
                    if y == 0.0 {
                        0.0
                    } else {
                        x % y
                    }
                }
                BinOp::Pow => x.powf(y),
                BinOp::Eq => b((x - y).abs() < 1e-9),
                BinOp::Ne => b((x - y).abs() >= 1e-9),
                BinOp::Lt => b(x < y),
                BinOp::Gt => b(x > y),
                BinOp::Le => b(x <= y),
                BinOp::Ge => b(x >= y),
                BinOp::And | BinOp::Or => unreachable!(),
            }
        }
        Expr::Call(name, args) => call(name, args, env),
    }
}

/// Flatten tuple arguments, so `max((a,b,c))` behaves like `max(a,b,c)`.
fn values(args: &[Expr], env: &dyn Env) -> Vec<f64> {
    let mut out = Vec::new();
    for a in args {
        match a {
            Expr::Tuple(items) => out.extend(items.iter().map(|x| eval(x, env))),
            other => out.push(eval(other, env)),
        }
    }
    out
}

fn call(name: &str, args: &[Expr], env: &dyn Env) -> f64 {
    let lname = name.to_ascii_lowercase();
    let v = |i: usize| args.get(i).map(|a| eval(a, env)).unwrap_or(0.0);
    match lname.as_str() {
        "floor" => v(0).floor(),
        "ceil" => v(0).ceil(),
        "round" => v(0).round(),
        "abs" => v(0).abs(),
        "trunc" | "intval" => v(0).trunc(),
        "sqrt" => v(0).max(0.0).sqrt(),
        "min" => values(args, env).into_iter().fold(f64::INFINITY, f64::min),
        "max" => values(args, env)
            .into_iter()
            .fold(f64::NEG_INFINITY, f64::max),
        "if" => {
            if truth(v(0)) {
                v(1)
            } else {
                v(2)
            }
        }
        // the d20 ability modifier
        "d20mod" => (v(0) / 2.0).floor() - 5.0,
        _ => env.call(name, args).unwrap_or_else(|| {
            env.unknown(&format!("{}()", name));
            0.0
        }),
    }
}

/// Evaluate formula text; unparseable text evaluates to zero.
pub fn eval_str(s: &str, env: &dyn Env) -> f64 {
    match Expr::parse(s) {
        Ok(e) => eval(&e, env),
        Err(_) => {
            env.unknown(s);
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    struct T {
        vars: HashMap<&'static str, f64>,
        unknown: RefCell<Vec<String>>,
    }

    impl Env for T {
        fn var(&self, n: &str) -> Option<f64> {
            self.vars.get(n).copied()
        }
        fn call(&self, n: &str, args: &[Expr]) -> Option<f64> {
            match (n, args.first()) {
                ("classlevel", Some(Expr::Str(c))) if c == "Mage" => Some(5.0),
                ("var", Some(Expr::Str(v))) => self.var(v),
                _ => None,
            }
        }
        fn unknown(&self, n: &str) {
            self.unknown.borrow_mut().push(n.to_string());
        }
    }

    fn env() -> T {
        T {
            vars: HashMap::from([("STR", 3.0), ("CL", 4.0), ("CL=Mage", 5.0), ("SCORE", 15.0)]),
            unknown: RefCell::new(Vec::new()),
        }
    }

    fn ev(s: &str) -> f64 {
        eval_str(s, &env())
    }

    #[test]
    fn arithmetic_table() {
        let cases: &[(&str, f64)] = &[
            ("1+2*3", 7.0),
            ("(1+2)*3", 9.0),
            ("7/2", 3.5),
            ("7%3", 1.0),
            ("2^3", 8.0),
            ("-2^2", -4.0),
            ("10-4-3", 3.0),
            ("1/0", 0.0),
            ("STR*2", 6.0),
            ("CL*3/4", 3.0),
            ("CL=Mage+1", 6.0),
            ("floor(SCORE/2)-5", 2.0),
            ("d20Mod(SCORE)", 2.0),
            ("d20Mod(8)", -1.0),
            ("floor(-0.5)", -1.0),
            ("ceil(1.2)", 2.0),
            ("round(2.5)", 3.0),
            ("abs(-3)", 3.0),
            ("min(4,STR,9)", 3.0),
            ("max(0,min(4,CL))", 4.0),
            ("max((1,5,2))", 5.0),
            ("if(CL>3,10,20)", 10.0),
            ("if(CL>9,10,20)", 20.0),
            ("1+(CL>=2)+(CL>=4)+(CL>=6)", 3.0),
            ("CL==4", 1.0),
            ("CL!=4", 0.0),
            ("CL=4", 0.0),
            ("!0", 1.0),
            ("1&&0", 0.0),
            ("1||0", 1.0),
            ("TRUNC(3.7)", 3.0),
            ("max(3-STR,0)", 0.0),
            ("classlevel(\"Mage\")", 5.0),
            ("var(\"STR\")", 3.0),
            ("\"4\"", 4.0),
        ];
        for (s, want) in cases {
            assert!(
                (ev(s) - want).abs() < 1e-9,
                "{} = {} (want {})",
                s,
                ev(s),
                want
            );
        }
    }

    #[test]
    fn unknowns_are_zero_and_reported() {
        let e = env();
        assert_eq!(eval_str("NOPE+1", &e), 1.0);
        assert_eq!(eval_str("mystery(1)", &e), 0.0);
        assert_eq!(eval_str("1+", &e), 0.0);
        assert_eq!(*e.unknown.borrow(), vec!["NOPE", "mystery()", "1+"]);
    }
}
