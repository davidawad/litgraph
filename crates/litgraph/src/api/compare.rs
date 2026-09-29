// SPDX-License-Identifier: GPL-3.0-or-later
//! `compare`: run one op under a base scenario and under the same scenario
//! merge-patched with a variant, and report the deltas.

use serde_json::{json, Value};

use super::{handle, Catalog, Op, Request, Response};
use crate::error::{Error, Result};
use crate::model::{merge_patch, Pack};
use crate::scenario::Scenario;

// `sc` is the already-resolved base scenario (named/composed/inline alike)
// and `packs` its already-resolved pack list: re-running `inner` under both
// scenarios must reuse that resolution exactly, not re-interpret `req`'s raw
// `scenario`/`packs` fields a second time (which, for a named scenario with
// an empty `req.packs`, would otherwise silently fall back to "every pack").
pub(super) fn compare(
    sc: &Scenario,
    packs: &[Pack],
    req: &Request,
    catalog: &Catalog,
    variant: &Value,
    inner: &Op,
) -> Result<Value> {
    let base_json = serde_json::to_value(sc).map_err(|e| Error::Invalid(e.to_string()))?;
    let mut merged = base_json.clone();
    merge_patch(&mut merged, variant);
    let variant_sc: Scenario =
        serde_json::from_value(merged).map_err(|e| Error::Parse(format!("variant: {e}")))?;
    let pack_ids: Vec<String> = packs.iter().map(|p| p.id.clone()).collect();
    let a = handle(
        &Request {
            op: inner.clone(),
            packs: pack_ids.clone(),
            scenario: base_json,
            ..req.clone()
        },
        catalog,
    );
    let b = handle(
        &Request {
            op: inner.clone(),
            packs: pack_ids,
            scenario: serde_json::to_value(&variant_sc)
                .map_err(|e| Error::Invalid(e.to_string()))?,
            ..req.clone()
        },
        catalog,
    );
    let get = |r: &Response, k: &str| {
        r.result
            .as_ref()
            .and_then(|x| x.get(k))
            .and_then(Value::as_f64)
    };
    let delta = |k: &str| match (get(&a, k), get(&b, k)) {
        (Some(x), Some(y)) => json!(y - x),
        _ => Value::Null,
    };
    Ok(json!({
        "delta": { "value": delta("value"), "expected_utility": delta("expected_utility"), "expected_net": delta("expected_net") },
        "base": a,
        "variant": b,
    }))
}
