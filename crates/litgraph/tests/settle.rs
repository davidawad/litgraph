// SPDX-License-Identifier: GPL-3.0-or-later
//! The `settle` op: closed-form ZOPA / Nash / Rubinstein prices on a
//! two-outcome game, the defendant's-eye view, risk attitudes, timing,
//! Rule 68 cost shifting, argument validation, and property tests (every
//! price inside the ZOPA; zero surplus is no deal).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::api::{handle_json, Catalog, Response};
use proptest::prelude::*;
use serde_json::{json, Value};

/// start --file (ours, 10h, 365 days)--> trial --defend (theirs, 20h)-->
/// ruling --p--> win (`W`) | lose (0). `mid` adds a second judgment
/// terminal paying `mid` with probability `p_mid` (taken from `lose`).
fn game(p: f64, w: f64, hours_p: f64, hours_d: f64, mid: Option<(f64, f64)>) -> Value {
    let (p_mid, mid_payoff) = mid.unwrap_or((0.0, 0.0));
    json!({
        "schemaVersion": 2, "id": "ct", "title": "ct", "startNodeId": "start",
        "roles": { "applicant": "self", "examiner": "opponent", "office": "nature" },
        "nodes": [
            { "id": "start", "kind": "decision", "label": "start" },
            { "id": "trial", "kind": "state", "label": "trial" },
            { "id": "ruling", "kind": "state", "label": "ruling" },
            { "id": "win", "kind": "terminal", "label": "win", "payoff": w, "outcome": ["win", "judgment"] },
            { "id": "mid", "kind": "terminal", "label": "mid", "payoff": mid_payoff, "outcome": ["win", "judgment"] },
            { "id": "lose", "kind": "terminal", "label": "lose", "payoff": 0.0, "outcome": ["loss", "judgment"] }
        ],
        "edges": [
            { "id": "file", "from": "start", "to": "trial", "label": "file", "actor": "applicant",
              "hours": hours_p, "duration": { "min": 365, "mode": 365, "max": 365 } },
            { "id": "defend", "from": "trial", "to": "ruling", "label": "defend", "actor": "examiner", "hours": hours_d },
            { "id": "to-win", "from": "ruling", "to": "win", "label": "win", "actor": "office", "probability": p },
            { "id": "to-mid", "from": "ruling", "to": "mid", "label": "mid", "actor": "office", "probability": p_mid },
            { "id": "to-lose", "from": "ruling", "to": "lose", "label": "lose", "actor": "office", "probability": 1.0 - p - p_mid }
        ]
    })
}

/// start --> motion: dismissed (0.4) | trial (0.6); trial --file--> ruling
/// --defend--> verdict: win (0.5) | lose. Surplus peaks after the motion.
fn motion_game() -> Value {
    json!({
        "schemaVersion": 2, "id": "tm", "title": "tm", "startNodeId": "start",
        "roles": { "applicant": "self", "examiner": "opponent", "office": "nature" },
        "nodes": [
            { "id": "start", "kind": "decision", "label": "start" },
            { "id": "motion", "kind": "state", "label": "motion" },
            { "id": "dismissed", "kind": "terminal", "label": "dismissed", "payoff": 0.0 },
            { "id": "trial", "kind": "decision", "label": "trial" },
            { "id": "ruling", "kind": "state", "label": "ruling" },
            { "id": "verdict", "kind": "state", "label": "verdict" },
            { "id": "win", "kind": "terminal", "label": "win", "payoff": 10000.0 },
            { "id": "lose", "kind": "terminal", "label": "lose", "payoff": 0.0 }
        ],
        "edges": [
            { "id": "move", "from": "start", "to": "motion", "label": "move", "actor": "applicant" },
            { "id": "grant", "from": "motion", "to": "dismissed", "label": "grant", "actor": "office", "probability": 0.4 },
            { "id": "deny", "from": "motion", "to": "trial", "label": "deny", "actor": "office", "probability": 0.6 },
            { "id": "file", "from": "trial", "to": "ruling", "label": "file", "actor": "applicant", "hours": 10.0 },
            { "id": "defend", "from": "ruling", "to": "verdict", "label": "defend", "actor": "examiner", "hours": 20.0 },
            { "id": "to-win", "from": "verdict", "to": "win", "label": "win", "actor": "office", "probability": 0.5 },
            { "id": "to-lose", "from": "verdict", "to": "lose", "label": "lose", "actor": "office", "probability": 0.5 }
        ]
    })
}

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

