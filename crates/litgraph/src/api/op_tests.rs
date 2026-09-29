// SPDX-License-Identifier: GPL-3.0-or-later
//! Unit tests for `op.rs`.

use super::*;

/// A single comma-separated string is split on `,`, trimmed, and empty
/// segments are dropped.
#[test]
fn one_or_many_splits_and_trims_a_comma_string() -> serde_json::Result<()> {
    let op: Op = serde_json::from_value(serde_json::json!({
        "op": "chain",
        "metrics": "dollars, elapsed,, hours "
    }))?;
    match op {
        Op::Chain { metrics, .. } => {
            assert_eq!(metrics, vec!["dollars", "elapsed", "hours"]);
        }
        other => panic!("expected Chain, got {other:?}"),
    }
    Ok(())
}

/// A JSON array is taken as-is (the `Many` branch of `OneOrMany`).
#[test]
fn one_or_many_accepts_an_array() -> serde_json::Result<()> {
    let op: Op = serde_json::from_value(serde_json::json!({
        "op": "chain",
        "metrics": ["dollars", "hours"]
    }))?;
    match op {
        Op::Chain { metrics, .. } => assert_eq!(metrics, vec!["dollars", "hours"]),
        other => panic!("expected Chain, got {other:?}"),
    }
    Ok(())
}
