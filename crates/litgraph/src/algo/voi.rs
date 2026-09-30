// SPDX-License-Identifier: GPL-3.0-or-later
//! Value of information: what is it worth to learn a chance node's outcome,
//! its probability, or a sample of `k` observations of it, before committing
//! to a policy?
//!
//! Classical preposterior analysis (Raiffa & Schlaifer 1961, *Applied
//! Statistical Decision Theory*; Howard 1966, "Information value theory",
//! *IEEE Trans. Syst. Sci. Cybern.* 2(1):22–26). With `π̄` the policy that is
//! optimal at the posterior mean and `V*(θ)` / `V^π̄(θ)` the start value with
//! the policy re-optimized / held at `π̄` under probabilities `θ`:
//!
//! ```text
//! EVPI(outcome of n) = Σ_i θ̄_i · [V*(n → i) − V^π̄(n → i)]          exact (one solve pair per outcome; acyclic n only)
//! EVPPI(θ_n)         = E_{θ_n ~ post}[V*(θ_n) − V^π̄(θ_n)]            Monte Carlo
//! EVPI(all θ)        = E_{θ ~ post}[V*(θ) − V^π̄(θ)]                  Monte Carlo
//! EVSI(k at n)       = E_{D ~ DirMult(k, α_n)}[V*(θ̄'_D) − V^π̄(θ̄'_D)]  Monte Carlo, θ̄'_D = (α_n + D)/(Σα_n + k)
//! ```
//!
//! Each is written as the expected *regret* of committing to `π̄`, so every
//! term is `≥ 0` and every estimate is exactly `0` when no possible answer
//! changes the decision. The regret form equals the textbook
//! `E[max] − max E` because the start value under a fixed policy is linear
//! in the probabilities of a node visited at most once per trajectory, and
//! the posterior mean is a martingale (`E_D[θ̄'_D] = θ̄`). Where a node
//! sits on a cycle, or an adversarial opponent re-optimizes, that linearity
//! fails and the numbers are the regret definition, not an exact
//! `E[max] − max E` (see `docs/UNCERTAINTY.md`).
//!
//! The outcome EVPI forces `n`'s draw to one outcome; at a node on a cycle
//! (revisited, e.g. a motion ruling that can grant leave to amend and come
//! back) that would force the *same* outcome on every visit — clairvoyance
//! about a single draw is not expressible node-by-node — so it is not
//! reported there, and such nodes always get the Monte Carlo EVPPI.
//!
//! Information is modeled as public: the opponent's modeled response is
//! re-solved along with ours, so the numbers isolate what changes in *our*
//! policy. Ordering `EVPI(outcome) ≥ EVPPI ≥ EVSI(k)` holds in expectation;
//! the Monte Carlo estimates carry standard errors.

use std::collections::BTreeMap;

use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::Serialize;

use super::dirichlet;
use super::mdp::{self, SolveOptions};
use super::posterior::risk_neutral;
use super::structure::{is_cyclic, scc};
use crate::error::{Error, Result};
use crate::model::NodeIx;
use crate::scenario::{Control, View};

/// Monte Carlo and screening settings.
#[derive(Debug, Clone)]
pub struct VoiOptions {
    /// Draws per Monte Carlo estimate.
    pub samples: usize,
    /// RNG seed.
    pub seed: u64,
    /// Rows (ranked by outcome EVPI) that also get the parameter EVPPI.
    pub top: usize,
    /// Cap on uncertain nodes screened (in graph order, reachable from start).
    pub max_nodes: usize,
}

/// A Monte Carlo estimate.
#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
pub struct Estimate {
    /// Mean.
    pub value: f64,
    /// Standard error of the mean.
    pub std_error: f64,
}

/// One uncertain node's information value.
#[derive(Debug, Clone)]
pub struct NodeVoi {
    /// Belief group index.
    pub group: usize,
    /// Perfect information about which outcome occurs here (`None` on a cycle).
    pub evpi_outcome: Option<f64>,
    /// Perfect information about this node's probabilities (top rows only).
    pub evppi: Option<Estimate>,
}

/// A sample to price: `k` further observations at a node's group.
#[derive(Debug, Clone, Copy)]
pub struct Study {
    /// Belief group index.
    pub group: usize,
    /// Observations the study is worth.
    pub k: u64,
}

