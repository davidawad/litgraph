// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared clock types: which time-computation regime governs, service
//! methods that add days, and the diagnostic result shape every regime
//! returns.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::date::Date;

/// Which forum's time-computation rule governs a deadline.
///
/// Sources (fetched 2026-09-28):
/// - `Frcp6`: <https://www.law.cornell.edu/rules/frcp/rule_6>
/// - `Rcfc6`: <https://www.uscfc.uscourts.gov/sites/cfc/files/Rules%207.27.2026.pdf>
///   (current rules, effective 2026-07-27), Rule 6 — text-for-text identical
///   to FRCP 6(a)/(d) after the 2009/2016/2023 restyling amendments, except
///   `RCFC 6(a)(6)(A)` also lists **Inauguration Day** (`5 U.S.C. § 6103(c)`:
///   January 20 of each fourth year after 1965) as a legal holiday — FRCP
///   6(a)(6) does not.
/// - `Frap26`: <https://www.law.cornell.edu/rules/frap/rule_26> — the day
///   count/roll mechanics in 26(a)(1) are identical to FRCP 6(a)(1), but the
///   26(c) 3-day service extension fires on a *different* set of service
///   methods (mail, third-party commercial carrier, prison mailing system —
///   not "leaving with the clerk" or "other means consented to"), and FRAP
///   has no 6(a)(2)-style hours provision or 6(a)(3)-style clerk
///   inaccessibility extension.
/// - `Itc210`: <https://www.law.cornell.edu/cfr/text/19/210.6> incorporating
///   <https://www.law.cornell.edu/cfr/text/19/201.14> and
///   <https://www.law.cornell.edu/cfr/text/19/201.16> — a genuinely
///   different algorithm (business-day anchor, business-day counting for
///   periods under 7 days); see `clock::itc` module doc.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuleSet {
    /// Federal Rules of Civil Procedure, Rule 6.
    Frcp6,
    /// Rules of the Court of Federal Claims, Rule 6.
    Rcfc6,
    /// Federal Rules of Appellate Procedure, Rule 26 (governs the Federal
    /// Circuit too, via `Fed. Cir. R. 47.1` incorporating the FRAP).
    Frap26,
    /// 19 CFR 210.6(a) (ITC Section 337 investigations).
    Itc210,
}

impl RuleSet {
    /// Short citation used in step traces and error messages.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            RuleSet::Frcp6 => "FRCP 6",
            RuleSet::Rcfc6 => "RCFC 6",
            RuleSet::Frap26 => "Fed. R. App. P. 26",
            RuleSet::Itc210 => "19 CFR 210.6(a)",
        }
    }

    /// The primary source URL fetched to author this rule set.
    #[must_use]
    pub fn source_url(self) -> &'static str {
        match self {
            RuleSet::Frcp6 => "https://www.law.cornell.edu/rules/frcp/rule_6",
            RuleSet::Rcfc6 => "https://www.uscfc.uscourts.gov/rules",
            RuleSet::Frap26 => "https://www.law.cornell.edu/rules/frap/rule_26",
            RuleSet::Itc210 => "https://www.law.cornell.edu/cfr/text/19/210.6",
        }
    }

    /// True for the one rule set whose legal-holiday list includes
    /// Inauguration Day (`RCFC 6(a)(6)(A)`; `5 U.S.C. § 6103(c)`).
    #[must_use]
    pub fn includes_inauguration_day(self) -> bool {
        matches!(self, RuleSet::Rcfc6)
    }

    /// True for the rule sets that define an hours-based period
    /// (`FRCP`/`RCFC 6(a)(2)`). FRAP 26 and 19 CFR 210 have no hours
    /// provision in the fetched text.
    #[must_use]
    pub fn supports_hours(self) -> bool {
        matches!(self, RuleSet::Frcp6 | RuleSet::Rcfc6)
    }

    /// True for the rule sets with a clerk's-office-inaccessibility
    /// extension (`FRCP`/`RCFC 6(a)(3)`).
    #[must_use]
    pub fn supports_clerk_inaccessibility(self) -> bool {
        matches!(self, RuleSet::Frcp6 | RuleSet::Rcfc6)
    }
}

/// Direction a day-counted period runs.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// "N days after" the trigger (the ordinary case; every pack-authored
    /// `deadline` is forward).
    #[default]
    Forward,
    /// "At least N days before" the trigger (`FRCP 6(a)(5)`'s backward
    /// counting; not currently authored on any pack edge, but a real rule
    /// shape — e.g. RCFC Appendix C's pre-filing notice window).
    Backward,
}

