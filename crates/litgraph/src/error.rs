use thiserror::Error;

/// Every error carries a stable `code` so agents can branch without parsing prose.
#[derive(Debug, Error)]
pub enum Error {
    #[error("parse: {0}")]
    Parse(String),
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("expression: {0}")]
    Expr(String),
    #[error("numeric: {0}")]
    Numeric(String),
    #[error("io: {0}")]
    Io(String),
}

impl Error {
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

pub type Result<T> = std::result::Result<T, Error>;
