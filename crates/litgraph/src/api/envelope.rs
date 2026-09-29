// SPDX-License-Identifier: GPL-3.0-or-later
//! The response envelope every op returns, and the error hints it carries.

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

use crate::error::Error;

/// A structured error.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ApiError {
    /// Stable code: parse, invalid, not-found, expr, numeric, io.
    pub code: String,
    /// Message.
    pub message: String,
    /// What to try next.
    pub hint: String,
}

/// All warnings with one code, collapsed.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GroupedWarning {
    /// Stable code (`probability-fill`, `mixed-node`, `payoff-not-authored`, ...).
    pub code: String,
    /// Occurrences.
    pub count: usize,
    /// A representative message.
    pub example: String,
    /// Up to eight locations.
    pub at: Vec<String>,
}

/// The response envelope.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Response {
    /// Success.
    pub ok: bool,
    /// Request/response contract version.
    pub api_version: u32,
    /// The op that ran.
    pub op: String,
    /// Op-specific result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Fallbacks and modeling choices the engine made.
    pub warnings: Vec<GroupedWarning>,
    /// Packs, fingerprints, parameters, modes, engine.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Value>,
    /// Error, when `ok` is false.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
    /// Wall time.
    pub elapsed_ms: f64,
}

/// An ungrouped warning.
#[derive(Debug, Clone)]
pub(crate) struct Warn {
    pub code: String,
    pub at: Option<String>,
    pub message: String,
}

pub(super) fn hint(e: &Error) -> &'static str {
    match e {
        Error::NotFound(_) => {
            r#"list ids with {"op":{"op":"graph"}} or {"op":{"op":"packs"}}; local ids work when unique"#
        }
        Error::Expr(_) => {
            r#"see {"op":{"op":"describe"}} for variables/functions; test with {"op":{"op":"metric","spec":"..."}}"#
        }
        Error::Numeric(_) => {
            "a forced or optimal choice loops forever; add a mask or policy to break the cycle"
        }
        Error::Parse(_) => {
            "see `litgraph schema <request|scenario|pack|links>` for the exact shape"
        }
        _ => "",
    }
}
