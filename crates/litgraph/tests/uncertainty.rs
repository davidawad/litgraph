// SPDX-License-Identifier: GPL-3.0-or-later
//! Probability uncertainty (`docs/UNCERTAINTY.md`): conjugate Dirichlet
//! updates against their closed forms, the robust solve's ordering and
//! limits, posterior propagation, and value of information against the
//! hand-derived Beta(1, 1) gamble.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::algo::posterior::{self, PosteriorOptions};
use litgraph::algo::voi::{self, Study, VoiOptions};
use litgraph::algo::{mdp, robust};
use litgraph::api::{handle, Catalog, Request};
use litgraph::model::{CompileOptions, Graph, LinkFile, Pack};
use litgraph::scenario::{GroupKind, Objective, PriorSource, Scenario, View, WAIT};
use serde_json::{json, Value};

/// Three options at the start: a sure $`safe`, a ruling that wins $100 with
/// probability `p` (else $0), and a three-way draw worth $29 in expectation
/// (20% $100, 30% $30, 50% $0).
fn gamble_pack(safe: f64, p: f64) -> String {
    json!({
        "schemaVersion": 2, "id": "g", "title": "gamble", "startNodeId": "start",
        "nodes": [
            {"id": "start", "label": "start"},
            {"id": "chance", "label": "the ruling"},
            {"id": "tri", "label": "three-way"},
            {"id": "win", "label": "win", "kind": "terminal", "payoff": 100.0},
            {"id": "lose", "label": "lose", "kind": "terminal", "payoff": 0.0},
            {"id": "mid", "label": "mid", "kind": "terminal", "payoff": 30.0},
            {"id": "sure", "label": "sure", "kind": "terminal", "payoff": safe}
        ],
        "edges": [
            {"id": "take-safe", "from": "start", "to": "sure", "label": "take the sure thing", "actor": "applicant"},
            {"id": "take-gamble", "from": "start", "to": "chance", "label": "litigate", "actor": "applicant"},
            {"id": "take-tri", "from": "start", "to": "tri", "label": "three-way", "actor": "applicant"},
            {"id": "granted", "from": "chance", "to": "win", "label": "granted", "actor": "examiner", "probability": p},
            {"id": "denied", "from": "chance", "to": "lose", "label": "denied", "actor": "examiner", "probability": 1.0 - p},
            {"id": "t-win", "from": "tri", "to": "win", "label": "a", "actor": "examiner", "probability": 0.2},
            {"id": "t-mid", "from": "tri", "to": "mid", "label": "b", "actor": "examiner", "probability": 0.3},
            {"id": "t-lose", "from": "tri", "to": "lose", "label": "c", "actor": "examiner", "probability": 0.5}
        ]
    })
    .to_string()
}

fn graph(text: &str) -> Graph {
    Graph::compile(
        &[Pack::from_json(text).unwrap()],
        &LinkFile::default(),
        &CompileOptions::default(),
    )
    .unwrap()
}

fn scenario(j: Value) -> Scenario {
    serde_json::from_value(j).unwrap()
}

fn prob(v: &View, id: &str) -> f64 {
    v.prob[v.g.edge(id).unwrap()].unwrap()
}

#[test]
fn beta_update_matches_the_closed_form() {
    let g = graph(&gamble_pack(50.0, 0.6));
    // Prior Beta(6, 4) (concentration 10); observe 3 granted of 4.
    let sc = scenario(json!({
        "uncertainty": {"concentration": {"g::chance": 10.0}},
        "observe": {"g::chance": {"g::granted": 3, "g::denied": 1}}
    }));
    let v = View::new(&g, &sc).unwrap();
    assert!((prob(&v, "g::granted") - 9.0 / 14.0).abs() < 1e-12);
    assert!((prob(&v, "g::denied") - 5.0 / 14.0).abs() < 1e-12);
    let gi = v.belief.group_at(g.node("g::chance").unwrap()).unwrap();
    let grp = &v.belief.groups[gi];
    assert_eq!(grp.alpha(), vec![9.0, 5.0]);
    assert_eq!(grp.source, PriorSource::Scenario);
    assert_eq!(grp.kind, GroupKind::Draws);
    // Every op then uses the posterior mean: the gamble is worth 100 · 9/14.
    let sol = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
    assert!((sol.value[v.start] - 100.0 * 9.0 / 14.0).abs() < 1e-9);
    // The observed node uses a scenario concentration: no estimate warning.
    assert!(!v
        .warnings
        .iter()
        .any(|w| w.code == "prior-concentration-estimated"));
}

