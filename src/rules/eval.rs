//
// rules/eval.rs -- formula evaluation
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! Evaluates [`Expr`] trees.  Names and data-specific functions are
//! resolved through an [`Env`]; arithmetic, comparisons, and the
//! common math functions are built in.

use std::cell::{Cell, RefCell};

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

    /// Would PCGen find a value for this name?  A formula naming one it
    /// would not is read by [`eval_str`]'s fallback.  This can hold for
    /// a name [`Env::var`] has no value for (one PCGen knows but this
    /// crate does not work out).
    fn resolves(&self, name: &str) -> bool {
        self.var(name).is_some()
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

/// Evaluate formula text the way PCGen does.  A formula that parses
/// and whose every name resolves is evaluated normally.  Otherwise
/// (an unknown name, even in a branch not taken, an unknown function,
/// or text that does not parse) PCGen falls back to its older reader,
/// reproduced by `legacy`: unknown names are reported as usual and
/// then count as zero, and operators no longer have precedence.
/// (PCGen also caches the result; that is not modeled.)
pub fn eval_str(s: &str, env: &dyn Env) -> f64 {
    eval_text(s, env, 0)
}

/// Deepest nesting of [`legacy`] readings within one formula.
const MAX_DEPTH: usize = 32;

fn eval_text(s: &str, env: &dyn Env, depth: usize) -> f64 {
    if let Some(n) = number(s) {
        return n;
    }
    match Expr::parse(s) {
        Ok(e) => {
            let probe = Probe {
                env,
                no_value: Default::default(),
                no_function: Default::default(),
                reported: Default::default(),
            };
            let v = eval(&e, &probe);
            let fallback = probe.no_function.get()
                || probe.no_value.borrow().iter().any(|n| !env.resolves(n))
                // names evaluation skipped matter only where it can skip
                || (branches(&e) && unresolved(&e, env));
            for n in probe.reported.borrow().iter() {
                env.unknown(n);
            }
            if !fallback {
                return v;
            }
        }
        Err(_) => env.unknown(s),
    }
    legacy(s, &Quiet(env), depth)
}

/// A plain number, as the whole text.
fn number(s: &str) -> Option<f64> {
    s.trim().parse().ok().filter(|n: &f64| n.is_finite())
}

/// Passes lookups through, noting what has no value and holding back
/// what would be reported.
struct Probe<'e> {
    env: &'e dyn Env,
    no_value: RefCell<Vec<String>>,
    no_function: Cell<bool>,
    reported: RefCell<Vec<String>>,
}

impl Env for Probe<'_> {
    fn var(&self, name: &str) -> Option<f64> {
        let v = self.env.var(name);
        if v.is_none() {
            self.no_value.borrow_mut().push(name.to_string());
        }
        v
    }
    fn call(&self, name: &str, args: &[Expr]) -> Option<f64> {
        let v = self.env.call(name, args);
        if v.is_none() && !PCGEN_FUNCTIONS.contains(&name.to_ascii_lowercase().as_str()) {
            self.no_function.set(true);
        }
        v
    }
    fn unknown(&self, name: &str) {
        self.reported.borrow_mut().push(name.to_string());
    }
}

/// Passes lookups through and reports nothing.
struct Quiet<'e>(&'e dyn Env);

impl Env for Quiet<'_> {
    fn var(&self, name: &str) -> Option<f64> {
        self.0.var(name)
    }
    fn call(&self, name: &str, args: &[Expr]) -> Option<f64> {
        self.0.call(name, args)
    }
    fn resolves(&self, name: &str) -> bool {
        self.0.resolves(name)
    }
}

/// Functions [`call`] evaluates itself; the arguments of any other are
/// handed to the environment unevaluated.
const BUILTINS: &[&str] = &[
    "floor", "ceil", "round", "abs", "trunc", "intval", "sqrt", "min", "max", "if", "d20mod",
];

