// SPDX-License-Identifier: GPL-3.0-or-later
//! Tokenizer for the expression language.

use crate::error::{Error, Result};

/// A token.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Tok {
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

/// Two-character operators first so `<=` wins over `<`. A bare `=` means `==`.
const OPS: [&str; 16] = [
    "==", "!=", "<=", ">=", "&&", "||", "<", ">", "+", "-", "*", "/", "%", "^", "!", "=",
];

fn number(chars: &[char], mut i: usize) -> Result<(Tok, usize)> {
    let start = i;
    while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == '_') {
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
    let n = s
        .parse()
        .map_err(|_| Error::Expr(format!("bad number '{s}'")))?;
    Ok((Tok::Num(n), i))
}

fn string(chars: &[char], i: usize) -> Result<(Tok, usize)> {
    let quote = chars[i];
    let start = i + 1;
    let end = chars[start..]
        .iter()
        .position(|&c| c == quote)
        .ok_or_else(|| Error::Expr("unterminated string".into()))?;
    Ok((
        Tok::Str(chars[start..start + end].iter().collect()),
        start + end + 1,
    ))
}

fn operator(chars: &[char], i: usize) -> Result<(Tok, usize)> {
    let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
    let op = OPS
        .iter()
        .find(|op| rest.starts_with(**op))
        .ok_or_else(|| Error::Expr(format!("unexpected character '{}' at {i}", chars[i])))?;
    Ok((Tok::Op(if *op == "=" { "==" } else { op }), i + op.len()))
}

/// Split source into tokens.
pub(super) fn lex(src: &str) -> Result<Vec<Tok>> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = vec![];
    while i < chars.len() {
        let c = chars[i];
        let single = match c {
            '(' => Some(Tok::LParen),
            ')' => Some(Tok::RParen),
            ',' => Some(Tok::Comma),
            '?' => Some(Tok::Question),
            ':' => Some(Tok::Colon),
            _ => None,
        };
        let (tok, next) = if c.is_whitespace() {
            i += 1;
            continue;
        } else if let Some(t) = single {
            (t, i + 1)
        } else if c.is_ascii_digit()
            || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit))
        {
            number(&chars, i)?
        } else if c.is_alphabetic() || c == '_' {
            let len = chars[i..]
                .iter()
                .take_while(|c| c.is_alphanumeric() || **c == '_' || **c == '.')
                .count();
            (Tok::Ident(chars[i..i + len].iter().collect()), i + len)
        } else if c == '"' || c == '\'' {
            string(&chars, i)?
        } else {
            operator(&chars, i)?
        };
        out.push(tok);
        i = next;
    }
    Ok(out)
}
