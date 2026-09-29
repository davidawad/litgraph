// SPDX-License-Identifier: GPL-3.0-or-later
//! Exploration ops: paths, Pareto frontiers, sweeps, sensitivity, structure,
//! and graph/metric inspection.

use serde_json::{json, Map, Value};

use super::op::StructureWhat;
use super::render::{choice_label, edge_ref, metric_list, node_ref, path_json, r, targets};
use crate::algo::{paths, structure, sweep};
use crate::error::{Error, Result};
use crate::metrics;
use crate::model::{qualify, Graph, NodeIx, NodeKind, Pack};
use crate::scenario::{resolve_spec, Scenario, View};

pub(super) fn metric_op(v: &View, spec: &str, top: usize) -> Result<Value> {
    let vals = v.metric(spec)?;
    let mut rows: Vec<(usize, f64)> = vals
        .into_iter()
        .enumerate()
        .filter(|(_, x)| x.is_finite())
        .collect();
    rows.sort_by(|a, b| b.1.total_cmp(&a.1));
    Ok(json!({
        "spec": spec,
        "resolved": resolve_spec(spec, &v.sc.metrics, metrics::METRICS),
        "sum": r(rows.iter().map(|x| x.1).sum()),
        "nonzero": rows.iter().filter(|x| x.1 != 0.0).count(),
        "top": rows.iter().take(top).map(|&(e, x)| { let mut j = edge_ref(v, e); j["value"] = r(x); j }).collect::<Vec<_>>(),
    }))
}

pub(super) fn path_op(
    v: &View,
    start: NodeIx,
    to: &str,
    metric: &str,
    k: usize,
    report: &[String],
) -> Result<Value> {
    let w = v.metric(metric)?;
    let ts = targets(v, to)?;
    let extra = metric_list(v, report)?;
    let mut found: Vec<paths::Path> = if k <= 1 {
        paths::shortest_to_any(v, start, &ts, &w)?
            .into_iter()
            .collect()
    } else {
        let mut all = vec![];
        for &t in &ts {
            all.extend(paths::k_shortest(v, start, t, &w, k)?);
        }
        all.sort_by(|a, b| a.totals[0].total_cmp(&b.totals[0]));
        all.truncate(k);
        all
    };
    for p in &mut found {
        for (_, vals) in &extra {
            p.totals.push(p.edges.iter().map(|&e| vals[e]).sum());
        }
    }
    let names: Vec<String> = std::iter::once(metric.to_string())
        .chain(report.iter().cloned())
        .collect();
    Ok(
        json!({ "metric": metric, "from": node_ref(v, start), "to": to, "paths": found.iter().map(|p| path_json(v, p, &names)).collect::<Vec<_>>() }),
    )
}

pub(super) fn pareto_op(
    v: &View,
    start: NodeIx,
    to: &str,
    objectives: &[String],
    max_labels: usize,
    limit: usize,
) -> Result<Value> {
    let ws: Vec<Vec<f64>> = objectives
        .iter()
        .map(|o| v.metric(o))
        .collect::<Result<_>>()?;
    let mut all: Vec<paths::Path> = vec![];
    let mut truncated = false;
    for t in targets(v, to)? {
        let fr = paths::pareto(v, start, t, &ws, max_labels)?;
        truncated |= fr.truncated;
        all.extend(fr.paths);
    }
    let dominated = |p: &paths::Path| {
        all.iter().any(|q| {
            q.totals.iter().zip(&p.totals).all(|(x, y)| x <= y)
                && q.totals.iter().zip(&p.totals).any(|(x, y)| x < y)
        })
    };
    let front: Vec<&paths::Path> = all.iter().filter(|p| !dominated(p)).collect();
    Ok(json!({
        "objectives": objectives, "to": to, "truncated": truncated, "frontier_size": front.len(),
        "frontier": front.iter().take(limit).map(|p| path_json(v, p, objectives)).collect::<Vec<_>>(),
    }))
}

pub(super) struct SweepArgs<'a> {
    pub param: &'a str,
    pub lo: f64,
    pub hi: f64,
    pub steps: usize,
    pub watch: &'a [String],
    pub tol: f64,
}

pub(super) fn sweep_op(v: &View, sc: &Scenario, a: &SweepArgs<'_>) -> Result<Value> {
    let watch: Vec<NodeIx> = a.watch.iter().map(|n| v.g.node(n)).collect::<Result<_>>()?;
    let res = sweep::sweep(
        v.g,
        sc,
        &sweep::SweepSpec {
            param: a.param,
            lo: a.lo,
            hi: a.hi,
            steps: a.steps,
            watch: &watch,
            tol: a.tol,
        },
    )?;
    Ok(json!({
        "param": res.param,
        "curve": res.curve.iter().map(|(x, y)| json!([r(*x), r(*y)])).collect::<Vec<_>>(),
        "breakpoints": res.breakpoints.iter().map(|b| json!({ "node": v.g.nodes[b.node].id, "at": r(b.at), "before": choice_label(v, b.before), "after": choice_label(v, b.after) })).collect::<Vec<_>>(),
    }))
}

pub(super) fn tornado_op(
    g: &Graph,
    sc: &Scenario,
    params: &[String],
    rel: f64,
    dp: f64,
    probabilities: bool,
    top: usize,
) -> Result<Value> {
    let (base, rows) = sweep::tornado(g, sc, params, rel, dp, probabilities)?;
    Ok(json!({ "base_value": r(base), "rows": rows.iter().take(top).collect::<Vec<_>>() }))
}