#[test]
fn dirichlet_update_with_the_default_concentration_is_warned() {
    let g = graph(&gamble_pack(50.0, 0.6));
    let sc = scenario(json!({"observe": {"g::tri": {"g::t-mid": 5}}}));
    let v = View::new(&g, &sc).unwrap();
    // Prior 10 · (0.2, 0.3, 0.5) = (2, 3, 5); + (0, 5, 0) → mean (2, 8, 5)/15.
    assert!((prob(&v, "g::t-win") - 2.0 / 15.0).abs() < 1e-12);
    assert!((prob(&v, "g::t-mid") - 8.0 / 15.0).abs() < 1e-12);
    assert!((prob(&v, "g::t-lose") - 5.0 / 15.0).abs() < 1e-12);
    let w: Vec<_> = v
        .warnings
        .iter()
        .filter(|w| w.code == "prior-concentration-estimated")
        .collect();
    assert_eq!(w.len(), 1);
    assert_eq!(w[0].at.as_deref(), Some("g::tri"));
    // A different default concentration moves the posterior accordingly.
    let sc = scenario(json!({
        "uncertainty": {"default_concentration": 5.0},
        "observe": {"g::tri": {"g::t-mid": 5}}
    }));
    let v = View::new(&g, &sc).unwrap();
    assert!((prob(&v, "g::t-mid") - 6.5 / 10.0).abs() < 1e-12);
}

#[test]
fn a_calibrated_sample_size_is_the_prior_strength() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let text = std::fs::read_to_string(format!("{root}/packs/ptab-patent-trial-appeal-board.json"))
        .unwrap();
    let mut g = graph(&text);
    litgraph::api::apply_calibration_named(&mut g, "ptab-fy2024").unwrap();
    let edge = "ptab-patent-trial-appeal-board::institution-decision->institution-granted#0";
    let node = "ptab-patent-trial-appeal-board::institution-decision";
    let base = View::new(&g, &Scenario::default()).unwrap();
    let gi = base.belief.group_at(g.node(node).unwrap()).unwrap();
    assert_eq!(
        base.belief.groups[gi].source,
        PriorSource::Calibration {
            set: "ptab-fy2024".into(),
            n: 1087
        }
    );
    let observed = View::new(&g, &scenario(json!({"observe": {node: {edge: 4}}}))).unwrap();
    // n = 1087 (740 instituted / 1087 decided): four more grants barely move it.
    let (p0, p1) = (prob(&base, edge), prob(&observed, edge));
    assert!((p0 - 0.681).abs() < 1e-6, "{p0}");
    let expect = (p0 * 1087.0 + 4.0) / (1087.0 + 4.0);
    assert!((p1 - expect).abs() < 1e-12, "{p1} vs {expect}");
    assert!(!observed
        .warnings
        .iter()
        .any(|w| w.code == "prior-concentration-estimated"));
}

#[test]
fn observation_errors_are_clear() {
    let g = graph(&gamble_pack(50.0, 0.6));
    for (sc, needle) in [
        (
            json!({"observe": {"g::start": {"g::take-safe": 1}}}),
            "not a chance outcome",
        ),
        (
            json!({"observe": {"g::chance": {"g::granted": -1}}}),
            "non-negative",
        ),
        (json!({"observe": {"g::chance": {"g::nope": 1}}}), "nope"),
        (
            json!({"uncertainty": {"default_concentration": 0.0}}),
            "positive",
        ),
        (
            json!({"uncertainty": {"concentration": {"g::chance": -2.0}}}),
            "positive",
        ),
    ] {
        let err = View::new(&g, &scenario(sc)).err().expect("rejected");
        assert!(err.to_string().contains(needle), "{err}");
    }
}

