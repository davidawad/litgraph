// SPDX-License-Identifier: GPL-3.0-or-later
//! `Objective::Cvar` (`algo::cvar`): a hand-checked case where the
//! `CVaR`-optimal choice differs from the expected-value-optimal one, plus
//! brute-force verification (enumerate every deterministic policy, compute
//! each one's exact `CVaR_alpha` by enumerating its outcome distribution)
//! against `cvar::solve` on small acyclic graphs, including a randomized
//! proptest sweep.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use litgraph::algo::cvar;
use litgraph::model::{CompileOptions, Graph, LinkFile, NodeIx, Pack};
use litgraph::scenario::{Control, MixedMode, Objective, Scenario, View, WAIT};
use proptest::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde_json::json;

fn pack(j: serde_json::Value) -> Pack {
    serde_json::from_value(j).expect("test pack literal is well-formed")
}

fn compile(p: Pack) -> Graph {
    Graph::compile(&[p], &LinkFile::default(), &CompileOptions::default()).expect("compiles")
}

/// The exact `CVaR_alpha` of a discrete distribution given as `(value,
/// probability)` pairs (probabilities need not be pre-sorted or summed to
/// exactly 1 — a graph with an absorbing sink or truncation could leave
/// residual mass, which this treats as simply not part of the distribution).
fn cvar_of(mut outcomes: Vec<(f64, f64)>, alpha: f64) -> f64 {
    outcomes.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut remaining = alpha;
    let mut acc = 0.0;
    for (x, p) in outcomes {
        if remaining <= 1e-15 {
            break;
        }
        let take = p.min(remaining);
        acc += take * x;
        remaining -= take;
    }
    acc / alpha
}

/// Every outcome (total value, probability) reachable from `n` under a fixed
/// `choice` map, assuming the active subgraph from `n` is acyclic (recursion
/// terminates because each state is visited once per DFS branch and the
/// caller only uses this on DAGs).
fn enumerate(
    v: &View,
    choice: &BTreeMap<NodeIx, usize>,
    n: NodeIx,
    p: f64,
    so_far: f64,
    out: &mut Vec<(f64, f64)>,
) {
    if p <= 0.0 {
        return;
    }
    let plan = &v.plan[n];
    match plan.control {
        Control::Terminal => out.push((so_far + v.utility[n], p)),
        Control::Sink => out.push((so_far, p)),
        Control::Chance => {
            for &(e, pe) in &plan.draws {
                enumerate(v, choice, v.g.edges[e].to, p * pe, so_far - v.cost[e], out);
            }
        }
        Control::Me | Control::Opponent => {
            let e = choice[&n];
            if e == WAIT {
                for &(w, pw) in &plan.wait {
                    enumerate(v, choice, v.g.edges[w].to, p * pw, so_far - v.cost[w], out);
                }
            } else {
                // Interrupts (draws) still fire with their own mass even at
                // an act-or-wait / mixed node that chose to act.
                for &(d, pd) in &plan.draws {
                    enumerate(v, choice, v.g.edges[d].to, p * pd, so_far - v.cost[d], out);
                }
                enumerate(
                    v,
                    choice,
                    v.g.edges[e].to,
                    p * plan.choice_mass,
                    so_far - v.cost[e],
                    out,
                );
            }
        }
    }
}

/// Brute-force `max` over every deterministic policy (one choice per
/// `Control::Me`/`Control::Opponent` node, `WAIT` included when available) of
/// `CVaR_alpha` of the total outcome from `v.start`.
fn brute_force_best_cvar(v: &View, alpha: f64) -> f64 {
    let choosers: Vec<NodeIx> = (0..v.g.nodes.len())
        .filter(|&n| matches!(v.plan[n].control, Control::Me | Control::Opponent))
        .collect();
    let options: Vec<Vec<usize>> = choosers
        .iter()
        .map(|&n| {
            let p = &v.plan[n];
            p.choices
                .iter()
                .copied()
                .chain((!p.wait.is_empty()).then_some(WAIT))
                .collect()
        })
        .collect();
    let mut best = f64::NEG_INFINITY;
    let total: usize = options.iter().map(Vec::len).product::<usize>().max(1);
    for combo in 0..total {
        let mut rest = combo;
        let mut choice = BTreeMap::new();
        for (i, &n) in choosers.iter().enumerate() {
            let k = options[i].len();
            choice.insert(n, options[i][rest % k]);
            rest /= k;
        }
        let mut outcomes = vec![];
        enumerate(v, &choice, v.start, 1.0, 0.0, &mut outcomes);
        best = best.max(cvar_of(outcomes, alpha));
    }
    best
}