/// Functions PCGen's formula parser knows, whether or not this crate
/// works them out; only a call to another one makes it fall back.
const PCGEN_FUNCTIONS: &[&str] = &[
    "abs",
    "acos",
    "asin",
    "atan",
    "ceil",
    "charbonusto",
    "classlevel",
    "cos",
    "count",
    "countdistinct",
    "exp",
    "floor",
    "getvar",
    "if",
    "ln",
    "log",
    "mastervar",
    "max",
    "min",
    "mod",
    "or",
    "rand",
    "roll",
    "round",
    "sin",
    "skillinfo",
    "sqrt",
    "sum",
    "tan",
    "var",
];

/// Can evaluation skip part of the expression (`if`, `&&`, `||`)?
fn branches(e: &Expr) -> bool {
    match e {
        Expr::Num(_) | Expr::Str(_) | Expr::Var(_) => false,
        Expr::Tuple(items) => items.iter().any(branches),
        Expr::Unary(_, x) => branches(x),
        Expr::Binary(op, l, r) => {
            matches!(op, BinOp::And | BinOp::Or) || branches(l) || branches(r)
        }
        Expr::Call(name, args) => name.eq_ignore_ascii_case("if") || args.iter().any(branches),
    }
}

/// Does a name anywhere in the expression fail to resolve?  The whole
/// expression counts, not just the branches evaluation took.
fn unresolved(e: &Expr, env: &dyn Env) -> bool {
    match e {
        Expr::Num(_) | Expr::Str(_) => false,
        // constants the formula parser knows itself
        Expr::Var(v) if matches!(v.as_str(), "TRUE" | "FALSE" | "e" | "pi") => false,
        Expr::Var(v) => !env.resolves(v),
        Expr::Tuple(items) => items.iter().any(|x| unresolved(x, env)),
        Expr::Unary(_, x) => unresolved(x, env),
        Expr::Binary(_, l, r) => unresolved(l, env) || unresolved(r, env),
        Expr::Call(name, args) => {
            BUILTINS.contains(&name.to_ascii_lowercase().as_str())
                && args.iter().any(|x| unresolved(x, env))
        }
    }
}

/// PCGen's older formula reader.  The text is upper-cased; the
/// innermost parenthesized part is evaluated as a formula of its own
/// (normally if it can be) and its value written in its place, until
/// no parenthesis is left.  Text of the form `.IF.a.OP.b.THEN.c.ELSE.d.`
/// (OP one of LT, LTEQ, EQ, GT, GTEQ) picks c or d.  Anything else is
/// split at `+ - * /` and worked out strictly left to right, with each
/// piece a name's value, a number, or zero.
fn legacy(s: &str, env: &dyn Env, depth: usize) -> f64 {
    if depth > MAX_DEPTH {
        return 0.0;
    }
    let mut s = s.to_uppercase();
    while let Some(open) = innermost_open(&s) {
        // the last `)` closing at the deepest level
        let Some(close) = innermost_close(&s) else {
            return 0.0;
        };
        if close <= open {
            return 0.0;
        }
        let v = eval_text(&s[open + 1..close], env, depth + 1);
        s = format!("{}{}{}", &s[..open], v, &s[close + 1..]);
    }
    if let Some(rest) = s.strip_prefix(".IF.") {
        if let Some(v) = dotted_if(rest, env, depth) {
            return v;
        }
    }
    left_to_right(&s, env)
}

/// Nesting depth of each parenthesis, and the deepest.
fn depths(s: &str) -> impl Iterator<Item = (usize, u8, i64)> + '_ {
    let mut d = 0i64;
    s.bytes().enumerate().filter_map(move |(i, c)| match c {
        b'(' => {
            d += 1;
            Some((i, c, d))
        }
        b')' => {
            d -= 1;
            Some((i, c, d + 1))
        }
        _ => None,
    })
}

/// The last `(` reaching the deepest nesting, if any `(` remains.
fn innermost_open(s: &str) -> Option<usize> {
    let mut best = None;
    let mut hi = 0;
    for (i, c, d) in depths(s) {
        if c == b'(' && d >= hi {
            hi = d;
            best = Some(i);
        }
    }
    best
}

/// The last `)` closing the deepest nesting reached so far.
fn innermost_close(s: &str) -> Option<usize> {
    let mut best = None;
    let mut hi = 0;
    for (i, c, d) in depths(s) {
        if c == b'(' {
            hi = hi.max(d);
        } else if d == hi {
            best = Some(i);
        }
    }
    best
}

