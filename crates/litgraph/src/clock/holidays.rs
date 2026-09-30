// SPDX-License-Identifier: GPL-3.0-or-later
//! Federal legal holidays, computed for any year (no hardcoded table).
//!
//! Source: `5 U.S.C. § 6103` (fetched 2026-09-28,
//! <https://www.law.cornell.edu/uscode/text/5/6103>).
//!
//! (a) The eleven statutory federal holidays:
//!
//! | Holiday | Rule |
//! |---|---|
//! | New Year's Day | January 1 |
//! | Birthday of Martin Luther King, Jr. | 3rd Monday in January |
//! | Washington's Birthday | 3rd Monday in February |
//! | Memorial Day | last Monday in May |
//! | Juneteenth National Independence Day | June 19 (added by Pub. L. 117-17, June 17, 2021) |
//! | Independence Day | July 4 |
//! | Labor Day | 1st Monday in September |
//! | Columbus Day | 2nd Monday in October |
//! | Veterans Day | November 11 |
//! | Thanksgiving Day | 4th Thursday in November |
//! | Christmas Day | December 25 |
//!
//! (b) Weekend observance: a holiday on Saturday is observed the preceding
//! Friday; a holiday on Sunday is observed the following Monday.
//!
//! Two of the eleven are recent enough that a deadline computed for a past
//! matter can straddle their creation, so they are applied only from the
//! year they first took effect: Juneteenth from 2021 (Pub. L. 117-17,
//! signed and effective June 17, 2021, first observed Friday June 18,
//! 2021), and Martin Luther King, Jr. Day from 1986 (Pub. L. 98-144, first
//! observed January 20, 1986). Earlier changes (the Uniform Monday Holiday
//! Act's move of Washington's Birthday, Memorial Day and Columbus Day to
//! Mondays, effective 1971) are not modeled, so dates before 1971 use
//! today's calendar.
//!
//! (c) Inauguration Day: "January 20 of each fourth year after 1965 ...
//! is a legal public holiday" for federal employees in the
//! Washington-metro area, and it is a `RCFC`-only addition here (see
//! [`crate::clock::RuleSet::includes_inauguration_day`]) because `RCFC
//! 6(a)(6)(A)` names it and `FRCP 6(a)(6)` does not. Subsection (c) states
//! the Sunday shift explicitly ("the next succeeding day selected for ...
//! observance ... is a legal public holiday"); it is silent on Saturday.
//! **Interpretive choice (flagged, not silently assumed):** this module
//! applies (b)'s general Saturday-to-Friday shift to Inauguration Day too,
//! since (c) makes it "a legal public holiday" and (b) shifts any such
//! holiday that falls on a Saturday — but (b) and (c) are not stitched
//! together in the statute's own text for this specific case, so treat the
//! Saturday shift as the best-available reading rather than verified text.
//! (Next occurrence: 2029-01-20, a Saturday.)

use std::collections::BTreeSet;

use super::date::{Date, MONDAY, SATURDAY, SUNDAY, THURSDAY};
use super::RuleSet;

fn observed(d: Date) -> Date {
    match d.weekday() {
        SATURDAY => d.add_days(-1),
        SUNDAY => d.add_days(1),
        _ => d,
    }
}

/// First year Juneteenth National Independence Day was a legal public
/// holiday (Pub. L. 117-17, effective June 17, 2021).
pub const JUNETEENTH_FIRST_YEAR: i32 = 2021;

/// First year Martin Luther King, Jr. Day was observed (Pub. L. 98-144,
/// effective 1986).
pub const MLK_DAY_FIRST_YEAR: i32 = 1986;

/// The eleven `5 U.S.C. § 6103(a)` holidays, common to every rule set,
/// as *observed* dates for `year` (fewer before 2021 and 1986; see the
/// module doc).
#[must_use]
pub fn federal_holidays(year: i32) -> BTreeSet<Date> {
    let fixed = [
        (Date::from_ymd(year, 1, 1), true), // New Year's Day
        (Date::from_ymd(year, 6, 19), year >= JUNETEENTH_FIRST_YEAR), // Juneteenth
        (Date::from_ymd(year, 7, 4), true), // Independence Day
        (Date::from_ymd(year, 11, 11), true), // Veterans Day
        (Date::from_ymd(year, 12, 25), true), // Christmas Day
    ];
    let floating = [
        (
            Date::nth_weekday_of_month(year, 1, MONDAY, 3),
            year >= MLK_DAY_FIRST_YEAR,
        ), // MLK Day
        (Date::nth_weekday_of_month(year, 2, MONDAY, 3), true), // Washington's Birthday
        (Date::nth_weekday_of_month(year, 5, MONDAY, -1), true), // Memorial Day
        (Date::nth_weekday_of_month(year, 9, MONDAY, 1), true), // Labor Day
        (Date::nth_weekday_of_month(year, 10, MONDAY, 2), true), // Columbus Day
        (Date::nth_weekday_of_month(year, 11, THURSDAY, 4), true), // Thanksgiving
    ];
    let in_force = |(d, yes): (crate::error::Result<Date>, bool)| d.ok().filter(|_| yes);
    fixed
        .into_iter()
        .filter_map(in_force)
        .map(observed)
        .chain(floating.into_iter().filter_map(in_force))
        .collect()
}