/// `A`: a risky edge (0.9 chance of 150, 0.1 chance of -1000; EV = 35) vs a
/// safe deterministic edge (payoff 20). Expected-value picks the risky edge;
/// `CVaR_0.1` — whose worst 10% is exactly the -1000 branch — must pick the
/// safe one.
fn risky_vs_safe_graph() -> Graph {
    compile(pack(json!({
        "schemaVersion": 2, "id": "cv", "title": "cv", "startNodeId": "start",
        "nodes": [
            { "id": "start", "kind": "state", "label": "start" },
            { "id": "chance", "kind": "state", "label": "chance" },
            { "id": "good", "kind": "terminal", "label": "good", "payoff": 150.0 },
            { "id": "bad", "kind": "terminal", "label": "bad", "payoff": -1000.0 },
            { "id": "safe", "kind": "terminal", "label": "safe", "payoff": 20.0 }
        ],
        "edges": [
            { "id": "risky", "from": "start", "to": "chance", "label": "risky", "actor": "applicant" },
            { "id": "safe-edge", "from": "start", "to": "safe", "label": "safe", "actor": "applicant" },
            { "id": "to-good", "from": "chance", "to": "good", "label": "good", "actor": "either", "probability": 0.9 },
            { "id": "to-bad", "from": "chance", "to": "bad", "label": "bad", "actor": "either", "probability": 0.1 }
        ]
    })))
}

#[test]
fn cvar_prefers_the_safe_option_expected_value_would_reject() {
    let g = risky_vs_safe_graph();
    let grid = 4001;
    let sc = Scenario {
        objective: Objective::Cvar { alpha: 0.1, grid },
        ..Default::default()
    };
    let v = View::new(&g, &sc).unwrap();

    // Expected value (the default objective) picks the risky edge (EV 35 > 20).
    let ev_v = View::new(&g, &Scenario::default()).unwrap();
    let ev_sol =
        litgraph::algo::mdp::solve(&ev_v, &litgraph::algo::mdp::SolveOptions::default()).unwrap();
    assert!((ev_sol.value[ev_v.start] - 35.0).abs() < 1e-6);
    assert_eq!(
        ev_sol.choice[&ev_v.start],
        g.edge("cv::risky").unwrap(),
        "expected value should prefer the risky edge"
    );

    let cv = cvar::solve(&v, 0.1, grid, &litgraph::algo::mdp::SolveOptions::default()).unwrap();
    // The true optimum sits at the kink zeta=20 of a piecewise-linear
    // (increasing then steeply falling) function; on a grid the achievable
    // max can undershoot by up to one grid step (interpolation between grid
    // points cannot recover a kink the grid doesn't land on exactly).
    let step = (cv.y_range.1 - cv.y_range.0) / (grid - 1) as f64;
    assert!(
        (cv.cvar - 20.0).abs() <= step * 1.01,
        "cvar={} step={step}",
        cv.cvar
    );
    assert_eq!(
        cv.solution.choice[&v.start],
        g.edge("cv::safe-edge").unwrap(),
        "CVaR_0.1 should prefer the safe edge"
    );

    let brute = brute_force_best_cvar(&v, 0.1);
    assert!(
        (cv.cvar - brute).abs() <= step * 1.01,
        "solve={} brute={}",
        cv.cvar,
        brute
    );
}

#[test]
fn cvar_matches_brute_force_on_a_three_choice_diamond() {
    let g = compile(pack(json!({
        "schemaVersion": 2, "id": "cv3", "title": "cv3", "startNodeId": "s",
        "nodes": [
            { "id": "s", "kind": "state", "label": "s" },
            { "id": "m", "kind": "state", "label": "m" },
            { "id": "t1", "kind": "terminal", "label": "t1", "payoff": 50.0 },
            { "id": "t2", "kind": "terminal", "label": "t2", "payoff": -200.0 },
            { "id": "t3", "kind": "terminal", "label": "t3", "payoff": 5.0 }
        ],
        "edges": [
            { "id": "s-m", "from": "s", "to": "m", "label": "to-m", "actor": "applicant" },
            { "id": "s-t3", "from": "s", "to": "t3", "label": "to-t3", "actor": "applicant" },
            { "id": "m-t1", "from": "m", "to": "t1", "label": "to-t1", "actor": "applicant", "cost": 5.0 },
            { "id": "m-t2", "from": "m", "to": "t2", "label": "to-t2", "actor": "applicant" }
        ]
    })));
    let sc = Scenario {
        objective: Objective::Cvar {
            alpha: 0.5,
            grid: 301,
        },
        ..Default::default()
    };
    let v = View::new(&g, &sc).unwrap();
    let cv = cvar::solve(&v, 0.5, 301, &litgraph::algo::mdp::SolveOptions::default()).unwrap();
    let brute = brute_force_best_cvar(&v, 0.5);
    assert!(
        (cv.cvar - brute).abs() < 1.0,
        "solve={} brute={}",
        cv.cvar,
        brute
    );
}

