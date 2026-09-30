// SPDX-License-Identifier: GPL-3.0-or-later
//! Golden graphs: tiny packs (2-6 nodes) whose answers are worked out by
//! hand. Each `tests/golden/<name>.json` carries the pack, one request op,
//! the expected slice of the op's `result`, and an `arithmetic` list that
//! shows how every expected number was computed, so a reviewer can check
//! it with a pencil. The runner sends the op through the public JSON
//! contract (`api::handle`), exactly as the CLI would.
//!
//! Matching is by subset: every key in `expect` must be present in the
//! result and equal; arrays must match element by element, in order. Numbers
//! must agree to 1e-9, unless `tolerance` names a looser bound for a JSON
//! pointer (used only for sampled quantities in `simulate`, with the
//! standard-error arithmetic shown).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use litgraph::api::{handle, Catalog, Request};
use serde::Deserialize;
use serde_json::{json, Value};

/// One golden file.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Golden {
    name: String,
    what: String,
    arithmetic: Vec<String>,
    pack: Value,
    op: Value,
    expect: Value,
    #[serde(default)]
    tolerance: BTreeMap<String, f64>,
}

fn load(name: &str) -> Golden {
    let path = format!(
        "{}/../../tests/golden/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let g: Golden = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(g.name, name, "{path}: `name` must match the file name");
    assert!(
        !g.what.is_empty() && !g.arithmetic.is_empty(),
        "{path}: explain the numbers"
    );
    g
}

/// Assert `actual` contains `expected` (see the module doc), naming the
/// JSON pointer of the first mismatch.
fn assert_subset(expected: &Value, actual: &Value, at: &str, tol: &BTreeMap<String, f64>) {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            for (k, ev) in e {
                let av = a
                    .get(k)
                    .unwrap_or_else(|| panic!("{at}/{k}: missing from result {actual}"));
                assert_subset(ev, av, &format!("{at}/{k}"), tol);
            }
        }
        (Value::Array(e), Value::Array(a)) => {
            assert_eq!(e.len(), a.len(), "{at}: length (result {actual})");
            for (i, (ev, av)) in e.iter().zip(a).enumerate() {
                assert_subset(ev, av, &format!("{at}/{i}"), tol);
            }
        }
        (Value::Number(e), Value::Number(a)) => {
            let (e, a) = (e.as_f64().unwrap(), a.as_f64().unwrap());
            let t = tol.get(at).copied().unwrap_or(1e-9);
            assert!((e - a).abs() <= t, "{at}: expected {e} ± {t}, got {a}");
        }
        _ => assert_eq!(expected, actual, "{at}"),
    }
}

fn check(name: &str) {
    let g = load(name);
    let id = g.pack["id"].as_str().expect("pack id").to_string();
    let catalog = Catalog::from_files(
        "golden".into(),
        vec![(format!("{id}.json"), g.pack.to_string())],
    )
    .expect("golden pack compiles");
    let req: Request =
        serde_json::from_value(json!({"packs": [id], "op": g.op})).expect("request parses");
    let resp = handle(&req, &catalog);
    assert!(resp.ok, "{name}: {resp:?}");
    assert_subset(&g.expect, &resp.result.unwrap(), "", &g.tolerance);
}

macro_rules! golden {
    ($test:ident, $file:literal) => {
        #[test]
        fn $test() {
            check($file);
        }
    };
}

golden!(
    a_single_step_is_worth_its_payoff_minus_fees_and_billed_hours,
    "one-step-solve"
);
golden!(
    suing_beats_settling_when_the_expected_verdict_exceeds_the_offer,
    "sue-or-settle-solve"
);
golden!(
    the_chain_splits_the_sue_line_between_win_and_loss,
    "sue-or-settle-chain"
);
golden!(
    simulating_the_sue_line_converges_to_the_exact_expectation,
    "sue-or-settle-simulate"
);
golden!(
    a_remand_loop_is_absorbed_with_geometric_retrial_costs,
    "remand-loop-chain"
);
golden!(
    k_shortest_paths_rank_the_three_routes_by_cost,
    "diamond-k-shortest-paths"
);
golden!(
    the_min_cut_is_the_two_edges_into_the_sink,
    "diamond-min-cut"
);
