// SPDX-License-Identifier: GPL-3.0-or-later
//! The uncertainty ops through the request/response contract: `posterior`,
//! `voi` (studies priced by `cost` or `cost_edge`), `objective: robust` at
//! interrupts and act-or-wait nodes, their errors and warnings, and their
//! discoverability through `describe` / `schema`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::algo::mdp;
use litgraph::api::{describe, handle, schema, Catalog, Request, Response};
use litgraph::model::{CompileOptions, Graph, LinkFile, Pack};
use litgraph::scenario::{GroupKind, Scenario, View};
use serde_json::{json, Value};

/// `start` is a nature-first interrupt (a 20% sua sponte dismissal) before
/// we choose to `settle` ($40), `try` the case (45% for $100), or `discover`
/// ($5) into `wait`, an act-or-wait node whose unauthored world edges fire
/// unless we `move` to settle.
fn pack() -> String {
    json!({
        "schemaVersion": 2, "id": "m", "title": "motions", "startNodeId": "start",
        "nodes": [
            {"id": "start", "label": "start"},
            {"id": "trial", "label": "trial"},
            {"id": "wait", "label": "discovery"},
            {"id": "win", "label": "win", "kind": "terminal", "payoff": 100.0},
            {"id": "lose", "label": "lose", "kind": "terminal", "payoff": 0.0},
            {"id": "settled", "label": "settled", "kind": "terminal", "payoff": 40.0},
            {"id": "dismissed", "label": "dismissed", "kind": "terminal", "payoff": 0.0}
        ],
        "edges": [
            {"id": "sua-sponte", "from": "start", "to": "dismissed", "label": "sua sponte dismissal", "actor": "examiner", "probability": 0.2},
            {"id": "settle", "from": "start", "to": "settled", "label": "settle", "actor": "applicant"},
            {"id": "try", "from": "start", "to": "trial", "label": "try the case", "actor": "applicant"},
            {"id": "discover", "from": "start", "to": "wait", "label": "take discovery", "actor": "applicant", "cost": 5.0},
            {"id": "verdict-win", "from": "trial", "to": "win", "label": "verdict for us", "actor": "examiner", "probability": 0.45},
            {"id": "verdict-lose", "from": "trial", "to": "lose", "label": "verdict against", "actor": "examiner", "probability": 0.55},
            {"id": "move", "from": "wait", "to": "settled", "label": "move to settle", "actor": "applicant"},
            {"id": "w-win", "from": "wait", "to": "win", "label": "discovery helps", "actor": "examiner"},
            {"id": "w-lose", "from": "wait", "to": "lose", "label": "discovery hurts", "actor": "examiner"}
        ]
    })
    .to_string()
}

fn catalog() -> Catalog {
    Catalog::from_files("test".into(), [("m.json".into(), pack())]).unwrap()
}

fn run(scenario: Value, op: Value) -> Response {
    let req = Request {
        packs: vec!["m".into()],
        scenario,
        op: serde_json::from_value(op).unwrap(),
        ..Request::default()
    };
    handle(&req, &catalog())
}

fn result(resp: &Response) -> &Value {
    assert!(resp.ok, "{:?}", resp.error);
    resp.result.as_ref().unwrap()
}

#[test]
fn interrupts_and_waits_are_uncertain_groups() {
    let g = Graph::compile(
        &[Pack::from_json(&pack()).unwrap()],
        &LinkFile::default(),
        &CompileOptions::default(),
    )
    .unwrap();
    let sc: Scenario = serde_json::from_value(json!({
        "observe": {"m::start": {"m::sua-sponte": 2}, "m::wait": {"m::w-win": 3}}
    }))
    .unwrap();
    let v = View::new(&g, &sc).unwrap();
    let start = &v.belief.groups[v.belief.group_at(v.start).unwrap()];
    assert!(start.residual && start.kind == GroupKind::Draws);
    // Prior 10 · (0.2, 0.8) + (2, 0) → (4, 8): the residual mass is 8/12.
    assert!((v.plan[v.start].choice_mass - 8.0 / 12.0).abs() < 1e-12);
    let w = g.node("m::wait").unwrap();
    let wait = &v.belief.groups[v.belief.group_at(w).unwrap()];
    assert_eq!(wait.kind, GroupKind::Wait);
    // Fill (0.5, 0.5) · 10 + (3, 0) → (8, 5)/13.
    assert!((v.plan[w].wait[0].1 - 8.0 / 13.0).abs() < 1e-12);
    // Robust never beats nominal at either kind of node.
    let robust: Scenario =
        serde_json::from_value(json!({"objective": {"type": "robust"}})).unwrap();
    let rv = View::new(&g, &robust).unwrap();
    let nv = View::new(&g, &Scenario::default()).unwrap();
    let r = mdp::solve(&rv, &mdp::SolveOptions::default()).unwrap();
    let n = mdp::solve(&nv, &mdp::SolveOptions::default()).unwrap();
    for i in 0..g.nodes.len() {
        assert!(r.value[i] <= n.value[i] + 1e-9, "node {i}");
    }
    assert!(r.value[w] < n.value[w]);
}

