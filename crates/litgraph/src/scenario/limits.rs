// SPDX-License-Identifier: GPL-3.0-or-later
//! Warnings for scenario field combinations the engine doesn't (fully)
//! support together: an expression that depends on path-dependent variables
//! where the algorithm evaluating it is Markov, and the `cvar`/
//! `opponent_objective` combinations documented in `docs/CRITIQUE.md`.

use super::{Objective, Scenario, Warning};
use crate::error::Result;
use crate::expr::{self, Expr};
use crate::metrics;

/// The path-dependent variable names (see [`metrics::PATH_VAR_NAMES`]) an
/// expression references, if any.
pub(super) fn path_vars_used(ex: &Expr) -> Vec<String> {
    ex.vars()
        .into_iter()
        .filter(|v| metrics::PATH_VAR_NAMES.contains(&v.as_str()))
        .collect()
}

/// A `path-variable-in-markov` warning for `what` (a description of the
/// expression and its source), if it references a path-dependent variable.
pub(super) fn path_var_warning_for(what: &str, ex: &Expr) -> Option<Warning> {
    let names = path_vars_used(ex);
    if names.is_empty() {
        return None;
    }
    Some(Warning {
        code: "path-variable-in-markov",
        at: None,
        message: format!(
            "{what} references path-dependent variable(s) {}: `solve`/`chain` are Markov on the \
             node (a terminal can be reached having spent different amounts on different paths) \
             and evaluate these as 0; only `simulate` accounts for the actual \
             spend/elapsed/steps along each sampled trajectory. See docs/COST_FUNCTIONS.md \
             (\"Path-dependent terminal variables\").",
            names.join(", ")
        ),
    })
}

/// `fee_shift.eligible` is always evaluated in a Markov context (`solve`'s
/// policy-iteration and `simulate`'s precomputed eligibility both check it
/// once per terminal, never per trajectory), unlike the utility expression
/// which `simulate` re-evaluates per run — warn if it references a
/// path-dependent variable.
///
/// # Errors
/// Propagates a parse error in `fee_shift.eligible`.
pub(super) fn fee_shift_path_vars(sc: &Scenario) -> Result<Option<Warning>> {
    let Some(fs) = sc.fee_shift.clone() else {
        return Ok(None);
    };
    let src = fs.eligible.unwrap_or_else(|| "tag('fee-eligible')".into());
    let ex = expr::parse(&src)?;
    Ok(path_var_warning_for(
        &format!("fee_shift.eligible `{src}`"),
        &ex,
    ))
}

/// `Objective::Cvar`'s Rockafellar–Uryasev augmentation assumes a plain
/// additive total (no discounting) and doesn't combine with fee-shift's
/// policy-dependent cost adjustment or with a general-sum opponent; warn
/// rather than silently ignoring any of the three. See `docs/CRITIQUE.md`
/// ("CVaR-optimal policies").
pub(super) fn cvar_limits(sc: &Scenario) -> Vec<Warning> {
    if !matches!(sc.objective, Objective::Cvar { .. }) {
        return vec![];
    }
    let mut out = vec![];
    let mut warn = |code, message: String| {
        out.push(Warning {
            code,
            at: None,
            message,
        });
    };
    if sc.discount_annual.is_some() {
        warn(
            "cvar-ignores-discount",
            "objective cvar ignores `discount_annual`: the augmented-state solve assumes an undiscounted additive total".into(),
        );
    }
    if sc.fee_shift.is_some() {
        warn(
            "cvar-ignores-fee-shift",
            "objective cvar ignores `fee_shift`: it uses unadjusted cost".into(),
        );
    }
    if sc.opponent_objective.is_some() {
        warn(
            "cvar-ignores-opponent-objective",
            "objective cvar does not compose with a general-sum `opponent_objective`; the opponent is modeled adversarially".into(),
        );
    }
    out
}

/// The general-sum equilibrium solver (`algo::equilibrium`) uses unadjusted
/// cost; it doesn't run `fee_shift`'s policy-iteration cost adjustment
/// (defined in terms of `self`'s policy only). See `docs/CRITIQUE.md`
/// ("General-sum opponents").
pub(super) fn opponent_objective_limits(sc: &Scenario) -> Vec<Warning> {
    if sc.opponent_objective.is_some() && sc.fee_shift.is_some() {
        return vec![Warning {
            code: "opponent-objective-ignores-fee-shift",
            at: None,
            message: "a general-sum `opponent_objective` ignores `fee_shift`: the equilibrium solve uses unadjusted cost".into(),
        }];
    }
    vec![]
}