/// Every information value, from `start`.
#[derive(Debug, Clone)]
pub struct Voi {
    /// Start value at the posterior mean.
    pub value: f64,
    /// Perfect information about every probability at once.
    pub evpi_total: Estimate,
    /// Screened nodes, ranked by outcome EVPI.
    pub nodes: Vec<NodeVoi>,
    /// More uncertain nodes were reachable than `max_nodes`.
    pub truncated: bool,
    /// EVSI per requested study, in order.
    pub studies: Vec<Estimate>,
    /// Solves (of all those run) that hit an iteration cap.
    pub unconverged: usize,
}

struct Eval<'g> {
    s: View<'g>,
    start: NodeIx,
    /// `π̄` as a forced-choice map (our nodes only).
    pi_bar: BTreeMap<NodeIx, usize>,
    opts: SolveOptions,
    unconverged: usize,
}

impl Eval<'_> {
    /// `V*(θ) − V^π̄(θ)` at the view's current probabilities.
    fn regret(&mut self) -> Result<f64> {
        let best = mdp::solve(&self.s, &self.opts)?;
        std::mem::swap(&mut self.s.forced, &mut self.pi_bar);
        let held = mdp::solve(&self.s, &self.opts);
        std::mem::swap(&mut self.s.forced, &mut self.pi_bar);
        let held = held?;
        self.unconverged += usize::from(!best.converged) + usize::from(!held.converged);
        Ok((best.value[self.start] - held.value[self.start]).max(0.0))
    }

    /// Regret with group `gi` set to `x`, then restored to its mean.
    fn regret_at(&mut self, gi: usize, x: &[f64]) -> Result<f64> {
        self.s.set_group(gi, x);
        let r = self.regret();
        let mean = self.s.belief.groups[gi].mean();
        self.s.set_group(gi, &mean);
        r
    }

    fn outcome_evpi(&mut self, gi: usize) -> Result<f64> {
        let mean = self.s.belief.groups[gi].mean();
        let mut total = 0.0;
        for (i, &m) in mean.iter().enumerate() {
            if m > 0.0 {
                let mut x = vec![0.0; mean.len()];
                x[i] = 1.0;
                total += m * self.regret_at(gi, &x)?;
            }
        }
        Ok(total)
    }

    fn mc(
        &mut self,
        samples: usize,
        mut draw: impl FnMut(&mut Self) -> Result<f64>,
    ) -> Result<Estimate> {
        let xs: Vec<f64> = (0..samples).map(|_| draw(self)).collect::<Result<_>>()?;
        let (value, std_error) = dirichlet::mean_se(&xs);
        Ok(Estimate { value, std_error })
    }

    fn evppi(&mut self, gi: usize, samples: usize, seed: u64) -> Result<Estimate> {
        let alpha = self.s.belief.groups[gi].alpha();
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        self.mc(samples, |ev| {
            let x = dirichlet::dirichlet(&mut rng, &alpha);
            ev.regret_at(gi, &x)
        })
    }

    fn evsi(&mut self, st: Study, samples: usize, seed: u64) -> Result<Estimate> {
        let grp = self.s.belief.groups[st.group].clone();
        let alpha = grp.alpha();
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        self.mc(samples, |ev| {
            let theta = dirichlet::dirichlet(&mut rng, &alpha);
            let data = dirichlet::multinomial(&mut rng, st.k, &theta);
            ev.regret_at(st.group, &grp.mean_with(&data))
        })
    }

    fn evpi_total(&mut self, groups: &[usize], samples: usize, seed: u64) -> Result<Estimate> {
        let alphas: Vec<Vec<f64>> = groups
            .iter()
            .map(|&g| self.s.belief.groups[g].alpha())
            .collect();
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let est = self.mc(samples, |ev| {
            for (k, &gi) in groups.iter().enumerate() {
                let x = dirichlet::dirichlet(&mut rng, &alphas[k]);
                ev.s.set_group(gi, &x);
            }
            ev.regret()
        });
        for &gi in groups {
            let mean = self.s.belief.groups[gi].mean();
            self.s.set_group(gi, &mean);
        }
        est
    }
}

/// Nodes reachable from `start` over active edges.
#[must_use]
pub fn reachable(v: &View, start: NodeIx) -> Vec<bool> {
    let mut seen = vec![false; v.g.nodes.len()];
    let mut stack = vec![start];
    seen[start] = true;
    while let Some(n) = stack.pop() {
        for e in v.outs(n) {
            let to = v.g.edges[e].to;
            if !seen[to] {
                seen[to] = true;
                stack.push(to);
            }
        }
    }
    seen
}