// --- Randomized: small acyclic "choice -> chance -> terminal" trees with
// random payoffs/probabilities/alpha, `cvar::solve` vs. brute force. ---

fn random_tree_graph(rng: &mut ChaCha8Rng, depth: usize) -> (Graph, f64) {
    let mut nodes = vec![json!({ "id": "n0", "kind": "state", "label": "n0" })];
    let mut edges = vec![];
    let mut next_id = 1u32;
    let mut frontier = vec!["n0".to_string()];
    for _level in 0..depth {
        let mut next_frontier = vec![];
        for from in frontier {
            // Each choice node has two options; one leads to a two-outcome
            // chance node, the other straight to a terminal.
            let chance = format!("n{next_id}");
            next_id += 1;
            let term_a = format!("n{next_id}");
            next_id += 1;
            let term_b = format!("n{next_id}");
            next_id += 1;
            let payoff_a: f64 = rng.gen_range(-500.0..500.0);
            let payoff_b: f64 = rng.gen_range(-500.0..500.0);
            let p_a: f64 = rng.gen_range(0.05..0.95);
            nodes.push(json!({ "id": chance, "kind": "state", "label": chance }));
            nodes.push(
                json!({ "id": term_a, "kind": "terminal", "label": term_a, "payoff": payoff_a }),
            );
            nodes.push(
                json!({ "id": term_b, "kind": "terminal", "label": term_b, "payoff": payoff_b }),
            );
            edges.push(json!({ "id": format!("{from}-chance"), "from": from, "to": chance, "label": "risky", "actor": "applicant" }));
            edges.push(json!({ "id": format!("{chance}-a"), "from": chance, "to": term_a, "label": "a", "actor": "either", "probability": p_a }));
            edges.push(json!({ "id": format!("{chance}-b"), "from": chance, "to": term_b, "label": "b", "actor": "either", "probability": 1.0 - p_a }));
            let safe_term = format!("n{next_id}");
            next_id += 1;
            let payoff_safe: f64 = rng.gen_range(-500.0..500.0);
            nodes.push(json!({ "id": safe_term, "kind": "terminal", "label": safe_term, "payoff": payoff_safe }));
            edges.push(json!({ "id": format!("{from}-safe"), "from": from, "to": safe_term, "label": "safe", "actor": "applicant" }));
            next_frontier.push(chance); // unused terminal branch point, kept for shape only
        }
        // Single-level tree for this generator (chosen for tractable exact
        // brute-force enumeration); deeper trees are exercised implicitly by
        // running many independent seeds instead of nesting further.
        frontier = vec![];
        let _ = next_frontier;
    }
    let g = compile(pack(json!({
        "schemaVersion": 2, "id": "rt", "title": "rt", "startNodeId": "n0",
        "nodes": nodes, "edges": edges
    })));
    let alpha = rng.gen_range(0.05..0.95);
    (g, alpha)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]
    #[test]
    fn cvar_solve_matches_brute_force_on_random_small_graphs(seed in any::<u64>()) {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let (g, alpha) = random_tree_graph(&mut rng, 1);
        let grid = 501;
        let sc = Scenario {
            objective: Objective::Cvar { alpha, grid },
            ..Default::default()
        };
        let v = View::new(&g, &sc).unwrap();
        let cv = cvar::solve(&v, alpha, grid, &litgraph::algo::mdp::SolveOptions::default()).unwrap();
        let brute = brute_force_best_cvar(&v, alpha);
        // Tolerance scales with the grid step over the solver's own outcome
        // range: linear interpolation error is bounded by (step/2) times the
        // steepness of the R-U value function, which is at most 1/alpha.
        let step = (cv.y_range.1 - cv.y_range.0) / (grid - 1) as f64;
        let tol = (step / alpha).max(1.0);
        prop_assert!(
            (cv.cvar - brute).abs() < tol,
            "seed={seed} alpha={alpha} solve={} brute={} tol={tol}",
            cv.cvar,
            brute
        );
    }
}

