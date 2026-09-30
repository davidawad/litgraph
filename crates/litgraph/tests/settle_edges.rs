// SPDX-License-Identifier: GPL-3.0-or-later
//! `settle` on awkward graphs: dead ends, forced moves, act-or-wait nodes,
//! a defendant-side Rule 68 offer, and an opponent who can stall forever
//! (non-convergence and truncated Monte Carlo runs reported, not hidden).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::api::{handle_json, Catalog, Response};
use serde_json::{json, Value};

fn run(pack: &Value, scenario: &Value, op: &Value) -> Response {
    let catalog =
        Catalog::from_files("t".into(), vec![("t.json".into(), pack.to_string())]).unwrap();
    let id = pack["id"].as_str().unwrap();
    let req = json!({ "packs": [id], "scenario": scenario, "op": op });
    handle_json(&req.to_string(), &catalog)
}

fn ok(pack: &Value, scenario: &Value, op: &Value) -> (Value, Vec<String>) {
    let r = run(pack, scenario, op);
    assert!(r.ok, "{:?}", r.error);
    let codes = r.warnings.iter().map(|w| w.code.clone()).collect();
    (r.result.unwrap(), codes)
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

/// start (ours): `file` → trial, `concede` → lose; trial (ours, act-or-wait):
/// `push` → ruling, or wait for the court (`court`, no probability) → ruling;
/// ruling --defend (theirs, 20h)--> verdict: win 10000 (0.6) | lose.
fn pack() -> Value {
    json!({
        "schemaVersion": 2, "id": "aw", "title": "aw", "startNodeId": "start",
        "roles": { "applicant": "self", "examiner": "opponent", "office": "nature" },
        "nodes": [
            { "id": "start", "kind": "decision", "label": "start" },
            { "id": "trial", "kind": "decision", "label": "trial" },
            { "id": "ruling", "kind": "state", "label": "ruling" },
            { "id": "verdict", "kind": "state", "label": "verdict" },
            { "id": "win", "kind": "terminal", "label": "win", "payoff": 10000.0, "outcome": ["judgment"] },
            { "id": "lose", "kind": "terminal", "label": "lose", "payoff": 0.0, "outcome": ["judgment"] }
        ],
        "edges": [
            { "id": "file", "from": "start", "to": "trial", "label": "file", "actor": "applicant", "hours": 10.0 },
            { "id": "concede", "from": "start", "to": "lose", "label": "concede", "actor": "applicant" },
            { "id": "push", "from": "trial", "to": "ruling", "label": "push", "actor": "applicant", "hours": 5.0 },
            { "id": "court", "from": "trial", "to": "ruling", "label": "court", "actor": "office" },
            { "id": "defend", "from": "ruling", "to": "verdict", "label": "defend", "actor": "examiner", "hours": 20.0 },
            { "id": "to-win", "from": "verdict", "to": "win", "label": "win", "actor": "office", "probability": 0.6 },
            { "id": "to-lose", "from": "verdict", "to": "lose", "label": "lose", "actor": "office", "probability": 0.4 }
        ]
    })
}

fn scenario() -> Value {
    json!({
        "params": { "rate": 100, "opp_rate": 50 },
        "cost": "self_dollars",
        "opponent_objective": "-payoff"
    })
}

#[test]
fn forced_moves_and_waiting_are_honored() {
    // Waiting at trial is free; forcing `push` costs 5h more.
    let mut sc = scenario();
    let (free, _) = ok(&pack(), &sc, &json!({ "op": "settle" }));
    sc["policy"] = json!({ "aw::trial": "aw::push" });
    let (pushed, _) = ok(&pack(), &sc, &json!({ "op": "settle" }));
    assert_eq!(
        f(&free["plaintiff_reservation"]) - f(&pushed["plaintiff_reservation"]),
        500.0
    );
    sc["policy"] = json!({ "aw::trial": "aw::court" });
    let (waited, _) = ok(&pack(), &sc, &json!({ "op": "settle" }));
    assert_eq!(
        waited["plaintiff_reservation"],
        free["plaintiff_reservation"]
    );
    assert!(f(&waited["timing"]["value_with_settlement"]) >= f(&waited["plaintiff_reservation"]));
}

#[test]
fn a_dead_end_is_worth_nothing_to_either_side() {
    let mut sc = scenario();
    sc["remove_edges"] = json!(["aw::defend"]);
    let op = json!({ "op": "settle", "node": "ruling", "opponent_risk": { "type": "cvar", "alpha": 0.5 }, "runs": 50 });
    let (r, codes) = ok(&pack(), &sc, &op);
    assert_eq!(f(&r["plaintiff_reservation"]), 0.0);
    assert_eq!(f(&r["defendant_reservation"]), 0.0);
    assert_eq!(r["deal"], json!(false));
    assert!(codes.contains(&"sink".to_string()), "{codes:?}");
    // From the start, filing now leads nowhere: the plaintiff concedes.
    let (r, _) = ok(&pack(), &sc, &json!({ "op": "settle" }));
    assert_eq!(f(&r["plaintiff_reservation"]), 0.0);
}

#[test]
fn a_defendant_self_evaluates_a_rule68_offer_it_serves() {
    let sc = json!({
        "params": { "rate": 50, "opp_rate": 100 },
        "perspective": { "applicant": "opponent", "examiner": "self" },
        "cost": "self_dollars",
        "utility": "-payoff",
        "opponent_objective": "payoff"
    });
    let op = json!({
        "op": "settle", "self_side": "defendant",
        "rule68": { "offer": 12000, "costs": 300 }
    });
    let (r, _) = ok(&pack(), &sc, &op);
    let x = &r["rule68"];
    assert_eq!(x["self_is_offeror"], json!(true));
    // Both judgments are within the offer, but only `win` is for the plaintiff.
    assert_eq!(x["triggered"], json!(["aw::win"]));
    assert!((f(&x["p_triggered"]) - 0.6).abs() < 1e-9);
    let base = f(&r["plaintiff_reservation"]);
    assert!((f(&x["plaintiff_reject_value"]) - (base - 180.0)).abs() < 1e-6);
    assert_eq!(x["plaintiff_accepts"], json!(true));
}

/// The opponent may stall forever on an edge that costs us each time.
fn stall_pack() -> Value {
    json!({
        "schemaVersion": 2, "id": "st", "title": "st", "startNodeId": "start",
        "roles": { "applicant": "self", "examiner": "opponent", "office": "nature" },
        "nodes": [
            { "id": "start", "kind": "state", "label": "start" },
            { "id": "paid", "kind": "terminal", "label": "paid", "payoff": 100.0 }
        ],
        "edges": [
            { "id": "stall", "from": "start", "to": "start", "label": "stall", "actor": "examiner", "hours": 1.0 },
            { "id": "pay", "from": "start", "to": "paid", "label": "pay", "actor": "examiner" }
        ]
    })
}

#[test]
fn a_stalling_opponent_is_reported_not_hidden() {
    let sc = json!({
        "params": { "rate": 100, "opp_rate": 0 },
        "cost": "dollars",
        "opponent_objective": "-payoff"
    });
    let op =
        json!({ "op": "settle", "opponent_risk": { "type": "cvar", "alpha": 0.5 }, "runs": 3 });
    let (_, codes) = ok(&stall_pack(), &sc, &op);
    assert!(codes.contains(&"not-converged".to_string()), "{codes:?}");
    assert!(codes.contains(&"settle-truncated".to_string()), "{codes:?}");
}
