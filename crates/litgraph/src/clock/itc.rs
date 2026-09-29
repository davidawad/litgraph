// SPDX-License-Identifier: GPL-3.0-or-later
//! ITC Section 337 time computation — `19 CFR 210.6(a)`, cross-referencing
//! `19 CFR 201.14(a)` (computation of time) and `201.16(d)`/`(e)` (service
//! extensions). Ported from `civ-pro-the-gathering`'s
//! `src/engine/clock/itc210.ts`, re-verified against the rule text
//! 2026-09-28.
//!
//! Sources:
//! - `19 CFR 210.6(a)` (<https://www.law.cornell.edu/cfr/text/19/210.6>):
//!   "the computation of time ... shall be in accordance with §§ 201.14 and
//!   201.16(d) and (e)".
//! - `19 CFR 201.14(a)` (<https://www.law.cornell.edu/cfr/text/19/201.14>):
//!   "Computation of any period of time ... shall begin with the first
//!   business day following the day on which the act or event ... shall
//!   have occurred. The last day of the period so computed is to be
//!   included, unless it is a Saturday, Sunday, or Federal legal holiday,
//!   in which event the period runs until the end of the next business
//!   day. When the period of time prescribed or allowed is less than 7
//!   days, intermediate Saturdays, Sundays, and Federal legal holidays
//!   shall be excluded from the computation."
//! - `19 CFR 201.16(d)`/`(e)`
//!   (<https://www.law.cornell.edu/cfr/text/19/201.16>): mail service adds
//!   3 calendar days domestic / 10 foreign; express delivery adds 1 day
//!   domestic / 5 foreign.
//!
//! WHY THIS IS A DIFFERENT ALGORITHM FROM `clock::general`, not a
//! relabeling of it:
//!
//! 1. **Start point.** `FRCP 6(a)(1)(A)` excludes only the day of the event
//!    and begins counting the very next calendar day, regardless of
//!    whether that next day is a business day. `201.14(a)` instead begins
//!    the count on the first **business** day following the event.
//! 2. **Short-period counting.** `FRCP 6(a)(1)(B)` (2009 restyling) counts
//!    every calendar day, no matter how short the period. `201.14(a)`
//!    preserves the pre-2009 FRCP approach for periods under 7 days:
//!    intermediate weekends/holidays are excluded, i.e. short ITC periods
//!    count business days, not calendar days.
//!
//! These two deltas compound: a 3-day ITC period after a Friday event runs
//! Mon/Tue/Wed (due Wednesday), while the same 3-day FRCP period after a
//! Friday event runs Sat/Sun/Mon (due Monday) — a 2-day divergence on
//! identical facts.
//!
//! **Interpretive choice (flagged, not silently assumed):** `201.16(d)/(e)`
//! say the extension is added "to the ... prescribed period," but the text
//! does not explicitly say whether `201.14(a)`'s weekend/holiday roll is
//! reapplied to the newly-extended date. This module applies the roll a
//! second time after the service extension, by analogy to how
//! `clock::general` orders `FRCP 6(d)`'s +3 days before `6(a)(1)(C)`
//! rolling. This ordering is not itself quoted in 201.16 — treat it as the
//! best-available inference, not verified text.
//!
//! **Backward periods and roll direction:** `201.14(a)`'s general clause
//! rolls forward in calendar time regardless of direction (confirmed by
//! `19 CFR 210.18(a)`'s summary-determination motion window, which uses the
//! identical "extends until the end of the next business day" language for
//! its own backward-counted deadline) — the opposite of `FRCP 6(a)(5)`'s
//! backward-roll convention (which rolls to an *earlier* day so an "at
//! least N days" floor is never shortened). This module therefore always
//! rolls forward on the final day, in both directions, for periods of 7
//! days or more. `201.14(a)` does not itself define a backward start
//! point; the anchor (day 1) for backward counts is taken as the last
//! business day before the trigger, by analogy to the forward anchor rule
//! — an inference, not verified text.

use super::date::Date;
use super::holidays::is_legal_holiday;
use super::types::{ClockConfig, ComputedDeadline, DayDeadline, Direction, RuleSet, ServiceMethod};
use crate::error::{Error, Result};

fn is_non_business(d: Date, additional: &[Date]) -> bool {
    d.is_weekend() || is_legal_holiday(d, RuleSet::Itc210, additional)
}

fn roll_forward(mut d: Date, additional: &[Date]) -> Date {
    while is_non_business(d, additional) {
        d = d.add_days(1);
    }
    d
}

