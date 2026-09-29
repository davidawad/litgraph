// SPDX-License-Identifier: GPL-3.0-or-later
//! The deadline clock: court-day/calendar-day time computation under
//! `FRCP 6`, `RCFC 6`, `FRAP 26`, and `19 CFR 210.6(a)` (ITC Section 337).
//! Ported from `civ-pro-the-gathering`'s `src/engine/clock/` (TypeScript),
//! re-verified against the rule text 2026-09-28 (see each submodule's doc
//! comment for exact source URLs and, where they exist, the flagged
//! interpretive choices).
//!
//! No date-library dependency: [`date::Date`] is a small, exact,
//! allocation-free proleptic-Gregorian day-count type (see its module doc
//! for why this beats pulling in `time`/`chrono` here — the crate's own
//! `deny.toml`/dependency-minimalism convention, and the fact that every
//! operation needed is exact integer arithmetic with no timezone concept
//! at all, which is precisely the case a general-purpose date crate is
//! *not* simpler for).
//!
//! Layout:
//! - [`date`]: calendar arithmetic.
//! - [`holidays`]: federal legal holidays, including RCFC's Inauguration
//!   Day addition.
//! - [`general`]: the shared `FRCP 6`/`RCFC 6`/`FRAP 26` day- and
//!   hours-counting engine.
//! - [`itc`]: the genuinely different `19 CFR 210.6(a)` algorithm.
//!
//! [`compute_due_date`] and [`compute_hour_deadline`] are the two public
//! entry points; they dispatch on [`RuleSet`] so a caller (the `deadlines`
//! API op) never has to know which submodule implements which forum.

pub mod date;
pub mod general;
pub mod holidays;
pub mod itc;
mod types;

pub use date::Date;
pub use types::{
    ClockConfig, ComputedDeadline, ComputedHourDeadline, DayDeadline, Direction, HourDeadline,
    RuleSet, ServiceMethod, Unit,
};

use crate::error::Result;

/// Compute a day-counted due date, dispatching to [`general`] or [`itc`]
/// by [`RuleSet`].
///
/// # Errors
/// See [`general::compute_due_date`] / [`itc::compute_due_date`].
pub fn compute_due_date(
    dl: &DayDeadline,
    rule_set: RuleSet,
    cfg: &ClockConfig,
) -> Result<ComputedDeadline> {
    if rule_set == RuleSet::Itc210 {
        itc::compute_due_date(dl, cfg)
    } else {
        general::compute_due_date(dl, rule_set, cfg)
    }
}

/// Compute an hours-based due instant (`FRCP`/`RCFC 6(a)(2)` only).
///
/// # Errors
/// See [`general::compute_hour_deadline`].
pub fn compute_hour_deadline(
    dl: &HourDeadline,
    rule_set: RuleSet,
    cfg: &ClockConfig,
) -> Result<ComputedHourDeadline> {
    general::compute_hour_deadline(dl, rule_set, cfg)
}

/// Best-effort guess of which [`RuleSet`] governs a pack, from its `forum`
/// key (preferred) or, failing that, a substring of its pack id. Returns
/// `None` if nothing matches, so the caller can warn instead of guessing
/// silently.
#[must_use]
pub fn ruleset_for_forum(forum: Option<&str>, pack_id: &str) -> Option<RuleSet> {
    if let Some(f) = forum {
        match f {
            "cofc" => return Some(RuleSet::Rcfc6),
            "cafc" | "frap" => return Some(RuleSet::Frap26),
            "itc" => return Some(RuleSet::Itc210),
            "frcp" => return Some(RuleSet::Frcp6),
            _ => {}
        }
    }
    let id = pack_id.to_lowercase();
    if id.contains("cofc") {
        Some(RuleSet::Rcfc6)
    } else if id.contains("frap") || id.contains("cafc") {
        Some(RuleSet::Frap26)
    } else if id.contains("itc") {
        Some(RuleSet::Itc210)
    } else if id.contains("frcp") {
        Some(RuleSet::Frcp6)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruleset_from_forum_key() {
        assert_eq!(
            ruleset_for_forum(Some("cofc"), "cofc"),
            Some(RuleSet::Rcfc6)
        );
        assert_eq!(
            ruleset_for_forum(Some("cafc"), "cafc"),
            Some(RuleSet::Frap26)
        );
        assert_eq!(
            ruleset_for_forum(Some("itc"), "itc-337"),
            Some(RuleSet::Itc210)
        );
    }

    #[test]
    fn ruleset_from_pack_id_when_forum_absent() {
        assert_eq!(
            ruleset_for_forum(None, "frcp-civil-procedure"),
            Some(RuleSet::Frcp6)
        );
        assert_eq!(
            ruleset_for_forum(None, "frap-appellate-procedure"),
            Some(RuleSet::Frap26)
        );
        assert_eq!(ruleset_for_forum(None, "itc-337"), Some(RuleSet::Itc210));
    }

    #[test]
    fn ruleset_unknown_for_unrelated_pack() {
        assert_eq!(ruleset_for_forum(None, "mpep-prosecution"), None);
    }

    #[test]
    fn dispatch_routes_itc_to_the_itc_module() {
        let d = DayDeadline {
            trigger: Date::parse_iso("2026-01-02").unwrap(),
            days: 3,
            direction: Direction::Forward,
            unit: Unit::Calendar,
            service_method: None,
        };
        let r = compute_due_date(&d, RuleSet::Itc210, &ClockConfig::default()).unwrap();
        assert_eq!(r.due_date, "2026-01-07");
    }
}