fn node_seed(seed: u64, n: NodeIx) -> u64 {
    seed ^ (n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

fn check(v: &View, uncertain: &[usize], studies: &[Study], o: &VoiOptions) -> Result<()> {
    if o.samples == 0 {
        return Err(Error::Invalid("voi: samples must be at least 1".into()));
    }
    for st in studies {
        if st.k == 0 {
            return Err(Error::Invalid("voi: a study's k must be at least 1".into()));
        }
        if !uncertain.contains(&st.group) {
            return Err(Error::Invalid(format!(
                "voi: {} has fewer than two possible outcomes; nothing to learn",
                v.g.nodes[v.belief.groups[st.group].node].id
            )));
        }
    }
    Ok(())
}

/// Nodes in a cyclic strongly connected component (revisitable).
fn on_cycle(v: &View) -> Vec<bool> {
    let mut out = vec![false; v.g.nodes.len()];
    for comp in scc(v) {
        if is_cyclic(v, &comp) {
            for n in comp {
                out[n] = true;
            }
        }
    }
    out
}

/// Outcome EVPI for every candidate (acyclic ones), EVPPI for the `top`
/// screened in, then the final ranking.
fn rank_nodes(
    ev: &mut Eval,
    v: &View,
    candidates: &[usize],
    o: &VoiOptions,
) -> Result<Vec<NodeVoi>> {
    let cyclic = on_cycle(v);
    let mut nodes = candidates
        .iter()
        .take(o.max_nodes)
        .map(|&gi| {
            let evpi_outcome = if cyclic[v.belief.groups[gi].node] {
                None
            } else {
                Some(ev.outcome_evpi(gi)?)
            };
            Ok(NodeVoi {
                group: gi,
                evpi_outcome,
                evppi: None,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    // Screen by outcome EVPI (an upper bound on EVPPI at an acyclic node);
    // nodes on a cycle can't be screened, so they go first.
    let screen = |r: &NodeVoi| r.evpi_outcome.unwrap_or(f64::INFINITY);
    nodes.sort_by(|a, b| screen(b).total_cmp(&screen(a)));
    for row in nodes.iter_mut().take(o.top) {
        let seed = node_seed(o.seed, v.belief.groups[row.group].node);
        row.evppi = Some(ev.evppi(row.group, o.samples, seed)?);
    }
    // Final rank: the parameter EVPPI where computed, then the rest by
    // outcome EVPI.
    let rank = |r: &NodeVoi| {
        (
            r.evppi.is_some(),
            r.evppi.map_or(r.evpi_outcome.unwrap_or(0.0), |e| e.value),
        )
    };
    nodes.sort_by(|a, b| {
        let (ka, kb) = (rank(a), rank(b));
        kb.0.cmp(&ka.0).then(kb.1.total_cmp(&ka.1))
    });
    Ok(nodes)
}

/// Value of information from `start` for every uncertain node (screened by
/// `max_nodes`) and every requested study.
///
/// # Errors
/// Zero `samples`, a study `k` of 0, a study group that isn't uncertain, or
/// a solve error.
pub fn voi(v: &View, start: NodeIx, studies: &[Study], o: &VoiOptions) -> Result<Voi> {
    let uncertain = v.belief.uncertain();
    check(v, &uncertain, studies, o)?;
    let s = risk_neutral(v);
    let opts = SolveOptions::default();
    let base = mdp::solve(&s, &opts)?;
    let pi_bar = base
        .choice
        .iter()
        .filter(|(n, _)| s.plan[**n].control == Control::Me)
        .map(|(n, e)| (*n, *e))
        .collect();
    let mut ev = Eval {
        s,
        start,
        pi_bar,
        opts,
        unconverged: 0,
    };
    let live = reachable(v, start);
    let candidates: Vec<usize> = uncertain
        .iter()
        .copied()
        .filter(|&gi| live[v.belief.groups[gi].node])
        .collect();
    let nodes = rank_nodes(&mut ev, v, &candidates, o)?;
    let evpi_total = ev.evpi_total(&candidates, o.samples, o.seed)?;
    let studies = studies
        .iter()
        .map(|&st| {
            let seed = node_seed(o.seed, v.belief.groups[st.group].node);
            ev.evsi(st, o.samples, seed)
        })
        .collect::<Result<_>>()?;
    Ok(Voi {
        value: base.value[start],
        evpi_total,
        nodes,
        truncated: candidates.len() > o.max_nodes,
        studies,
        unconverged: ev.unconverged,
    })
}
