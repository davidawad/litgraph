// SPDX-License-Identifier: GPL-3.0-or-later
//! Probability uncertainty: a Dirichlet belief over every chance draw.
//!
//! Every node whose plan draws from a distribution — a chance node, a
//! nature-first interrupt (the interrupt edges plus the residual mass left to
//! the chooser), or an act-or-wait node's `WAIT` — gets a Dirichlet whose
//! mean is the view's resolved probability vector `p` and whose strength is a
//! pseudo-count total `c` (the *concentration*): prior `α = c·p`. `c` comes
//! from, in order,
//!
//! 1. `scenario.uncertainty.concentration[node]` (the matter's own view);
//! 2. the sample size `n` of a calibration entry (`calibration/*.json`) that
//!    set one of the node's edge probabilities (the smallest, if several);
//! 3. `scenario.uncertainty.default_concentration`, else
//!    [`DEFAULT_CONCENTRATION`] — an **estimate**, reported with a
//!    `prior-concentration-estimated` warning wherever it matters.
//!
//! `scenario.observe` (`{node ref: {edge ref: count}}`) is the conjugate
//! update: posterior `α' = α + counts`, and every op then runs on the
//! posterior mean `α' / Σα'` — the closed form a Dirichlet–multinomial
//! model gives (e.g. Gelman et al., *Bayesian Data Analysis*, 3rd ed.,
//! §3.4). Beta is the two-outcome case.
//!
//! A component with `α = 0` (a probability the scenario resolved to 0, e.g.
//! the sibling of a `scenario.facts` branch) is structurally impossible and
//! stays 0 in every draw; a node with fewer than two possible outcomes is
//! known, not uncertain. See `docs/UNCERTAINTY.md`.

use std::collections::{BTreeMap, HashMap};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Control, NodePlan, Scenario, View, Warning};
use crate::error::{Error, Result};
use crate::model::{EdgeIx, Graph, NodeIx};

/// Pseudo-count total used where neither the scenario nor a calibration
/// entry says how strong the prior is. An **estimate**: "as if the authored
/// probability had been seen in ten comparable matters". Always warned.
pub const DEFAULT_CONCENTRATION: f64 = 10.0;

/// `scenario.uncertainty`: how sure we are of each chance node's probabilities.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Uncertainty {
    /// Pseudo-count total at a chance node with no calibrated sample size
    /// and no `concentration` entry (default 10; an estimate, warned as
    /// `prior-concentration-estimated`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_concentration: Option<f64>,
    /// Node ref → pseudo-count total, overriding calibration `n` and the
    /// default (e.g. a population rate that fits this judge less well
    /// deserves fewer pseudo-observations than the population's `n`).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub concentration: BTreeMap<String, f64>,
}

/// Which of a node's plan distributions a [`Group`] models.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum GroupKind {
    /// `NodePlan::draws` (plus the residual choice mass at an interrupt).
    Draws,
    /// `NodePlan::wait` (an act-or-wait node's world edges).
    Wait,
}

/// Where a group's concentration came from.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "from", rename_all = "kebab-case")]
pub enum PriorSource {
    /// A calibration entry's sample size.
    Calibration {
        /// Calibration set id.
        set: String,
        /// The entry's `n`.
        n: u64,
    },
    /// `scenario.uncertainty.concentration`.
    Scenario,
    /// The default: an estimate.
    Estimate,
}

/// One Dirichlet: a node's outcome distribution.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// The node.
    pub node: NodeIx,
    /// Which plan distribution.
    pub kind: GroupKind,
    /// Outcome edges, in plan order.
    pub edges: Vec<EdgeIx>,
    /// A last slot for "no interrupt: the chooser acts" (nature-first).
    pub residual: bool,
    /// Prior mean per slot (the view's resolved probabilities).
    pub prior_mean: Vec<f64>,
    /// Prior pseudo-count total.
    pub concentration: f64,
    /// Where `concentration` came from.
    pub source: PriorSource,
    /// Observed counts per slot (`scenario.observe`; the residual is never observed).
    pub counts: Vec<f64>,
}