fn plaintiff_scenario() -> Value {
    json!({
        "params": { "rate": 100, "opp_rate": 50 },
        "cost": "self_dollars",
        "opponent_objective": "-payoff"
    })
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * a.abs().max(b.abs()).max(1.0)
}

#[test]
fn two_outcome_game_matches_the_hand_computation() {
    let op = json!({
        "op": "settle", "bargaining_power": 0.25,
        "discount_annual": { "plaintiff": 0.1, "defendant": 0.2 }
    });
    let (r, _) = ok(
        &game(0.6, 10000.0, 10.0, 20.0, None),
        &plaintiff_scenario(),
        &op,
    );
    // Plaintiff: 0.6 * 10000 - 10h * $100 = 5000. Defendant pays at most
    // 0.6 * 10000 + 20h * $50 = 7000.
    assert_eq!(f(&r["plaintiff_reservation"]), 5000.0);
    assert_eq!(f(&r["defendant_reservation"]), 7000.0);
    assert_eq!(r["deal"], json!(true));
    assert_eq!(f(&r["zopa"]["surplus"]), 2000.0);
    assert_eq!(f(&r["values"]["opponent"]), -7000.0);
    let p = &r["prices"];
    assert_eq!(f(&p["midpoint"]), 6000.0);
    assert_eq!(f(&p["nash_symmetric"]), 6000.0);
    assert_eq!(f(&p["nash_weighted"]["price"]), 5500.0);
    // One 365-day round: δp = 1/1.1, δd = 1/1.2; the proposer keeps
    // (1 - δ_other) / (1 - δp·δd) of the surplus.
    let rb = &p["rubinstein"];
    assert_eq!(f(&rb["round_days"]), 365.0);
    assert!(close(f(&rb["plaintiff_first"]), 6375.0));
    assert!(close(f(&rb["defendant_first"]), 6250.0));
    let share = 1.2f64.ln() / (1.1f64.ln() + 1.2f64.ln());
    assert!(close(f(&rb["limit"]), 5000.0 + 2000.0 * share));
}

#[test]
fn walk_away_values_match_solve() {
    let pack = game(0.6, 10000.0, 10.0, 20.0, None);
    let (s, _) = ok(&pack, &plaintiff_scenario(), &json!({ "op": "solve" }));
    let (r, _) = ok(&pack, &plaintiff_scenario(), &json!({ "op": "settle" }));
    assert_eq!(r["values"]["self"], s["value"]);
    assert_eq!(r["values"]["opponent"], s["opponent_value"]);
}

#[test]
fn the_defendants_view_gives_the_same_range() {
    let sc = json!({
        "params": { "rate": 50, "opp_rate": 100 },
        "perspective": { "applicant": "opponent", "examiner": "self" },
        "cost": "self_dollars",
        "utility": "-payoff",
        "opponent_objective": "payoff"
    });
    let op = json!({ "op": "settle", "self_side": "defendant" });
    let (r, _) = ok(&game(0.6, 10000.0, 10.0, 20.0, None), &sc, &op);
    assert_eq!(f(&r["plaintiff_reservation"]), 5000.0);
    assert_eq!(f(&r["defendant_reservation"]), 7000.0);
    assert_eq!(f(&r["prices"]["midpoint"]), 6000.0);
    assert_eq!(r["self_side"], json!("defendant"));
}

#[test]
fn zero_sum_has_no_surplus_and_says_why() {
    let sc = json!({ "params": { "rate": 100 }, "cost": "self_dollars" });
    let (r, codes) = ok(
        &game(0.6, 10000.0, 10.0, 20.0, None),
        &sc,
        &json!({ "op": "settle" }),
    );
    assert_eq!(r["deal"], json!(false));
    assert_eq!(f(&r["no_deal_gap"]), 0.0);
    assert!(r["prices"].is_null());
    assert!(r["zopa"].is_null());
    assert!(codes.contains(&"settle-zero-sum".to_string()));
}

