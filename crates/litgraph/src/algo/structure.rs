// SPDX-License-Identifier: GPL-3.0-or-later
//! Structural analyses over the active edges of a view: SCCs, dominators,
//! reachability, weighted min cut, betweenness.

use std::collections::VecDeque;

use crate::model::NodeIx;
use crate::scenario::View;

/// Tarjan SCC. Components are returned in reverse topological order (every
/// component appears before any component that can reach it), which is the
/// order a backward (Bellman) pass wants.
pub fn scc(v: &View) -> Vec<Vec<NodeIx>> {
    let n = v.g.nodes.len();
    let mut index = vec![usize::MAX; n];
    let mut low = vec![0; n];
    let mut on = vec![false; n];
    let mut stack = vec![];
    let mut comps = vec![];
    let mut next = 0;
    // Iterative Tarjan: (node, out-edge cursor).
    for root in 0..n {
        if index[root] != usize::MAX {
            continue;
        }
        let mut call: Vec<(NodeIx, usize)> = vec![(root, 0)];
        index[root] = next;
        low[root] = next;
        next += 1;
        stack.push(root);
        on[root] = true;
        while let Some(&mut (u, ref mut cur)) = call.last_mut() {
            let outs = &v.g.out[u];
            if *cur < outs.len() {
                let e = outs[*cur];
                *cur += 1;
                if !v.active[e] {
                    continue;
                }
                let w = v.g.edges[e].to;
                if index[w] == usize::MAX {
                    index[w] = next;
                    low[w] = next;
                    next += 1;
                    stack.push(w);
                    on[w] = true;
                    call.push((w, 0));
                } else if on[w] {
                    low[u] = low[u].min(index[w]);
                }
            } else {
                call.pop();
                if let Some(&(p, _)) = call.last() {
                    low[p] = low[p].min(low[u]);
                }
                if low[u] == index[u] {
                    let mut comp = vec![];
                    loop {
                        let w = stack.pop().unwrap();
                        on[w] = false;
                        comp.push(w);
                        if w == u {
                            break;
                        }
                    }
                    comp.sort_unstable();
                    comps.push(comp);
                }
            }
        }
    }
    comps
}

/// True if the component has an internal cycle (size > 1 or a self-loop).
pub fn is_cyclic(v: &View, comp: &[NodeIx]) -> bool {
    comp.len() > 1 || v.outs(comp[0]).any(|e| v.g.edges[e].to == comp[0])
}

pub fn reachable(v: &View, from: NodeIx) -> Vec<bool> {
    let mut seen = vec![false; v.g.nodes.len()];
    let mut q = VecDeque::from([from]);
    seen[from] = true;
    while let Some(u) = q.pop_front() {
        for e in v.outs(u) {
            let w = v.g.edges[e].to;
            if !seen[w] {
                seen[w] = true;
                q.push_back(w);
            }
        }
    }
    seen
}

/// Nodes that can reach any node in `targets`.
pub fn coreachable(v: &View, targets: &[NodeIx]) -> Vec<bool> {
    let mut seen = vec![false; v.g.nodes.len()];
    let mut q: VecDeque<NodeIx> = targets.iter().copied().collect();
    for &t in targets {
        seen[t] = true;
    }
    while let Some(u) = q.pop_front() {
        for &e in &v.g.inc[u] {
            if !v.active[e] {
                continue;
            }
            let w = v.g.edges[e].from;
            if !seen[w] {
                seen[w] = true;
                q.push_back(w);
            }
        }
    }
    seen
}

/// Immediate dominators from `root` (Cooper–Harvey–Kennedy). `None` for
/// unreachable nodes and the root.
pub fn dominators(v: &View, root: NodeIx) -> Vec<Option<NodeIx>> {
    let n = v.g.nodes.len();
    // Reverse postorder.
    let mut order = vec![];
    let mut seen = vec![false; n];
    let mut stack: Vec<(NodeIx, usize)> = vec![(root, 0)];
    seen[root] = true;
    while let Some(&mut (u, ref mut cur)) = stack.last_mut() {
        let outs = &v.g.out[u];
        if *cur < outs.len() {
            let e = outs[*cur];
            *cur += 1;
            if !v.active[e] {
                continue;
            }
            let w = v.g.edges[e].to;
            if !seen[w] {
                seen[w] = true;
                stack.push((w, 0));
            }
        } else {
            order.push(u);
            stack.pop();
        }
    }
    order.reverse();
    let mut rpo = vec![usize::MAX; n];
    for (i, &u) in order.iter().enumerate() {
        rpo[u] = i;
    }
    let mut idom: Vec<Option<NodeIx>> = vec![None; n];
    idom[root] = Some(root);
    let intersect = |idom: &Vec<Option<NodeIx>>, mut a: NodeIx, mut b: NodeIx| {
        while a != b {
            while rpo[a] > rpo[b] {
                a = idom[a].unwrap();
            }
            while rpo[b] > rpo[a] {
                b = idom[b].unwrap();
            }
        }
        a
    };
    let mut changed = true;
    while changed {
        changed = false;
        for &u in order.iter().skip(1) {
            let mut new: Option<NodeIx> = None;
            for &e in &v.g.inc[u] {
                if !v.active[e] {
                    continue;
                }
                let p = v.g.edges[e].from;
                if rpo[p] == usize::MAX || idom[p].is_none() {
                    continue;
                }
                new = Some(match new {
                    None => p,
                    Some(x) => intersect(&idom, p, x),
                });
            }
            if new.is_some() && idom[u] != new {
                idom[u] = new;
                changed = true;
            }
        }
    }
    idom[root] = None;
    idom
}