/// Inauguration Day (`5 U.S.C. § 6103(c)`; `RCFC 6(a)(6)(A)`), if `year` is
/// one of the quadrennial observance years (every fourth year after 1965:
/// 1969, 1973, ..., 2025, 2029, ...).
#[must_use]
pub fn inauguration_day(year: i32) -> Option<Date> {
    if year <= 1965 || (year - 1965) % 4 != 0 {
        return None;
    }
    let d = Date::from_ymd(year, 1, 20).ok()?;
    Some(match d.weekday() {
        SUNDAY => d.add_days(1),
        SATURDAY => d.add_days(-1), // interpretive choice; see module doc
        _ => d,
    })
}

/// Every observed holiday for `rule_set` in `year`: the common eleven, plus
/// Inauguration Day when `rule_set` includes it.
#[must_use]
pub fn holidays_for(rule_set: RuleSet, year: i32) -> BTreeSet<Date> {
    let mut set = federal_holidays(year);
    if rule_set.includes_inauguration_day() {
        if let Some(d) = inauguration_day(year) {
            set.insert(d);
        }
    }
    set
}

/// True if `date` is a legal holiday under `rule_set`: the computed federal
/// set (year-scoped, so this also picks up a holiday observed in the
/// adjacent year — e.g. New Year's Day observed Dec 31) plus `additional`
/// (state-declared or presidential/congressional holidays).
#[must_use]
pub fn is_legal_holiday(date: Date, rule_set: RuleSet, additional: &[Date]) -> bool {
    let (year, _, _) = date.ymd();
    // A year's own holidays, plus the neighboring years' in case a fixed
    // holiday's weekend observance spilled into Dec 31 / Jan 2.
    [year - 1, year, year + 1]
        .into_iter()
        .any(|y| holidays_for(rule_set, y).contains(&date))
        || additional.contains(&date)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eleven_holidays_in_an_ordinary_year() {
        assert_eq!(federal_holidays(2026).len(), 11);
    }

    #[test]
    fn christmas_2021_observed_friday() {
        // Dec 25, 2021 is a Saturday -> observed Friday Dec 24.
        assert!(federal_holidays(2021).contains(&Date::from_ymd(2021, 12, 24).unwrap()));
        assert!(!federal_holidays(2021).contains(&Date::from_ymd(2021, 12, 25).unwrap()));
    }

    #[test]
    fn juneteenth_2021_observed_friday_before_the_saturday_date() {
        // June 19, 2021 is a Saturday -> observed Friday June 18 (5 U.S.C.
        // § 6103(b)); the observed date, not the raw statutory date, is
        // what a `Rule 6(a)(6)` deadline roll actually checks.
        assert!(is_legal_holiday(
            Date::from_ymd(2021, 6, 18).unwrap(),
            RuleSet::Frcp6,
            &[]
        ));
        assert!(!is_legal_holiday(
            Date::from_ymd(2021, 6, 19).unwrap(),
            RuleSet::Frcp6,
            &[]
        ));
    }

    #[test]
    fn juneteenth_is_not_a_holiday_before_2021() {
        // June 20, 2011 (a Monday) would be "Juneteenth observed" under
        // today's calendar; in 2011 it was an ordinary business day.
        assert!(!is_legal_holiday(
            Date::from_ymd(2011, 6, 20).unwrap(),
            RuleSet::Itc210,
            &[]
        ));
        assert_eq!(federal_holidays(2020).len(), 10);
        assert_eq!(federal_holidays(2021).len(), 11);
    }

    #[test]
    fn mlk_day_is_a_holiday_from_1986() {
        assert!(!is_legal_holiday(
            Date::from_ymd(1985, 1, 21).unwrap(),
            RuleSet::Frcp6,
            &[]
        ));
        assert!(is_legal_holiday(
            Date::from_ymd(1986, 1, 20).unwrap(),
            RuleSet::Frcp6,
            &[]
        ));
        assert_eq!(federal_holidays(1985).len(), 9);
    }

    #[test]
    fn inauguration_day_only_under_rcfc() {
        // 2029, not 2025: in 2025 Inauguration Day and MLK Day both fell on
        // Jan 20, which would make this test pass for the wrong reason.
        let jan19_2029 = inauguration_day(2029).unwrap(); // observed Friday, see below
        assert!(is_legal_holiday(jan19_2029, RuleSet::Rcfc6, &[]));
        assert!(!is_legal_holiday(jan19_2029, RuleSet::Frcp6, &[]));
        assert!(!is_legal_holiday(jan19_2029, RuleSet::Frap26, &[]));
    }

    #[test]
    fn inauguration_day_2029_falls_on_saturday_observed_friday() {
        // 2029-01-20 is a Saturday; the interpretive Saturday shift lands
        // observance on 2029-01-19 (Friday).
        let d = inauguration_day(2029).unwrap();
        assert_eq!(d.to_iso(), "2029-01-19");
    }

    #[test]
    fn inauguration_day_absent_in_non_quadrennial_years() {
        assert!(inauguration_day(2026).is_none());
        assert!(inauguration_day(2027).is_none());
    }

    #[test]
    fn a_random_weekday_is_not_a_holiday() {
        assert!(!is_legal_holiday(
            Date::from_ymd(2026, 3, 16).unwrap(),
            RuleSet::Frcp6,
            &[]
        ));
    }
}
