//! A tiny, safe expression language for custom cost / weight / utility /
//! mask / capacity functions.
//!
//! Every built-in metric is itself an expression (see `metrics.rs`), so
//! "built-in" and "custom" functions are the same mechanism and an agent can
//! read the source of any metric it is about to use.
//!
//! ```text
//! hours * rate + fees                       // the classic dollar cost
//! tag("sanctions") ? 1e9 : hours * rate     // price sanctions edges out
//! is_self * (hours * rate + fees) * (1 - fee_shift * to_tag("fee-eligible"))
//! payoff < 0 ? loss_aversion * payoff : payoff    // terminal utility
//! -ln(p)                                    // "surprise": most-probable path
//! ```
//!
//! Grammar (precedence low→high): `c ? a : b`, `||`, `&&`,
//! `== != < <= > >=`, `+ -`, `* / %`, unary `- !`, `^` (right-assoc),
//! atoms: numbers, `"strings"` (function args only), identifiers
//! (`a.b.c` allowed), calls `f(x, ...)`, parentheses. Booleans are 1.0/0.0.
//! Unknown identifiers are a compile error that lists what is available.

use crate::error::{Error, Result};

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f64),
    Str(String),
    Var(String),
    Unary(char, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

/// What an expression can see. Implemented per evaluation site (edge, terminal).
pub trait Env {
    fn var(&self, name: &str) -> Option<f64>;
    /// String-argument predicates/lookups: `tag("x")`, `attr("x", d)`, ...
    fn func(&self, name: &str, args: &[Arg]) -> Option<Result<f64>>;
}

#[derive(Debug, Clone)]
pub enum Arg {
    Num(f64),
    Str(String),
}

impl Arg {
    pub fn str(&self) -> Result<&str> {
        match self {
            Arg::Str(s) => Ok(s),
            Arg::Num(n) => Err(Error::Expr(format!("expected a string argument, got {n}"))),
        }
    }
    pub fn num(&self) -> Result<f64> {
        match self {
            Arg::Num(n) => Ok(*n),
            Arg::Str(s) => Err(Error::Expr(format!("expected a number, got \"{s}\""))),
        }
    }
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
    Question,
    Colon,
}

const OPS: [&str; 16] = [
    "==", "!=", "<=", ">=", "&&", "||", "<", ">", "+", "-", "*", "/", "%", "^", "!", "=",
];

fn lex(src: &str) -> Result<Vec<Tok>> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = vec![];
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit()
            || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()))
        {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == '_')
            {
                i += 1;
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                i += 1;
                if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
                    i += 1;
                }
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let s: String = chars[start..i].iter().filter(|c| **c != '_').collect();
            out.push(Tok::Num(
                s.parse()
                    .map_err(|_| Error::Expr(format!("bad number '{s}'")))?,
            ));
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.')
            {
                i += 1;
            }
            out.push(Tok::Ident(chars[start..i].iter().collect()));
        } else if c == '"' || c == '\'' {
            let q = c;
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != q {
                i += 1;
            }
            if i >= chars.len() {
                return Err(Error::Expr("unterminated string".into()));
            }
            out.push(Tok::Str(chars[start..i].iter().collect()));
            i += 1;
        } else if c == '(' {
            out.push(Tok::LParen);
            i += 1;
        } else if c == ')' {
            out.push(Tok::RParen);
            i += 1;
        } else if c == ',' {
            out.push(Tok::Comma);
            i += 1;
        } else if c == '?' {
            out.push(Tok::Question);
            i += 1;
        } else if c == ':' {
            out.push(Tok::Colon);
            i += 1;
        } else {
            let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
            let op = OPS
                .iter()
                .find(|op| rest.starts_with(**op))
                .ok_or_else(|| Error::Expr(format!("unexpected character '{c}' at {i}")))?;
            out.push(Tok::Op(if *op == "=" { "==" } else { op }));
            i += op.len();
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Parser (precedence climbing)
// ---------------------------------------------------------------------------

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }
    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }
    fn expect(&mut self, t: Tok) -> Result<()> {
        match self.next() {
            Some(ref got) if *got == t => Ok(()),
            got => Err(Error::Expr(format!("expected {t:?}, got {got:?}"))),
        }
    }

    fn ternary(&mut self) -> Result<Expr> {
        let c = self.binary(0)?;
        if self.peek() == Some(&Tok::Question) {
            self.next();
            let a = self.ternary()?;
            self.expect(Tok::Colon)?;
            let b = self.ternary()?;
            return Ok(Expr::Cond(Box::new(c), Box::new(a), Box::new(b)));
        }
        Ok(c)
    }

    fn binary(&mut self, min_level: usize) -> Result<Expr> {
        const LEVELS: [&[&str]; 5] = [
            &["||"],
            &["&&"],
            &["==", "!=", "<", "<=", ">", ">="],
            &["+", "-"],
            &["*", "/", "%"],
        ];
        if min_level >= LEVELS.len() {
            return self.unary();
        }
        let mut lhs = self.binary(min_level + 1)?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op(op)) if LEVELS[min_level].contains(op) => *op,
                _ => break,
            };
            self.next();
            let rhs = self.binary(min_level + 1)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr> {
        match self.peek() {
            Some(Tok::Op("-")) => {
                self.next();
                Ok(Expr::Unary('-', Box::new(self.unary()?)))
            }
            Some(Tok::Op("!")) => {
                self.next();
                Ok(Expr::Unary('!', Box::new(self.unary()?)))
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Result<Expr> {
        let base = self.atom()?;
        if self.peek() == Some(&Tok::Op("^")) {
            self.next();
            let exp = self.unary()?;
            return Ok(Expr::Binary("^", Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    fn atom(&mut self) -> Result<Expr> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::Str(s)) => Ok(Expr::Str(s)),
            Some(Tok::LParen) => {
                let e = self.ternary()?;
                self.expect(Tok::RParen)?;
                Ok(e)
            }
            Some(Tok::Ident(name)) => {
                if self.peek() == Some(&Tok::LParen) {
                    self.next();
                    let mut args = vec![];
                    if self.peek() != Some(&Tok::RParen) {
                        loop {
                            args.push(self.ternary()?);
                            if self.peek() == Some(&Tok::Comma) {
                                self.next();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(Tok::RParen)?;
                    Ok(Expr::Call(name, args))
                } else {
                    match name.as_str() {
                        "true" => Ok(Expr::Num(1.0)),
                        "false" => Ok(Expr::Num(0.0)),
                        "inf" => Ok(Expr::Num(f64::INFINITY)),
                        _ => Ok(Expr::Var(name)),
                    }
                }
            }
            t => Err(Error::Expr(format!("unexpected token {t:?}"))),
        }
    }
}

pub fn parse(src: &str) -> Result<Expr> {
    let toks = lex(src)?;
    if toks.is_empty() {
        return Err(Error::Expr("empty expression".into()));
    }
    let mut p = Parser { toks, pos: 0 };
    let e = p
        .ternary()
        .map_err(|e| Error::Expr(format!("{e} in `{src}`")))?;
    if p.pos < p.toks.len() {
        return Err(Error::Expr(format!(
            "trailing input after position {} in `{src}`",
            p.pos
        )));
    }
    Ok(e)
}

// ---------------------------------------------------------------------------
// Evaluation
// ---------------------------------------------------------------------------

/// Pure numeric functions available everywhere.
pub const MATH_FUNCS: &[(&str, &str)] = &[
    ("min(a, b, ...)", "smallest argument"),
    ("max(a, b, ...)", "largest argument"),
    ("abs(x)", "absolute value"),
    ("exp(x)", "e^x"),
    ("ln(x)", "natural log"),
    ("log10(x)", "base-10 log"),
    ("sqrt(x)", "square root"),
    ("pow(x, y)", "x^y (also `x ^ y`)"),
    ("clamp(x, lo, hi)", "bound x to [lo, hi]"),
    ("if(c, a, b)", "a when c != 0 else b (also `c ? a : b`)"),
    ("isnan(x)", "1 if x is NaN"),
    ("default(x, d)", "x unless NaN, else d"),
    ("step(x)", "1 if x > 0 else 0"),
];

fn truthy(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}

fn b(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
    }
}

impl Expr {
    pub fn eval(&self, env: &dyn Env) -> Result<f64> {
        Ok(match self {
            Expr::Num(n) => *n,
            Expr::Str(s) => return Err(Error::Expr(format!("string \"{s}\" used as a number"))),
            Expr::Var(name) => env
                .var(name)
                .ok_or_else(|| Error::Expr(format!("unknown variable `{name}`")))?,
            Expr::Unary('-', x) => -x.eval(env)?,
            Expr::Unary(_, x) => b(!truthy(x.eval(env)?)),
            Expr::Cond(c, a, bb) => {
                if truthy(c.eval(env)?) {
                    a.eval(env)?
                } else {
                    bb.eval(env)?
                }
            }
            Expr::Binary(op, l, r) => {
                // Short-circuit logic.
                if *op == "&&" {
                    return Ok(b(truthy(l.eval(env)?) && truthy(r.eval(env)?)));
                }
                if *op == "||" {
                    return Ok(b(truthy(l.eval(env)?) || truthy(r.eval(env)?)));
                }
                let (x, y) = (l.eval(env)?, r.eval(env)?);
                match *op {
                    "+" => x + y,
                    "-" => x - y,
                    "*" => x * y,
                    "/" => x / y,
                    "%" => x % y,
                    "^" => x.powf(y),
                    "==" => b((x - y).abs() < 1e-12),
                    "!=" => b((x - y).abs() >= 1e-12),
                    "<" => b(x < y),
                    "<=" => b(x <= y),
                    ">" => b(x > y),
                    ">=" => b(x >= y),
                    _ => unreachable!(),
                }
            }
            Expr::Call(name, args) => return call(name, args, env),
        })
    }

    /// Variable names referenced (for validation / dependency reporting).
    pub fn vars(&self, out: &mut Vec<String>) {
        match self {
            Expr::Var(v) => {
                if !out.contains(v) {
                    out.push(v.clone())
                }
            }
            Expr::Unary(_, x) => x.vars(out),
            Expr::Binary(_, l, r) => {
                l.vars(out);
                r.vars(out);
            }
            Expr::Cond(c, a, b) => {
                c.vars(out);
                a.vars(out);
                b.vars(out);
            }
            Expr::Call(_, args) => args.iter().for_each(|a| a.vars(out)),
            _ => {}
        }
    }
}

fn call(name: &str, args: &[Expr], env: &dyn Env) -> Result<f64> {
    // `if` is lazy.
    if name == "if" {
        if args.len() != 3 {
            return Err(Error::Expr("if(c, a, b) takes 3 arguments".into()));
        }
        return if truthy(args[0].eval(env)?) {
            args[1].eval(env)
        } else {
            args[2].eval(env)
        };
    }
    let vals: Vec<Arg> = args
        .iter()
        .map(|a| match a {
            Expr::Str(s) => Ok(Arg::Str(s.clone())),
            e => e.eval(env).map(Arg::Num),
        })
        .collect::<Result<_>>()?;
    if let Some(r) = env.func(name, &vals) {
        return r;
    }
    let n = |i: usize| -> Result<f64> {
        vals.get(i)
            .ok_or_else(|| Error::Expr(format!("{name}: missing argument {}", i + 1)))?
            .num()
    };
    Ok(match name {
        "min" => vals
            .iter()
            .map(|a| a.num())
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .fold(f64::INFINITY, f64::min),
        "max" => vals
            .iter()
            .map(|a| a.num())
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .fold(f64::NEG_INFINITY, f64::max),
        "abs" => n(0)?.abs(),
        "exp" => n(0)?.exp(),
        "ln" => n(0)?.ln(),
        "log10" => n(0)?.log10(),
        "sqrt" => n(0)?.sqrt(),
        "pow" => n(0)?.powf(n(1)?),
        "clamp" => n(0)?.max(n(1)?).min(n(2)?),
        "isnan" => b(n(0)?.is_nan()),
        "default" => {
            let x = n(0)?;
            if x.is_nan() {
                n(1)?
            } else {
                x
            }
        }
        "step" => b(n(0)? > 0.0),
        _ => return Err(Error::Expr(format!("unknown function `{name}`"))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct M(HashMap<&'static str, f64>);
    impl Env for M {
        fn var(&self, n: &str) -> Option<f64> {
            self.0.get(n).copied()
        }
        fn func(&self, name: &str, args: &[Arg]) -> Option<Result<f64>> {
            if name == "tag" {
                Some(args[0].str().map(|s| b(s == "x")))
            } else {
                None
            }
        }
    }

    fn ev(s: &str) -> f64 {
        let env = M(HashMap::from([
            ("hours", 10.0),
            ("rate", 500.0),
            ("fees", 350.0),
            ("p", 0.25),
        ]));
        parse(s).unwrap().eval(&env).unwrap()
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(ev("hours * rate + fees"), 5350.0);
        assert_eq!(ev("2 + 3 * 4 ^ 2"), 50.0);
        assert_eq!(ev("-2 ^ 2"), -4.0);
        assert_eq!(ev("(1 + 2) * 3"), 9.0);
        assert_eq!(ev("1_000 * 2"), 2000.0);
        assert_eq!(ev("1.5e3"), 1500.0);
    }

    #[test]
    fn logic_ternary_functions() {
        assert_eq!(ev("hours > 5 ? 1 : 2"), 1.0);
        assert_eq!(ev("hours > 5 && rate < 100"), 0.0);
        assert_eq!(ev("max(1, hours, 3)"), 10.0);
        assert_eq!(ev("clamp(hours, 0, 4)"), 4.0);
        assert!((ev("-ln(p)") - 1.3862943611).abs() < 1e-9);
        assert_eq!(ev("tag('x') * 7"), 7.0);
        assert_eq!(ev("if(0, 1/0, 3)"), 3.0);
        assert_eq!(ev("hours = 10 ? 1 : 0"), 1.0);
    }

    #[test]
    fn errors_are_reported() {
        assert!(parse("1 +").is_err());
        assert!(parse("(1").is_err());
        let env = M(HashMap::new());
        assert!(parse("nope").unwrap().eval(&env).is_err());
    }
}