impl Group {
    /// Posterior Dirichlet parameters `c·p + counts`.
    #[must_use]
    pub fn alpha(&self) -> Vec<f64> {
        self.prior_mean
            .iter()
            .zip(&self.counts)
            .map(|(p, n)| self.concentration * p + n)
            .collect()
    }

    /// Posterior mean after `extra` further counts per slot (the empty slice
    /// for none).
    #[must_use]
    pub fn mean_with(&self, extra: &[f64]) -> Vec<f64> {
        let a: Vec<f64> = self
            .alpha()
            .iter()
            .enumerate()
            .map(|(i, a)| a + extra.get(i).copied().unwrap_or(0.0))
            .collect();
        let s: f64 = a.iter().sum();
        a.iter().map(|x| x / s).collect()
    }

    /// Posterior mean.
    #[must_use]
    pub fn mean(&self) -> Vec<f64> {
        self.mean_with(&[])
    }

    /// At least two outcomes remain possible.
    #[must_use]
    pub fn uncertain(&self) -> bool {
        self.alpha().iter().filter(|&&a| a > 0.0).count() >= 2
    }

    /// Any observation landed here.
    #[must_use]
    pub fn observed(&self) -> bool {
        self.counts.iter().any(|&c| c > 0.0)
    }
}

/// Every chance draw's Dirichlet, resolved against a view's plans.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Belief {
    /// One group per node that draws over two or more outcomes.
    pub groups: Vec<Group>,
    by_node: HashMap<NodeIx, usize>,
}

fn positive(what: &str, x: f64) -> Result<f64> {
    if x.is_finite() && x > 0.0 {
        Ok(x)
    } else {
        Err(Error::Invalid(format!(
            "{what} must be a positive number, got {x}"
        )))
    }
}

/// `scenario.observe` resolved to per-edge counts over every state-flag copy
/// of each observed edge (an observation is about the pack's edge, not one
/// flag history — same reasoning as `scenario.facts`).
fn observations(g: &Graph, sc: &Scenario) -> Result<Vec<(String, Vec<EdgeIx>, f64)>> {
    let mut out = vec![];
    for (node_ref, counts) in &sc.observe {
        let ni = g.node(node_ref)?;
        for (edge_ref, &k) in counts {
            if !k.is_finite() || k < 0.0 {
                return Err(Error::Invalid(format!(
                    "observe[{node_ref}][{edge_ref}] must be a non-negative count, got {k}"
                )));
            }
            let ei = g.edge_at(ni, edge_ref)?;
            out.push((g.edges[ei].id.clone(), g.edge_family(ei), k));
        }
    }
    Ok(out)
}

impl Belief {
    /// Build every group from the resolved plans, then fold in `scenario.observe`.
    ///
    /// # Errors
    /// Unknown refs, a non-positive concentration, a negative count, or an
    /// observation on an edge that is not a chance outcome under this scenario.
    pub(super) fn build(g: &Graph, sc: &Scenario, plan: &[NodePlan]) -> Result<Belief> {
        let default_c = positive(
            "uncertainty.default_concentration",
            sc.uncertainty
                .default_concentration
                .unwrap_or(DEFAULT_CONCENTRATION),
        )?;
        let mut set_c: HashMap<NodeIx, f64> = HashMap::new();
        for (r, &c) in &sc.uncertainty.concentration {
            let c = positive(&format!("uncertainty.concentration[{r}]"), c)?;
            for n in g.node_family(g.node(r)?) {
                set_c.insert(n, c);
            }
        }
        let mut belief = Belief::default();
        for (n, p) in plan.iter().enumerate() {
            let (kind, dist, residual) = if p.draws.is_empty() {
                (GroupKind::Wait, &p.wait, false)
            } else {
                let residual = p.control != Control::Chance && p.choice_mass > 0.0;
                (GroupKind::Draws, &p.draws, residual)
            };
            if dist.len() + usize::from(residual) < 2 {
                continue;
            }
            let edges: Vec<EdgeIx> = dist.iter().map(|d| d.0).collect();
            let mut prior_mean: Vec<f64> = dist.iter().map(|d| d.1).collect();
            if residual {
                prior_mean.push(p.choice_mass);
            }
            let calibrated = edges
                .iter()
                .filter_map(|e| g.sample_size.get(e))
                .filter(|(_, n)| *n > 0)
                .min_by_key(|(_, n)| *n);
            let (concentration, source) = match (set_c.get(&n), calibrated) {
                (Some(&c), _) => (c, PriorSource::Scenario),
                (None, Some((set, k))) => (
                    *k as f64,
                    PriorSource::Calibration {
                        set: set.clone(),
                        n: *k,
                    },
                ),
                (None, None) => (default_c, PriorSource::Estimate),
            };
            belief.by_node.insert(n, belief.groups.len());
            belief.groups.push(Group {
                node: n,
                kind,
                counts: vec![0.0; prior_mean.len()],
                edges,
                residual,
                prior_mean,
                concentration,
                source,
            });
        }
        for (id, family, k) in observations(g, sc)? {
            let mut hit = false;
            for e in family {
                let Some(&gi) = belief.by_node.get(&g.edges[e].from) else {
                    continue;
                };
                let grp = &mut belief.groups[gi];
                if let Some(slot) = grp.edges.iter().position(|&x| x == e) {
                    grp.counts[slot] += k;
                    hit = true;
                }
            }
            if !hit {
                return Err(Error::Invalid(format!(
                    "observe: {id} is not a chance outcome under this scenario (only chance-node draws, nature-first interrupts and act-or-wait world edges with at least two possible outcomes can be observed)"
                )));
            }
        }
        Ok(belief)
    }

