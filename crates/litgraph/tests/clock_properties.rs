// SPDX-License-Identifier: GPL-3.0-or-later
//! Property-based tests over `litgraph::clock`: invariants every rule set
//! must satisfy for arbitrary triggers/day counts, ported in spirit from
//! `civ-pro-the-gathering`'s `fast-check` properties in
//! `tests/engine/clock/clock.test.ts`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use litgraph::clock::holidays::is_legal_holiday;
use litgraph::clock::{
    compute_due_date, ClockConfig, Date, DayDeadline, Direction, RuleSet, ServiceMethod, Unit,
};
use proptest::prelude::*;

fn any_date() -> impl Strategy<Value = Date> {
    (2000i32..2090, 1u32..=12, 1u32..=28)
        .prop_map(|(y, m, d)| Date::from_ymd(y, m, d).expect("y/m/1..=28 is always a valid date"))
}

fn calendar_rule_set() -> impl Strategy<Value = RuleSet> {
    prop_oneof![
        Just(RuleSet::Frcp6),
        Just(RuleSet::Rcfc6),
        Just(RuleSet::Frap26)
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// FRCP 6(a)(1)(C) / RCFC 6(a)(1)(C) / FRAP 26(a)(1)(C): a forward due
    /// date never lands on a weekend or that rule set's legal holiday.
    #[test]
    fn forward_due_date_never_a_weekend_or_holiday(
        trigger in any_date(), days in 1i64..400, rs in calendar_rule_set(),
    ) {
        let r = compute_due_date(
            &DayDeadline { trigger, days, direction: Direction::Forward, unit: Unit::Calendar, service_method: None },
            rs,
            &ClockConfig::default(),
        ).unwrap();
        let due = Date::parse_iso(&r.due_date).unwrap();
        prop_assert!(!due.is_weekend());
        prop_assert!(!is_legal_holiday(due, rs, &[]));
    }

    /// Same invariant, backward direction (FRCP 6(a)(5) et al.).
    #[test]
    fn backward_due_date_never_a_weekend_or_holiday(
        trigger in any_date(), days in 1i64..400, rs in calendar_rule_set(),
    ) {
        let r = compute_due_date(
            &DayDeadline { trigger, days, direction: Direction::Backward, unit: Unit::Calendar, service_method: None },
            rs,
            &ClockConfig::default(),
        ).unwrap();
        let due = Date::parse_iso(&r.due_date).unwrap();
        prop_assert!(!due.is_weekend());
        prop_assert!(!is_legal_holiday(due, rs, &[]));
    }

    /// A forward due date is never before the trigger (0 days can equal it).
    #[test]
    fn forward_due_date_on_or_after_trigger(
        trigger in any_date(), days in 0i64..400, rs in calendar_rule_set(),
    ) {
        let r = compute_due_date(
            &DayDeadline { trigger, days, direction: Direction::Forward, unit: Unit::Calendar, service_method: None },
            rs,
            &ClockConfig::default(),
        ).unwrap();
        prop_assert!(Date::parse_iso(&r.due_date).unwrap() >= trigger);
    }

    /// A backward due date is never after the trigger.
    #[test]
    fn backward_due_date_on_or_before_trigger(
        trigger in any_date(), days in 0i64..400, rs in calendar_rule_set(),
    ) {
        let r = compute_due_date(
            &DayDeadline { trigger, days, direction: Direction::Backward, unit: Unit::Calendar, service_method: None },
            rs,
            &ClockConfig::default(),
        ).unwrap();
        prop_assert!(Date::parse_iso(&r.due_date).unwrap() <= trigger);
    }

    /// Forward counting is monotonic in `days`.
    #[test]
    fn forward_counting_is_monotonic(
        trigger in any_date(), d1 in 0i64..200, d2 in 0i64..200, rs in calendar_rule_set(),
    ) {
        let due = |days| compute_due_date(
            &DayDeadline { trigger, days, direction: Direction::Forward, unit: Unit::Calendar, service_method: None },
            rs,
            &ClockConfig::default(),
        ).unwrap().due_date;
        if d1 < d2 {
            prop_assert!(due(d1) <= due(d2));
        } else if d1 == d2 {
            prop_assert_eq!(due(d1), due(d2));
        }
    }

    /// Mail service never produces an *earlier* due date than no service.
    #[test]
    fn mail_service_never_earlier_than_no_service(
        trigger in any_date(), days in 1i64..400, rs in calendar_rule_set(),
    ) {
        let method = match rs {
            RuleSet::Frap26 => ServiceMethod::CommercialCarrier,
            _ => ServiceMethod::Mail,
        };
        let base = compute_due_date(
            &DayDeadline { trigger, days, direction: Direction::Forward, unit: Unit::Calendar, service_method: None },
            rs,
            &ClockConfig::default(),
        ).unwrap().due_date;
        let with_service = compute_due_date(
            &DayDeadline { trigger, days, direction: Direction::Forward, unit: Unit::Calendar, service_method: Some(method) },
            rs,
            &ClockConfig::default(),
        ).unwrap().due_date;
        prop_assert!(with_service >= base);
    }

    /// Court-day ("business day") counting always lands on a business day,
    /// forward or backward, for every calendar-family rule set.
    #[test]
    fn court_days_always_land_on_a_business_day(
        trigger in any_date(), days in 1i64..60, forward in any::<bool>(), rs in calendar_rule_set(),
    ) {
        let direction = if forward { Direction::Forward } else { Direction::Backward };
        let r = compute_due_date(
            &DayDeadline { trigger, days, direction, unit: Unit::Court, service_method: None },
            rs,
            &ClockConfig::default(),
        ).unwrap();
        let due = Date::parse_iso(&r.due_date).unwrap();
        prop_assert!(!due.is_weekend());
        prop_assert!(!is_legal_holiday(due, rs, &[]));
    }

    /// ITC 210: a forward due date (>= 1 day, the only valid range) is
    /// never a weekend/holiday and never before the trigger.
    #[test]
    fn itc_forward_due_date_never_a_weekend_or_holiday(
        trigger in any_date(), days in 1i64..400,
    ) {
        let r = compute_due_date(
            &DayDeadline { trigger, days, direction: Direction::Forward, unit: Unit::Calendar, service_method: None },
            RuleSet::Itc210,
            &ClockConfig::default(),
        ).unwrap();
        let due = Date::parse_iso(&r.due_date).unwrap();
        prop_assert!(!due.is_weekend());
        prop_assert!(!is_legal_holiday(due, RuleSet::Itc210, &[]));
        prop_assert!(due >= trigger);
    }

    /// ITC 210's business-day anchor is always itself a business day.
    #[test]
    fn itc_anchor_is_always_a_business_day(trigger in any_date(), days in 1i64..400) {
        let r = compute_due_date(
            &DayDeadline { trigger, days, direction: Direction::Forward, unit: Unit::Calendar, service_method: None },
            RuleSet::Itc210,
            &ClockConfig::default(),
        ).unwrap();
        let anchor = Date::parse_iso(&r.anchor_date).unwrap();
        prop_assert!(!anchor.is_weekend());
        prop_assert!(!is_legal_holiday(anchor, RuleSet::Itc210, &[]));
    }
}
