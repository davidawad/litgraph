// SPDX-License-Identifier: GPL-3.0-or-later
//! Algorithms over a resolved [`crate::scenario::View`].

/// Absorbing Markov chain: exact expectations under a fixed policy.
pub mod chain;
/// `CVaR`-optimal policies (Rockafellar–Uryasev augmented-state solve).
pub mod cvar;
/// General-sum opponents: subgame-perfect equilibrium by backward induction.
pub mod equilibrium;
/// Stochastic-game solver (value iteration over SCCs).
pub mod mdp;
/// Shortest/k-shortest paths and Pareto frontiers.
pub mod paths;
/// Monte Carlo simulation: outcome distributions, `CVaR`, tails.
pub mod sim;
/// Graph structure: SCCs, dominators, min-cut, betweenness, reachability.
pub mod structure;
/// Parameter sweeps and tornado (one-at-a-time sensitivity) analyses.
pub mod sweep;
