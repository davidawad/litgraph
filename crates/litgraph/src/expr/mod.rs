// SPDX-License-Identifier: GPL-3.0-or-later
//! A tiny, safe expression language for custom cost / weight / utility /
//! mask / probability / capacity functions.
//!
//! Every built-in metric is itself an expression (see `metrics`), so
//! built-in and custom functions are the same mechanism and an agent can read
//! the source of any metric it is about to use.
//!
//! ```
//! use litgraph::expr::{parse, Arg, Env};
//! struct E;
//! impl Env for E {
//!     fn var(&self, n: &str) -> Option<f64> {
//!         match n { "hours" => Some(10.0), "rate" => Some(500.0), "fees" => Some(350.0), _ => None }
//!     }
//!     fn func(&self, _: &str, _: &[Arg]) -> Option<litgraph::Result<f64>> { None }
//! }
//! assert_eq!(parse("hours * rate + fees").unwrap().eval(&E).unwrap(), 5350.0);
//! assert_eq!(parse("hours > 5 ? max(1, 2) : 0").unwrap().eval(&E).unwrap(), 2.0);
//! ```
//!
//! Grammar (precedence low→high): `c ? a : b`, `||`, `&&`,
//! `== != < <= > >=` (`=` is `==`), `+ -`, `* / %`, unary `- !`, `^`
//! (right-assoc), atoms: numbers (`1_000`, `1.5e3`), `"strings"` (function
//! arguments only), identifiers (`a.b.c`), calls `f(x, ...)`, parentheses.
//! Booleans are 1.0/0.0; `&&`, `||`, `?:` and `if()` short-circuit.

mod lexer;
mod parser;

pub use parser::parse;

use crate::error::{Error, Result};

/// A parsed expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Number literal.
    Num(f64),
    /// String literal (only valid as a function argument).
    Str(String),
    /// Variable reference.
    Var(String),
    /// `-x` or `!x`.
    Unary(char, Box<Expr>),
    /// Binary operator.
    Binary(&'static str, Box<Expr>, Box<Expr>),
    /// `c ? a : b`.
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    /// Function call.
    Call(String, Vec<Expr>),
}

/// What an expression can see. Implemented per evaluation site (edge, terminal).
pub trait Env {
    /// Value of a variable, or `None` if unknown.
    fn var(&self, name: &str) -> Option<f64>;
    /// Site-specific functions (`tag("x")`, `attr("x", d)`, ...); `None` if unknown.
    fn func(&self, name: &str, args: &[Arg]) -> Option<Result<f64>>;
}

/// An evaluated function argument.
#[derive(Debug, Clone)]
pub enum Arg {
    /// Number.
    Num(f64),
    /// String literal.
    Str(String),
}

impl Arg {
    /// The argument as a string.
    ///
    /// # Errors
    /// If the argument is a number.
    pub fn str(&self) -> Result<&str> {
        match self {
            Arg::Str(s) => Ok(s),
            Arg::Num(n) => Err(Error::Expr(format!("expected a string argument, got {n}"))),
        }
    }

    /// The argument as a number.
    ///
    /// # Errors
    /// If the argument is a string.
    pub fn num(&self) -> Result<f64> {
        match self {
            Arg::Num(n) => Ok(*n),
            Arg::Str(s) => Err(Error::Expr(format!("expected a number, got \"{s}\""))),
        }
    }
}

/// Pure numeric functions available everywhere: `(signature, doc)`.
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

pub(crate) fn truthy(x: f64) -> bool {
    x != 0.0 && !x.is_nan()
}

pub(crate) fn b(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
    }
}

fn binary(op: &str, x: f64, y: f64) -> f64 {
    match op {
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
        _ => f64::NAN,
    }
}

impl Expr {
    /// Evaluate against an environment.
    ///
    /// # Errors
    /// Unknown variables or functions, type errors, wrong arity.
    pub fn eval(&self, env: &dyn Env) -> Result<f64> {
        Ok(match self {
            Expr::Num(n) => *n,
            Expr::Str(s) => return Err(Error::Expr(format!("string \"{s}\" used as a number"))),
            Expr::Var(name) => env.var(name).ok_or_else(|| Error::Expr(format!("unknown variable `{name}`")))?,
            Expr::Unary('-', x) => -x.eval(env)?,
            Expr::Unary(_, x) => b(!truthy(x.eval(env)?)),
            Expr::Cond(c, a, e) => {
                if truthy(c.eval(env)?) {
                    a.eval(env)?
                } else {
                    e.eval(env)?
                }
            }
            Expr::Binary("&&", l, r) => b(truthy(l.eval(env)?) && truthy(r.eval(env)?)),
            Expr::Binary("||", l, r) => b(truthy(l.eval(env)?) || truthy(r.eval(env)?)),
            Expr::Binary(op, l, r) => binary(op, l.eval(env)?, r.eval(env)?),
            Expr::Call(name, args) => return call(name, args, env),
        })
    }

    /// Variable names referenced, in first-use order.
    #[must_use]
    pub fn vars(&self) -> Vec<String> {
        fn walk(e: &Expr, out: &mut Vec<String>) {
            match e {
                Expr::Var(v) if !out.contains(v) => out.push(v.clone()),
                Expr::Unary(_, x) => walk(x, out),
                Expr::Binary(_, l, r) => {
                    walk(l, out);
                    walk(r, out);
                }
                Expr::Cond(c, a, e) => {
                    walk(c, out);
                    walk(a, out);
                    walk(e, out);
                }
                Expr::Call(_, args) => args.iter().for_each(|a| walk(a, out)),
                _ => {}
            }
        }
        let mut out = vec![];
        walk(self, &mut out);
        out
    }
}

fn call(name: &str, args: &[Expr], env: &dyn Env) -> Result<f64> {
    if name == "if" {
        let [c, a, e] = args else {
            return Err(Error::Expr("if(c, a, b) takes 3 arguments".into()));
        };
        return if truthy(c.eval(env)?) { a.eval(env) } else { e.eval(env) };
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
    math(name, &vals)
}

fn math(name: &str, vals: &[Arg]) -> Result<f64> {
    let n = |i: usize| -> Result<f64> {
        vals.get(i).ok_or_else(|| Error::Expr(format!("{name}: missing argument {}", i + 1)))?.num()
    };
    let all = || vals.iter().map(Arg::num).collect::<Result<Vec<_>>>();
    Ok(match name {
        "min" => all()?.into_iter().fold(f64::INFINITY, f64::min),
        "max" => all()?.into_iter().fold(f64::NEG_INFINITY, f64::max),
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
mod tests;