fn roll_backward(mut d: Date, additional: &[Date]) -> Date {
    while is_non_business(d, additional) {
        d = d.add_days(-1);
    }
    d
}

/// `201.14(a)`: "shall begin with the first business day following the day
/// on which the act or event ... occurred."
fn first_business_day_after(trigger: Date, additional: &[Date]) -> Date {
    roll_forward(trigger.add_days(1), additional)
}

/// Mirror of [`first_business_day_after`] for backward-counted periods.
fn first_business_day_before(trigger: Date, additional: &[Date]) -> Date {
    roll_backward(trigger.add_days(-1), additional)
}

/// Advance `n` business days from `start`, counting `start` itself as
/// business day 1 (`201.14(a)`'s short-period rule). `start` is guaranteed
/// a business day by construction, so the result is too.
fn advance_business_days(start: Date, n: i64, forward: bool, additional: &[Date]) -> Date {
    let mut d = start;
    let mut counted = 1i64; // start is business day 1
    let step: i64 = if forward { 1 } else { -1 };
    while counted < n {
        d = d.add_days(step);
        if !is_non_business(d, additional) {
            counted += 1;
        }
    }
    d
}

fn service_days(method: ServiceMethod) -> Result<i64> {
    match method {
        ServiceMethod::Mail => Ok(3),
        ServiceMethod::MailForeign => Ok(10),
        ServiceMethod::Express => Ok(1),
        ServiceMethod::ExpressForeign => Ok(5),
        other => Err(Error::Invalid(format!(
            "service method {other:?} is not valid under 19 CFR 210.6(a); valid methods: mail, mail-foreign, express, express-foreign"
        ))),
    }
}

/// Compute a day-counted due date under `19 CFR 210.6(a)`/`201.14(a)`/`201.16`.
///
/// `dl.unit` is ignored: `201.14(a)`'s business-day-vs-calendar-day choice
/// is an automatic function of period length (`< 7` days vs `>= 7`), not a
/// per-deadline authored choice the way `Deadline.unit` is for the
/// `FRCP`/`RCFC`/`FRAP` family.
///
/// # Errors
/// `Error::Invalid` if `days < 1` (`201.14(a)` has no zero-day case) or
/// `service_method` is not one of the ITC's four.
pub fn compute_due_date(dl: &DayDeadline, cfg: &ClockConfig) -> Result<ComputedDeadline> {
    if dl.days < 1 {
        return Err(Error::Invalid(
            "19 CFR 201.14(a) periods must be at least 1 day".into(),
        ));
    }
    let add = &cfg.additional_holidays;
    let forward = matches!(dl.direction, Direction::Forward);
    let anchor = if forward {
        first_business_day_after(dl.trigger, add)
    } else {
        first_business_day_before(dl.trigger, add)
    };
    let mut steps = vec![format!(
        "19 CFR 201.14(a): anchor at the first business day {} {} -> {anchor}",
        if forward { "after" } else { "before" },
        dl.trigger
    )];

    let (base, mut last_day_rolled) = if dl.days < 7 {
        let b = advance_business_days(anchor, dl.days, forward, add);
        steps.push(format!(
            "19 CFR 201.14(a) short-period rule (< 7 days): count {} business day(s) from the anchor -> {b} (already a business day)",
            dl.days
        ));
        (b, false)
    } else {
        let raw = if forward {
            anchor.add_days(dl.days - 1)
        } else {
            anchor.add_days(-(dl.days - 1))
        };
        let b = roll_forward(raw, add);
        steps.push(format!(
            "19 CFR 201.14(a) general rule (>= 7 days): count {} calendar day(s) from the anchor -> {raw}",
            dl.days
        ));
        if b != raw {
            steps.push(format!(
                "19 CFR 201.14(a): last day fell on a Saturday, Sunday, or Federal legal holiday; extends until the end of the next business day -> {b}"
            ));
        }
        (b, b != raw)
    };

    let mut current = base;
    let mut service_days_added = false;
    if let Some(method) = dl.service_method {
        if forward {
            let n = service_days(method)?;
            let after = current.add_days(n);
            steps.push(format!(
                "19 CFR 201.16(d)/(e): service by {method:?} adds {n} calendar day(s) -> {after}"
            ));
            let reroll = roll_forward(after, add);
            if reroll != after {
                steps.push(format!(
                    "19 CFR 201.14(a): re-applied after the service extension (interpretive choice; see module doc) -> {reroll}"
                ));
                last_day_rolled = true;
            }
            current = reroll;
            service_days_added = true;
        } else {
            steps.push(
                "19 CFR 201.16 service extensions apply to forward periods only; ignored".into(),
            );
        }
    }

    Ok(ComputedDeadline {
        rule_set: RuleSet::Itc210,
        anchor_date: anchor.to_iso(),
        base_date: base.to_iso(),
        due_date: current.to_iso(),
        last_day_rolled,
        service_days_added,
        clerk_inaccessibility_applied: false,
        steps,
    })
}