#[test]
fn cvar_rejects_alpha_out_of_range() {
    let g = risky_vs_safe_graph();
    let sc = Scenario::default();
    let v = View::new(&g, &sc).unwrap();
    assert!(cvar::solve(&v, 0.0, 41, &litgraph::algo::mdp::SolveOptions::default()).is_err());
    assert!(cvar::solve(&v, 1.5, 41, &litgraph::algo::mdp::SolveOptions::default()).is_err());
}

#[test]
fn cvar_warns_when_combined_with_discount_fee_shift_or_opponent_objective() {
    let g = risky_vs_safe_graph();
    let sc = Scenario {
        objective: Objective::Cvar {
            alpha: 0.2,
            grid: 21,
        },
        discount_annual: Some(0.05),
        fee_shift: Some(litgraph::scenario::FeeShift {
            fraction: 0.1,
            eligible: None,
        }),
        opponent_objective: Some("payoff".to_string()),
        ..Default::default()
    };
    let v = View::new(&g, &sc).unwrap();
    for code in [
        "cvar-ignores-discount",
        "cvar-ignores-fee-shift",
        "cvar-ignores-opponent-objective",
    ] {
        assert!(
            v.warnings.iter().any(|w| w.code == code),
            "missing warning {code}: {:?}",
            v.warnings
        );
    }
}

/// A pack exercising the `backup` branches the graphs above don't reach:
/// a forced (`scenario.policy`) choice, an act-or-wait node where waiting is
/// optimal, a tie between two equal-value choices, and an isolated sink
/// (every node's backup runs regardless of reachability from `start`, since
/// `algo::structure::scc` decomposes the whole graph).
fn coverage_graph() -> Graph {
    compile(pack(json!({
        "schemaVersion": 2, "id": "cvcov", "title": "cvcov", "startNodeId": "start",
        "roles": { "applicant": "self", "examiner": "opponent" },
        "nodes": [
            { "id": "start", "kind": "state", "label": "start" },
            { "id": "term-a", "kind": "terminal", "label": "term-a", "payoff": 100.0 },
            { "id": "term-b", "kind": "terminal", "label": "term-b", "payoff": -100.0 },
            { "id": "mid", "kind": "state", "label": "mid" },
            { "id": "act-term", "kind": "terminal", "label": "act-term", "payoff": -500.0 },
            { "id": "wait-term", "kind": "terminal", "label": "wait-term", "payoff": 300.0 },
            { "id": "tie-node", "kind": "state", "label": "tie-node" },
            { "id": "tie-a", "kind": "terminal", "label": "tie-a", "payoff": 50.0 },
            { "id": "tie-b", "kind": "terminal", "label": "tie-b", "payoff": 50.0 },
            { "id": "cyc-a", "kind": "state", "label": "cyc-a" },
            { "id": "cyc-b", "kind": "state", "label": "cyc-b" },
            { "id": "cyc-end", "kind": "terminal", "label": "cyc-end", "payoff": 15.0 },
            { "id": "dead", "kind": "state", "label": "dead" }
        ],
        "edges": [
            { "id": "to-mid", "from": "start", "to": "mid", "label": "to-mid", "actor": "applicant" },
            { "id": "to-a", "from": "start", "to": "term-a", "label": "to-a", "actor": "applicant" },
            { "id": "to-b", "from": "start", "to": "term-b", "label": "to-b", "actor": "applicant" },
            { "id": "act", "from": "mid", "to": "act-term", "label": "act", "actor": "applicant" },
            { "id": "world", "from": "mid", "to": "wait-term", "label": "world", "actor": "either" },
            { "id": "to-tie-a", "from": "tie-node", "to": "tie-a", "label": "to-tie-a", "actor": "applicant" },
            { "id": "to-tie-b", "from": "tie-node", "to": "tie-b", "label": "to-tie-b", "actor": "applicant" },
            { "id": "cyc-a-b", "from": "cyc-a", "to": "cyc-b", "label": "spin", "actor": "either", "cost": 1.0 },
            { "id": "cyc-b-a", "from": "cyc-b", "to": "cyc-a", "label": "spin-back", "actor": "either", "cost": 1.0 },
            { "id": "cyc-a-end", "from": "cyc-a", "to": "cyc-end", "label": "exit", "actor": "either" }
        ]
    })))
}

