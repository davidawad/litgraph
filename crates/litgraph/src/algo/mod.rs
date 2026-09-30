// SPDX-License-Identifier: GPL-3.0-or-later
//! Algorithms over a resolved [`crate::scenario::View`].

/// Absorbing Markov chain: exact expectations under a fixed policy.
pub mod chain;
/// `CVaR`-optimal policies (Rockafellar–Uryasev augmented-state solve).
pub mod cvar;
/// Seeded Gamma / Dirichlet / multinomial sampling for the uncertainty ops.
pub mod dirichlet;
/// General-sum opponents: subgame-perfect equilibrium by backward induction.
pub mod equilibrium;
/// Stochastic-game solver (value iteration over SCCs).
pub mod mdp;
/// Shortest/k-shortest paths and Pareto frontiers.
pub mod paths;
/// Posterior propagation: credible intervals on values and P(option optimal).
pub mod posterior;
/// Robust solve over Dirichlet credible (L1) ambiguity sets.
pub mod robust;
/// Monte Carlo simulation: outcome distributions, `CVaR`, tails.
pub mod sim;
/// Graph structure: SCCs, dominators, min-cut, betweenness, reachability.
pub mod structure;
/// Parameter sweeps and tornado (one-at-a-time sensitivity) analyses.
pub mod sweep;
/// Value of information: EVPI (outcome and parameter) and EVSI.
pub mod voi;