/// `a.OP.b.THEN.c.ELSE.d.` (after `.IF.`); `None` unless the condition
/// and both outcomes are all there.
fn dotted_if(rest: &str, env: &dyn Env, depth: usize) -> Option<f64> {
    // pieces split at `.`, each `.` a piece of its own
    let mut pieces = Vec::new();
    let mut start = 0;
    for (i, _) in rest.match_indices('.') {
        if i > start {
            pieces.push(&rest[start..i]);
        }
        pieces.push(".");
        start = i + 1;
    }
    if start < rest.len() {
        pieces.push(&rest[start..]);
    }
    let sub = |part: &str| {
        // drop the `.` before the keyword
        let part = part.strip_suffix('.').unwrap_or(part);
        eval_text(part, env, depth + 1)
    };
    let (mut lhs, mut rhs, mut then) = (None, None, None);
    let mut op = "";
    let mut part = String::new();
    let mut it = pieces.into_iter();
    while let Some(p) = it.next() {
        match p {
            "LT" | "LTEQ" | "EQ" | "GT" | "GTEQ" => {
                lhs = Some(sub(&part));
                op = p;
            }
            "THEN" => rhs = Some(sub(&part)),
            "ELSE" => then = Some(sub(&part)),
            _ => {
                part.push_str(p);
                continue;
            }
        }
        // the `.` after the keyword
        it.next();
        part.clear();
    }
    let (a, b, t) = (lhs?, rhs?, then?);
    let holds = match op {
        "LT" => a < b,
        "LTEQ" => a <= b,
        "EQ" => (a - b).abs() < 0.0001,
        "GT" => a > b,
        _ => a >= b,
    };
    // the closing `.` stays on the last outcome
    Some(if holds {
        t
    } else {
        eval_text(&part, env, depth + 1)
    })
}

