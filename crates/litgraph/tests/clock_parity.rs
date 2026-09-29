// SPDX-License-Identifier: GPL-3.0-or-later
//! Parity fixtures ported from `civ-pro-the-gathering`'s
//! `tests/engine/clock/clock.test.ts` (FRCP 6 clock engine), which the
//! comment there cites to:
//! - FRCP Rule 6(a)-(d) (`law.cornell.edu/rules/frcp/rule_6`)
//! - Advisory Committee Notes, 2009 Amendment (calendar-day counting)
//! - Advisory Committee Notes, 2016 Amendment (removing electronic service
//!   from 6(d))
//!
//! Every case below is the same trigger/days/expected-due-date as the
//! TypeScript source, re-expressed against `litgraph::clock`. Known-answer
//! cases specific to `RCFC 6`, `FRAP 26`, and `19 CFR 210.6(a)` (not present
//! in the TS engine, which only ever modeled FRCP + a separate ITC module)
//! live alongside them since they exercise the same API.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::clock::{
    compute_due_date, compute_hour_deadline, ClockConfig, Date, DayDeadline, Direction,
    HourDeadline, RuleSet, ServiceMethod, Unit,
};

fn forward(trigger: &str, days: i64) -> String {
    compute_due_date(
        &DayDeadline {
            trigger: Date::parse_iso(trigger).unwrap(),
            days,
            direction: Direction::Forward,
            unit: Unit::Calendar,
            service_method: None,
        },
        RuleSet::Frcp6,
        &ClockConfig::default(),
    )
    .unwrap()
    .due_date
}

fn backward(trigger: &str, days: i64) -> String {
    compute_due_date(
        &DayDeadline {
            trigger: Date::parse_iso(trigger).unwrap(),
            days,
            direction: Direction::Backward,
            unit: Unit::Calendar,
            service_method: None,
        },
        RuleSet::Frcp6,
        &ClockConfig::default(),
    )
    .unwrap()
    .due_date
}

fn forward_mail(trigger: &str, days: i64) -> String {
    compute_due_date(
        &DayDeadline {
            trigger: Date::parse_iso(trigger).unwrap(),
            days,
            direction: Direction::Forward,
            unit: Unit::Calendar,
            service_method: Some(ServiceMethod::Mail),
        },
        RuleSet::Frcp6,
        &ClockConfig::default(),
    )
    .unwrap()
    .due_date
}

// ---------------------------------------------------------------------------
// Rule 6(a)(1): forward counting — basic cases
// ---------------------------------------------------------------------------

#[test]
fn excludes_the_trigger_day() {
    assert_eq!(forward("2026-01-06", 1), "2026-01-07");
}

#[test]
fn counts_every_day_including_weekends() {
    assert_eq!(forward("2026-07-22", 7), "2026-07-29");
}

#[test]
fn rolls_forward_when_last_day_is_saturday() {
    assert_eq!(forward("2026-07-18", 7), "2026-07-27");
}

#[test]
fn rolls_forward_when_last_day_is_sunday() {
    assert_eq!(forward("2026-07-19", 7), "2026-07-27");
}

#[test]
fn fourteen_day_period_standard_case() {
    assert_eq!(forward("2026-07-01", 14), "2026-07-15");
}

#[test]
fn zero_days_due_same_day_as_trigger() {
    assert_eq!(forward("2026-07-15", 0), "2026-07-15");
}

#[test]
fn thirty_day_removal_clock_from_a_friday() {
    assert_eq!(forward("2025-06-13", 30), "2025-07-14");
}

#[test]
fn period_ending_on_observed_holiday_mlk_2026() {
    assert_eq!(forward("2026-01-08", 11), "2026-01-20");
}

#[test]
fn period_ending_on_christmas_2021_observed_friday() {
    assert_eq!(forward("2021-12-17", 7), "2021-12-27");
}

#[test]
fn period_ending_on_juneteenth_2026() {
    assert_eq!(forward("2026-06-08", 11), "2026-06-22");
}

#[test]
fn period_ending_on_independence_day_2026_observed_friday() {
    assert_eq!(forward("2026-06-22", 11), "2026-07-06");
}

#[test]
fn crosses_a_year_boundary() {
    assert_eq!(forward("2025-12-28", 14), "2026-01-12");
}