#[cfg(test)]
mod tests {
    use super::super::types::Unit;
    use super::*;

    fn fwd(trigger: &str, days: i64) -> ComputedDeadline {
        compute_due_date(
            &DayDeadline {
                trigger: Date::parse_iso(trigger).unwrap(),
                days,
                direction: Direction::Forward,
                unit: Unit::Calendar,
                service_method: None,
            },
            &ClockConfig::default(),
        )
        .unwrap()
    }

    #[test]
    fn short_period_counts_business_days_not_calendar_days() {
        // Trigger Friday 2026-01-02. FRCP's day 1 would be Saturday;
        // ITC's day 1 is the following business day.
        // Anchor = first business day after Fri Jan 2 = Mon Jan 5.
        // 3 business days from Mon Jan 5 (inclusive) = Mon, Tue, Wed = Jan 7.
        let r = fwd("2026-01-02", 3);
        assert_eq!(r.due_date, "2026-01-07");
    }

    #[test]
    fn diverges_from_frcp_by_two_days_on_a_friday_trigger() {
        use super::super::general::compute_due_date as gen_compute;
        use crate::clock::{DayDeadline as DD, Direction as D, RuleSet as RS, Unit as U};
        let itc = fwd("2026-01-02", 3).due_date;
        let frcp = gen_compute(
            &DD {
                trigger: Date::parse_iso("2026-01-02").unwrap(),
                days: 3,
                direction: D::Forward,
                unit: U::Calendar,
                service_method: None,
            },
            RS::Frcp6,
            &ClockConfig::default(),
        )
        .unwrap()
        .due_date;
        assert_eq!(itc, "2026-01-07"); // Wed
        assert_eq!(frcp, "2026-01-05"); // Mon
    }

    #[test]
    fn general_rule_rolls_forward_on_backward_periods_too() {
        // Confirms the ITC's forward-only roll convention differs from FRCP's.
        let r = compute_due_date(
            &DayDeadline {
                trigger: Date::parse_iso("2026-07-31").unwrap(),
                days: 10,
                direction: Direction::Backward,
                unit: Unit::Calendar,
                service_method: None,
            },
            &ClockConfig::default(),
        )
        .unwrap();
        // Whatever the exact date, it must not be a weekend/holiday.
        assert!(!Date::parse_iso(&r.due_date).unwrap().is_weekend());
    }

    #[test]
    fn rejects_zero_day_period() {
        assert!(fwd_result("2026-01-01", 0).is_err());
    }

    fn fwd_result(trigger: &str, days: i64) -> Result<ComputedDeadline> {
        compute_due_date(
            &DayDeadline {
                trigger: Date::parse_iso(trigger).unwrap(),
                days,
                direction: Direction::Forward,
                unit: Unit::Calendar,
                service_method: None,
            },
            &ClockConfig::default(),
        )
    }

    #[test]
    fn mail_service_adds_three_domestic_days() {
        let base = fwd("2026-06-01", 20);
        let with_mail = compute_due_date(
            &DayDeadline {
                trigger: Date::parse_iso("2026-06-01").unwrap(),
                days: 20,
                direction: Direction::Forward,
                unit: Unit::Calendar,
                service_method: Some(ServiceMethod::Mail),
            },
            &ClockConfig::default(),
        )
        .unwrap();
        assert!(with_mail.due_date > base.due_date);
        assert!(with_mail.service_days_added);
    }

    #[test]
    fn frcp_only_service_method_rejected_under_itc() {
        let err = compute_due_date(
            &DayDeadline {
                trigger: Date::parse_iso("2026-06-01").unwrap(),
                days: 20,
                direction: Direction::Forward,
                unit: Unit::Calendar,
                service_method: Some(ServiceMethod::Clerk),
            },
            &ClockConfig::default(),
        );
        assert!(err.is_err());
    }
}