#[test]
fn a_fact_leaves_nothing_to_learn() {
    let g = graph(&gamble_pack(50.0, 0.6));
    let v = View::new(&g, &scenario(json!({"probabilities": {"g::granted": 1.0}}))).unwrap();
    let gi = v.belief.group_at(g.node("g::chance").unwrap()).unwrap();
    assert!(!v.belief.groups[gi].uncertain());
    assert!(!v.belief.uncertain().contains(&gi));
}

fn robust_sc(extra: Value) -> Scenario {
    let Value::Object(extra) = extra else {
        panic!("robust_sc takes an object")
    };
    let mut obj = serde_json::Map::from_iter([("type".to_string(), json!("robust"))]);
    obj.extend(extra);
    scenario(json!({"objective": obj, "uncertainty": {"default_concentration": 4.0}}))
}

#[test]
fn robust_value_is_at_most_nominal_and_can_change_the_policy() {
    // Nominal: gamble 60 > sure 55 > three-way 29. With only 4 pseudo-counts,
    // the credible set around 0.6 is wide and the sure thing wins.
    let g = graph(&gamble_pack(55.0, 0.6));
    let nominal = View::new(&g, &Scenario::default()).unwrap();
    let nom = mdp::solve(&nominal, &mdp::SolveOptions::default()).unwrap();
    let v = View::new(&g, &robust_sc(json!({}))).unwrap();
    let rob = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap();
    assert!((nom.value[v.start] - 60.0).abs() < 1e-9);
    assert!((rob.value[v.start] - 55.0).abs() < 1e-9);
    assert_eq!(rob.choice[&v.start], g.edge("g::take-safe").unwrap());

    // Radius 0 is nominal; radius 2 is the worst case over the support.
    let zero = View::new(&g, &robust_sc(json!({"radius": 0.0}))).unwrap();
    let zero = mdp::solve(&zero, &mdp::SolveOptions::default()).unwrap();
    assert_eq!(zero.value, nom.value);
    let two = View::new(&g, &robust_sc(json!({"radius": 2.0}))).unwrap();
    let two = mdp::solve(&two, &mdp::SolveOptions::default()).unwrap();
    let worst = View::new(&g, &scenario(json!({"objective": {"type": "worst"}}))).unwrap();
    let worst = mdp::solve(&worst, &mdp::SolveOptions::default()).unwrap();
    assert_eq!(two.value, worst.value);

    // More data shrinks the set: the robust value climbs back toward nominal.
    let tight = View::new(
        &g,
        &scenario(json!({
            "objective": {"type": "robust"},
            "uncertainty": {"concentration": {"g::chance": 10000.0, "g::tri": 10000.0}}
        })),
    )
    .unwrap();
    let tight = mdp::solve(&tight, &mdp::SolveOptions::default()).unwrap();
    assert!(tight.value[v.start] > 59.0 && tight.value[v.start] <= 60.0);
}

#[test]
fn robust_solve_reports_nominal_value_and_policy_changes() {
    let catalog =
        Catalog::from_files("test".into(), [("g.json".into(), gamble_pack(55.0, 0.6))]).unwrap();
    let req: Request = serde_json::from_value(json!({
        "packs": ["g"],
        "scenario": {"objective": {"type": "robust", "credibility": 0.9}},
        "op": {"op": "solve"}
    }))
    .unwrap();
    let resp = handle(&req, &catalog);
    assert!(resp.ok, "{:?}", resp.error);
    let r = &resp.result.unwrap()["robust"];
    assert_eq!(r["nominal_value"], json!(60.0));
    assert_eq!(r["robust_value"], json!(55.0));
    assert_eq!(r["price_of_robustness"], json!(5.0));
    assert_eq!(r["ambiguous_nodes"], json!(2));
    let changes = r["policy_changes"].as_array().unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0]["nominal"]["edge"], json!("g::take-gamble"));
    assert_eq!(changes[0]["robust"]["edge"], json!("g::take-safe"));
    assert!(resp
        .warnings
        .iter()
        .any(|w| w.code == "prior-concentration-estimated" && w.count == 2));
}

