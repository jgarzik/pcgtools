//
// token/formula.rs -- formula expressions
//
// Copyright (c) 2024 Jeff Garzik
//
// This file is part of the pcgtools software project covered under
// the MIT License.  For the full license text, please see the LICENSE
// file in the root directory of this project.
// SPDX-License-Identifier: MIT

//! Parser for the arithmetic formulas found in BONUS, DEFINE, and
//! similar tokens.  The language is a small infix expression grammar
//! with function calls; variable names may carry legacy suffixes such
//! as `CL=Fighter` or `COUNT[FEATTYPE=Metamagic]`.
//!
//! ```text
//! expr    = or ;
//! or      = and { "||" and } ;
//! and     = eq { "&&" eq } ;
//! eq      = rel { ("==" | "!=") rel } ;
//! rel     = add { ("<" | ">" | "<=" | ">=") add } ;
//! add     = mul { ("+" | "-") mul } ;
//! mul     = unary { ("*" | "/" | "%") unary } ;
//! unary   = ("-" | "+" | "!") unary | pow ;
//! pow     = atom [ "^" unary ] ;
//! atom    = number | string | name [ "(" [ expr { "," expr } ] ")" ]
//!         | "(" expr { "," expr } ")" ;
//! ```

use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Expr {
    Num(f64),
    Str(String),
    /// a variable or legacy term, e.g. `STR`, `CL=Fighter`,
    /// `COUNT[FEATS.ALL]`
    Var(String),
    Call(String, Vec<Expr>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// a parenthesized tuple `(a,b,...)` (seen as a lone argument to
    /// max/min in some data)
    Tuple(Vec<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum UnOp {
    Neg,
    Plus,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum BinOp {
    Or,
    And,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
}

impl BinOp {
    fn prec(self) -> u8 {
        match self {
            BinOp::Or => 1,
            BinOp::And => 2,
            BinOp::Eq | BinOp::Ne => 3,
            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => 4,
            BinOp::Add | BinOp::Sub => 5,
            BinOp::Mul | BinOp::Div | BinOp::Rem => 6,
            BinOp::Pow => 8,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            BinOp::Or => "||",
            BinOp::And => "&&",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Gt => ">",
            BinOp::Le => "<=",
            BinOp::Ge => ">=",
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::Pow => "^",
        }
    }
}

const UNARY_PREC: u8 = 7;

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Name(String),
    Postfix(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
}

/// A formula syntax error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormulaError {
    pub pos: usize,
    pub message: String,
}

impl fmt::Display for FormulaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "at {}: {}", self.pos, self.message)
    }
}

fn err<T>(pos: usize, m: impl Into<String>) -> Result<T, FormulaError> {
    Err(FormulaError {
        pos,
        message: m.into(),
    })
}

const OPS: [&str; 17] = [
    "||", "&&", "==", "!=", "<=", ">=", "<", ">", "+", "-", "*", "/", "%", "^", "!", "=", "&",
];

fn is_name_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '~' | '%' | '\'' | ':')
}

