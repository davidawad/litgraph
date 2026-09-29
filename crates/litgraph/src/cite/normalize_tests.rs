// SPDX-License-Identifier: GPL-3.0-or-later
//! Unit tests for `normalize.rs`.

use super::*;

fn one(raw: &str, forum: Option<&str>) -> CiteRef {
    let mut refs = parse_cite_string(raw, forum);
    assert_eq!(
        refs.len(),
        1,
        "expected exactly one cite in {raw:?}: {refs:?}"
    );
    refs.remove(0)
}

// The five forms named in the bead brief -----------------------------

#[test]
fn frcp_short_form() {
    let c = one("FRCP 12(b)(6)", None);
    assert_eq!(c.family, Family::Frcp);
    assert_eq!(c.section, "12");
    assert_eq!(c.subsections, vec!["b", "6"]);
    assert_eq!(c.heading().as_deref(), Some("Rule 12"));
}

#[test]
fn frcp_long_form_matches_short_form() {
    let long = one("Fed. R. Civ. P. 12(b)(6)", None);
    let short = one("FRCP 12(b)(6)", None);
    assert_eq!(long.family, short.family);
    assert_eq!(long.section, short.section);
    assert_eq!(long.subsections, short.subsections);
}

#[test]
fn usc_28_with_section_symbol() {
    let c = one("28 U.S.C. § 1498(a)", None);
    assert_eq!(c.family, Family::Usc(28));
    assert_eq!(c.section, "1498");
    assert_eq!(c.subsections, vec!["a"]);
    assert_eq!(c.heading().as_deref(), Some("28 U.S.C. § 1498"));
}

#[test]
fn usc_35_bare_abbreviation() {
    let c = one("35 USC 315(e)", None);
    assert_eq!(c.family, Family::Usc(35));
    assert_eq!(c.section, "315");
    assert_eq!(c.subsections, vec!["e"]);
}

#[test]
fn cfr_37_with_section_symbol() {
    let c = one("37 C.F.R. § 42.108", None);
    assert_eq!(c.family, Family::Cfr(37));
    assert_eq!(c.section, "42.108");
    assert!(c.subsections.is_empty());
    assert_eq!(c.heading().as_deref(), Some("37 C.F.R. § 42.108"));
}

#[test]
fn rcfc_bare_rule_number() {
    let c = one("RCFC 56", None);
    assert_eq!(c.family, Family::Rcfc);
    assert_eq!(c.section, "56");
    assert_eq!(c.heading().as_deref(), Some("RCFC 56"));
}

// Equivalent forms should normalize identically -----------------------

#[test]
fn cfr_no_dots_no_section_symbol() {
    let c = one("37 CFR 1.104", None);
    assert_eq!(c.family, Family::Cfr(37));
    assert_eq!(c.section, "1.104");
}

#[test]
fn usc_double_section_symbol() {
    let refs = parse_cite_string("28 U.S.C. §§ 1292(a)(1), 1292(b)", None);
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0].family, Family::Usc(28));
    assert_eq!(refs[0].section, "1292");
    assert_eq!(refs[0].subsections, vec!["a", "1"]);
    assert_eq!(refs[1].family, Family::Usc(28));
    assert_eq!(refs[1].section, "1292");
    assert_eq!(refs[1].subsections, vec!["b"]);
}

// Compound citations: carry-forward and subsection continuation -------

#[test]
fn semicolon_separated_carries_title_forward() {
    let refs = parse_cite_string("28 U.S.C. 1291; 2106", None);
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0].heading().as_deref(), Some("28 U.S.C. § 1291"));
    assert_eq!(refs[1].family, Family::Usc(28));
    assert_eq!(refs[1].section, "2106");
}

#[test]
fn semicolon_separated_carries_rule_family_forward() {
    let refs = parse_cite_string("FRAP 28; 34", None);
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[1].family, Family::Frap);
    assert_eq!(refs[1].section, "34");
}

#[test]
fn trailing_subsection_continuation() {
    let refs = parse_cite_string("18 U.S.C. § 3142(b), (c)", None);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].section, "3142");
    assert_eq!(refs[0].subsections, vec!["b", "c"]);
}

#[test]
fn mixed_families_in_one_string() {
    let refs = parse_cite_string("35 U.S.C. 314(a); 37 CFR 42.108", None);
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0].family, Family::Usc(35));
    assert_eq!(refs[1].family, Family::Cfr(37));
    assert_eq!(refs[1].section, "42.108");
}

// Out-of-scope shapes: recognized, not treated as malformed rule cites -