// ---------------------------------------------------------------------------
// Rule 6(a)(1) + Rule 6(a)(5): backward counting
// ---------------------------------------------------------------------------

#[test]
fn basic_backward_fourteen_days_before_a_hearing() {
    assert_eq!(backward("2026-07-31", 14), "2026-07-17");
}

#[test]
fn backward_lands_on_saturday_rolls_to_friday() {
    assert_eq!(backward("2026-07-27", 2), "2026-07-24");
}

#[test]
fn backward_lands_on_sunday_rolls_to_friday() {
    assert_eq!(backward("2026-07-28", 2), "2026-07-24");
}

#[test]
fn backward_lands_on_mlk_day_2026_rolls_to_friday() {
    assert_eq!(backward("2026-01-22", 3), "2026-01-16");
}

#[test]
fn backward_lands_on_observed_christmas_2021_rolls_to_thursday() {
    assert_eq!(backward("2021-12-31", 7), "2021-12-23");
}

#[test]
fn backward_counting_crosses_a_year_boundary() {
    assert_eq!(backward("2026-01-05", 14), "2025-12-22");
}

// ---------------------------------------------------------------------------
// Rule 6(d): +3 days after certain service methods
// ---------------------------------------------------------------------------

#[test]
fn mail_service_adds_three_days_before_rolling() {
    assert_eq!(forward_mail("2026-07-20", 14), "2026-08-06");
}

#[test]
fn clerk_service_adds_three_days() {
    let r = compute_due_date(
        &DayDeadline {
            trigger: Date::parse_iso("2026-07-20").unwrap(),
            days: 14,
            direction: Direction::Forward,
            unit: Unit::Calendar,
            service_method: Some(ServiceMethod::Clerk),
        },
        RuleSet::Frcp6,
        &ClockConfig::default(),
    )
    .unwrap();
    assert_eq!(r.due_date, "2026-08-06");
}

#[test]
fn other_consented_service_adds_three_days() {
    let r = compute_due_date(
        &DayDeadline {
            trigger: Date::parse_iso("2026-07-20").unwrap(),
            days: 14,
            direction: Direction::Forward,
            unit: Unit::Calendar,
            service_method: Some(ServiceMethod::OtherConsented),
        },
        RuleSet::Frcp6,
        &ClockConfig::default(),
    )
    .unwrap();
    assert_eq!(r.due_date, "2026-08-06");
}

#[test]
fn service_extension_then_rolling_no_roll_needed() {
    assert_eq!(forward_mail("2026-07-17", 14), "2026-08-03");
    assert_eq!(forward_mail("2026-07-05", 14), "2026-07-22");
}

#[test]
fn service_extension_lands_on_holiday_rolls_forward() {
    assert_eq!(forward_mail("2026-01-02", 14), "2026-01-20");
}

#[test]
fn no_service_method_means_no_extension() {
    let base = forward("2026-07-20", 14);
    let no_service = compute_due_date(
        &DayDeadline {
            trigger: Date::parse_iso("2026-07-20").unwrap(),
            days: 14,
            direction: Direction::Forward,
            unit: Unit::Calendar,
            service_method: None,
        },
        RuleSet::Frcp6,
        &ClockConfig::default(),
    )
    .unwrap();
    assert_eq!(no_service.due_date, base);
    assert!(!no_service.service_days_added);
}

#[test]
fn service_extension_does_not_apply_to_backward_periods() {
    let r = compute_due_date(
        &DayDeadline {
            trigger: Date::parse_iso("2026-07-31").unwrap(),
            days: 14,
            direction: Direction::Backward,
            unit: Unit::Calendar,
            service_method: Some(ServiceMethod::Mail),
        },
        RuleSet::Frcp6,
        &ClockConfig::default(),
    )
    .unwrap();
    assert!(!r.service_days_added);
    assert_eq!(r.due_date, "2026-07-17");
}

// ---------------------------------------------------------------------------
// Rule 6(a)(3): clerk's office inaccessibility
// ---------------------------------------------------------------------------

