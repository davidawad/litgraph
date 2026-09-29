// SPDX-License-Identifier: GPL-3.0-or-later
//! The shared day/hour-counting engine for `FRCP 6`, `RCFC 6`, and
//! `FRAP 26`: their day-counting mechanics (exclude the trigger day, count
//! every calendar day, roll a Saturday/Sunday/holiday last day forward or
//! backward) are textually identical. What differs between them —
//! legal-holiday calendar, which service methods add days, whether hours
//! and clerk-inaccessibility exist at all — is expressed as
//! [`RuleSet`]-keyed data, not three copy-pasted algorithms.
//!
//! Sources (fetched 2026-09-28):
//! - `FRCP 6`: <https://www.law.cornell.edu/rules/frcp/rule_6>
//! - `RCFC 6`: <https://www.uscfc.uscourts.gov/rules> (current rules PDF,
//!   effective 2026-07-27) — identical restyled text to FRCP 6(a)/(d),
//!   `RCFC 6(a)(6)(A)` additionally lists Inauguration Day (see
//!   `clock::holidays`).
//! - `FRAP 26`: <https://www.law.cornell.edu/rules/frap/rule_26> — 26(a)(1)
//!   day mechanics match FRCP 6(a)(1) exactly; 26(c)'s 3-day service
//!   extension is keyed to mail / third-party commercial carrier / prison
//!   mailing system (not "leaving with the clerk" or "other consented
//!   means" — those are FRCP/RCFC-only service methods under Rule
//!   5(b)(2)(D)/(F), which FRAP's Rule 25 does not define); FRAP has no
//!   hours provision and no clerk-inaccessibility extension in the fetched
//!   text, so [`RuleSet::supports_hours`] /
//!   [`RuleSet::supports_clerk_inaccessibility`] are both false for it.

use super::date::Date;
use super::holidays::is_legal_holiday;
use super::types::{
    ClockConfig, ComputedDeadline, ComputedHourDeadline, DayDeadline, Direction, HourDeadline,
    RuleSet, ServiceMethod, Unit,
};
use crate::error::{Error, Result};

fn is_non_business(d: Date, rule_set: RuleSet, additional: &[Date]) -> bool {
    d.is_weekend() || is_legal_holiday(d, rule_set, additional)
}

fn roll_forward(mut d: Date, rule_set: RuleSet, additional: &[Date]) -> Date {
    while is_non_business(d, rule_set, additional) {
        d = d.add_days(1);
    }
    d
}

fn roll_backward(mut d: Date, rule_set: RuleSet, additional: &[Date]) -> Date {
    while is_non_business(d, rule_set, additional) {
        d = d.add_days(-1);
    }
    d
}

fn advance_court_days(
    start: Date,
    n: i64,
    forward: bool,
    rule_set: RuleSet,
    additional: &[Date],
) -> Date {
    let mut d = start;
    let mut counted = 0i64;
    let step: i64 = if forward { 1 } else { -1 };
    while counted < n {
        d = d.add_days(step);
        if !is_non_business(d, rule_set, additional) {
            counted += 1;
        }
    }
    d
}

/// Days added by `method` under `rule_set`, or an error naming the valid
/// methods for that rule set.
///
/// # Errors
/// `Error::Invalid` if `method` is not one this `rule_set` recognizes.
pub fn service_days(rule_set: RuleSet, method: ServiceMethod) -> Result<i64> {
    use ServiceMethod::{Clerk, CommercialCarrier, Mail, OtherConsented, PrisonMail};
    match (rule_set, method) {
        (RuleSet::Frcp6 | RuleSet::Rcfc6, Mail | Clerk | OtherConsented)
        | (RuleSet::Frap26, Mail | CommercialCarrier | PrisonMail) => Ok(3),
        _ => Err(Error::Invalid(format!(
            "service method {method:?} is not valid under {}; valid methods: {}",
            rule_set.label(),
            match rule_set {
                RuleSet::Frcp6 | RuleSet::Rcfc6 => "mail, clerk, other-consented",
                RuleSet::Frap26 => "mail, commercial-carrier, prison-mail",
                RuleSet::Itc210 => "mail, mail-foreign, express, express-foreign",
            }
        ))),
    }
}

fn validate_days(days: i64) -> Result<()> {
    if days < 0 {
        return Err(Error::Invalid(format!(
            "days must be non-negative, got {days}"
        )));
    }
    Ok(())
}