    /// The group at node `n`, if it draws over two or more outcomes.
    #[must_use]
    pub fn group_at(&self, n: NodeIx) -> Option<usize> {
        self.by_node.get(&n).copied()
    }

    /// Indices of the groups with at least two possible outcomes.
    #[must_use]
    pub fn uncertain(&self) -> Vec<usize> {
        (0..self.groups.len())
            .filter(|&i| self.groups[i].uncertain())
            .collect()
    }

    /// One `prior-concentration-estimated` warning per selected group whose
    /// prior strength is the default estimate.
    #[must_use]
    pub fn estimated_warnings(&self, g: &Graph, select: &[usize], why: &str) -> Vec<Warning> {
        select
            .iter()
            .map(|&i| &self.groups[i])
            .filter(|grp| grp.source == PriorSource::Estimate)
            .map(|grp| Warning {
                code: "prior-concentration-estimated",
                at: Some(g.nodes[grp.node].id.clone()),
                message: format!(
                    "{why} rests on a default prior strength of {} pseudo-observations here (an estimate, not a sample size); set scenario.uncertainty.concentration or apply a calibration set with `n`",
                    grp.concentration
                ),
            })
            .collect()
    }
}

/// Overwrite plan `p`'s distribution for `grp` with slot probabilities `x`.
pub(super) fn write(p: &mut NodePlan, prob: &mut [Option<f64>], grp: &Group, x: &[f64]) {
    let dist = match grp.kind {
        GroupKind::Draws => &mut p.draws,
        GroupKind::Wait => &mut p.wait,
    };
    for (d, &xi) in dist.iter_mut().zip(x) {
        d.1 = xi;
        prob[d.0] = Some(xi);
    }
    if grp.residual {
        p.choice_mass = x[grp.edges.len()];
    }
}

/// Replace every observed group's probabilities with its posterior mean, and
/// warn where that mean leans on an estimated prior strength.
pub(super) fn apply_observed(
    g: &Graph,
    belief: &Belief,
    plan: &mut [NodePlan],
    prob: &mut [Option<f64>],
) -> Vec<Warning> {
    let observed: Vec<usize> = (0..belief.groups.len())
        .filter(|&i| belief.groups[i].observed())
        .collect();
    for &i in &observed {
        let grp = &belief.groups[i];
        write(&mut plan[grp.node], prob, grp, &grp.mean());
    }
    belief.estimated_warnings(g, &observed, "the observed posterior mean")
}

impl View<'_> {
    /// Set group `gi`'s slot probabilities (edges in plan order, then the
    /// residual choice mass if any) — how the uncertainty ops evaluate a
    /// sampled or hypothetical probability vector without re-resolving the
    /// scenario.
    pub fn set_group(&mut self, gi: usize, x: &[f64]) {
        let grp = &self.belief.groups[gi];
        write(&mut self.plan[grp.node], &mut self.prob, grp, x);
    }
}