/// Calendar days versus business ("court") days, per `Deadline.unit` in
/// `model::schema` (`docs/PACK_SCHEMA.md`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    /// Every day counts (the modern FRCP/RCFC/FRAP default since the 2009
    /// restyling).
    #[default]
    Calendar,
    /// Only business days count, and the day landed on is a business day by
    /// construction (no separate last-day roll is needed). Not itself a
    /// numbered subsection of FRCP/RCFC/FRAP — a generic pack-authored
    /// escape hatch for a forum whose *own* local rule counts business
    /// days, modeled here as: exclude the trigger day, then advance `n`
    /// business days (skipping weekends/holidays of the governing rule
    /// set's calendar), landing on the `n`th business day itself.
    Court,
}

/// Service methods that add calendar days before the last-day roll, unioned
/// across every rule set. Each rule set accepts only its own subset —
/// see `general::service_days` and `itc::service_days`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceMethod {
    /// `FRCP`/`RCFC 5(b)(2)(C)`; `FRAP 25`; domestic mail under `19 CFR
    /// 201.16(d)`.
    Mail,
    /// `FRCP`/`RCFC 5(b)(2)(D)` — leaving with the clerk.
    Clerk,
    /// `FRCP`/`RCFC 5(b)(2)(F)` — other means consented to.
    OtherConsented,
    /// `FRAP 26(c)` — third-party commercial carrier.
    CommercialCarrier,
    /// `FRAP 26(c)` — prison mailing system.
    PrisonMail,
    /// `19 CFR 201.16(d)` — mail to a foreign address (+10 days, vs. +3
    /// domestic).
    MailForeign,
    /// `19 CFR 201.16(e)` — domestic express delivery (+1 day).
    Express,
    /// `19 CFR 201.16(e)` — express delivery to a foreign destination (+5
    /// days).
    ExpressForeign,
}

/// A day-counted (not hours-based) deadline computation request.
#[derive(Debug, Clone)]
pub struct DayDeadline {
    /// The triggering event date. Always excluded from the count.
    pub trigger: Date,
    /// Length of the period in days (non-negative integer).
    pub days: i64,
    /// Forward ("after") or backward ("before") counting.
    pub direction: Direction,
    /// Calendar or business-day counting.
    pub unit: Unit,
    /// Service method adding days before the roll, if any.
    pub service_method: Option<ServiceMethod>,
}

/// Extra calendar configuration.
#[derive(Debug, Clone, Default)]
pub struct ClockConfig {
    /// State-declared or presidential/congressional holidays beyond the
    /// computed federal set (`FRCP`/`RCFC 6(a)(6)(B)`/`(C)`; `FRAP
    /// 26(a)(6)`).
    pub additional_holidays: Vec<Date>,
    /// Clerk's office inaccessible on the last day (`FRCP`/`RCFC 6(a)(3)`).
    /// Ignored (with a warning from the caller) for rule sets where
    /// [`RuleSet::supports_clerk_inaccessibility`] is false.
    pub clerk_inaccessible: bool,
}

/// The computed due date, plus a step-by-step trace for teaching/audit.
#[derive(Debug, Clone, Serialize)]
pub struct ComputedDeadline {
    /// Which rule set computed this.
    pub rule_set: RuleSet,
    /// The date before any counting (the trigger date itself).
    pub anchor_date: String,
    /// The date after counting and any service extension, but before the
    /// final last-day roll (`FRCP`/`RCFC 6`'s `baseDate` convention: the
    /// service extension is added to the *unrolled* count, then the whole
    /// thing is rolled once — see `general::apply_service`'s doc comment).
    pub base_date: String,
    /// The final due date, after every step.
    pub due_date: String,
    /// Whether the final date differs from the pre-service-extension,
    /// pre-roll anchor+count purely because of weekend/holiday rolling.
    pub last_day_rolled: bool,
    /// Whether a service-method extension was applied.
    pub service_days_added: bool,
    /// Whether clerk-inaccessibility extension was applied.
    pub clerk_inaccessibility_applied: bool,
    /// Ordered, human-readable computation steps (each cites its rule).
    pub steps: Vec<String>,
}

/// An hours-based deadline computation request (`FRCP`/`RCFC 6(a)(2)`).
#[derive(Debug, Clone)]
pub struct HourDeadline {
    /// Exact trigger instant, as whole seconds since the Unix epoch
    /// (UTC-less / calendar-local, matching the TS port).
    pub trigger_epoch_seconds: i64,
    /// Number of hours (non-negative integer).
    pub hours: i64,
}

/// The computed due instant for an hours-based period.
#[derive(Debug, Clone, Serialize)]
pub struct ComputedHourDeadline {
    /// The due date (date portion only).
    pub due_date: String,
    /// The full due instant, `"YYYY-MM-DDTHH:MM:SS"`.
    pub due_datetime: String,
    /// Whether the period rolled past a weekend/holiday.
    pub last_day_rolled: bool,
    /// Ordered, human-readable computation steps.
    pub steps: Vec<String>,
}