/// Steps 6(a)(1)(A)/(B) (or 6(a)(5) backward), or the generic court-days
/// counting mode. No rolling yet: per the ordering confirmed below, a
/// service extension (if any) applies to this *unrolled* count, and only
/// the resulting date gets rolled — once.
fn count_only(
    dl: &DayDeadline,
    rule_set: RuleSet,
    forward: bool,
    add: &[Date],
    steps: &mut Vec<String>,
) -> Date {
    match dl.unit {
        Unit::Calendar => {
            let r = dl
                .trigger
                .add_days(if forward { dl.days } else { -dl.days });
            steps.push(format!(
                "{}(a)(1)(B): count {} calendar day(s) {} the trigger, including weekends/holidays -> {r}",
                rule_set.label(),
                dl.days,
                if forward { "after" } else { "before" }
            ));
            r
        }
        Unit::Court => {
            let r = advance_court_days(dl.trigger, dl.days, forward, rule_set, add);
            steps.push(format!(
                "court days: advance {} business day(s) {} {} (skipping weekends/holidays) -> {r}",
                dl.days,
                if forward {
                    "forward from"
                } else {
                    "backward from"
                },
                dl.trigger
            ));
            r
        }
    }
}

/// Apply the 6(d)/26(c) service extension to the *unrolled* count, if any.
/// Returns `(after_service, service_days_added)`.
///
/// Ordering (`FRCP 6(d)`: "3 days are added **after the period would
/// otherwise expire under Rule 6(a)**"): the civ-pro-the-gathering
/// TypeScript port (`src/engine/clock/index.ts`, doc comment on
/// `computeDueDate`) resolves the ordering question this phrase raises —
/// does "expire under Rule 6(a)" mean the raw count or the
/// already-rolled date? — by adding the 3 days to the raw count and
/// rolling once at the end, citing the 2009 and 2016 Advisory Committee
/// Notes. This module preserves that reading for parity with the ported
/// engine. `RCFC 6(d)` is textually identical, so the same reading
/// applies; `FRAP 26(c)`'s "3 days are added after the period would
/// otherwise expire" is structurally the same provision, so it is applied
/// the same way here by direct analogy (not itself separately verified
/// against Advisory Committee commentary on FRAP 26).
fn apply_service(
    dl: &DayDeadline,
    rule_set: RuleSet,
    forward: bool,
    raw: Date,
    steps: &mut Vec<String>,
) -> Result<(Date, bool)> {
    let Some(method) = dl.service_method else {
        return Ok((raw, false));
    };
    if !forward {
        steps.push(format!(
            "{}(d)/(c) does not apply to backward-counted periods; ignored",
            rule_set.label()
        ));
        return Ok((raw, false));
    }
    let n = service_days(rule_set, method)?;
    let after = raw.add_days(n);
    steps.push(format!(
        "{}(d)/(c): service by {method:?} adds {n} day(s) (to the unrolled count) -> {after}",
        rule_set.label()
    ));
    Ok((after, true))
}

/// Roll the (possibly service-extended) date past weekends/holidays —
/// exactly once, per [`apply_service`]'s doc comment. Returns
/// `(rolled, last_day_rolled)`.
fn roll_once(
    dl: &DayDeadline,
    rule_set: RuleSet,
    forward: bool,
    add: &[Date],
    after_service: Date,
) -> (Date, bool) {
    let rolled = match (dl.unit, forward) {
        (Unit::Calendar, false) => roll_backward(after_service, rule_set, add),
        // Court days: already a business day unless a (forward-only)
        // service extension pushed it off one; roll_forward is then a
        // no-op below anyway.
        (Unit::Calendar, true) | (Unit::Court, _) => roll_forward(after_service, rule_set, add),
    };
    (rolled, rolled != after_service)
}

