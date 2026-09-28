// SPDX-License-Identifier: GPL-3.0-or-later
//! Precedence-climbing parser.

use super::lexer::{lex, Tok};
use super::Expr;
use crate::error::{Error, Result};

/// Binary operator levels, lowest precedence first.
const LEVELS: [&[&str]; 5] = [&["||"], &["&&"], &["==", "!=", "<", "<=", ">", ">="], &["+", "-"], &["*", "/", "%"]];

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

    fn expect(&mut self, t: &Tok) -> Result<()> {
        match self.next() {
            Some(ref got) if got == t => Ok(()),
            got => Err(Error::Expr(format!("expected {t:?}, got {got:?}"))),
        }
    }

    fn ternary(&mut self) -> Result<Expr> {
        let c = self.binary(0)?;
        if self.peek() != Some(&Tok::Question) {
            return Ok(c);
        }
        self.next();
        let a = self.ternary()?;
        self.expect(&Tok::Colon)?;
        let b = self.ternary()?;
        Ok(Expr::Cond(Box::new(c), Box::new(a), Box::new(b)))
    }

    fn binary(&mut self, level: usize) -> Result<Expr> {
        if level >= LEVELS.len() {
            return self.unary();
        }
        let mut lhs = self.binary(level + 1)?;
        while let Some(Tok::Op(op)) = self.peek() {
            let op = *op;
            if !LEVELS[level].contains(&op) {
                break;
            }
            self.next();
            let rhs = self.binary(level + 1)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr> {
        match self.peek() {
            Some(Tok::Op(op @ ("-" | "!"))) => {
                let c = if *op == "-" { '-' } else { '!' };
                self.next();
                Ok(Expr::Unary(c, Box::new(self.unary()?)))
            }
            _ => self.power(),
        }
    }

    /// `^` is right-associative and binds tighter than unary minus on its
    /// left (`-2 ^ 2 == -4`).
    fn power(&mut self) -> Result<Expr> {
        let base = self.atom()?;
        if self.peek() != Some(&Tok::Op("^")) {
            return Ok(base);
        }
        self.next();
        let exp = self.unary()?;
        Ok(Expr::Binary("^", Box::new(base), Box::new(exp)))
    }

    fn call_args(&mut self) -> Result<Vec<Expr>> {
        let mut args = vec![];
        if self.peek() != Some(&Tok::RParen) {
            loop {
                args.push(self.ternary()?);
                if self.peek() != Some(&Tok::Comma) {
                    break;
                }
                self.next();
            }
        }
        self.expect(&Tok::RParen)?;
        Ok(args)
    }

    fn atom(&mut self) -> Result<Expr> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::Str(s)) => Ok(Expr::Str(s)),
            Some(Tok::LParen) => {
                let e = self.ternary()?;
                self.expect(&Tok::RParen)?;
                Ok(e)
            }
            Some(Tok::Ident(name)) if self.peek() == Some(&Tok::LParen) => {
                self.next();
                Ok(Expr::Call(name, self.call_args()?))
            }
            Some(Tok::Ident(name)) => Ok(match name.as_str() {
                "true" => Expr::Num(1.0),
                "false" => Expr::Num(0.0),
                "inf" => Expr::Num(f64::INFINITY),
                _ => Expr::Var(name),
            }),
            t => Err(Error::Expr(format!("unexpected token {t:?}"))),
        }
    }
}

/// Parse an expression.
///
/// # Errors
/// `Error::Expr` describing the first syntax error, quoting the source.
pub fn parse(src: &str) -> Result<Expr> {
    let toks = lex(src)?;
    if toks.is_empty() {
        return Err(Error::Expr("empty expression".into()));
    }
    let mut p = Parser { toks, pos: 0 };
    let e = p.ternary().map_err(|e| Error::Expr(format!("{e} in `{src}`")))?;
    if p.pos < p.toks.len() {
        return Err(Error::Expr(format!("trailing input after token {} in `{src}`", p.pos)));
    }
    Ok(e)
}