#[test]
fn a_negative_gap_is_reported_as_no_deal() {
    // The defendant thinks the plaintiff wins far less often than the
    // plaintiff does: its objective discounts the judgment to 20%.
    let sc = json!({
        "params": { "rate": 100, "opp_rate": 50 },
        "cost": "self_dollars",
        "opponent_objective": "-0.2 * payoff"
    });
    let (r, _) = ok(
        &game(0.6, 10000.0, 10.0, 20.0, None),
        &sc,
        &json!({ "op": "settle" }),
    );
    // Plaintiff 5000; defendant pays at most 0.2 * 6000 + 1000 = 2200.
    assert_eq!(r["deal"], json!(false));
    assert_eq!(f(&r["no_deal_gap"]), 2800.0);
    assert!(r["timing"]["peak"].is_null());
}

#[test]
fn risk_attitudes_move_the_walk_away_points() {
    let pack = game(0.6, 10000.0, 10.0, 20.0, None);
    // A worst-case or CVaR(0.4) defendant sees the judgment for sure.
    for risk in [
        json!({ "type": "worst" }),
        json!({ "type": "cvar", "alpha": 0.4 }),
    ] {
        let op = json!({ "op": "settle", "opponent_risk": risk, "runs": 2000 });
        let (r, codes) = ok(&pack, &plaintiff_scenario(), &op);
        assert_eq!(f(&r["defendant_reservation"]), 11000.0, "{risk}");
        assert_eq!(
            codes.contains(&"settle-cvar-sampled".to_string()),
            risk["type"] == "cvar"
        );
    }
    // A CARA plaintiff's certainty equivalent is below its expectation.
    let a = 1e-4;
    let mut sc = plaintiff_scenario();
    sc["objective"] = json!({ "type": "cara", "a": a });
    let (r, _) = ok(&pack, &sc, &json!({ "op": "settle" }));
    let ce = -(0.6 * (-a * 10000.0f64).exp() + 0.4).ln() / a - 1000.0;
    assert!(close(f(&r["plaintiff_reservation"]), ce));
    // ... and it concedes part of the surplus in the Nash solution.
    assert!(f(&r["prices"]["nash_symmetric"]) < f(&r["prices"]["midpoint"]));
}

#[test]
fn a_cvar_self_objective_is_sampled_and_flagged() {
    let mut sc = plaintiff_scenario();
    sc["objective"] = json!({ "type": "cvar", "alpha": 0.3 });
    let (r, codes) = ok(
        &game(0.6, 10000.0, 10.0, 20.0, None),
        &sc,
        &json!({ "op": "settle", "runs": 500 }),
    );
    // The worst 30% of the plaintiff's outcomes all lose: -1000.
    assert_eq!(f(&r["plaintiff_reservation"]), -1000.0);
    assert!(codes.contains(&"settle-cvar-policy".to_string()));
}

#[test]
fn timing_finds_the_peak_after_the_ruling_and_the_stopping_policy() {
    let (r, _) = ok(
        &motion_game(),
        &plaintiff_scenario(),
        &json!({ "op": "settle" }),
    );
    let t = &r["timing"];
    // Joint remaining costs are 0.6 * 2000 before the motion is decided
    // and the full 2000 once it is denied.
    assert_eq!(f(&r["zopa"]["surplus"]), 1200.0);
    assert_eq!(t["peak"], json!("tm::trial"));
    assert_eq!(f(&t["peak_surplus"]), 2000.0);
    assert!(f(&t["value_with_settlement"]) >= f(&t["value_litigate"]));
    let line = t["line"].as_array().unwrap();
    assert_eq!(line[0]["node"]["id"], json!("tm::start"));
    // After the defense's step there is nothing left to save: no deal.
    let verdict = line
        .iter()
        .find(|p| p["node"]["id"] == "tm::verdict")
        .unwrap();
    assert_eq!(verdict["deal"], json!(false));
    assert!(verdict["price"].is_null());
    // Settling at trial beats paying for it: the policy there changes.
    let trial = line
        .iter()
        .find(|p| p["node"]["id"] == "tm::trial")
        .unwrap();
    assert_eq!(trial["self_settles"], json!(true));
    let changes = t["policy_changes"].as_array().unwrap();
    assert!(changes
        .iter()
        .any(|c| c["node"] == "tm::trial" && c["with_settlement"] == "settle"));
    assert_eq!(t["first_settle"], json!("tm::trial"));
}