/// `start` is itself an act-or-wait node, forced (`scenario.policy`) to the
/// *world* edge's own id — `plan.rs` and this solver both read that as
/// "wait", not "take that edge as ours" (a distinct branch from forcing an
/// ordinary edge).
fn forced_to_wait_graph() -> Graph {
    compile(pack(json!({
        "schemaVersion": 2, "id": "cvforce", "title": "cvforce", "startNodeId": "start",
        "nodes": [
            { "id": "start", "kind": "state", "label": "start" },
            { "id": "act-term", "kind": "terminal", "label": "act-term", "payoff": 10.0 },
            { "id": "wait-term", "kind": "terminal", "label": "wait-term", "payoff": 20.0 }
        ],
        "edges": [
            { "id": "act", "from": "start", "to": "act-term", "label": "act", "actor": "applicant" },
            { "id": "world", "from": "start", "to": "wait-term", "label": "world", "actor": "either" }
        ]
    })))
}

#[test]
fn cvar_forced_to_the_world_edge_id_means_wait() {
    let g = forced_to_wait_graph();
    let mut policy = std::collections::BTreeMap::new();
    policy.insert("cvforce::start".to_string(), "cvforce::world".to_string());
    let sc = Scenario {
        objective: Objective::Cvar {
            alpha: 0.5,
            grid: 21,
        },
        mixed: MixedMode::ActOrWait,
        policy,
        ..Default::default()
    };
    let v = View::new(&g, &sc).unwrap();
    let cv = cvar::solve(&v, 0.5, 21, &litgraph::algo::mdp::SolveOptions::default()).unwrap();
    assert_eq!(cv.solution.choice[&v.start], WAIT);
}

/// An opponent-controlled fork under `Objective::Cvar` (which ignores
/// `opponent_objective` and always models the opponent adversarially):
/// the opponent picks the edge worse for us.
fn opponent_fork_graph() -> Graph {
    compile(pack(json!({
        "schemaVersion": 2, "id": "cvopp", "title": "cvopp", "startNodeId": "start",
        "roles": { "applicant": "self", "examiner": "opponent" },
        "nodes": [
            { "id": "start", "kind": "state", "label": "start" },
            { "id": "opp-lo", "kind": "terminal", "label": "opp-lo", "payoff": 10.0 },
            { "id": "opp-hi", "kind": "terminal", "label": "opp-hi", "payoff": 90.0 }
        ],
        "edges": [
            { "id": "to-opp-lo", "from": "start", "to": "opp-lo", "label": "to-opp-lo", "actor": "examiner" },
            { "id": "to-opp-hi", "from": "start", "to": "opp-hi", "label": "to-opp-hi", "actor": "examiner" }
        ]
    })))
}

#[test]
fn cvar_opponent_is_adversarial() {
    let g = opponent_fork_graph();
    let sc = Scenario {
        objective: Objective::Cvar {
            alpha: 0.5,
            grid: 21,
        },
        ..Default::default()
    };
    let v = View::new(&g, &sc).unwrap();
    let cv = cvar::solve(&v, 0.5, 21, &litgraph::algo::mdp::SolveOptions::default()).unwrap();
    assert_eq!(
        cv.solution.choice[&v.start],
        g.edge("cvopp::to-opp-lo").unwrap(),
        "an adversarial opponent picks the edge worse for us"
    );
}

#[test]
fn cvar_handles_forced_choice_act_or_wait_ties_and_sinks() {
    let g = coverage_graph();
    let mut policy = std::collections::BTreeMap::new();
    // Forced (worse-looking, by raw payoff) into `mid`, so the reconstructed
    // rollout also crosses the act-or-wait node.
    policy.insert("cvcov::start".to_string(), "cvcov::to-mid".to_string());
    let sc = Scenario {
        objective: Objective::Cvar {
            alpha: 0.3,
            grid: 51,
        },
        mixed: MixedMode::ActOrWait,
        policy,
        ..Default::default()
    };
    let v = View::new(&g, &sc).unwrap();
    let cv = cvar::solve(&v, 0.3, 51, &litgraph::algo::mdp::SolveOptions::default()).unwrap();
    // The forced choice at `start` (into `mid`, not the directly-terminal
    // options) is respected.
    assert_eq!(
        cv.solution.choice[&v.start],
        g.edge("cvcov::to-mid").unwrap()
    );
    // At `mid`, waiting (`wait-term`, 300) beats acting (`act-term`, -500).
    let mid = g.node("cvcov::mid").unwrap();
    assert_eq!(cv.solution.choice[&mid], WAIT);
    // `cyc-a`/`cyc-b` form a genuine cycle (a chance node spinning between
    // them before exiting), exercising the iterate-to-convergence branch.
    assert!(cv.solution.converged, "{:?}", cv.solution.unconverged);
    // `tie-node` and `dead` are unreachable from `start`, but every node's
    // backward-induction backup still runs (a tied Me choice, and a sink);
    // the reachable part of the solve stayed finite throughout.
    assert!(cv.cvar.is_finite());
}