/// Work out `+ - * /` strictly left to right.  A sign directly after an
/// operator (or at the start) belongs to the number that follows.
fn left_to_right(s: &str, env: &dyn Env) -> f64 {
    let is_op = |c: char| matches!(c, '+' | '-' | '*' | '/');
    let mut total = 0.0;
    let mut op = '+';
    let mut piece = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        piece.push(c);
        let last = chars.peek().is_none();
        if !last && !is_op(c) {
            continue;
        }
        if is_op(c) && piece.len() == 1 {
            continue;
        }
        if is_op(c) {
            piece.pop();
        }
        if !piece.is_empty() {
            let v = env.var(&piece).or_else(|| number(&piece)).unwrap_or(0.0);
            match op {
                '+' => total += v,
                '-' => total -= v,
                '*' => total *= v,
                // dividing by zero gives zero, as in `eval`
                _ => total = if v == 0.0 { 0.0 } else { total / v },
            }
        }
        op = if is_op(c) { c } else { '+' };
        piece.clear();
    }
    total
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
            // names are case-insensitive, as they are for a character
            self.vars
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(n))
                .map(|(_, v)| *v)
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
        // unparseable text goes to the left-to-right reading too
        assert_eq!(eval_str("1+", &e), 1.0);
        assert_eq!(*e.unknown.borrow(), vec!["NOPE", "mystery()", "1+"]);
    }

    fn close(s: &str, got: f64, want: f64) {
        assert!((got - want).abs() < 1e-9, "{} = {} (want {})", s, got, want);
    }

    #[test]
    fn unresolved_formulas_read_left_to_right() {
        let cases: &[(&str, f64)] = &[
            // a resolved formula keeps operator precedence
            ("1+2*3", 7.0),
            ("STR+2*3", 9.0),
            // any unresolved name drops it
            ("1+2*3+NOPE", 9.0),
            ("NOPE+1+2*3", 9.0),
            ("2+3*NOPE", 0.0),
            ("NOPE+2*3", 6.0),
            ("10-NOPE-3", 7.0),
            ("NOPE+7/2", 3.5),
            ("NOPE+8/0", 0.0),
            ("NOPE*2+5", 5.0),
            // a sign right after an operator belongs to the number
            ("NOPE+-2*3", -6.0),
            ("-2+NOPE", -2.0),
            ("NOPE-+2", -2.0),
            // names are read upper-cased
            ("str+nope*2", 6.0),
            ("cl=mage*2+nope", 10.0),
            // innermost parentheses first, each as a formula of its own
            ("NOPE+3+(STR-NOPE)/4", 1.5),
            ("(NOPE+1)+2*(1+2)", 9.0),
            ("((NOPE+2)*3)+1*2", 14.0),
            ("(STR*2+1)+NOPE*2", 14.0),
            // a function's name is left glued to its value: zero
            ("max(NOPE,5)+1", 1.0),
            ("floor(STR/2)+NOPE", 0.0),
            ("mystery(1)+2*3", 6.0),
            ("1+2*mystery(1)+3", 3.0),
            // a function PCGen knows does not, though it has no value here
            ("1+2*mastervar(\"X\")+3", 4.0),
            // also when only an untaken branch is unresolved
            ("if(CL>3,1,NOPE)+2*3", 6.0),
            ("max(0,min(4,CL))+2*3", 10.0),
            // unbalanced parentheses give zero
            ("NOPE+(1", 0.0),
            ("(1", 0.0),
            ("NOPE+1)", 0.0),
            // unparseable text
            ("1+", 1.0),
            ("3 +", 3.0),
            ("2 3", 0.0),
            ("  4 ", 4.0),
            ("", 0.0),
        ];
        for (s, want) in cases {
            close(s, ev(s), *want);
        }
    }

    #[test]
    fn dotted_if() {
        let cases: &[(&str, f64)] = &[
            (".IF.STR.GT.2.THEN.10.ELSE.20.", 10.0),
            (".IF.STR.GT.3.THEN.10.ELSE.20.", 20.0),
            (".IF.STR.GTEQ.3.THEN.10.ELSE.20.", 10.0),
            (".IF.STR.LT.3.THEN.10.ELSE.20.", 20.0),
            (".IF.STR.LT.4.THEN.10.ELSE.20.", 10.0),
            (".IF.STR.LTEQ.3.THEN.10.ELSE.20.", 10.0),
            (".IF.STR.LTEQ.2.THEN.10.ELSE.20.", 20.0),
            (".IF.STR.EQ.3.THEN.10.ELSE.20.", 10.0),
            (".IF.STR.EQ.3.00001.THEN.10.ELSE.20.", 10.0),
            (".IF.STR.EQ.3.01.THEN.10.ELSE.20.", 20.0),
            (".if.str.gt.2.then.10.else.20.", 10.0),
            // parts are formulas; decimals survive the dotted split
            (".IF.STR+1.EQ.2*2.THEN.CL*2.ELSE.0.", 8.0),
            (".IF.(STR/2).EQ.1.5.THEN.1.ELSE.0.", 1.0),
            (".IF.(STR+1).GT.(CL-1).THEN.(CL*CL).ELSE.0.", 16.0),
            // the closing dot stays on the last part, so a name there
            // reads as nothing
            (".IF.STR.LT.2.THEN.1.ELSE.STR.", 0.0),
            (".IF.STR.LT.2.THEN.1.ELSE.7.", 7.0),
            // without THEN and ELSE it is plain left-to-right text
            (".IF.1.GT.0.THEN.5", 0.0),
            (".IF.1.GT.0", 0.0),
        ];
        for (s, want) in cases {
            close(s, ev(s), *want);
        }
    }

    #[test]
    fn fallback_reports_unknowns_as_evaluated() {
        // each use evaluated is reported once, as without the fallback;
        // the reading itself reports nothing more
        let e = env();
        close("x", eval_str("NOPE+3+(STR-NOPE)/4", &e), 1.5);
        assert_eq!(*e.unknown.borrow(), vec!["NOPE", "NOPE"]);
        close("y", eval_str("if(CL>3,1,NOPE)+2", &e), 2.0);
        assert_eq!(e.unknown.borrow().len(), 2);
    }

    #[test]
    fn deep_nesting_is_bounded() {
        let deep = format!("NOPE+{}1{}", "(".repeat(200), ")".repeat(200));
        close("deep", ev(&deep), 1.0);
        let dots = ".IF.".repeat(2000);
        close("dots", ev(&dots), 0.0);
    }
}