#[test]
fn evaluating_from_a_later_node_uses_its_range() {
    let op = json!({ "op": "settle", "node": "trial" });
    let (r, _) = ok(
        &game(0.6, 10000.0, 10.0, 20.0, None),
        &plaintiff_scenario(),
        &op,
    );
    assert_eq!(f(&r["plaintiff_reservation"]), 6000.0);
    assert_eq!(f(&r["defendant_reservation"]), 7000.0);
}

#[test]
fn rule68_shifts_post_offer_costs_on_a_judgment_not_more_favorable() {
    // win 10000 (0.5), mid 3000 (0.3), lose 0 (0.2): plaintiff expects
    // 5900 - 1000 = 4900. An offer of 4000 with $500 of costs: only `mid`
    // (positive, at most the offer) triggers 68(d); `lose` is a judgment
    // for the defendant, where the rule does not apply.
    let pack = game(0.5, 10000.0, 10.0, 20.0, Some((0.3, 3000.0)));
    let op = json!({ "op": "settle", "rule68": { "offer": 4000, "costs": 500 } });
    let (r, _) = ok(&pack, &plaintiff_scenario(), &op);
    let x = &r["rule68"];
    assert_eq!(x["triggered"], json!(["ct::mid"]));
    assert!(close(f(&x["p_triggered"]), 0.3));
    assert!(close(f(&x["plaintiff_reject_value"]), 4900.0 - 150.0));
    assert!(close(f(&x["defendant_reject_value"]), -6900.0 + 150.0));
    assert!(close(f(&x["zopa"]["surplus"]), 2000.0));
    assert_eq!(x["plaintiff_accepts"], json!(false));
    assert_eq!(x["self_is_offeror"], json!(false));
    // At 4750 or more the plaintiff takes it.
    let op = json!({ "op": "settle", "rule68": { "offer": 4800, "costs": 500 } });
    let (r, _) = ok(&pack, &plaintiff_scenario(), &op);
    assert_eq!(r["rule68"]["plaintiff_accepts"], json!(true));
    // A custom eligibility expression sees the offer as `offer`.
    let op = json!({ "op": "settle", "rule68": { "offer": 4000, "costs": 500, "eligible": "payoff <= offer" } });
    let (r, _) = ok(&pack, &plaintiff_scenario(), &op);
    assert_eq!(r["rule68"]["triggered"], json!(["ct::mid", "ct::lose"]));
}

#[test]
fn rule68_under_a_cvar_plaintiff_is_sampled() {
    let pack = game(0.5, 10000.0, 10.0, 20.0, Some((0.3, 3000.0)));
    let mut sc = plaintiff_scenario();
    sc["objective"] = json!({ "type": "cvar", "alpha": 0.1 });
    let op = json!({ "op": "settle", "runs": 1000, "rule68": { "offer": 4000, "costs": 500 } });
    let (r, _) = ok(&pack, &sc, &op);
    // The worst 10% lose outright (no judgment for the plaintiff, no shift).
    assert_eq!(f(&r["rule68"]["plaintiff_reject_value"]), -1000.0);
}

#[test]
fn bad_arguments_are_rejected_by_name() {
    let pack = game(0.6, 10000.0, 10.0, 20.0, None);
    let bad = [
        json!({ "op": "settle", "bargaining_power": 1.5 }),
        json!({ "op": "settle", "delta": { "plaintiff": 0.0 } }),
        json!({ "op": "settle", "discount_annual": { "defendant": -2.0 } }),
        json!({ "op": "settle", "runs": 0 }),
        json!({ "op": "settle", "rule68": { "offer": 1.0, "costs": -1.0 } }),
        json!({ "op": "settle", "opponent_risk": { "type": "cvar", "alpha": 0.0 } }),
        json!({ "op": "settle", "node": "nowhere" }),
    ];
    for op in bad {
        let r = run(&pack, &plaintiff_scenario(), &op);
        assert!(!r.ok, "{op}");
    }
    let r = run(
        &pack,
        &plaintiff_scenario(),
        &json!({ "op": "settle", "unknown": 1 }),
    );
    assert_eq!(r.error.unwrap().code, "parse");
}