#[test]
fn case_citation_is_recognized_and_out_of_scope() {
    let c = one("Bowles v. Russell, 551 U.S. 205 (2007)", None);
    assert_eq!(c.family, Family::Case);
    assert!(c.is_out_of_scope());
    assert!(c.heading().is_none());
}

#[test]
fn case_citation_does_not_poison_carry_forward() {
    // A trailing bare cite after a case name should NOT inherit the
    // case "family" (there isn't one) — it falls back to Other.
    let refs = parse_cite_string(
        "Bowles v. Russell, 551 U.S. 205 (2007); 28 U.S.C. § 2107",
        None,
    );
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0].family, Family::Case);
    assert_eq!(refs[1].family, Family::Usc(28));
}

#[test]
fn doctrinal_shorthand_is_other_not_a_crash() {
    let c = one("Fintiv factor 4 (parallel litigation status)", None);
    assert_eq!(c.family, Family::Other);
    assert!(c.is_out_of_scope());
}

#[test]
fn sup_ct_rule_is_other() {
    let c = one("Sup. Ct. R. 10", None);
    assert_eq!(c.family, Family::Other);
}

// ITC's internal cfr-slug shorthand, forum-gated -----------------------

#[test]
fn itc_cfr_slug_resolves_with_itc_forum() {
    let c = one("cfr210.16.b.4", Some("itc"));
    assert_eq!(c.family, Family::Cfr(19));
    assert_eq!(c.section, "210.16");
    assert_eq!(c.subsections, vec!["b", "4"]);
    assert_eq!(c.heading().as_deref(), Some("19 C.F.R. § 210.16"));
}

#[test]
fn itc_cfr_slug_is_other_without_itc_forum() {
    let c = one("cfr210.16.b.4", None);
    assert_eq!(c.family, Family::Other);
}

// MPEP -----------------------------------------------------------------

#[test]
fn mpep_section() {
    let c = one("MPEP 2106", None);
    assert_eq!(c.family, Family::Mpep);
    assert_eq!(c.section, "2106");
    assert_eq!(c.heading().as_deref(), Some("MPEP § 2106"));
}

// Misc edge cases --------------------------------------------------------

#[test]
fn empty_and_whitespace_only_segments_are_dropped() {
    assert!(parse_cite_string("", None).is_empty());
    assert!(parse_cite_string("  ; ; ", None).is_empty());
}

#[test]
fn unrecognized_shape_passes_through_as_other() {
    let c = one("frcp/r45-objections-waived", None);
    assert_eq!(c.family, Family::Other);
    assert_eq!(c.raw, "frcp/r45-objections-waived");
}

#[test]
fn bare_number_helper_agrees_with_parse_one() {
    assert!(bare_number("2106").is_some());
    assert!(bare_number("314(d)").is_some());
    assert!(bare_number("U.S.S.G.").is_none());
}

#[test]
fn bare_number_rejects_empty_and_subsection_with_no_leading_section() {
    assert!(bare_number("").is_none());
    assert!(bare_number("   ").is_none());
    // A subsection chain with nothing in front of it ("(b)(6)") has no
    // section number to anchor to.
    assert!(bare_number("(b)(6)").is_none());
}

// heading() for every family that resolves against the corpus -----------

#[test]
fn heading_covers_every_resolvable_family() {
    assert_eq!(one("FRAP 4", None).heading().as_deref(), Some("FRAP 4"));
    assert_eq!(
        one("Fed. R. Crim. P. 12", None).heading().as_deref(),
        Some("Fed. R. Crim. P. 12")
    );
    assert_eq!(
        one("Fed. Cir. R. 36", None).heading().as_deref(),
        Some("Fed. Cir. R. 36")
    );
}

// A rule cite whose family prefix matches but supplies no rule number ---

#[test]
fn rule_prefix_with_no_number_is_other_not_a_panic() {
    // "FRCP (b)(6)" — the rule number was dropped, leaving only
    // subsections; `try_rule_family` must fail closed to `Other`, not
    // panic or silently invent a section.
    let c = one("FRCP (b)(6)", None);
    assert_eq!(c.family, Family::Other);
}

// A trailing comma leaves an empty piece to skip, not a phantom cite ----

#[test]
fn trailing_comma_produces_no_phantom_citation() {
    let refs = parse_cite_string("35 U.S.C. 314(a),", None);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].section, "314");
}

// ITC cfr-slug malformed shapes -----------------------------------------

#[test]
fn itc_cfr_slug_rejects_non_numeric_part() {
    let c = one("cfrPART.10.a", Some("itc"));
    assert_eq!(c.family, Family::Other);
}

#[test]
fn itc_cfr_slug_rejects_missing_section_number() {
    let c = one("cfr210.", Some("itc"));
    assert_eq!(c.family, Family::Other);
}

