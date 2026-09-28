//! The agent-facing surface: one JSON request in, one JSON response out.
//!
//! ```json
//! { "packs": ["frcp", "frap"], "scenario": { "params": { "rate": 900 } },
//!   "op": { "op": "explain", "node": "frcp::answer-due" } }
//! ```
//!
//! Every response carries `warnings` (what the engine had to assume) and
//! `provenance` (which packs, which scenario, which engine), so an agent can
//! tell authored facts from fallbacks without reading source.

use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path as FsPath, PathBuf};

use crate::algo::{chain, mdp, paths, sim, structure, sweep};
use crate::error::{Error, Result};
use crate::lint;
use crate::metrics;
use crate::model::{CompileOptions, Graph, LinkFile, NodeIx, Pack};
use crate::scenario::{Control, Scenario, View, WAIT};

pub const ENGINE: &str = concat!("litgraph ", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct Request {
    /// Pack ids, file stems, or paths. Empty = every pack in the packs dir.
    pub packs: Vec<String>,
    /// Apply packs/links.json between loaded packs (default true).
    pub links: Option<bool>,
    /// Keep v1 absorbing-terminal semantics (no `accept` continuations).
    pub no_continuations: bool,
    pub scenario: Scenario,
    pub op: Value,
}

// ---------------------------------------------------------------------------
// Pack discovery
// ---------------------------------------------------------------------------

/// `$LITGRAPH_PACKS`, else the nearest `packs/` walking up from cwd, else the
/// repo's own `packs/`.
pub fn packs_dir() -> PathBuf {
    if let Ok(p) = std::env::var("LITGRAPH_PACKS") {
        return PathBuf::from(p);
    }
    if let Ok(mut d) = std::env::current_dir() {
        loop {
            let c = d.join("packs");
            if c.join("links.json").exists()
                || c.is_dir()
                    && std::fs::read_dir(&c)
                        .map(|r| r.count() > 0)
                        .unwrap_or(false)
            {
                return c;
            }
            if !d.pop() {
                break;
            }
        }
    }
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../packs"))
}

pub struct Catalog {
    pub dir: PathBuf,
    pub packs: Vec<(PathBuf, Pack, String)>,
    pub links: LinkFile,
}

fn fingerprint(bytes: &[u8]) -> String {
    // Small dependency-free FNV-1a 64 content fingerprint (not cryptographic):
    // enough to detect that a pack changed between two analyses.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("fnv1a64:{h:016x}")
}

impl Catalog {
    pub fn load(dir: &FsPath) -> Result<Catalog> {
        let mut packs = vec![];
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| Error::Io(format!("{}: {e}", dir.display())))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension().is_some_and(|x| x == "json")
                    && p.file_name().is_some_and(|f| f != "links.json")
            })
            .collect();
        entries.sort();
        for p in entries {
            let text = std::fs::read_to_string(&p)
                .map_err(|e| Error::Io(format!("{}: {e}", p.display())))?;
            let pack = Pack::from_json(&text)
                .map_err(|e| Error::Parse(format!("{}: {e}", p.display())))?;
            packs.push((p, pack, fingerprint(text.as_bytes())));
        }
        let lp = dir.join("links.json");
        let links = if lp.exists() {
            let text = std::fs::read_to_string(&lp).map_err(|e| Error::Io(e.to_string()))?;
            serde_json::from_str::<LinkFile>(&text)
                .map_err(|e| Error::Parse(format!("links.json: {e}")))?
        } else {
            LinkFile::default()
        };
        Ok(Catalog {
            dir: dir.to_path_buf(),
            packs,
            links,
        })
    }

    /// Resolve pack refs: id, forum, file stem, or a path to a JSON file.
    pub fn select(&self, refs: &[String]) -> Result<Vec<(Pack, String)>> {
        if refs.is_empty() || refs.iter().any(|r| r == "all") {
            return Ok(self
                .packs
                .iter()
                .map(|(_, p, h)| (p.clone(), h.clone()))
                .collect());
        }
        refs.iter()
            .map(|r| {
                if r.ends_with(".json") && FsPath::new(r).exists() {
                    let text = std::fs::read_to_string(r).map_err(|e| Error::Io(e.to_string()))?;
                    return Ok((Pack::from_json(&text)?, fingerprint(text.as_bytes())));
                }
                self.packs
                    .iter()
                    .find(|(path, p, _)| {
                        p.id == *r
                            || p.forum.as_deref() == Some(r)
                            || path.file_stem().is_some_and(|s| s == r.as_str())
                    })
                    .map(|(_, p, h)| (p.clone(), h.clone()))
                    .ok_or_else(|| {
                        Error::NotFound(format!(
                            "pack {r}; available: {}",
                            self.packs
                                .iter()
                                .map(|(_, p, _)| p.id.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ))
                    })
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Rendering helpers
// ---------------------------------------------------------------------------

fn r(x: f64) -> Value {
    if x.is_finite() {
        json!((x * 1e6).round() / 1e6)
    } else {
        Value::Null
    }
}

fn node_ref(v: &View, n: NodeIx) -> Value {
    let node = &v.g.nodes[n];
    json!({ "id": node.id, "label": node.label })
}

fn edge_ref(v: &View, e: usize) -> Value {
    let ed = &v.g.edges[e];
    let mut m = Map::new();
    m.insert("id".into(), json!(ed.id));
    m.insert("label".into(), json!(ed.label));
    m.insert("from".into(), json!(v.g.nodes[ed.from].id));
    m.insert("to".into(), json!(v.g.nodes[ed.to].id));
    m.insert("role".into(), json!(v.role[e]));
    if let Some(a) = &ed.authority {
        m.insert("authority".into(), json!(a));
    }
    if let Some(d) = &ed.deadline {
        m.insert("deadline_days".into(), json!(d.length));
    }
    if let Some(p) = v.prob[e] {
        m.insert("p".into(), r(p));
    }
    Value::Object(m)
}

fn path_json(v: &View, p: &paths::Path, names: &[String]) -> Value {
    json!({
        "totals": names.iter().zip(&p.totals).map(|(k, x)| (k.clone(), r(*x))).collect::<Map<_, _>>(),
        "probability": r(p.probability),
        "steps": p.edges.len(),
        "line": p.edges.iter().map(|&e| format!("{} --[{}]--> {}", v.g.nodes[v.g.edges[e].from].local_id, v.g.edges[e].label, v.g.nodes[v.g.edges[e].to].local_id)).collect::<Vec<_>>(),
        "edges": p.edges.iter().map(|&e| v.g.edges[e].id.clone()).collect::<Vec<_>>(),
    })
}

/// Resolve a target spec: node ref, `terminals`, or `tag:<outcome>`.
fn targets(v: &View, spec: &str) -> Result<Vec<NodeIx>> {
    if spec == "terminals" {
        return Ok(v.g.terminals().collect());
    }
    if let Some(tag) = spec.strip_prefix("tag:") {
        let ts: Vec<NodeIx> =
            v.g.terminals()
                .filter(|&t| v.g.nodes[t].outcome.iter().any(|o| o == tag))
                .collect();
        if ts.is_empty() {
            return Err(Error::NotFound(format!("no terminals tagged {tag}")));
        }
        return Ok(ts);
    }
    Ok(vec![v.g.node(spec)?])
}

fn s<'a>(op: &'a Value, k: &str) -> Option<&'a str> {
    op.get(k).and_then(Value::as_str)
}
fn f(op: &Value, k: &str, d: f64) -> f64 {
    op.get(k).and_then(Value::as_f64).unwrap_or(d)
}
fn u(op: &Value, k: &str, d: usize) -> usize {
    op.get(k)
        .and_then(Value::as_u64)
        .map(|x| x as usize)
        .unwrap_or(d)
}
fn strs(op: &Value, k: &str) -> Vec<String> {
    match op.get(k) {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect(),
        Some(Value::String(x)) => vec![x.clone()],
        _ => vec![],
    }
}

fn metric_list(v: &View, names: &[String]) -> Result<Vec<(String, Vec<f64>)>> {
    names
        .iter()
        .map(|n| Ok((n.clone(), v.metric(n)?)))
        .collect()
}

/// Human label for a chosen option (an edge, or WAIT).
pub fn choice_label(v: &View, e: usize) -> String {
    if e == WAIT {
        "wait (let the proceeding move)".into()
    } else {
        v.g.edges[e].label.clone()
    }
}

fn unconverged_ids(v: &View, sol: &mdp::Solution) -> Vec<String> {
    sol.unconverged
        .iter()
        .take(12)
        .map(|&n| v.g.nodes[n].id.clone())
        .collect()
}

fn choice_id(v: &View, e: usize) -> String {
    if e == WAIT {
        "WAIT".into()
    } else {
        v.g.edges[e].id.clone()
    }
}

/// The best line from `start`: our choices, the opponent's choices, and the
/// most likely draw at chance points.
fn best_line(v: &View, sol: &mdp::Solution, start: NodeIx, max: usize) -> Vec<Value> {
    let mut out = vec![];
    let mut cur = start;
    let mut seen = vec![false; v.g.nodes.len()];
    while out.len() < max && !seen[cur] {
        seen[cur] = true;
        let plan = &v.plan[cur];
        if matches!(plan.control, Control::Terminal | Control::Sink) {
            break;
        }
        let dist = chain::step_dist(v, &sol.choice, cur);
        let Some(&(e, p)) = dist.iter().max_by(|a, b| a.1.total_cmp(&b.1)) else {
            break;
        };
        let kind = if sol.choice.get(&cur) == Some(&e) {
            match plan.control {
                Control::Opponent => "opponent-choice",
                Control::Chance => "worst-case-draw",
                _ => "choice",
            }
        } else if sol.choice.get(&cur) == Some(&WAIT) {
            if plan.control == Control::Opponent {
                "opponent-waits-likely-draw"
            } else {
                "wait-likely-draw"
            }
        } else {
            "likely-draw"
        };
        let mut j = edge_ref(v, e);
        j["kind"] = json!(kind);
        j["step_p"] = r(p);
        j["q"] = r(sol.q[e]);
        out.push(j);
        cur = v.g.edges[e].to;
    }
    out
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

pub fn handle(req: &Request, catalog: &Catalog) -> Value {
    let t0 = std::time::Instant::now();
    let op_name = req
        .op
        .get("op")
        .and_then(Value::as_str)
        .unwrap_or("describe")
        .to_string();
    let result = run(req, catalog, &op_name);
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    match result {
        Ok((result, warnings, provenance)) => json!({
            "ok": true, "op": op_name, "result": result, "warnings": warnings, "provenance": provenance, "elapsed_ms": r(ms),
        }),
        Err(e) => json!({
            "ok": false, "op": op_name,
            "error": { "code": e.code(), "message": e.to_string(), "hint": hint(&e) },
            "elapsed_ms": r(ms),
        }),
    }
}

fn hint(e: &Error) -> &'static str {
    match e {
        Error::NotFound(_) => "run {\"op\":{\"op\":\"graph\"}} or {\"op\":{\"op\":\"packs\"}} to list ids; local ids work when unique",
        Error::Expr(_) => "run {\"op\":{\"op\":\"describe\"}} for variables/functions; test a metric with {\"op\":{\"op\":\"metric\",\"spec\":\"...\"}}",
        Error::Numeric(_) => "a forced or optimal choice loops forever; add a mask or policy to break the cycle",
        _ => "",
    }
}

type Out = (Value, Vec<Value>, Value);

fn run(req: &Request, catalog: &Catalog, op_name: &str) -> Result<Out> {
    if op_name == "describe" {
        return Ok((describe(catalog), vec![], json!({ "engine": ENGINE })));
    }
    let selected = catalog.select(&req.packs)?;
    let packs: Vec<Pack> = selected.iter().map(|(p, _)| p.clone()).collect();
    let links = if req.links.unwrap_or(true) {
        catalog.links.clone()
    } else {
        LinkFile::default()
    };
    let g = Graph::compile(
        &packs,
        &links,
        &CompileOptions {
            no_continuations: req.no_continuations,
        },
    )?;
    if op_name == "lint" {
        let diags = lint::lint(&g, &packs);
        return Ok((
            json!({ "diagnostics": diags, "count": diags.len() }),
            vec![],
            provenance(&g, &selected, &req.scenario),
        ));
    }
    if op_name == "packs" {
        return Ok((
            packs_summary(&g, &packs),
            vec![],
            provenance(&g, &selected, &req.scenario),
        ));
    }
    if op_name == "batch" {
        let ops = req
            .op
            .get("ops")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let results: Vec<Value> = ops
            .into_iter()
            .map(|op| {
                let sub = Request { op, ..req.clone() };
                handle(&sub, catalog)
            })
            .collect();
        return Ok((
            json!(results),
            vec![],
            provenance(&g, &selected, &req.scenario),
        ));
    }
    if op_name == "compare" {
        return compare(req, catalog, &g, &selected);
    }
    let view = View::new(&g, &req.scenario)?;
    let result = run_op(&view, &req.op, op_name, &req.scenario, &g)?;
    let mut all: Vec<(String, Option<String>, String)> = view
        .warnings
        .iter()
        .map(|w| (w.code.to_string(), w.at.clone(), w.message.clone()))
        .collect();
    all.extend(
        g.notes
            .iter()
            .map(|n| ("compile".to_string(), None, n.clone())),
    );
    if let Some(nc) = result.get("not_converged").and_then(Value::as_array) {
        for n in nc {
            all.push((
                "not-converged".into(),
                n.as_str().map(String::from),
                "value iteration hit the cap in a cycle: someone can force a costly loop forever, so values here are unreliable (check masks, opponent mode, or add an exit)".into(),
            ));
        }
    }
    Ok((
        result,
        group_warnings(all),
        provenance(&g, &selected, &req.scenario),
    ))
}

/// One entry per warning code: count, a representative message, and up to
/// eight locations. Keeps responses small without hiding anything (the
/// `lint` op lists every location).
fn group_warnings(all: Vec<(String, Option<String>, String)>) -> Vec<Value> {
    let mut by: BTreeMap<String, (usize, String, Vec<String>)> = BTreeMap::new();
    for (code, at, msg) in all {
        let e = by.entry(code).or_insert((0, msg, vec![]));
        e.0 += 1;
        if let Some(a) = at {
            if e.2.len() < 8 {
                e.2.push(a);
            }
        }
    }
    by.into_iter().map(|(code, (count, message, at))| json!({ "code": code, "count": count, "example": message, "at": at })).collect()
}

fn provenance(g: &Graph, selected: &[(Pack, String)], sc: &Scenario) -> Value {
    let mut params = metrics::default_params();
    params.extend(sc.params.clone());
    json!({
        "engine": ENGINE,
        "packs": selected.iter().map(|(p, h)| json!({ "id": p.id, "schema_version": p.schema_version, "fingerprint": h, "sources": p.sources.len() })).collect::<Vec<_>>(),
        "nodes": g.nodes.len(), "edges": g.edges.len(),
        "params": params,
        "modes": { "mixed": sc.mixed, "opponent": sc.opponent, "prob_fill": sc.prob_fill, "objective": sc.objective, "cost": sc.cost.clone().unwrap_or("dollars".into()), "utility": sc.utility.clone().unwrap_or("ev".into()) },
    })
}

fn run_op(v: &View, op: &Value, name: &str, sc: &Scenario, g: &Graph) -> Result<Value> {
    let start = match s(op, "from") {
        Some(x) => g.node(x)?,
        None => v.start,
    };
    Ok(match name {
        "graph" => graph_dump(v, s(op, "node")),
        "metric" => {
            let spec = s(op, "spec").ok_or_else(|| Error::Invalid("metric needs `spec`".into()))?;
            let vals = v.metric(spec)?;
            let mut rows: Vec<(usize, f64)> = vals
                .iter()
                .copied()
                .enumerate()
                .filter(|(_, x)| x.is_finite())
                .collect();
            rows.sort_by(|a, b| b.1.total_cmp(&a.1));
            let top = u(op, "top", 25);
            json!({
                "spec": spec,
                "resolved": crate::scenario::resolve_spec(spec, &sc.metrics, metrics::METRICS),
                "sum": r(rows.iter().map(|x| x.1).sum()),
                "nonzero": rows.iter().filter(|x| x.1 != 0.0).count(),
                "top": rows.iter().take(top).map(|&(e, x)| { let mut j = edge_ref(v, e); j["value"] = r(x); j }).collect::<Vec<_>>(),
            })
        }
        "solve" => {
            let sol = mdp::solve(v, &mdp::SolveOptions::default())?;
            let policy: Vec<Value> = sol
                .my_policy(v)
                .map(|(n, e)| json!({ "node": v.g.nodes[n].id, "edge": choice_id(v, e), "label": choice_label(v, e), "q": r(sol.option_q(v, n, e)) }))
                .collect();
            json!({
                "start": node_ref(v, start),
                "value": r(sol.value[start]),
                "converged": sol.converged, "iterations": sol.iterations, "fee_shift_rounds": sol.fee_shift_rounds,
                "not_converged": unconverged_ids(v, &sol),
                "best_line": best_line(v, &sol, start, u(op, "max_steps", 40)),
                "policy": if op.get("full_policy").and_then(Value::as_bool).unwrap_or(false) { json!(policy) } else { json!(policy.len()) },
                "values": if op.get("all_values").and_then(Value::as_bool).unwrap_or(false) {
                    json!((0..v.g.nodes.len()).map(|n| (v.g.nodes[n].id.clone(), r(sol.value[n]))).collect::<Map<_, _>>())
                } else { Value::Null },
            })
        }
        "explain" => explain(v, g, op, start)?,
        "chain" => {
            let sol = mdp::solve(v, &mdp::SolveOptions::default())?;
            let not_converged = unconverged_ids(v, &sol);
            let names = {
                let m = strs(op, "metrics");
                if m.is_empty() {
                    vec!["dollars".into(), "elapsed".into(), "hours".into()]
                } else {
                    m
                }
            };
            let ms = metric_list(v, &names)?;
            let c = chain::chain(v, &sol, start, &ms)?;
            json!({
                "start": node_ref(v, start),
                "absorption": c.absorption.iter().map(|&(t, p)| json!({ "terminal": v.g.nodes[t].id, "label": v.g.nodes[t].label, "p": r(p), "utility": r(v.utility[t]), "outcome": v.g.nodes[t].outcome })).collect::<Vec<_>>(),
                "expected": c.expected.iter().map(|(k, x)| (k.clone(), r(*x))).collect::<Map<_, _>>(),
                "expected_steps": r(c.expected_steps),
                "not_converged": not_converged,
                "expected_utility": r(c.expected_utility),
                "expected_net": r(c.expected_utility - c.expected.get(names.first().map(String::as_str).unwrap_or("dollars")).copied().unwrap_or(0.0)),
                "sink_mass": r(c.sink_mass),
                "top_visits": c.visits.iter().take(u(op, "top", 15)).map(|&(n, x)| json!({ "node": v.g.nodes[n].id, "visits": r(x) })).collect::<Vec<_>>(),
            })
        }
        "simulate" => {
            let sol = mdp::solve(v, &mdp::SolveOptions::default())?;
            let names = {
                let m = strs(op, "metrics");
                if m.is_empty() {
                    vec!["dollars".into(), "elapsed".into()]
                } else {
                    m
                }
            };
            let ms = metric_list(v, &names)?;
            let o = sim::SimOptions {
                runs: u(op, "runs", 10_000),
                seed: op.get("seed").and_then(Value::as_u64).unwrap_or(7),
                alpha: f(op, "alpha", 0.1),
                max_steps: u(op, "max_steps", 10_000),
                sample_durations: op
                    .get("sample_durations")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                keep_samples: u(op, "samples", 3),
            };
            let res = sim::simulate(v, &sol, start, &ms, &o)?;
            json!({
                "start": node_ref(v, start), "runs": res.runs, "seed": res.seed,
                "net": res.net, "cvar": r(res.cvar), "alpha": res.alpha, "p_loss": r(res.p_loss),
                "metrics": res.metrics, "truncated": res.truncated,
                "terminals": res.terminals.iter().map(|&(t, p)| json!({ "terminal": v.g.nodes[t].id, "p": r(p) })).collect::<Vec<_>>(),
                "samples": res.samples.iter().map(|tr| tr.iter().map(|&e| v.g.edges[e].label.clone()).collect::<Vec<_>>()).collect::<Vec<_>>(),
            })
        }
        "path" => {
            let metric = s(op, "metric").unwrap_or("dollars");
            let w = v.metric(metric)?;
            let to = s(op, "to").unwrap_or("terminals");
            let ts = targets(v, to)?;
            let k = u(op, "k", 1);
            let extra = strs(op, "report");
            let ex = metric_list(v, &extra)?;
            let names: Vec<String> = std::iter::once(metric.to_string())
                .chain(extra.iter().cloned())
                .collect();
            let mut found: Vec<paths::Path> = vec![];
            if k <= 1 {
                if let Some(p) = paths::shortest_to_any(v, start, &ts, &w)? {
                    found.push(p);
                }
            } else {
                for &t in &ts {
                    found.extend(paths::k_shortest(v, start, t, &w, k)?);
                }
                found.sort_by(|a, b| a.totals[0].total_cmp(&b.totals[0]));
                found.truncate(k);
            }
            for p in &mut found {
                for (_, vals) in &ex {
                    p.totals.push(p.edges.iter().map(|&e| vals[e]).sum());
                }
            }
            json!({ "metric": metric, "from": node_ref(v, start), "to": to, "paths": found.iter().map(|p| { let mut j = path_json(v, p, &names); j["end"] = node_ref(v, *p.nodes.last().unwrap()); j }).collect::<Vec<_>>() })
        }
        "pareto" => {
            let objs = {
                let o = strs(op, "objectives");
                if o.is_empty() {
                    vec!["dollars".into(), "elapsed".into(), "surprise".into()]
                } else {
                    o
                }
            };
            let ws: Vec<Vec<f64>> = objs.iter().map(|o| v.metric(o)).collect::<Result<_>>()?;
            let to = s(op, "to").unwrap_or("terminals");
            let ts = targets(v, to)?;
            let max_labels = u(op, "max_labels", 200_000);
            let mut all: Vec<paths::Path> = vec![];
            let mut truncated = false;
            for &t in &ts {
                let fr = paths::pareto(v, start, t, &ws, max_labels)?;
                truncated |= fr.truncated;
                all.extend(fr.paths);
            }
            let dom = |a: &paths::Path, b: &paths::Path| {
                a.totals.iter().zip(&b.totals).all(|(x, y)| x <= y)
                    && a.totals.iter().zip(&b.totals).any(|(x, y)| x < y)
            };
            let front: Vec<&paths::Path> = all
                .iter()
                .filter(|p| !all.iter().any(|q| dom(q, p)))
                .collect();
            let limit = u(op, "limit", 25);
            json!({ "objectives": objs, "to": to, "truncated": truncated, "frontier_size": front.len(),
                    "frontier": front.iter().take(limit).map(|p| { let mut j = path_json(v, p, &objs); j["end"] = node_ref(v, *p.nodes.last().unwrap()); j }).collect::<Vec<_>>() })
        }
        "sweep" => {
            let param =
                s(op, "param").ok_or_else(|| Error::Invalid("sweep needs `param`".into()))?;
            let watch: Vec<NodeIx> = strs(op, "watch")
                .iter()
                .map(|n| g.node(n))
                .collect::<Result<_>>()?;
            let res = sweep::sweep(
                g,
                sc,
                param,
                f(op, "lo", 100.0),
                f(op, "hi", 1500.0),
                u(op, "steps", 15),
                &watch,
                f(op, "tol", 1e-3),
            )?;
            json!({
                "param": res.param,
                "curve": res.curve.iter().map(|(x, y)| json!([r(*x), r(*y)])).collect::<Vec<_>>(),
                "breakpoints": res.breakpoints.iter().map(|b| json!({ "node": v.g.nodes[b.node].id, "at": r(b.at), "before": choice_label(v, b.before), "after": choice_label(v, b.after) })).collect::<Vec<_>>(),
            })
        }
        "tornado" => {
            let params = {
                let p = strs(op, "params");
                if p.is_empty() {
                    vec!["rate".into(), "stakes".into()]
                } else {
                    p
                }
            };
            let (base, rows) = sweep::tornado(
                g,
                sc,
                &params,
                f(op, "rel", 0.25),
                f(op, "dp", 0.1),
                op.get("probabilities")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            )?;
            json!({ "base_value": r(base), "rows": rows.iter().take(u(op, "top", 20)).collect::<Vec<_>>() })
        }
        "structure" => structure_op(v, op, start)?,
        other => {
            return Err(Error::Invalid(format!(
                "unknown op `{other}`; see describe"
            )))
        }
    })
}

fn explain(v: &View, g: &Graph, op: &Value, start: NodeIx) -> Result<Value> {
    let n = match s(op, "node") {
        Some(x) => g.node(x)?,
        None => start,
    };
    let sol = mdp::solve(v, &mdp::SolveOptions::default())?;
    let node = &g.nodes[n];
    let plan = &v.plan[n];
    let best = sol.choice.get(&n).copied();
    let mut options: Vec<(f64, Value)> = vec![];
    for e in v.outs(n) {
        let ed = &g.edges[e];
        let q = sol.q[e];
        // What happens after taking this edge: absorption from its target under the policy.
        let after = chain::chain(v, &sol, ed.to, &[]).ok();
        let mut j = edge_ref(v, e);
        j["q"] = r(q);
        j["cost"] = r(v.cost[e]);
        j["hours"] = r(ed.hours);
        j["fees"] = r(ed.cost);
        if let Some(du) = &ed.duration {
            j["elapsed_days"] = json!(du);
        }
        if !ed.tags.is_empty() {
            j["tags"] = json!(ed.tags);
        }
        if let Some(nt) = &ed.note {
            j["note"] = json!(nt);
        }
        j["chosen"] = json!(best == Some(e));
        j["kind"] = json!(if plan.choices.contains(&e) {
            "choice"
        } else {
            "draw"
        });
        if plan.wait.iter().any(|&(w, _)| w == e) {
            j["kind"] = json!("draw-if-waiting");
        }
        if let (Some(b), true) = (best, plan.choices.contains(&e)) {
            let qb = sol.option_q(v, n, b);
            j["regret"] = r(if plan.minimize { q - qb } else { qb - q });
        }
        if let Some(a) = after {
            j["then"] = json!(a
                .absorption
                .iter()
                .take(4)
                .map(|&(t, p)| json!({ "terminal": g.nodes[t].local_id, "p": r(p) }))
                .collect::<Vec<_>>());
        }
        options.push((q, j));
    }
    if !plan.wait.is_empty() {
        let qw = sol.option_q(v, n, WAIT);
        let mut j = json!({
            "id": "WAIT", "label": choice_label(v, WAIT), "kind": "wait", "q": r(qw),
            "chosen": best == Some(WAIT),
            "draws": plan.wait.iter().map(|&(e, p)| json!({ "edge": g.edges[e].id, "label": g.edges[e].label, "p": r(p) })).collect::<Vec<_>>(),
        });
        if let Some(b) = best {
            let qb = sol.option_q(v, n, b);
            j["regret"] = r(if plan.minimize { qw - qb } else { qb - qw });
        }
        options.push((qw, j));
    }
    options.sort_by(|a, b| b.0.total_cmp(&a.0));
    let warns: Vec<&crate::scenario::Warning> = v
        .warnings
        .iter()
        .filter(|w| w.at.as_deref() == Some(node.id.as_str()))
        .collect();
    Ok(json!({
        "node": { "id": node.id, "label": node.label, "kind": node.kind, "cite": node.cite, "note": node.note },
        "control": plan.control,
        "who_decides": match plan.control { Control::Me => "you", Control::Opponent => if plan.minimize { "opponent (adversarial)" } else { "opponent" }, Control::Chance => "tribunal/chance", Control::Terminal => "nobody (terminal)", Control::Sink => "nobody (dead end)" },
        "value": r(sol.value[n]),
        "interrupt_mass": r(1.0 - plan.choice_mass),
        "options": options.into_iter().map(|x| x.1).collect::<Vec<_>>(),
        "warnings_here": warns,
    }))
}

fn structure_op(v: &View, op: &Value, start: NodeIx) -> Result<Value> {
    let what = s(op, "what").unwrap_or("summary");
    Ok(match what {
        "scc" => {
            let comps: Vec<Vec<String>> = structure::scc(v)
                .into_iter()
                .filter(|c| structure::is_cyclic(v, c))
                .map(|c| c.iter().map(|&n| v.g.nodes[n].id.clone()).collect())
                .collect();
            json!({ "cycles": comps })
        }
        "dominators" => {
            let idom = structure::dominators(v, start);
            // Unavoidable gateways to a target: its dominator chain.
            let chain_to = |t: NodeIx| {
                let mut c = vec![];
                let mut x = t;
                while let Some(d) = idom[x] {
                    c.push(v.g.nodes[d].id.clone());
                    x = d;
                }
                c.reverse();
                c
            };
            match s(op, "to") {
                Some(t) => json!({ "gateways": chain_to(v.g.node(t)?) }),
                None => {
                    json!({ "idom": (0..v.g.nodes.len()).filter_map(|n| idom[n].map(|d| (v.g.nodes[n].id.clone(), json!(v.g.nodes[d].id)))).collect::<Map<_, _>>() })
                }
            }
        }
        "mincut" => {
            let to = s(op, "to").ok_or_else(|| Error::Invalid("mincut needs `to`".into()))?;
            let t = v.g.node(to)?;
            let cap = v.metric(s(op, "capacity").unwrap_or("1"))?;
            let c = structure::min_cut(v, start, t, &cap);
            json!({ "value": r(c.value), "cut": c.edges.iter().map(|&e| edge_ref(v, e)).collect::<Vec<_>>() })
        }
        "betweenness" => {
            let b = structure::betweenness(v);
            let mut rows: Vec<(usize, f64)> = b.into_iter().enumerate().collect();
            rows.sort_by(|a, b| b.1.total_cmp(&a.1));
            json!({ "top": rows.iter().take(u(op, "top", 15)).map(|&(n, x)| json!({ "node": v.g.nodes[n].id, "betweenness": r(x) })).collect::<Vec<_>>() })
        }
        "reachability" => {
            let reach = structure::reachable(v, start);
            let terms: Vec<String> =
                v.g.terminals()
                    .filter(|&t| reach[t])
                    .map(|t| v.g.nodes[t].id.clone())
                    .collect();
            let unreachable: Vec<String> = (0..v.g.nodes.len())
                .filter(|&n| !reach[n])
                .map(|n| v.g.nodes[n].id.clone())
                .collect();
            json!({ "reachable_terminals": terms, "unreachable": unreachable })
        }
        _ => {
            let cyc = structure::scc(v)
                .into_iter()
                .filter(|c| structure::is_cyclic(v, c))
                .count();
            json!({ "nodes": v.g.nodes.len(), "edges": v.g.edges.len(), "active_edges": v.active.iter().filter(|a| **a).count(), "terminals": v.g.terminals().count(), "cycles": cyc, "what": ["scc", "dominators", "mincut", "betweenness", "reachability"] })
        }
    })
}

fn compare(
    req: &Request,
    catalog: &Catalog,
    g: &Graph,
    selected: &[(Pack, String)],
) -> Result<Out> {
    let variant = req.op.get("variant").cloned().unwrap_or(json!({}));
    let inner = req
        .op
        .get("inner")
        .cloned()
        .unwrap_or(json!({ "op": "chain" }));
    let mut base_json =
        serde_json::to_value(&req.scenario).map_err(|e| Error::Invalid(e.to_string()))?;
    merge(&mut base_json, &variant);
    let variant_sc: Scenario =
        serde_json::from_value(base_json).map_err(|e| Error::Invalid(format!("variant: {e}")))?;
    let a = handle(
        &Request {
            op: inner.clone(),
            ..req.clone()
        },
        catalog,
    );
    let b = handle(
        &Request {
            op: inner,
            scenario: variant_sc,
            ..req.clone()
        },
        catalog,
    );
    let delta = |k: &str| match (a["result"][k].as_f64(), b["result"][k].as_f64()) {
        (Some(x), Some(y)) => json!(y - x),
        _ => Value::Null,
    };
    let result = json!({
        "delta": { "value": delta("value"), "expected_utility": delta("expected_utility"), "expected_net": delta("expected_net") },
        "base": a, "variant": b,
    });
    Ok((result, vec![], provenance(g, selected, &req.scenario)))
}

/// JSON merge-patch (RFC 7386).
fn merge(base: &mut Value, patch: &Value) {
    if let (Some(b), Some(p)) = (base.as_object_mut(), patch.as_object()) {
        for (k, v) in p {
            if v.is_null() {
                b.remove(k);
            } else {
                merge(b.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
    } else {
        *base = patch.clone();
    }
}

fn graph_dump(v: &View, around: Option<&str>) -> Value {
    let keep: Vec<bool> = match around.and_then(|a| v.g.node(a).ok()) {
        None => vec![true; v.g.nodes.len()],
        Some(n) => {
            let mut k = vec![false; v.g.nodes.len()];
            k[n] = true;
            for &e in v.g.out[n].iter().chain(&v.g.inc[n]) {
                k[v.g.edges[e].from] = true;
                k[v.g.edges[e].to] = true;
            }
            k
        }
    };
    json!({
        "nodes": (0..v.g.nodes.len()).filter(|&n| keep[n]).map(|n| { let nd = &v.g.nodes[n]; json!({ "id": nd.id, "kind": nd.kind, "label": nd.label, "control": v.plan[n].control, "payoff": if nd.is_terminal() { r(v.payoff[n]) } else { Value::Null }, "payoff_source": nd.payoff_source, "outcome": nd.outcome }) }).collect::<Vec<_>>(),
        "edges": (0..v.g.edges.len()).filter(|&e| keep[v.g.edges[e].from] && keep[v.g.edges[e].to]).map(|e| { let mut j = edge_ref(v, e); j["active"] = json!(v.active[e]); j["hours"] = r(v.g.edges[e].hours); j["fees"] = r(v.g.edges[e].cost); j }).collect::<Vec<_>>(),
    })
}

fn packs_summary(g: &Graph, packs: &[Pack]) -> Value {
    json!(packs.iter().map(|p| {
        let terms: Vec<&crate::model::RawNode> = p.nodes.iter().filter(|n| n.kind == Some(crate::model::NodeKind::Terminal)).collect();
        let non_applicant = p.edges.iter().filter(|e| e.actor != "applicant").count();
        json!({
            "id": p.id, "title": p.title, "forum": p.forum, "schema_version": p.schema_version,
            "start": crate::model::qualify(&p.id, &p.start_node_id),
            "nodes": p.nodes.len(), "edges": p.edges.len(), "terminals": terms.len(),
            "quality": {
                "terminals_with_payoff": terms.iter().filter(|n| n.payoff.is_some()).count(),
                "world_edges_with_probability": format!("{}/{}", p.edges.iter().filter(|e| e.actor != "applicant" && e.probability.is_some()).count(), non_applicant),
                "edges_with_hours": p.edges.iter().filter(|e| e.hours.is_some()).count(),
                "edges_with_duration": p.edges.iter().filter(|e| e.duration.is_some()).count(),
                "sources": p.sources.len(),
            },
            "links_out": g.edges.iter().filter(|e| e.link && g.nodes[e.from].pack == p.id).map(|e| g.nodes[e.to].id.clone()).collect::<Vec<_>>(),
        })
    }).collect::<Vec<_>>())
}

pub fn describe(catalog: &Catalog) -> Value {
    let b = |xs: &[metrics::Builtin]| {
        xs.iter()
            .map(|m| json!({ "name": m.name, "expr": m.expr, "doc": m.doc }))
            .collect::<Vec<_>>()
    };
    let kv = |xs: &[(&str, &str)]| {
        xs.iter()
            .map(|(k, d)| json!({ "name": k, "doc": d }))
            .collect::<Vec<_>>()
    };
    json!({
        "engine": ENGINE,
        "packs_dir": catalog.dir.display().to_string(),
        "packs": catalog.packs.iter().map(|(_, p, _)| json!({ "id": p.id, "forum": p.forum, "title": p.title })).collect::<Vec<_>>(),
        "links": catalog.links.links.len(),
        "instances": catalog.links.instances.iter().map(|(k, i)| json!({ "id": k, "pack": i.pack, "note": i.note })).collect::<Vec<_>>(),
        "request": {
            "shape": { "packs": "[ids] (empty = all)", "links": "bool (default true)", "no_continuations": "bool", "scenario": "Scenario", "op": "{op: <name>, ...}" },
            "scenario": {
                "params": "{name: number} — visible to every expression",
                "metrics": "{name: edge-expr} — custom cost/weight functions",
                "utilities": "{name: terminal-expr}",
                "cost": "metric name or inline expr (default dollars)",
                "utility": "utility name or inline expr (default ev)",
                "perspective": "{actor: self|opponent|nature}",
                "mask": "edge-expr; edges where 0 are removed",
                "remove_edges": "[edge refs]",
                "probabilities": "{edge ref: p} (siblings rescaled)",
                "probability_fn": "edge-expr rewriting p on world edges (p = authored or NaN), chance nodes renormalized",
                "payoffs": "{node ref: usd}",
                "policy": "{node ref: edge ref} force our move",
                "mixed": "nature-first (default; falls back to act-or-wait) | act-or-wait | self-only | optimistic",
                "opponent": "auto | adversarial | chance",
                "prob_fill": "residual | uniform",
                "objective": "{type: expected} | {type: cara, a} | {type: worst}",
                "discount_annual": "number",
                "fee_shift": "{fraction, eligible?: terminal-expr}",
                "start": "node ref",
            },
        },
        "ops": {
            "describe": "this document",
            "packs": "packs with data-quality stats",
            "lint": "content diagnostics",
            "graph": "{node?} nodes/edges (neighborhood if node)",
            "metric": "{spec, top?} evaluate a metric on every edge — test custom functions here",
            "explain": "{node?} who decides, every option with q/regret/cost/what-happens-next",
            "solve": "{from?, full_policy?, all_values?} value + best line under the scenario",
            "chain": "{from?, metrics?} absorption probabilities + expected totals under the optimal policy",
            "simulate": "{runs?, seed?, metrics?, alpha?, sample_durations?} outcome distribution, CVaR, P(loss)",
            "path": "{to?, metric?, k?, report?} shortest / k-shortest lines; to = node | terminals | tag:<outcome>",
            "pareto": "{to?, objectives?, limit?} N-objective frontier (default dollars × elapsed × surprise)",
            "sweep": "{param, lo, hi, steps?, watch?} value curve + policy breakpoints for ANY param",
            "tornado": "{params?, rel?, dp?, probabilities?} what the answer is most sensitive to",
            "structure": "{what: scc|dominators|mincut|betweenness|reachability, to?, capacity?}",
            "compare": "{variant: scenario merge-patch, inner: op} base vs variant + deltas",
            "batch": "{ops: [op, ...]} several ops on one graph",
        },
        "metrics": b(metrics::METRICS),
        "utilities": b(metrics::UTILITIES),
        "params": metrics::PARAMS.iter().map(|(k, v, d)| json!({ "name": k, "default": v, "doc": d })).collect::<Vec<_>>(),
        "edge_variables": kv(metrics::EDGE_VARS),
        "edge_functions": kv(metrics::EDGE_FUNCS),
        "terminal_variables": kv(metrics::TERMINAL_VARS),
        "math_functions": kv(crate::expr::MATH_FUNCS),
        "expression_syntax": "c ? a : b, || && == != < <= > >= + - * / % ^, unary - !, calls f(x), \"strings\" as function args, identifiers may contain dots (edge.x, to.x)",
    })
}

/// Convenience for library callers: compile the named packs from a catalog.
pub fn compile(catalog: &Catalog, refs: &[String], links: bool) -> Result<Graph> {
    let packs: Vec<Pack> = catalog.select(refs)?.into_iter().map(|(p, _)| p).collect();
    Graph::compile(
        &packs,
        &(if links {
            catalog.links.clone()
        } else {
            LinkFile::default()
        }),
        &CompileOptions::default(),
    )
}