#[test]
fn settle_is_discoverable() {
    let catalog = Catalog::embedded().unwrap();
    let d = handle_json(r#"{"op":{"op":"describe"}}"#, &catalog);
    assert!(d.result.unwrap()["ops"]["settle"].is_string());
    let s = litgraph::api::schema("request").unwrap().to_string();
    assert!(s.contains("\"settle\"") && s.contains("bargaining_power") && s.contains("rule68"));
}

#[test]
fn the_cofc_1498_example_runs() {
    let catalog = Catalog::embedded()
        .unwrap()
        .with_embedded_scenarios()
        .unwrap();
    let text = include_str!("../../../examples/cofc-1498-settle.json");
    let r = handle_json(text, &catalog);
    assert!(r.ok, "{:?}", r.error);
    let res = r.result.unwrap();
    assert_eq!(res["deal"], json!(true));
    // The surplus peaks once the government's RCFC 12(b) motion is denied.
    assert_eq!(res["timing"]["peak"], json!("cofc::answer-filed"));
    let low = f(&res["zopa"]["low"]);
    let high = f(&res["zopa"]["high"]);
    for k in ["midpoint", "nash_symmetric"] {
        let p = f(&res["prices"][k]);
        assert!(low <= p && p <= high);
    }
    assert!(res["rule68"]["plaintiff_accepts"].is_boolean());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Every predicted price lies inside the ZOPA, and the risk-neutral
    /// surplus is exactly the two sides' remaining costs.
    #[test]
    fn prices_stay_inside_the_zopa(
        p in 0.05f64..0.95, w in 1e3f64..1e6, hp in 0.0f64..100.0, hd in 0.0f64..100.0,
        beta in 0.0f64..=1.0, a in 0.0f64..1e-5, rp in 0.0f64..0.3, rd in 0.0f64..0.3,
    ) {
        let mut sc = plaintiff_scenario();
        sc["objective"] = json!({ "type": "cara", "a": a });
        let op = json!({
            "op": "settle", "bargaining_power": beta,
            "opponent_risk": { "type": "cara", "a": a / 2.0 },
            "discount_annual": { "plaintiff": rp, "defendant": rd }
        });
        let (r, _) = ok(&game(p, w, hp, hd, None), &sc, &op);
        if r["deal"] == json!(true) {
            let (lo, hi) = (f(&r["zopa"]["low"]), f(&r["zopa"]["high"]));
            let tol = 1e-6 * hi.abs().max(1.0);
            let pr = &r["prices"];
            for x in [&pr["midpoint"], &pr["nash_symmetric"], &pr["nash_weighted"]["price"],
                      &pr["rubinstein"]["plaintiff_first"], &pr["rubinstein"]["defendant_first"],
                      &pr["rubinstein"]["limit"]] {
                prop_assert!(f(x) >= lo - tol && f(x) <= hi + tol, "{x} outside [{lo}, {hi}]");
            }
            for pt in r["timing"]["line"].as_array().unwrap() {
                if let Some(x) = pt["price"].as_f64() {
                    prop_assert!(x >= f(&pt["plaintiff_reservation"]) - tol);
                    prop_assert!(x <= f(&pt["defendant_reservation"]) + tol);
                }
            }
        } else {
            prop_assert!(r["prices"].is_null());
        }
    }

    /// With nothing left to spend, a risk-neutral case has zero surplus:
    /// no deal, whatever the stakes.
    #[test]
    fn zero_surplus_is_no_deal(p in 0.0f64..1.0, w in 0.0f64..1e7) {
        let (r, _) = ok(&game(p, w, 0.0, 0.0, None), &plaintiff_scenario(), &json!({ "op": "settle" }));
        prop_assert_eq!(&r["deal"], &json!(false));
        prop_assert!(f(&r["no_deal_gap"]).abs() <= 1e-6 * w.max(1.0));
        prop_assert!(r["timing"]["peak"].is_null());
        prop_assert!(r["timing"]["first_settle"].is_null());
    }

    /// Risk-neutral surplus equals the remaining costs of both sides.
    #[test]
    fn surplus_is_the_remaining_joint_cost(p in 0.0f64..1.0, w in 0.0f64..1e6, hp in 0.1f64..100.0, hd in 0.1f64..100.0) {
        let (r, _) = ok(&game(p, w, hp, hd, None), &plaintiff_scenario(), &json!({ "op": "settle" }));
        let want = hp * 100.0 + hd * 50.0;
        prop_assert!(close(f(&r["zopa"]["surplus"]), want));
    }
}