fn lex(s: &str) -> Result<Vec<(usize, Tok)>, FormulaError> {
    let b: Vec<(usize, char)> = s.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let (pos, c) = b[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && b.get(i + 1).is_some_and(|x| x.1.is_ascii_digit())) {
            let start = i;
            while i < b.len() && (b[i].1.is_ascii_digit() || b[i].1 == '.') {
                i += 1;
            }
            // digits running into letters form a name (e.g. "1stLevel")
            if i < b.len() && (b[i].1.is_alphabetic() || b[i].1 == '_') {
                i = start;
            } else {
                let text: String = b[start..i].iter().map(|x| x.1).collect();
                match text.parse::<f64>() {
                    Ok(n) => out.push((pos, Tok::Num(n))),
                    Err(_) => return err(pos, format!("bad number '{}'", text)),
                }
                continue;
            }
        }
        if c == '"' {
            let mut v = String::new();
            i += 1;
            loop {
                match b.get(i) {
                    None => return err(pos, "unterminated string"),
                    Some((_, '"')) => break,
                    Some((_, '\\')) if i + 1 < b.len() => {
                        v.push(b[i + 1].1);
                        i += 2;
                    }
                    Some((_, ch)) => {
                        v.push(*ch);
                        i += 1;
                    }
                }
            }
            i += 1;
            out.push((pos, Tok::Str(v)));
            continue;
        }
        let pct_name = c == '%' && b.get(i + 1).is_some_and(|x| x.1.is_alphabetic());
        if c.is_alphanumeric() || c == '_' || pct_name {
            let start = i;
            while i < b.len() && is_name_char(b[i].1) {
                i += 1;
            }
            // legacy term suffixes: NAME=value and NAME[...]
            loop {
                if i < b.len() && b[i].1 == '=' && b.get(i + 1).is_some_and(|x| x.1 != '=') {
                    // legacy NAME=value: the value may contain spaces
                    // and balanced parentheses, e.g. CL=Monk (Elder)
                    i += 1;
                    let mut depth = 0;
                    let mut last = i;
                    while i < b.len() {
                        let ch = b[i].1;
                        if ch == '(' {
                            depth += 1;
                        } else if ch == ')' {
                            if depth == 0 {
                                break;
                            }
                            depth -= 1;
                        } else if depth == 0 && "+-*/^,<>!&|".contains(ch) {
                            break;
                        }
                        i += 1;
                        if !ch.is_whitespace() {
                            last = i;
                        }
                    }
                    i = last;
                    continue;
                }
                if i < b.len() && b[i].1 == '[' {
                    let mut depth = 0;
                    while i < b.len() {
                        match b[i].1 {
                            '[' => depth += 1,
                            ']' => {
                                depth -= 1;
                                if depth == 0 {
                                    i += 1;
                                    break;
                                }
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                    if depth != 0 {
                        return err(pos, "unbalanced '['");
                    }
                    while i < b.len() && is_name_char(b[i].1) {
                        i += 1;
                    }
                    continue;
                }
                break;
            }
            let text: String = b[start..i].iter().map(|x| x.1).collect();
            out.push((pos, Tok::Name(text)));
            continue;
        }
        if c == '.' && b.get(i + 1).is_some_and(|x| x.1.is_alphabetic()) {
            // legacy postfix operator, e.g. (x).TRUNC
            let start = i + 1;
            i += 1;
            while i < b.len() && b[i].1.is_alphanumeric() {
                i += 1;
            }
            let text: String = b[start..i].iter().map(|x| x.1).collect();
            out.push((pos, Tok::Postfix(text)));
            continue;
        }
        match c {
            '(' => out.push((pos, Tok::LParen)),
            ')' => out.push((pos, Tok::RParen)),
            ',' => out.push((pos, Tok::Comma)),
            _ => {
                let rest = &s[pos..];
                match OPS.iter().find(|op| rest.starts_with(**op)) {
                    Some(op) => {
                        out.push((pos, Tok::Op(op)));
                        i += op.chars().count();
                        continue;
                    }
                    None => return err(pos, format!("unexpected character '{}'", c)),
                }
            }
        }
        i += 1;
    }
    Ok(out)
}

/// Split a legacy `MAXn` / `MINn` infix name into function and operand.
fn legacy_minmax(n: &str) -> Option<(&'static str, &str)> {
    let up = n.to_ascii_uppercase();
    for (k, f) in [("MAX", "max"), ("MIN", "min")] {
        if let Some(rest) = up.strip_prefix(k) {
            if rest.is_empty() || rest.starts_with(|c: char| c.is_ascii_digit()) {
                return Some((f, &n[3..]));
            }
        }
    }
    None
}

struct Parser {
    toks: Vec<(usize, Tok)>,
    i: usize,
    end: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i).map(|t| &t.1)
    }

    fn pos(&self) -> usize {
        self.toks.get(self.i).map(|t| t.0).unwrap_or(self.end)
    }

    fn binop(&self) -> Option<BinOp> {
        Some(match self.peek()? {
            Tok::Op("||") => BinOp::Or,
            Tok::Op("&&") | Tok::Op("&") => BinOp::And,
            Tok::Op("==") | Tok::Op("=") => BinOp::Eq,
            Tok::Op("!=") => BinOp::Ne,
            Tok::Op("<") => BinOp::Lt,
            Tok::Op(">") => BinOp::Gt,
            Tok::Op("<=") => BinOp::Le,
            Tok::Op(">=") => BinOp::Ge,
            Tok::Op("+") => BinOp::Add,
            Tok::Op("-") => BinOp::Sub,
            Tok::Op("*") => BinOp::Mul,
            Tok::Op("/") => BinOp::Div,
            Tok::Op("%") => BinOp::Rem,
            Tok::Op("^") => BinOp::Pow,
            _ => return None,
        })
    }

    /// precedence climbing
    fn expr(&mut self, min: u8) -> Result<Expr, FormulaError> {
        let mut lhs = self.unary()?;
        while let Some(op) = self.binop() {
            let p = op.prec();
            if p < min {
                break;
            }
            self.i += 1;
            // ^ is right associative
            let next = if op == BinOp::Pow { p } else { p + 1 };
            let rhs = if op == BinOp::Pow {
                self.unary()?
            } else {
                self.expr(next)?
            };
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr, FormulaError> {
        let op = match self.peek() {
            Some(Tok::Op("-")) => Some(UnOp::Neg),
            Some(Tok::Op("+")) => Some(UnOp::Plus),
            Some(Tok::Op("!")) => Some(UnOp::Not),
            _ => None,
        };
        if let Some(op) = op {
            self.i += 1;
            let e = self.expr(UNARY_PREC)?;
            return Ok(Expr::Unary(op, Box::new(e)));
        }
        let mut a = self.atom()?;
        loop {
            match self.peek() {
                Some(Tok::Postfix(n)) => {
                    let n = n.clone();
                    self.i += 1;
                    a = Expr::Call(n, vec![a]);
                }
                // legacy infix: xMAXy, e.g. (3-INT)MAX0 or (x)MIN4
                Some(Tok::Name(n)) if legacy_minmax(n).is_some() => {
                    let n = n.clone();
                    let (op, rest) = legacy_minmax(&n).unwrap();
                    self.i += 1;
                    let rhs = if rest.is_empty() {
                        self.atom()?
                    } else {
                        Expr::parse(rest).map_err(|e| FormulaError {
                            pos: self.pos(),
                            message: e.message,
                        })?
                    };
                    a = Expr::Call(op.into(), vec![a, rhs]);
                }
                _ => break,
            }
        }
        if let Some(Tok::Op("^")) = self.peek() {
            self.i += 1;
            let r = self.unary()?;
            return Ok(Expr::Binary(BinOp::Pow, Box::new(a), Box::new(r)));
        }
        Ok(a)
    }

    fn args(&mut self) -> Result<Vec<Expr>, FormulaError> {
        // after '('
        let mut args = Vec::new();
        if let Some(Tok::RParen) = self.peek() {
            self.i += 1;
            return Ok(args);
        }
        loop {
            args.push(self.expr(0)?);
            match self.peek() {
                Some(Tok::Comma) => self.i += 1,
                Some(Tok::RParen) => {
                    self.i += 1;
                    return Ok(args);
                }
                _ => return err(self.pos(), "expected ',' or ')'"),
            }
        }
    }

    fn atom(&mut self) -> Result<Expr, FormulaError> {
        let pos = self.pos();
        let t = match self.toks.get(self.i) {
            Some(t) => t.1.clone(),
            None => return err(pos, "unexpected end of formula"),
        };
        self.i += 1;
        match t {
            Tok::Num(n) => Ok(Expr::Num(n)),
            Tok::Str(s) => Ok(Expr::Str(s)),
            Tok::Name(n) => {
                if let Some(Tok::LParen) = self.peek() {
                    self.i += 1;
                    let a = self.args()?;
                    Ok(Expr::Call(n, a))
                } else {
                    Ok(Expr::Var(n))
                }
            }
            Tok::LParen => {
                let mut a = self.args()?;
                if a.len() == 1 {
                    Ok(a.pop().unwrap())
                } else if a.is_empty() {
                    err(pos, "empty parentheses")
                } else {
                    Ok(Expr::Tuple(a))
                }
            }
            other => err(pos, format!("unexpected {:?}", other)),
        }
    }
}

impl Expr {
    pub fn parse(s: &str) -> Result<Expr, FormulaError> {
        let toks = lex(s)?;
        let mut p = Parser {
            toks,
            i: 0,
            end: s.len(),
        };
        let e = p.expr(0)?;
        if p.i != p.toks.len() {
            return err(p.pos(), "unexpected trailing input");
        }
        Ok(e)
    }

    fn prec(&self) -> u8 {
        match self {
            Expr::Binary(op, _, _) => op.prec(),
            Expr::Unary(_, _) => UNARY_PREC,
            _ => 10,
        }
    }

    /// Variables referenced by this expression.
    pub fn vars(&self) -> Vec<&str> {
        let mut out = Vec::new();
        self.walk(&mut |e| {
            if let Expr::Var(v) = e {
                out.push(v.as_str());
            }
        });
        out
    }

    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Expr)) {
        f(self);
        match self {
            Expr::Call(_, a) | Expr::Tuple(a) => a.iter().for_each(|x| x.walk(f)),
            Expr::Unary(_, e) => e.walk(f),
            Expr::Binary(_, l, r) => {
                l.walk(f);
                r.walk(f);
            }
            _ => {}
        }
    }
}

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