#[test]
fn posterior_op_reports_intervals_and_optimality() {
    let resp = run(
        json!({}),
        json!({"op": "posterior", "samples": 300, "seed": 9}),
    );
    let res = result(&resp);
    assert_eq!(res["uncertain_nodes"], json!(3));
    let v = &res["value"];
    assert!(v["lo"].as_f64().unwrap() <= v["mean"].as_f64().unwrap());
    assert!(v["mean"].as_f64().unwrap() <= v["hi"].as_f64().unwrap());
    let opts = res["options"].as_array().unwrap();
    assert_eq!(opts.len(), 3);
    let total: f64 = opts.iter().map(|o| o["p_optimal"].as_f64().unwrap()).sum();
    assert!((total - 1.0).abs() < 1e-6);
    assert_eq!(
        opts.iter()
            .filter(|o| o["nominal_best"] == json!(true))
            .count(),
        1
    );
    assert!(resp
        .warnings
        .iter()
        .any(|w| w.code == "prior-concentration-estimated" && w.count == 3));

    // At the act-or-wait node, WAIT is an option.
    let resp = run(
        json!({}),
        json!({"op": "posterior", "node": "m::wait", "samples": 50}),
    );
    let opts = result(&resp)["options"].as_array().unwrap().clone();
    assert!(opts.iter().any(|o| o["edge"] == json!("WAIT")));

    // A non-expected objective is set aside, and says so.
    let resp = run(
        json!({"objective": {"type": "cara", "a": 0.01}}),
        json!({"op": "posterior", "samples": 10}),
    );
    assert!(resp.ok);
    assert!(resp
        .warnings
        .iter()
        .any(|w| w.code == "uncertainty-risk-neutral"));
}

#[test]
fn voi_op_prices_studies_against_their_cost() {
    let op = json!({
        "op": "voi", "samples": 300, "top": 5,
        "studies": [
            {"node": "m::trial", "k": 10, "cost": 1.0, "label": "mock jury"},
            {"node": "m::trial", "k": 10, "cost": 1000.0},
            {"node": "m::wait", "k": 2, "cost_edge": "m::discover"},
            {"node": "m::trial", "k": 1}
        ]
    });
    let resp = run(json!({}), op);
    let res = result(&resp);
    assert!(res["evpi_total"]["value"].as_f64().unwrap() >= 0.0);
    let nodes = res["nodes"].as_array().unwrap();
    assert_eq!(nodes.len(), 3);
    for n in nodes {
        assert!(n["evpi_outcome"].as_f64().unwrap() >= 0.0);
        assert!(n["evppi"]["value"].as_f64().unwrap() >= 0.0);
        assert_eq!(n["prior"], json!({"from": "estimate"}));
    }
    let st = res["studies"].as_array().unwrap();
    let evsi = st[0]["evsi"]["value"].as_f64().unwrap();
    assert!(evsi > 1.0, "{evsi}");
    assert_eq!(st[0]["label"], json!("mock jury"));
    assert_eq!(st[0]["worth_paying"], json!(true));
    assert_eq!(st[1]["worth_paying"], json!(false));
    assert!((st[1]["net"].as_f64().unwrap() - (evsi - 1000.0)).abs() < 1e-3);
    assert_eq!(st[2]["cost"], json!(5.0));
    assert_eq!(st[3]["cost"], Value::Null);
    assert_eq!(st[3]["worth_paying"], Value::Null);
    assert!(evsi <= st[0]["evpi_outcome"].as_f64().unwrap() + 1e-9);
}