#[test]
fn robust_parameter_errors_are_clear() {
    let g = graph(&gamble_pack(55.0, 0.6));
    for (extra, needle) in [
        (json!({"credibility": 1.0}), "credibility"),
        (json!({"radius": -1.0}), "radius"),
    ] {
        let v = View::new(&g, &robust_sc(extra)).unwrap();
        let err = mdp::solve(&v, &mdp::SolveOptions::default()).unwrap_err();
        assert!(err.to_string().contains(needle), "{err}");
    }
    // The credible radius grows as the credibility does.
    let v = View::new(&g, &robust_sc(json!({}))).unwrap();
    let r50 = robust::radii(&v, 0.5, None, 500, 1).unwrap();
    let r95 = robust::radii(&v, 0.95, None, 500, 1).unwrap();
    let n = g.node("g::chance").unwrap();
    assert!(r95[n] > r50[n] && r50[n] > 0.0);
    assert_eq!(r95[v.start], 0.0);
}

/// Beta(1, 1) on the ruling (`concentration` 2 around p = 0.5), sure thing 50.
fn uniform_gamble() -> Graph {
    graph(&gamble_pack(50.0, 0.5))
}

fn uniform_view(g: &Graph) -> View<'_> {
    View::new(
        g,
        &scenario(json!({"uncertainty": {"concentration": {"g::chance": 2.0, "g::tri": 1e9}}})),
    )
    .unwrap()
}

#[test]
fn voi_matches_the_beta_closed_forms() {
    let g = uniform_gamble();
    let v = uniform_view(&g);
    let chance = v.belief.group_at(g.node("g::chance").unwrap()).unwrap();
    let o = VoiOptions {
        samples: 4000,
        seed: 11,
        top: 10,
        max_nodes: 100,
    };
    let studies = [
        Study {
            group: chance,
            k: 1,
        },
        Study {
            group: chance,
            k: 1000,
        },
    ];
    let out = voi::voi(&v, v.start, &studies, &o).unwrap();
    assert_eq!(out.value, 50.0);
    let row = out.nodes.iter().find(|r| r.group == chance).unwrap();
    // Knowing the ruling: win → gamble (100), lose → sure (50): 0.5·50 = 25.
    assert!((row.evpi_outcome.unwrap() - 25.0).abs() < 1e-9);
    // Knowing θ: E[max(0, 100θ − 50)] for θ ~ U(0, 1) = 12.5.
    let evppi = row.evppi.unwrap();
    assert!(
        (evppi.value - 12.5).abs() < 4.0 * evppi.std_error,
        "{evppi:?}"
    );
    // One observation: a win (prob 1/2) lifts the mean to 2/3: 0.5 · (200/3 − 50) = 25/3.
    let one = out.studies[0];
    assert!(
        (one.value - 25.0 / 3.0).abs() < 4.0 * one.std_error,
        "{one:?}"
    );
    // EVPI(outcome) ≥ EVPPI ≥ EVSI(k) ≥ 0, and EVSI approaches EVPPI as k grows.
    let many = out.studies[1];
    assert!(row.evpi_outcome.unwrap() >= evppi.value);
    assert!(evppi.value + 4.0 * evppi.std_error >= many.value);
    assert!(many.value > one.value && one.value > 0.0);
    assert!((many.value - 12.5).abs() < 1.0);
    assert!(out.evpi_total.value >= 0.0 && out.unconverged == 0);
}