/// Apply the 6(a)(3) clerk-inaccessibility extension, if requested and
/// applicable. Returns `(current, applied)`.
fn apply_clerk_inaccessibility(
    rule_set: RuleSet,
    forward: bool,
    clerk_inaccessible: bool,
    add: &[Date],
    current: Date,
    steps: &mut Vec<String>,
) -> (Date, bool) {
    if !clerk_inaccessible {
        return (current, false);
    }
    if forward && rule_set.supports_clerk_inaccessibility() {
        let next = roll_forward(current.add_days(1), rule_set, add);
        steps.push(format!(
            "{}(a)(3): clerk's office inaccessible on the last day; extend -> {next}",
            rule_set.label()
        ));
        (next, true)
    } else {
        steps.push(format!(
            "clerk_inaccessible requested but {} has no applicable clerk-inaccessibility extension for this direction; ignored",
            rule_set.label()
        ));
        (current, false)
    }
}

/// Compute a day-counted due date under `FRCP 6`, `RCFC 6`, or `FRAP 26`.
///
/// # Errors
/// `Error::Invalid` for `rule_set == Itc210` (use `clock::itc` instead), a
/// negative day count, or a `service_method` not valid under `rule_set`.
pub fn compute_due_date(
    dl: &DayDeadline,
    rule_set: RuleSet,
    cfg: &ClockConfig,
) -> Result<ComputedDeadline> {
    if rule_set == RuleSet::Itc210 {
        return Err(Error::Invalid(
            "RuleSet::Itc210 is computed by clock::itc, not clock::general".into(),
        ));
    }
    validate_days(dl.days)?;
    let add = &cfg.additional_holidays;
    let forward = matches!(dl.direction, Direction::Forward);
    let mut steps = vec![format!(
        "{}(a)(1)(A): exclude the trigger date {}",
        rule_set.label(),
        dl.trigger
    )];

    let raw = count_only(dl, rule_set, forward, add, &mut steps);
    let (after_service, service_days_added) =
        apply_service(dl, rule_set, forward, raw, &mut steps)?;
    let (rolled, last_day_rolled) = roll_once(dl, rule_set, forward, add, after_service);
    if last_day_rolled {
        let subsection = if forward { "(a)(1)(C)" } else { "(a)(5)" };
        steps.push(format!(
            "{}{subsection}: last day fell on a Saturday, Sunday, or legal holiday; roll {} -> {rolled}",
            rule_set.label(),
            if forward { "forward" } else { "backward" }
        ));
    }
    let (current, clerk_inaccessibility_applied) = apply_clerk_inaccessibility(
        rule_set,
        forward,
        cfg.clerk_inaccessible,
        add,
        rolled,
        &mut steps,
    );

    Ok(ComputedDeadline {
        rule_set,
        anchor_date: dl.trigger.to_iso(),
        base_date: after_service.to_iso(),
        due_date: current.to_iso(),
        last_day_rolled,
        service_days_added,
        clerk_inaccessibility_applied,
        steps,
    })
}