fn ids(v: &View, ns: impl IntoIterator<Item = NodeIx>) -> Vec<String> {
    ns.into_iter().map(|n| v.g.nodes[n].id.clone()).collect()
}

pub(super) fn structure_op(
    v: &View,
    start: NodeIx,
    what: StructureWhat,
    to: Option<&str>,
    capacity: &str,
    top: usize,
) -> Result<Value> {
    let cycles = || {
        structure::scc(v)
            .into_iter()
            .filter(|c| structure::is_cyclic(v, c))
    };
    Ok(match what {
        StructureWhat::Scc => json!({ "cycles": cycles().map(|c| ids(v, c)).collect::<Vec<_>>() }),
        StructureWhat::Dominators => {
            let idom = structure::dominators(v, start);
            if let Some(t) = to {
                // Unavoidable gateways to the target: its dominator chain.
                let mut chain = vec![];
                let mut x = v.g.node(t)?;
                while let Some(d) = idom[x] {
                    chain.push(d);
                    x = d;
                }
                chain.reverse();
                json!({ "gateways": ids(v, chain) })
            } else {
                json!({ "idom": (0..v.g.nodes.len()).filter_map(|n| idom[n].map(|d| (v.g.nodes[n].id.clone(), json!(v.g.nodes[d].id)))).collect::<Map<_, _>>() })
            }
        }
        StructureWhat::Mincut => {
            let t =
                v.g.node(to.ok_or_else(|| Error::Invalid("mincut needs `to`".into()))?)?;
            let c = structure::min_cut(v, start, t, &v.metric(capacity)?);
            json!({ "value": r(c.value), "cut": c.edges.iter().map(|&e| edge_ref(v, e)).collect::<Vec<_>>() })
        }
        StructureWhat::Betweenness => {
            let mut rows: Vec<(usize, f64)> =
                structure::betweenness(v).into_iter().enumerate().collect();
            rows.sort_by(|a, b| b.1.total_cmp(&a.1));
            json!({ "top": rows.iter().take(top).map(|&(n, x)| json!({ "node": v.g.nodes[n].id, "betweenness": r(x) })).collect::<Vec<_>>() })
        }
        StructureWhat::Reachability => {
            let reach = structure::reachable(v, start);
            json!({
                "reachable_terminals": ids(v, v.g.terminals().filter(|&t| reach[t])),
                "unreachable": ids(v, (0..v.g.nodes.len()).filter(|&n| !reach[n])),
            })
        }
        StructureWhat::Summary => json!({
            "nodes": v.g.nodes.len(), "edges": v.g.edges.len(),
            "active_edges": v.active.iter().filter(|a| **a).count(),
            "terminals": v.g.terminals().count(), "cycles": cycles().count(),
        }),
    })
}

pub(super) fn graph_op(v: &View, around: Option<&str>) -> Result<Value> {
    let keep: Vec<bool> = match around {
        None => vec![true; v.g.nodes.len()],
        Some(a) => {
            let n = v.g.node(a)?;
            let mut k = vec![false; v.g.nodes.len()];
            k[n] = true;
            for &e in v.g.out[n].iter().chain(&v.g.inc[n]) {
                k[v.g.edges[e].from] = true;
                k[v.g.edges[e].to] = true;
            }
            k
        }
    };
    let nodes: Vec<Value> = (0..v.g.nodes.len())
        .filter(|&n| keep[n])
        .map(|n| {
            let nd = &v.g.nodes[n];
            let payoff = if nd.is_terminal() { r(v.payoff[n]) } else { Value::Null };
            json!({ "id": nd.id, "kind": nd.kind, "label": nd.label, "control": v.plan[n].control, "payoff": payoff, "payoff_source": nd.payoff_source, "outcome": nd.outcome })
        })
        .collect();
    let edges: Vec<Value> = (0..v.g.edges.len())
        .filter(|&e| keep[v.g.edges[e].from] && keep[v.g.edges[e].to])
        .map(|e| {
            let mut j = edge_ref(v, e);
            j["active"] = json!(v.active[e]);
            j["hours"] = r(v.g.edges[e].hours);
            j["fees"] = r(v.g.edges[e].cost);
            j
        })
        .collect();
    Ok(json!({ "nodes": nodes, "edges": edges }))
}

pub(super) fn packs_op(g: &Graph, packs: &[Pack]) -> Value {
    let rows: Vec<Value> = packs
        .iter()
        .map(|p| {
            let terms: Vec<_> = p.nodes.iter().filter(|n| n.kind == Some(NodeKind::Terminal)).collect();
            let world = p.edges.iter().filter(|e| e.actor != "applicant").count();
            let world_p = p.edges.iter().filter(|e| e.actor != "applicant" && e.probability.is_some()).count();
            json!({
                "id": p.id, "title": p.title, "forum": p.forum, "schema_version": p.schema_version,
                "start": qualify(&p.id, &p.start_node_id),
                "nodes": p.nodes.len(), "edges": p.edges.len(), "terminals": terms.len(),
                "quality": {
                    "terminals_with_payoff": terms.iter().filter(|n| n.payoff.is_some()).count(),
                    "world_edges_with_probability": format!("{world_p}/{world}"),
                    "edges_with_hours": p.edges.iter().filter(|e| e.hours.is_some()).count(),
                    "edges_with_duration": p.edges.iter().filter(|e| e.duration.is_some()).count(),
                    "sources": p.sources.len(),
                },
                "links_out": g.edges.iter().filter(|e| e.link && g.nodes[e.from].pack == p.id).map(|e| g.nodes[e.to].id.clone()).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!(rows)
}