#[test]
fn voi_op_errors_are_clear() {
    for (studies, needle) in [
        (
            json!([{"node": "m::trial", "k": 1, "cost": 1.0, "cost_edge": "m::try"}]),
            "not both",
        ),
        (json!([{"node": "m::win", "k": 1}]), "not a chance draw"),
        (json!([{"node": "m::trial", "k": 0}]), "k must be"),
        (
            json!([{"node": "m::trial", "k": 1, "cost_edge": "m::nope"}]),
            "nope",
        ),
    ] {
        let resp = run(
            json!({}),
            json!({"op": "voi", "samples": 5, "studies": studies}),
        );
        assert!(!resp.ok);
        let msg = resp.error.unwrap().message;
        assert!(msg.contains(needle), "{msg}");
    }
    // An inactive cost edge can't price anything.
    let resp = run(
        json!({"remove_edges": ["m::discover"]}),
        json!({"op": "voi", "samples": 5, "studies": [{"node": "m::trial", "k": 1, "cost_edge": "m::discover"}]}),
    );
    assert!(resp.error.unwrap().message.contains("inactive"));
    // A known outcome has nothing to learn.
    let resp = run(
        json!({"probabilities": {"m::verdict-win": 1.0}}),
        json!({"op": "voi", "samples": 5, "studies": [{"node": "m::trial", "k": 1}]}),
    );
    assert!(resp.error.unwrap().message.contains("nothing to learn"));
    let resp = run(json!({}), json!({"op": "posterior", "samples": 0}));
    assert!(!resp.ok);
}

#[test]
fn robust_solve_through_the_api_and_its_limits() {
    let resp = run(
        json!({"objective": {"type": "robust", "radius": 0.5}}),
        json!({"op": "solve"}),
    );
    let r = &result(&resp)["robust"];
    assert_eq!(r["max_radius"], json!(0.5));
    assert!(r["robust_value"].as_f64().unwrap() <= r["nominal_value"].as_f64().unwrap());
    // A fixed radius is not a credible radius: no estimate warning.
    assert!(!resp
        .warnings
        .iter()
        .any(|w| w.code == "prior-concentration-estimated"));
    // With a general-sum opponent the robust block is skipped, and warned.
    let resp = run(
        json!({"objective": {"type": "robust"}, "opponent_objective": "payoff"}),
        json!({"op": "solve"}),
    );
    assert!(result(&resp).get("robust").is_none());
    assert!(resp
        .warnings
        .iter()
        .any(|w| w.code == "robust-ignores-opponent-objective"));
    // chain/explain run under the robust policy.
    let resp = run(
        json!({"objective": {"type": "robust"}}),
        json!({"op": "chain"}),
    );
    assert!(resp.ok);
    let resp = run(
        json!({"objective": {"type": "robust", "credibility": 2.0}}),
        json!({"op": "solve"}),
    );
    assert!(resp.error.unwrap().message.contains("credibility"));
    // Observations are recorded in provenance.
    let resp = run(
        json!({"observe": {"m::trial": {"m::verdict-win": 1}}}),
        json!({"op": "solve"}),
    );
    let prov = resp.provenance.unwrap();
    assert_eq!(
        prov["modes"]["observe"]["m::trial"]["m::verdict-win"],
        json!(1.0)
    );
}

#[test]
fn the_new_surface_is_discoverable() {
    let d = describe(&Catalog::embedded().unwrap());
    for op in ["posterior", "voi"] {
        assert!(d["ops"][op].is_string(), "describe lists {op}");
    }
    for field in ["uncertainty", "observe"] {
        assert!(
            d["scenario"][field].is_string(),
            "describe lists scenario.{field}"
        );
    }
    assert!(d["scenario"]["objective"]
        .as_str()
        .unwrap()
        .contains("robust"));
    let s = schema("scenario").unwrap().to_string();
    assert!(s.contains("default_concentration") && s.contains("observe"));
    let s = schema("request").unwrap().to_string();
    assert!(s.contains("\"voi\"") && s.contains("cost_edge"));
}

/// `cofc::ruling-12b` can grant leave to amend and come back around: forcing
/// one outcome on every visit is not clairvoyance about a draw (it loops
/// forever), so a node on a cycle reports no outcome EVPI and is always
/// screened in for the Monte Carlo EVPPI instead.
#[test]
fn a_node_on_a_cycle_gets_evppi_not_outcome_evpi() {
    let req: Request = serde_json::from_value(json!({
        "scenario": "cofc-1498-patent-case",
        "op": {"op": "voi", "samples": 40, "top": 1}
    }))
    .unwrap();
    let resp = handle(&req, &Catalog::embedded().unwrap());
    let res = result(&resp);
    let row = res["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["node"]["id"] == json!("cofc::ruling-12b"))
        .expect("the 12(b) ruling is an uncertain node");
    assert_eq!(row["evpi_outcome"], Value::Null);
    assert!(row["evppi"]["value"].as_f64().unwrap() >= 0.0);
    assert_eq!(res["unconverged_solves"], json!(0));
    for n in res["nodes"].as_array().unwrap() {
        if let Some(x) = n["evpi_outcome"].as_f64() {
            assert!(x < 1e7, "outcome EVPI stays on the case's scale: {x}");
        }
    }
}