#[test]
fn clerk_inaccessible_extends_to_next_accessible_day() {
    let r = compute_due_date(
        &DayDeadline {
            trigger: Date::parse_iso("2026-07-20").unwrap(),
            days: 14,
            direction: Direction::Forward,
            unit: Unit::Calendar,
            service_method: None,
        },
        RuleSet::Frcp6,
        &ClockConfig {
            clerk_inaccessible: true,
            ..ClockConfig::default()
        },
    )
    .unwrap();
    assert!(r.clerk_inaccessibility_applied);
    assert_eq!(r.due_date, "2026-08-04");
}

#[test]
fn clerk_inaccessible_on_friday_extends_to_next_monday() {
    let r = compute_due_date(
        &DayDeadline {
            trigger: Date::parse_iso("2026-07-06").unwrap(),
            days: 14,
            direction: Direction::Forward,
            unit: Unit::Calendar,
            service_method: None,
        },
        RuleSet::Frcp6,
        &ClockConfig {
            clerk_inaccessible: true,
            ..ClockConfig::default()
        },
    )
    .unwrap();
    assert_eq!(r.due_date, "2026-07-21");
}

#[test]
fn clerk_inaccessibility_false_by_default() {
    let r = compute_due_date(
        &DayDeadline {
            trigger: Date::parse_iso("2026-07-06").unwrap(),
            days: 14,
            direction: Direction::Forward,
            unit: Unit::Calendar,
            service_method: None,
        },
        RuleSet::Frcp6,
        &ClockConfig::default(),
    )
    .unwrap();
    assert!(!r.clerk_inaccessibility_applied);
}

// ---------------------------------------------------------------------------
// Rule 6(a)(2): hours-based periods
// ---------------------------------------------------------------------------

fn hours_from(trigger: &str, hour: i64, hours: i64) -> litgraph::clock::ComputedHourDeadline {
    let trigger_epoch_seconds = Date::parse_iso(trigger).unwrap().epoch_day() * 86400 + hour * 3600;
    compute_hour_deadline(
        &HourDeadline {
            trigger_epoch_seconds,
            hours,
        },
        RuleSet::Frcp6,
        &ClockConfig::default(),
    )
    .unwrap()
}

#[test]
fn basic_seventy_two_hour_period_from_monday_noon() {
    let r = hours_from("2026-07-27", 12, 72);
    assert_eq!(r.due_date, "2026-07-30");
    assert!(r.due_datetime.starts_with("2026-07-30T"));
    assert!(!r.last_day_rolled);
}

#[test]
fn period_ending_on_saturday_rolls_to_same_time_monday() {
    let r = hours_from("2026-07-24", 12, 24);
    assert_eq!(r.due_date, "2026-07-27");
    assert!(r.last_day_rolled);
}

#[test]
fn period_ending_on_sunday_rolls_to_same_time_monday() {
    let r = hours_from("2026-07-25", 12, 24);
    assert_eq!(r.due_date, "2026-07-27");
    assert!(r.last_day_rolled);
}

#[test]
fn counting_begins_immediately_not_excluding_trigger_day() {
    let r = hours_from("2026-07-27", 0, 1);
    assert_eq!(r.due_date, "2026-07-27");
}

#[test]
fn period_ending_on_a_federal_holiday_rolls_to_next_business_day() {
    let r = hours_from("2026-01-18", 12, 24);
    assert_eq!(r.due_date, "2026-01-20");
    assert!(r.last_day_rolled);
}

// ---------------------------------------------------------------------------
// State holidays (Rule 6(a)(6)(C))
// ---------------------------------------------------------------------------

#[test]
fn state_holiday_is_treated_as_a_legal_holiday() {
    let base = forward("2026-07-27", 14);
    assert_eq!(base, "2026-08-10");
    let with_state = compute_due_date(
        &DayDeadline {
            trigger: Date::parse_iso("2026-07-27").unwrap(),
            days: 14,
            direction: Direction::Forward,
            unit: Unit::Calendar,
            service_method: None,
        },
        RuleSet::Frcp6,
        &ClockConfig {
            additional_holidays: vec![Date::parse_iso("2026-08-10").unwrap()],
            clerk_inaccessible: false,
        },
    )
    .unwrap();
    assert_eq!(with_state.due_date, "2026-08-11");
}

// federalHolidays/isLegalHoliday re-export parity, and RCFC 6/FRAP 26/ITC
// 210 known-answer cases, live in clock_parity_other_forums.rs (kept this
// file under the 500-line cap).