#[test]
fn evpi_is_zero_when_the_decision_cannot_change() {
    // A sure $200 beats every outcome of every alternative.
    let g = graph(&gamble_pack(200.0, 0.5));
    let v = View::new(&g, &Scenario::default()).unwrap();
    let groups = v.belief.uncertain();
    let studies: Vec<Study> = groups.iter().map(|&group| Study { group, k: 3 }).collect();
    let o = VoiOptions {
        samples: 200,
        seed: 3,
        top: 10,
        max_nodes: 100,
    };
    let out = voi::voi(&v, v.start, &studies, &o).unwrap();
    assert_eq!(out.evpi_total.value, 0.0);
    for row in &out.nodes {
        assert_eq!(row.evpi_outcome, Some(0.0));
        assert_eq!(row.evppi.unwrap().value, 0.0);
    }
    assert!(out.studies.iter().all(|s| s.value == 0.0));
}

#[test]
fn voi_argument_errors() {
    let g = uniform_gamble();
    let v = uniform_view(&g);
    let chance = v.belief.group_at(g.node("g::chance").unwrap()).unwrap();
    let o = VoiOptions {
        samples: 0,
        seed: 1,
        top: 1,
        max_nodes: 10,
    };
    assert!(voi::voi(&v, v.start, &[], &o).is_err());
    let o = VoiOptions { samples: 5, ..o };
    let err = voi::voi(
        &v,
        v.start,
        &[Study {
            group: chance,
            k: 0,
        }],
        &o,
    )
    .unwrap_err();
    assert!(err.to_string().contains("k must be"));
    // Screening cap: one of two reachable nodes screened.
    let o = VoiOptions { max_nodes: 1, ..o };
    let out = voi::voi(&v, v.start, &[], &o).unwrap();
    assert!(out.truncated && out.nodes.len() == 1);
}

#[test]
fn posterior_probability_of_optimality() {
    let g = uniform_gamble();
    let v = uniform_view(&g);
    let o = PosteriorOptions {
        samples: 2000,
        seed: 5,
        credibility: 0.9,
    };
    let p = posterior::posterior(&v, v.start, v.start, &o).unwrap();
    let gamble = g.edge("g::take-gamble").unwrap();
    let safe = g.edge("g::take-safe").unwrap();
    let stat = |e| p.options.iter().find(|s| s.edge == e).unwrap();
    // P(θ > 1/2) = 1/2 under Beta(1, 1).
    assert!((stat(gamble).p_optimal - 0.5).abs() < 0.04);
    let total: f64 = p.options.iter().map(|s| s.p_optimal).sum();
    assert!((total - 1.0).abs() < 1e-9);
    // The gamble's Q is 100θ: its 90% interval is about [5, 95].
    let q = &stat(gamble).q;
    assert!(
        (q.lo - 5.0).abs() < 2.0 && (q.hi - 95.0).abs() < 2.0,
        "{q:?}"
    );
    assert!((q.mean - 50.0).abs() < 2.0 && q.nominal == 50.0);
    // The sure thing has no uncertainty at all.
    assert_eq!((stat(safe).q.lo, stat(safe).q.hi), (50.0, 50.0));
    // E[max(50, 100θ)] = 62.5 ≥ the nominal value 50.
    assert!((p.value.mean - 62.5).abs() < 1.5 && p.value.nominal == 50.0);
    assert_eq!(p.unconverged, 0);
    for bad in [
        PosteriorOptions {
            samples: 0,
            ..o.clone()
        },
        PosteriorOptions {
            credibility: 0.0,
            ..o.clone()
        },
    ] {
        assert!(posterior::posterior(&v, v.start, v.start, &bad).is_err());
    }
    assert!(posterior::options_at(&v, g.node("g::chance").unwrap()).is_empty());
    assert!(!posterior::options_at(&v, v.start).contains(&WAIT));
}

#[test]
fn risk_neutral_copies_drop_the_objective() {
    let g = uniform_gamble();
    let v = View::new(
        &g,
        &scenario(json!({"objective": {"type": "cara", "a": 0.01}})),
    )
    .unwrap();
    assert_eq!(
        posterior::risk_neutral(&v).sc.objective,
        Objective::Expected
    );
}