pub struct Cut {
    pub value: f64,
    pub edges: Vec<usize>,
}

/// Min s–t cut with per-edge capacities (Edmonds–Karp). Capacity 1 on every
/// edge = fewest edges whose loss disconnects t (v1 `minCut`). Capacities
/// from a metric (e.g. `p`, `dollars`) give weighted chokepoints. Self-loops
/// ignored; non-finite or negative capacities are clamped to 0.
pub fn min_cut(v: &View, s: NodeIx, t: NodeIx, cap: &[f64]) -> Cut {
    let n = v.g.nodes.len();
    let mut to = vec![];
    let mut c = vec![];
    let mut orig: Vec<Option<usize>> = vec![];
    let mut adj: Vec<Vec<usize>> = vec![vec![]; n];
    for (e, edge) in v.g.edges.iter().enumerate() {
        if !v.active[e] || edge.from == edge.to {
            continue;
        }
        let w = if cap[e].is_finite() && cap[e] > 0.0 {
            cap[e]
        } else {
            0.0
        };
        adj[edge.from].push(to.len());
        to.push(edge.to);
        c.push(w);
        orig.push(Some(e));
        adj[edge.to].push(to.len());
        to.push(edge.from);
        c.push(0.0);
        orig.push(None);
    }
    let mut flow = 0.0;
    loop {
        let mut parent: Vec<Option<usize>> = vec![None; n];
        let mut seen = vec![false; n];
        seen[s] = true;
        let mut q = VecDeque::from([s]);
        while let Some(u) = q.pop_front() {
            if u == t {
                break;
            }
            for &a in &adj[u] {
                if c[a] > 1e-12 && !seen[to[a]] {
                    seen[to[a]] = true;
                    parent[to[a]] = Some(a);
                    q.push_back(to[a]);
                }
            }
        }
        if !seen[t] || s == t {
            break;
        }
        let mut bottleneck = f64::INFINITY;
        let mut x = t;
        while x != s {
            let a = parent[x].unwrap();
            bottleneck = bottleneck.min(c[a]);
            x = to[a ^ 1];
        }
        if !bottleneck.is_finite() {
            break;
        }
        let mut x = t;
        while x != s {
            let a = parent[x].unwrap();
            c[a] -= bottleneck;
            c[a ^ 1] += bottleneck;
            x = to[a ^ 1];
        }
        flow += bottleneck;
    }
    let mut in_s = vec![false; n];
    in_s[s] = true;
    let mut q = VecDeque::from([s]);
    while let Some(u) = q.pop_front() {
        for &a in &adj[u] {
            if c[a] > 1e-12 && !in_s[to[a]] {
                in_s[to[a]] = true;
                q.push_back(to[a]);
            }
        }
    }
    let edges = (0..v.g.edges.len())
        .filter(|&e| {
            v.active[e]
                && v.g.edges[e].from != v.g.edges[e].to
                && in_s[v.g.edges[e].from]
                && !in_s[v.g.edges[e].to]
        })
        .collect();
    Cut { value: flow, edges }
}

/// Brandes betweenness centrality (unweighted, directed) over active edges.
/// High-betweenness nodes are the procedural bottlenecks most lines pass through.
pub fn betweenness(v: &View) -> Vec<f64> {
    let n = v.g.nodes.len();
    let mut cb = vec![0.0; n];
    for s in 0..n {
        let mut stack = vec![];
        let mut pred: Vec<Vec<NodeIx>> = vec![vec![]; n];
        let mut sigma = vec![0.0; n];
        let mut dist = vec![-1i64; n];
        sigma[s] = 1.0;
        dist[s] = 0;
        let mut q = VecDeque::from([s]);
        while let Some(u) = q.pop_front() {
            stack.push(u);
            for e in v.outs(u) {
                let w = v.g.edges[e].to;
                if dist[w] < 0 {
                    dist[w] = dist[u] + 1;
                    q.push_back(w);
                }
                if dist[w] == dist[u] + 1 {
                    sigma[w] += sigma[u];
                    if !pred[w].contains(&u) {
                        pred[w].push(u);
                    }
                }
            }
        }
        let mut delta = vec![0.0; n];
        while let Some(w) = stack.pop() {
            for &u in &pred[w] {
                delta[u] += sigma[u] / sigma[w] * (1.0 + delta[w]);
            }
            if w != s {
                cb[w] += delta[w];
            }
        }
    }
    cb
}
