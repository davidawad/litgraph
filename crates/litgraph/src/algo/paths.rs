// SPDX-License-Identifier: GPL-3.0-or-later
//! Path queries over any metric: shortest (Dijkstra, or Bellman–Ford when a
//! custom metric goes negative), Yen's k-shortest loopless alternatives, and
//! the N-objective Pareto frontier (Martins label-setting).
//!
//! Paths treat every active edge as traversable — a path is a *hoped-for
//! line*, not an expectation. Each path also reports its probability (the
//! product of draw probabilities along it) so a cheap-but-unlikely line is
//! visible as such.

use serde::Serialize;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashSet};

use crate::error::{Error, Result};
use crate::model::NodeIx;
use crate::scenario::View;

/// One concrete route through the graph, with its objective totals.
#[derive(Debug, Clone, Serialize)]
pub struct Path {
    /// Nodes visited, from source to target (inclusive).
    pub nodes: Vec<NodeIx>,
    /// Edges taken, from source to target.
    pub edges: Vec<usize>,
    /// Objective totals, in the order requested.
    pub totals: Vec<f64>,
    /// Product of draw probabilities along the path (1 for pure choices).
    pub probability: f64,
}

#[derive(PartialEq)]
struct Item(f64, NodeIx);
impl Eq for Item {}
impl Ord for Item {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.total_cmp(&self.0).then(self.1.cmp(&o.1))
    }
}
impl PartialOrd for Item {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

/// Product of draw probabilities along `edges` (1.0 for pure-choice edges).
#[must_use]
pub fn path_probability(v: &View, edges: &[usize]) -> f64 {
    edges.iter().map(|&e| v.prob[e].unwrap_or(1.0)).product()
}

fn build(v: &View, s: NodeIx, t: NodeIx, prev: &[Option<usize>], w: &[f64]) -> Option<Path> {
    let mut edges = vec![];
    let mut cur = t;
    while cur != s {
        let e = prev[cur]?;
        edges.push(e);
        cur = v.g.edges[e].from;
        if edges.len() > v.g.nodes.len() {
            return None;
        }
    }
    edges.reverse();
    let mut nodes = vec![s];
    nodes.extend(edges.iter().map(|&e| v.g.edges[e].to));
    let total = edges.iter().map(|&e| w[e]).sum();
    Some(Path {
        probability: path_probability(v, &edges),
        nodes,
        edges,
        totals: vec![total],
    })
}

/// Single-source shortest paths by metric `w`; `banned` edges/nodes excluded.
fn sssp(
    v: &View,
    s: NodeIx,
    w: &[f64],
    banned_e: &HashSet<usize>,
    banned_n: &HashSet<NodeIx>,
) -> Result<(Vec<f64>, Vec<Option<usize>>)> {
    let n = v.g.nodes.len();
    let usable = |e: usize| {
        v.active[e]
            && !banned_e.contains(&e)
            && !banned_n.contains(&v.g.edges[e].to)
            && w[e].is_finite()
    };
    let mut dist = vec![f64::INFINITY; n];
    let mut prev = vec![None; n];
    dist[s] = 0.0;
    let negative = (0..v.g.edges.len()).any(|e| usable(e) && w[e] < 0.0);
    if negative {
        // Bellman–Ford.
        for round in 0..n {
            let mut changed = false;
            // Relaxes every edge by index: `usable`, `w`, and `v.g.edges` are
            // all indexed by the same edge id, so an iterator adapter would
            // need to zip three parallel collections for no clarity gain.
            #[allow(clippy::needless_range_loop)]
            for e in 0..v.g.edges.len() {
                if !usable(e) {
                    continue;
                }
                let (a, b) = (v.g.edges[e].from, v.g.edges[e].to);
                if dist[a].is_finite() && dist[a] + w[e] < dist[b] - 1e-12 {
                    dist[b] = dist[a] + w[e];
                    prev[b] = Some(e);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
            if round == n - 1 {
                return Err(Error::Numeric("metric has a negative cycle reachable from the source; shortest path undefined".into()));
            }
        }
        return Ok((dist, prev));
    }
    let mut heap = BinaryHeap::from([Item(0.0, s)]);
    while let Some(Item(d, u)) = heap.pop() {
        if d > dist[u] {
            continue;
        }
        for &e in &v.g.out[u] {
            if !usable(e) {
                continue;
            }
            let to = v.g.edges[e].to;
            let nd = d + w[e];
            if nd < dist[to] {
                dist[to] = nd;
                prev[to] = Some(e);
                heap.push(Item(nd, to));
            }
        }
    }
    Ok((dist, prev))
}

/// Shortest path from `s` to `t` by metric `w` (Dijkstra, or Bellman–Ford if
/// `w` goes negative on any active edge).
///
/// # Errors
/// [`Error::Numeric`] if `w` has a negative cycle reachable from `s`.
pub fn shortest(v: &View, s: NodeIx, t: NodeIx, w: &[f64]) -> Result<Option<Path>> {
    let (dist, prev) = sssp(v, s, w, &HashSet::new(), &HashSet::new())?;
    if !dist[t].is_finite() {
        return Ok(None);
    }
    Ok(build(v, s, t, &prev, w))
}

/// Shortest path to the nearest of several targets.
///
/// # Errors
/// [`Error::Numeric`] if `w` has a negative cycle reachable from `s`.
pub fn shortest_to_any(v: &View, s: NodeIx, targets: &[NodeIx], w: &[f64]) -> Result<Option<Path>> {
    let (dist, prev) = sssp(v, s, w, &HashSet::new(), &HashSet::new())?;
    let best = targets
        .iter()
        .copied()
        .filter(|&t| dist[t].is_finite())
        .min_by(|&a, &b| dist[a].total_cmp(&dist[b]));
    Ok(best.and_then(|t| build(v, s, t, &prev, w)))
}

/// Yen's k shortest loopless paths.
pub fn k_shortest(v: &View, s: NodeIx, t: NodeIx, w: &[f64], k: usize) -> Result<Vec<Path>> {
    let mut found: Vec<Path> = vec![];
    let Some(first) = shortest(v, s, t, w)? else {
        return Ok(found);
    };
    found.push(first);
    let mut candidates: Vec<Path> = vec![];
    while found.len() < k {
        let last = found.last().unwrap().clone();
        for i in 0..last.edges.len() {
            let spur = last.nodes[i];
            let root_edges = &last.edges[..i];
            let mut banned_e = HashSet::new();
            for p in &found {
                if p.edges.len() > i && p.edges[..i] == *root_edges {
                    banned_e.insert(p.edges[i]);
                }
            }
            let banned_n: HashSet<NodeIx> = last.nodes[..i].iter().copied().collect();
            let (dist, prev) = sssp(v, spur, w, &banned_e, &banned_n)?;
            if !dist[t].is_finite() {
                continue;
            }
            if let Some(spur_path) = build(v, spur, t, &prev, w) {
                let mut edges = root_edges.to_vec();
                edges.extend(&spur_path.edges);
                if found.iter().chain(&candidates).any(|p| p.edges == edges) {
                    continue;
                }
                let mut nodes = vec![s];
                nodes.extend(edges.iter().map(|&e| v.g.edges[e].to));
                let total = edges.iter().map(|&e| w[e]).sum();
                candidates.push(Path {
                    probability: path_probability(v, &edges),
                    nodes,
                    edges,
                    totals: vec![total],
                });
            }
        }
        if candidates.is_empty() {
            break;
        }
        candidates.sort_by(|a, b| a.totals[0].total_cmp(&b.totals[0]));
        found.push(candidates.remove(0));
    }
    Ok(found)
}

#[derive(Debug, Clone, Serialize)]
pub struct Frontier {
    pub paths: Vec<Path>,
    pub labels_created: usize,
    pub truncated: bool,
}

/// N-objective Pareto frontier from `s` to `t` (Martins). All objectives
/// must be non-negative on active edges. Loopless by construction: a label
/// may not revisit a node already on its own path.
pub fn pareto(
    v: &View,
    s: NodeIx,
    t: NodeIx,
    ws: &[Vec<f64>],
    max_labels: usize,
) -> Result<Frontier> {
    let k = ws.len();
    if k == 0 {
        return Err(Error::Invalid("pareto needs at least one objective".into()));
    }
    for w in ws {
        if (0..v.g.edges.len()).any(|e| v.active[e] && w[e] < 0.0) {
            return Err(Error::Invalid(
                "pareto objectives must be non-negative (negate a reward into a cost first)".into(),
            ));
        }
    }
    struct Label {
        node: NodeIx,
        cost: Vec<f64>,
        parent: Option<usize>,
        edge: Option<usize>,
    }
    let dominated = |a: &[f64], b: &[f64]| {
        a.iter().zip(b).all(|(x, y)| x <= y) && a.iter().zip(b).any(|(x, y)| x < y)
    };
    let equal = |a: &[f64], b: &[f64]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9);
    let mut labels: Vec<Label> = vec![Label {
        node: s,
        cost: vec![0.0; k],
        parent: None,
        edge: None,
    }];
    let mut perm: Vec<Vec<usize>> = vec![vec![]; v.g.nodes.len()]; // permanent labels per node
    let mut open: Vec<usize> = vec![0];
    let mut truncated = false;
    let on_path = |labels: &Vec<Label>, mut l: usize, n: NodeIx| loop {
        if labels[l].node == n {
            return true;
        }
        match labels[l].parent {
            Some(p) => l = p,
            None => return false,
        }
    };
    while !open.is_empty() {
        // Lexicographically smallest open label.
        let (pos, _) = open
            .iter()
            .enumerate()
            .min_by(|(_, &a), (_, &b)| {
                labels[a]
                    .cost
                    .iter()
                    .zip(&labels[b].cost)
                    .map(|(x, y)| x.total_cmp(y))
                    .find(|o| *o != Ordering::Equal)
                    .unwrap_or(Ordering::Equal)
            })
            .unwrap();
        let l = open.swap_remove(pos);
        let u = labels[l].node;
        perm[u].push(l);
        if u == t {
            continue;
        }
        for &e in &v.g.out[u] {
            if !v.active[e] {
                continue;
            }
            let to = v.g.edges[e].to;
            if on_path(&labels, l, to) {
                continue;
            }
            let cost: Vec<f64> = (0..k).map(|i| labels[l].cost[i] + ws[i][e]).collect();
            let beaten = perm[to]
                .iter()
                .chain(open.iter().filter(|&&o| labels[o].node == to))
                .any(|&o| dominated(&labels[o].cost, &cost) || equal(&labels[o].cost, &cost));
            if beaten {
                continue;
            }
            open.retain(|&o| !(labels[o].node == to && dominated(&cost, &labels[o].cost)));
            if labels.len() >= max_labels {
                truncated = true;
                break;
            }
            labels.push(Label {
                node: to,
                cost,
                parent: Some(l),
                edge: Some(e),
            });
            open.push(labels.len() - 1);
        }
        if truncated {
            break;
        }
    }
    let mut paths: Vec<Path> = perm[t]
        .iter()
        .map(|&l| {
            let mut edges = vec![];
            let mut cur = l;
            while let Some(e) = labels[cur].edge {
                edges.push(e);
                cur = labels[cur].parent.unwrap();
            }
            edges.reverse();
            let mut nodes = vec![s];
            nodes.extend(edges.iter().map(|&e| v.g.edges[e].to));
            Path {
                probability: path_probability(v, &edges),
                totals: labels[l].cost.clone(),
                nodes,
                edges,
            }
        })
        .collect();
    paths.sort_by(|a, b| {
        a.totals
            .iter()
            .zip(&b.totals)
            .map(|(x, y)| x.total_cmp(y))
            .find(|o| *o != Ordering::Equal)
            .unwrap_or(Ordering::Equal)
    });
    Ok(Frontier {
        paths,
        labels_created: labels.len(),
        truncated,
    })
}