/// Compute a due instant for an hours-based period (`FRCP`/`RCFC 6(a)(2)`).
///
/// # Errors
/// `Error::Invalid` if `rule_set` has no hours provision
/// ([`RuleSet::supports_hours`]) or `hours` is negative.
pub fn compute_hour_deadline(
    dl: &HourDeadline,
    rule_set: RuleSet,
    cfg: &ClockConfig,
) -> Result<ComputedHourDeadline> {
    if !rule_set.supports_hours() {
        return Err(Error::Invalid(format!(
            "{} has no hours-based period provision in the fetched text",
            rule_set.label()
        )));
    }
    if dl.hours < 0 {
        return Err(Error::Invalid(format!(
            "hours must be non-negative, got {}",
            dl.hours
        )));
    }
    let due_instant = dl.trigger_epoch_seconds + dl.hours * 3600;
    let due_day = Date::from_epoch_day(due_instant.div_euclid(86400));
    let mut steps = vec![format!(
        "{}(a)(2)(A): begin counting immediately (the trigger hour counts); +{} hour(s)",
        rule_set.label(),
        dl.hours
    )];
    let mut final_day = due_day;
    let mut last_day_rolled = false;
    while is_non_business(final_day, rule_set, &cfg.additional_holidays) {
        final_day = final_day.add_days(1);
        last_day_rolled = true;
    }
    if last_day_rolled {
        steps.push(format!(
            "{}(a)(2)(C): period would end on a Saturday, Sunday, or legal holiday; continue to the same time on {final_day}",
            rule_set.label()
        ));
    }
    let seconds_in_day = due_instant.rem_euclid(86400);
    let (h, m, s) = (
        seconds_in_day / 3600,
        (seconds_in_day / 60) % 60,
        seconds_in_day % 60,
    );
    Ok(ComputedHourDeadline {
        due_date: final_day.to_iso(),
        due_datetime: format!("{final_day}T{h:02}:{m:02}:{s:02}"),
        last_day_rolled,
        steps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fwd(rs: RuleSet, trigger: &str, days: i64) -> ComputedDeadline {
        compute_due_date(
            &DayDeadline {
                trigger: Date::parse_iso(trigger).unwrap(),
                days,
                direction: Direction::Forward,
                unit: Unit::Calendar,
                service_method: None,
            },
            rs,
            &ClockConfig::default(),
        )
        .unwrap()
    }

    fn bwd(rs: RuleSet, trigger: &str, days: i64) -> ComputedDeadline {
        compute_due_date(
            &DayDeadline {
                trigger: Date::parse_iso(trigger).unwrap(),
                days,
                direction: Direction::Backward,
                unit: Unit::Calendar,
                service_method: None,
            },
            rs,
            &ClockConfig::default(),
        )
        .unwrap()
    }

    #[test]
    fn excludes_trigger_day() {
        assert_eq!(fwd(RuleSet::Frcp6, "2026-01-06", 1).due_date, "2026-01-07");
    }

    #[test]
    fn rolls_forward_past_saturday() {
        assert_eq!(fwd(RuleSet::Frcp6, "2026-07-18", 7).due_date, "2026-07-27");
    }

    #[test]
    fn rolls_forward_past_mlk_holiday() {
        assert_eq!(fwd(RuleSet::Frcp6, "2026-01-08", 11).due_date, "2026-01-20");
    }

    #[test]
    fn backward_rolls_to_earlier_business_day() {
        assert_eq!(bwd(RuleSet::Frcp6, "2026-07-27", 2).due_date, "2026-07-24");
    }

    #[test]
    fn frap_rejects_frcp_only_service_method() {
        let err = compute_due_date(
            &DayDeadline {
                trigger: Date::parse_iso("2026-07-20").unwrap(),
                days: 14,
                direction: Direction::Forward,
                unit: Unit::Calendar,
                service_method: Some(ServiceMethod::Clerk),
            },
            RuleSet::Frap26,
            &ClockConfig::default(),
        );
        assert!(err.is_err());
    }

    #[test]
    fn rcfc_matches_frcp_on_ordinary_mail_service() {
        let with_mail = |rs| {
            compute_due_date(
                &DayDeadline {
                    trigger: Date::parse_iso("2026-07-20").unwrap(),
                    days: 14,
                    direction: Direction::Forward,
                    unit: Unit::Calendar,
                    service_method: Some(ServiceMethod::Mail),
                },
                rs,
                &ClockConfig::default(),
            )
            .unwrap()
            .due_date
        };
        assert_eq!(with_mail(RuleSet::Frcp6), "2026-08-06");
        assert_eq!(with_mail(RuleSet::Frcp6), with_mail(RuleSet::Rcfc6));
    }

    #[test]
    fn itc210_is_rejected_by_the_general_engine() {
        assert!(compute_due_date(
            &DayDeadline {
                trigger: Date::parse_iso("2026-01-01").unwrap(),
                days: 5,
                direction: Direction::Forward,
                unit: Unit::Calendar,
                service_method: None,
            },
            RuleSet::Itc210,
            &ClockConfig::default(),
        )
        .is_err());
    }

    #[test]
    fn hours_unsupported_under_frap() {
        assert!(compute_hour_deadline(
            &HourDeadline {
                trigger_epoch_seconds: 0,
                hours: 24
            },
            RuleSet::Frap26,
            &ClockConfig::default(),
        )
        .is_err());
    }

    #[test]
    fn hours_roll_past_weekend() {
        // Fri 2026-07-24 12:00:00 (calendar-local) + 24h -> Sat -> rolls to Mon.
        let trigger = Date::parse_iso("2026-07-24").unwrap();
        let trigger_seconds = trigger.epoch_day() * 86400 + 12 * 3600;
        let r = compute_hour_deadline(
            &HourDeadline {
                trigger_epoch_seconds: trigger_seconds,
                hours: 24,
            },
            RuleSet::Frcp6,
            &ClockConfig::default(),
        )
        .unwrap();
        assert_eq!(r.due_date, "2026-07-27");
        assert!(r.last_day_rolled);
    }
}