// A family prefix that requires an internal space doesn't match when the
// input omits it (no space-collapsing false positive).
#[test]
fn rule_prefix_missing_required_space_does_not_match() {
    let c = one("Fed.R.Civ.P. 12", None);
    assert_eq!(c.family, Family::Other);
}

// Range decomposition -----------------------------------------------------

#[test]
fn plain_integer_range_expands_to_every_member() {
    let refs = parse_cite_string("FRAP 28-31", None);
    let sections: Vec<&str> = refs.iter().map(|c| c.section.as_str()).collect();
    assert_eq!(sections, vec!["28", "29", "30", "31"]);
    assert!(refs.iter().all(|c| c.family == Family::Frap));
    assert_eq!(refs[0].heading().as_deref(), Some("FRAP 28"));
    assert_eq!(refs[3].heading().as_deref(), Some("FRAP 31"));
}

#[test]
fn dotted_range_with_repeated_prefix_expands() {
    let refs = parse_cite_string("37 C.F.R. §§ 42.120-42.121", None);
    let sections: Vec<&str> = refs.iter().map(|c| c.section.as_str()).collect();
    assert_eq!(sections, vec!["42.120", "42.121"]);
}

#[test]
fn dotted_range_with_dropped_prefix_on_high_side_expands() {
    // Legal writing shorthand: "42.120-121" instead of repeating "42.120-42.121".
    let refs = parse_cite_string("37 C.F.R. § 42.120-121", None);
    let sections: Vec<&str> = refs.iter().map(|c| c.section.as_str()).collect();
    assert_eq!(sections, vec!["42.120", "42.121"]);
}

#[test]
fn usc_range_expands() {
    let refs = parse_cite_string("18 U.S.C. §§ 3161-3162", None);
    let sections: Vec<&str> = refs.iter().map(|c| c.section.as_str()).collect();
    assert_eq!(sections, vec!["3161", "3162"]);
    assert!(refs.iter().all(|c| c.family == Family::Usc(18)));
}

#[test]
fn a_non_range_hyphenated_section_is_left_alone() {
    // Not a range at all (descending / mismatched prefix): don't guess.
    let c = one("37 C.F.R. § 90.3-42.1", None);
    assert_eq!(c.section, "90.3-42.1");
    assert_eq!(c.heading().as_deref(), Some("37 C.F.R. § 90.3-42.1"));
}

#[test]
fn a_cite_with_subsections_is_never_range_expanded() {
    // Ambiguous which member a subsection would bind to if the section part
    // were a range -- expand_range bails out on any non-empty subsections
    // rather than guessing. `parse_cite_string` returns exactly one CiteRef
    // here specifically because no expansion happened (`one()` panics if it
    // returns more than one).
    let c = one("35 U.S.C. § 315(e)", None);
    assert_eq!(c.section, "315");
    assert_eq!(c.subsections, vec!["e"]);
}

#[test]
fn an_oversized_range_is_left_alone() {
    let c = one("FRAP 1-500", None);
    assert_eq!(c.section, "1-500");
}

// ITC usc-slug shorthand ---------------------------------------------------

#[test]
fn itc_usc_slug_resolves_only_for_the_itc_forum() {
    let c = one("usc1337.b.1", Some("itc"));
    assert_eq!(c.family, Family::Usc(19));
    assert_eq!(c.section, "1337");
    assert_eq!(c.subsections, vec!["b", "1"]);
    assert_eq!(c.heading().as_deref(), Some("19 U.S.C. § 1337"));

    let unresolved = one("usc1337.b.1", None);
    assert_eq!(unresolved.family, Family::Other);
}

#[test]
fn itc_usc_slug_rejects_non_numeric_section() {
    let c = one("usc.b.1", Some("itc"));
    assert_eq!(c.family, Family::Other);
}

// Carry-forward chain-breaking --------------------------------------------

#[test]
fn an_unrelated_citation_species_breaks_the_carry_forward_chain() {
    // "Sup. Ct. R. 13.1" isn't a recognized family and has its own text (not
    // a bare trailing number), so it must not leave `28 U.S.C.` active for
    // "13.3" to wrongly inherit -- both should resolve as out-of-scope, not
    // as "28 U.S.C. § 13.3".
    let refs = parse_cite_string("28 U.S.C. § 2101(c); Sup. Ct. R. 13.1, 13.3", None);
    assert_eq!(refs.len(), 3);
    assert_eq!(refs[0].family, Family::Usc(28));
    assert_eq!(refs[0].section, "2101");
    assert_eq!(refs[1].family, Family::Other);
    assert_eq!(refs[2].family, Family::Other);
    assert!(refs[1].is_out_of_scope() && refs[2].is_out_of_scope());
}
