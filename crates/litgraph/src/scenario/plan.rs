// SPDX-License-Identifier: GPL-3.0-or-later
//! Node plans: who acts at each node, what can interrupt them, and with what
//! probabilities.

use super::{Control, MixedMode, NodePlan, Objective, OpponentMode, ProbFill, Scenario, Warning};
use crate::model::{Graph, NodeIx, Role};

/// Inputs shared by every node's plan.
pub(super) struct PlanInputs<'a> {
    pub g: &'a Graph,
    pub sc: &'a Scenario,
    pub active: &'a [bool],
    pub role: &'a [Role],
    pub authored: &'a [Option<f64>],
}

/// Plans for every node, the effective per-edge draw probabilities, and warnings.
pub(super) struct Plans {
    pub plan: Vec<NodePlan>,
    pub prob: Vec<Option<f64>>,
    pub warnings: Vec<Warning>,
}

/// Probability fill for a draw over `edges`, per [`ProbFill`]. Complete
/// authored distributions are renormalized; an all-zero one becomes uniform.
#[must_use]
pub fn fill(edges: &[usize], authored: &[Option<f64>], mode: ProbFill) -> Vec<(usize, f64)> {
    let n = edges.len() as f64;
    let missing = edges.iter().filter(|&&e| authored[e].is_none()).count();
    let sum: f64 = edges.iter().filter_map(|&e| authored[e]).sum();
    let uniform = || edges.iter().map(|&e| (e, 1.0 / n)).collect();
    if missing == 0 {
        if sum <= 0.0 {
            return uniform();
        }
        return edges.iter().map(|&e| (e, authored[e].unwrap_or(0.0) / sum)).collect();
    }
    match mode {
        ProbFill::Uniform => uniform(),
        ProbFill::Residual => {
            let residual = 1.0 - sum;
            if residual <= 1e-9 {
                // Authored mass already ≥ 1: unauthored get 0, authored renormalized.
                return edges.iter().map(|&e| (e, authored[e].map_or(0.0, |p| p / sum))).collect();
            }
            let each = residual / missing as f64;
            edges.iter().map(|&e| (e, authored[e].unwrap_or(each))).collect()
        }
    }
}