impl fmt::Display for Expr {
    /// Canonical text: no spaces, minimal parentheses.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let list = |f: &mut fmt::Formatter<'_>, a: &[Expr]| -> fmt::Result {
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    f.write_str(",")?;
                }
                write!(f, "{}", x)?;
            }
            Ok(())
        };
        match self {
            Expr::Num(n) => f.write_str(&fmt_num(*n)),
            Expr::Str(s) => write!(f, "\"{}\"", s.replace('"', "\\\"")),
            Expr::Var(v) => f.write_str(v),
            Expr::Call(n, a) => {
                write!(f, "{}(", n)?;
                list(f, a)?;
                f.write_str(")")
            }
            Expr::Tuple(a) => {
                f.write_str("(")?;
                list(f, a)?;
                f.write_str(")")
            }
            Expr::Unary(op, e) => {
                f.write_str(match op {
                    UnOp::Neg => "-",
                    UnOp::Plus => "+",
                    UnOp::Not => "!",
                })?;
                if e.prec() < UNARY_PREC || matches!(**e, Expr::Unary(..)) {
                    write!(f, "({})", e)
                } else {
                    write!(f, "{}", e)
                }
            }
            Expr::Binary(op, l, r) => {
                let p = op.prec();
                let (lp, rp) = if *op == BinOp::Pow {
                    (
                        l.prec() <= p,
                        r.prec() < p && !matches!(**r, Expr::Unary(..)),
                    )
                } else {
                    (l.prec() < p, r.prec() <= p)
                };
                if lp {
                    write!(f, "({})", l)?;
                } else {
                    write!(f, "{}", l)?;
                }
                f.write_str(op.as_str())?;
                if rp {
                    write!(f, "({})", r)
                } else {
                    write!(f, "{}", r)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canon(s: &str) -> String {
        Expr::parse(s)
            .unwrap_or_else(|e| panic!("{}: {}", s, e))
            .to_string()
    }

    #[test]
    fn basics() {
        assert_eq!(canon("1 + 2 * 3"), "1+2*3");
        assert_eq!(canon("(1+2)*3"), "(1+2)*3");
        assert_eq!(canon("10-(4-3)"), "10-(4-3)");
        assert_eq!(canon("floor(SCORE/2)-5"), "floor(SCORE/2)-5");
        assert_eq!(canon("-floor((WIS+1)/4)"), "-floor((WIS+1)/4)");
        assert_eq!(canon("1+(TL>=6)+(TL>=14)"), "1+(TL>=6)+(TL>=14)");
        assert_eq!(canon("2^3^2"), "2^3^2");
        assert_eq!(canon("(2^3)^2"), "(2^3)^2");
        assert_eq!(canon("-2^2"), "-2^2");
    }

    #[test]
    fn legacy_terms() {
        assert_eq!(
            Expr::parse("CL=Fighter").unwrap(),
            Expr::Var("CL=Fighter".into())
        );
        assert_eq!(
            Expr::parse("COUNT[FEATTYPE=Metamagic]*2").unwrap().vars(),
            vec!["COUNT[FEATTYPE=Metamagic]"]
        );
        assert_eq!(
            Expr::parse("SKILL.Ride.MISC").unwrap().vars(),
            vec!["SKILL.Ride.MISC"]
        );
        assert_eq!(canon("if(a==0&&b>=9,1,0)"), "if(a==0&&b>=9,1,0)");
        assert_eq!(
            canon(r#"count("ABILITIES", "CATEGORY=FEAT","KEY=Dodge")/2"#),
            r#"count("ABILITIES","CATEGORY=FEAT","KEY=Dodge")/2"#
        );
        assert_eq!(
            canon("max(((STR-10),(DEX-10))-BASE)"),
            "max((STR-10,DEX-10)-BASE)"
        );
    }

    #[test]
    fn legacy_ops() {
        assert_eq!(canon("((TL+1)/2).TRUNC*10"), "TRUNC((TL+1)/2)*10");
        assert_eq!(canon("(3-INTSCORE)max0"), "max(3-INTSCORE,0)");
        assert_eq!(canon("(CL=Sea Witch+1)/3"), "(CL=Sea Witch+1)/3");
        assert_eq!(canon("(CL=Monk (Elder))/2"), "CL=Monk (Elder)/2");
    }

    #[test]
    fn errors() {
        assert!(Expr::parse("1+").is_err());
        assert!(Expr::parse("(1").is_err());
        assert!(Expr::parse("\"abc").is_err());
        assert!(Expr::parse("1 2").is_err());
    }

    #[test]
    fn reparse_stable() {
        for s in [
            "min(floor((P3LVL+3)/6*2),4)",
            "1+floor(X/5)-max(floor((X+4)/6),1)",
            "if((A==0&&B>=9),1,0)",
            "!(A>1)||B",
            "10+(HD/2)+WIS",
        ] {
            let once = canon(s);
            assert_eq!(canon(&once), once, "{}", s);
            assert_eq!(Expr::parse(&once).unwrap(), Expr::parse(s).unwrap());
        }
    }

    /// (input, canonical output) pairs
    const CANON: &[(&str, &str)] = &[
        ("0", "0"),
        ("42", "42"),
        ("1.5", "1.5"),
        (".5", "0.5"),
        ("007", "7"),
        ("  3  ", "3"),
        ("STR", "STR"),
        ("Var_Name2", "Var_Name2"),
        ("a+b", "a+b"),
        ("a - b", "a-b"),
        ("a*b/c", "a*b/c"),
        ("a/(b*c)", "a/(b*c)"),
        ("a-(b+c)", "a-(b+c)"),
        ("(a-b)+c", "a-b+c"),
        ("a%3", "a%3"),
        ("(a+b)%3", "(a+b)%3"),
        ("-a", "-a"),
        ("--a", "-(-a)"),
        ("+a", "+a"),
        ("!a", "!a"),
        ("!(a&&b)", "!(a&&b)"),
        ("-(a+b)", "-(a+b)"),
        ("a*-b", "a*-b"),
        ("a<b", "a<b"),
        ("a<=b", "a<=b"),
        ("a>b", "a>b"),
        ("a>=b", "a>=b"),
        ("a==b", "a==b"),
        ("a!=b", "a!=b"),
        ("a=b", "a=b"),
        ("a&&b||c", "a&&b||c"),
        ("a&&(b||c)", "a&&(b||c)"),
        ("a||b&&c", "a||b&&c"),
        ("(a||b)&&c", "(a||b)&&c"),
        ("a<b==c<d", "a<b==c<d"),
        ("(a==b)<c", "(a==b)<c"),
        ("a+b<c*d", "a+b<c*d"),
        ("2^10", "2^10"),
        ("a*b^2", "a*b^2"),
        ("(a*b)^2", "(a*b)^2"),
        ("f()", "f()"),
        ("f(1)", "f(1)"),
        ("f(1, 2 ,3)", "f(1,2,3)"),
        ("f(g(h(x)))", "f(g(h(x)))"),
        ("max(0,min(4,x))", "max(0,min(4,x))"),
        ("if(a>1,b,c)+1", "if(a>1,b,c)+1"),
        ("floor(x/2)*2", "floor(x/2)*2"),
        ("\"str\"", "\"str\""),
        ("var(\"CL=Mage\")", "var(\"CL=Mage\")"),
        ("f(\"a,b\",\"(c)\")", "f(\"a,b\",\"(c)\")"),
        ("CL=Mage", "CL=Mage"),
        ("CL=Mage+1", "CL=Mage+1"),
        ("CL=Mage*2", "CL=Mage*2"),
        ("CL=Old Mage-1", "CL=Old Mage-1"),
        ("COUNT[FEATS.ALL]", "COUNT[FEATS.ALL]"),
        ("COUNT[FEATTYPE=A]/2", "COUNT[FEATTYPE=A]/2"),
        ("COUNT[X[Y]]", "COUNT[X[Y]]"),
        ("SKILL.Ride.RANK", "SKILL.Ride.RANK"),
        ("Var~1", "Var~1"),
        ("%LIST", "%LIST"),
        ("%CHOICE+1", "%CHOICE+1"),
        ("1stLevel", "1stLevel"),
        ("x.TRUNC", "x.TRUNC"),
        ("(x).TRUNC", "TRUNC(x)"),
        ("(x).INTVAL+1", "INTVAL(x)+1"),
        ("(a)MAX1", "max(a,1)"),
        ("(a)MIN(b)", "min(a,b)"),
        ("(a)max0+1", "max(a,0)+1"),
        ("((a,b,c))", "(a,b,c)"),
    ];

    #[test]
    fn canonical_table() {
        for (input, want) in CANON {
            let got = Expr::parse(input)
                .unwrap_or_else(|e| panic!("{:?}: {}", input, e))
                .to_string();
            assert_eq!(&got, want, "input {:?}", input);
            // canonical output re-parses to the same tree
            assert_eq!(
                Expr::parse(&got).unwrap(),
                Expr::parse(input).unwrap(),
                "reparse {:?}",
                input
            );
        }
    }

    const BAD: &[&str] = &[
        "", " ", "+", "a+", "*a", "a**b", "(", ")", "()", "(a", "a)", "f(", "f(a", "f(a,", "f(,a)",
        "a b", "1 2", "\"x", "COUNT[x", "a $ b", "a ; b", "a,b",
    ];

    #[test]
    fn rejects_table() {
        for input in BAD {
            assert!(Expr::parse(input).is_err(), "accepted {:?}", input);
        }
    }

    #[test]
    fn tree_shapes() {
        use BinOp::*;
        let v = |s: &str| Box::new(Expr::Var(s.into()));
        let n = |x: f64| Box::new(Expr::Num(x));
        assert_eq!(
            Expr::parse("a+b*c").unwrap(),
            Expr::Binary(Add, v("a"), Box::new(Expr::Binary(Mul, v("b"), v("c"))))
        );
        assert_eq!(
            Expr::parse("a-b-c").unwrap(),
            Expr::Binary(Sub, Box::new(Expr::Binary(Sub, v("a"), v("b"))), v("c"))
        );
        assert_eq!(
            Expr::parse("2^3^2").unwrap(),
            Expr::Binary(Pow, n(2.0), Box::new(Expr::Binary(Pow, n(3.0), n(2.0))))
        );
        assert_eq!(
            Expr::parse("-a^2").unwrap(),
            Expr::Unary(UnOp::Neg, Box::new(Expr::Binary(Pow, v("a"), n(2.0))))
        );
        assert_eq!(
            Expr::parse("a&b").unwrap(),
            Expr::Binary(And, v("a"), v("b"))
        );
        assert_eq!(
            Expr::parse("f(a)").unwrap(),
            Expr::Call("f".into(), vec![Expr::Var("a".into())])
        );
        assert_eq!(Expr::parse("\"q\\\"x\"").unwrap(), Expr::Str("q\"x".into()));
    }

    #[test]
    fn vars_listed() {
        let e = Expr::parse("max(STR,DEX)+CL=Mage*COUNT[X]-f(\"s\",HD)").unwrap();
        assert_eq!(e.vars(), vec!["STR", "DEX", "CL=Mage", "COUNT[X]", "HD"]);
    }

    #[test]
    fn error_positions() {
        assert_eq!(Expr::parse("a+").unwrap_err().pos, 2);
        assert_eq!(Expr::parse("a $").unwrap_err().pos, 2);
        assert_eq!(Expr::parse("(a").unwrap_err().pos, 2);
    }
}
