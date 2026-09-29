// SPDX-License-Identifier: GPL-3.0-or-later
use thiserror::Error;

/// Every error carries a stable `code` so agents can branch without parsing prose.
#[derive(Debug, Error)]
pub enum Error {
    /// A pack or scenario document failed to parse (malformed JSON, wrong shape).
    #[error("parse: {0}")]
    Parse(String),
    /// The request or document parsed, but its content violates a model invariant.
    #[error("invalid: {0}")]
    Invalid(String),
    /// A referenced id (pack, node, edge, scenario field) does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// The custom-function expression language failed to lex, parse, or evaluate.
    #[error("expression: {0}")]
    Expr(String),
    /// A numeric computation could not produce a valid result (e.g. singular system).
    #[error("numeric: {0}")]
    Numeric(String),
    /// Reading or writing a file failed.
    #[error("io: {0}")]
    Io(String),
}

impl Error {
    /// Returns the stable, machine-readable error code for this variant.
    ///
    /// Codes are part of the public API surface: they are what an agent
    /// branches on, not the human-readable message.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Error::Parse(_) => "parse",
            Error::Invalid(_) => "invalid",
            Error::NotFound(_) => "not-found",
            Error::Expr(_) => "expr",
            Error::Numeric(_) => "numeric",
            Error::Io(_) => "io",
        }
    }
}

/// This crate's `Result` alias: every fallible operation returns `Result<T, Error>`.
pub type Result<T> = std::result::Result<T, Error>;