impl PlanInputs<'_> {
    pub(super) fn build(&self) -> Plans {
        let mut out = Plans { plan: Vec::with_capacity(self.g.nodes.len()), prob: vec![None; self.g.edges.len()], warnings: vec![] };
        for n in 0..self.g.nodes.len() {
            let p = self.node(n, &mut out);
            out.plan.push(p);
        }
        out
    }

    fn warn(&self, out: &mut Plans, n: NodeIx, code: &'static str, message: String) {
        out.warnings.push(Warning { code, at: Some(self.g.nodes[n].id.clone()), message });
    }

    fn node(&self, n: NodeIx, out: &mut Plans) -> NodePlan {
        if self.g.nodes[n].is_terminal() {
            return NodePlan::of(Control::Terminal);
        }
        let outs: Vec<usize> = self.g.out[n].iter().copied().filter(|&e| self.active[e]).collect();
        if outs.is_empty() {
            self.warn(out, n, "sink", "non-terminal with no active out-edges; valued at 0".into());
            return NodePlan::of(Control::Sink);
        }
        let of = |r: Role| outs.iter().copied().filter(|&e| self.role[e] == r).collect::<Vec<_>>();
        let (mine, opp, nat) = (of(Role::Me), of(Role::Opponent), of(Role::Nature));
        // Who chooses, and what interrupts them. When we have a move, the
        // opponent's and the world's edges are interrupts at this node;
        // `opponent` mode governs nodes where only the opponent acts.
        if !mine.is_empty() {
            return self.chooser(n, Control::Me, mine, &[opp, nat].concat(), out);
        }
        if !opp.is_empty() {
            let as_chance = match self.sc.opponent {
                OpponentMode::Chance => true,
                OpponentMode::Adversarial => false,
                OpponentMode::Auto => opp.iter().all(|&e| self.authored[e].is_some()),
            };
            if !as_chance {
                return self.chooser(n, Control::Opponent, opp, &nat, out);
            }
            return self.chance(n, &outs, out);
        }
        self.chance(n, &nat, out)
    }

    fn chance(&self, n: NodeIx, edges: &[usize], out: &mut Plans) -> NodePlan {
        let missing = edges.iter().filter(|&&e| self.authored[e].is_none()).count();
        if missing > 0 {
            let msg = format!("{missing} of {} out-edges lack probabilities; filled ({:?})", edges.len(), self.sc.prob_fill);
            self.warn(out, n, "probability-fill", msg);
        } else {
            let s: f64 = edges.iter().filter_map(|&e| self.authored[e]).sum();
            if (s - 1.0).abs() > 1e-3 {
                self.warn(out, n, "probability-renormalized", format!("authored probabilities sum to {s:.3} after masking; renormalized"));
            }
        }
        let dist = fill(edges, self.authored, self.sc.prob_fill);
        for &(e, p) in &dist {
            out.prob[e] = Some(p);
        }
        NodePlan { draws: dist, ..NodePlan::of(Control::Chance) }
    }

    fn chooser(&self, n: NodeIx, control: Control, choices: Vec<usize>, interrupts: &[usize], out: &mut Plans) -> NodePlan {
        let base = NodePlan { choices, choice_mass: 1.0, minimize: control == Control::Opponent, ..NodePlan::of(control) };
        if interrupts.is_empty() {
            return base;
        }
        match self.sc.mixed {
            MixedMode::Optimistic => NodePlan { choices: [base.choices, interrupts.to_vec()].concat(), ..base },
            MixedMode::SelfOnly => base,
            MixedMode::ActOrWait => self.act_or_wait(base, interrupts, out),
            MixedMode::NatureFirst => {
                let complete = interrupts.iter().all(|&e| self.authored[e].is_some());
                let mass: f64 = interrupts.iter().filter_map(|&e| self.authored[e]).sum();
                if complete && mass < 1.0 - 1e-9 {
                    let draws: Vec<(usize, f64)> = interrupts.iter().map(|&e| (e, self.authored[e].unwrap_or(0.0))).collect();
                    for &(e, p) in &draws {
                        out.prob[e] = Some(p);
                    }
                    return NodePlan { draws, choice_mass: 1.0 - mass, ..base };
                }
                let who = if control == Control::Me { "self" } else { "opponent" };
                let why = if complete { "take all the probability mass" } else { "have no probabilities" };
                let msg = format!(
                    "{who} may act here, and {} world edge(s) {why}; modeled as act-or-wait (act, or let the world edges fire). Author probabilities summing < 1 on them to model a true interrupt",
                    interrupts.len()
                );
                self.warn(out, n, "mixed-node", msg);
                self.act_or_wait(base, interrupts, out)
            }
        }
    }

    fn act_or_wait(&self, base: NodePlan, interrupts: &[usize], out: &mut Plans) -> NodePlan {
        let wait = fill(interrupts, self.authored, self.sc.prob_fill);
        for &(e, p) in &wait {
            out.prob[e] = Some(p);
        }
        NodePlan { wait, ..base }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_modes() {
        let a = [Some(0.9), None, Some(0.0)];
        assert_eq!(fill(&[0, 1], &a, ProbFill::Uniform), vec![(0, 0.5), (1, 0.5)]);
        let r = fill(&[0, 1], &a, ProbFill::Residual);
        assert!((r[1].1 - 0.1).abs() < 1e-12);
        assert_eq!(fill(&[0, 2], &a, ProbFill::Residual), vec![(0, 1.0), (2, 0.0)]);
        assert_eq!(fill(&[2], &a, ProbFill::Residual), vec![(2, 1.0)]);
        let over = [Some(0.8), Some(0.7), None];
        let r = fill(&[0, 1, 2], &over, ProbFill::Residual);
        assert!((r.iter().map(|x| x.1).sum::<f64>() - 1.0).abs() < 1e-12 && r[2].1 == 0.0);
    }
}
