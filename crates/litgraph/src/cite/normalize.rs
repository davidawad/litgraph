// SPDX-License-Identifier: GPL-3.0-or-later
//! Normalize the many surface forms of a legal citation ("FRCP 12(b)(6)",
//! "Fed. R. Civ. P. 12(b)(6)", "28 U.S.C. § 1498(a)", "35 USC 315(e)",
//! "37 C.F.R. § 42.108", "RCFC 56", ...) into a canonical [`CiteRef`] whose
//! [`CiteRef::heading`] matches the `## <ref>` section headings in the
//! vendored corpus (`sources/*.txt`, see `cite::corpus`).
//!
//! Pack `cite`/`authority` strings are often compound ("28 U.S.C. 1291;
//! 2106", "35 U.S.C. 314(a); 314(d)", "18 U.S.C. § 3142(b), (c)") and
//! sometimes aren't rule/statute citations at all (case names, "Sup. Ct.
//! R. 10", doctrinal shorthand like "Fintiv factor 4"). [`parse_cite_string`]
//! splits a raw string into one [`CiteRef`] per citation, carrying the
//! family/title of the previous citation forward onto a bare trailing
//! number the way legal writing does ("§§ 1292(a)(1), 1292(b)").

/// Which body of law a citation belongs to. The title number on
/// [`Family::Usc`]/[`Family::Cfr`] is what makes `28 U.S.C. § 1291` and
/// `35 U.S.C. § 1291` (hypothetically) distinct citations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Family {
    /// Federal Rules of Civil Procedure.
    Frcp,
    /// Federal Rules of Appellate Procedure.
    Frap,
    /// Federal Rules of Criminal Procedure.
    Frcrimp,
    /// Rules of the U.S. Court of Federal Claims.
    Rcfc,
    /// Federal Circuit local rules.
    FedCirRule,
    /// United States Code, title `.0`.
    Usc(u16),
    /// Code of Federal Regulations, title `.0`.
    Cfr(u16),
    /// Manual of Patent Examining Procedure.
    Mpep,
    /// A case citation ("*Bowles v. Russell*, 551 U.S. 205 (2007)") — not
    /// checkable against an L0 rule/statute source; recognized so it is
    /// skipped rather than misreported as a malformed rule cite.
    Case,
    /// Recognized text that isn't a rule/statute/case citation (a Supreme
    /// Court Rule, a sentencing guideline, doctrinal shorthand like "Fintiv
    /// factor 4") or text this parser doesn't understand at all.
    Other,
}

/// One normalized citation: a family plus a section/rule number and any
/// subsection chain (`"12(b)(6)"` → section `"12"`, subsections
/// `["b", "6"]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CiteRef {
    /// Body of law.
    pub family: Family,
    /// Rule/section number, as written (may contain a dot: `"1.53"`,
    /// `"42.108"`; may contain a range dash: `"302–303"`).
    pub section: String,
    /// Chained parenthetical/subsection markers, in order (`["d", "2", "B"]`).
    pub subsections: Vec<String>,
    /// The original text this was parsed from (for diagnostics).
    pub raw: String,
}

impl CiteRef {
    /// The canonical `## <heading>` key this citation resolves to in the
    /// vendored corpus, e.g. `"Rule 12"`, `"28 U.S.C. § 1291"`,
    /// `"37 C.F.R. § 42.108"`. `None` for [`Family::Case`]/[`Family::Other`],
    /// which have no corpus to resolve against.
    #[must_use]
    pub fn heading(&self) -> Option<String> {
        match self.family {
            Family::Frcp => Some(format!("Rule {}", self.section)),
            Family::Frap => Some(format!("FRAP {}", self.section)),
            Family::Frcrimp => Some(format!("Fed. R. Crim. P. {}", self.section)),
            Family::Rcfc => Some(format!("RCFC {}", self.section)),
            Family::FedCirRule => Some(format!("Fed. Cir. R. {}", self.section)),
            Family::Usc(t) if t > 0 => Some(format!("{t} U.S.C. § {}", self.section)),
            Family::Cfr(t) if t > 0 => Some(format!("{t} C.F.R. § {}", self.section)),
            Family::Mpep => Some(format!("MPEP § {}", self.section)),
            Family::Usc(_) | Family::Cfr(_) | Family::Case | Family::Other => None,
        }
    }

    /// Whether this citation is a kind [`parse_cite_string`] recognized but
    /// deliberately never verifies against the L0 corpus (a case citation,
    /// or text with no recognized rule/statute shape).
    #[must_use]
    pub fn is_out_of_scope(&self) -> bool {
        matches!(self.family, Family::Case | Family::Other)
    }
}

/// Split `raw` into one [`CiteRef`] per citation and normalize each.
///
/// `forum` is the pack's `forum` field (`"itc"`, `"cofc"`, ...); it
/// disambiguates the handful of forums that write citations without a
/// title number (ITC's internal `cfr210.16.b.4` shorthand for 19 C.F.R. §
/// 210.16(b)(4)).
#[must_use]
pub fn parse_cite_string(raw: &str, forum: Option<&str>) -> Vec<CiteRef> {
    let mut out = vec![];
    let mut last_family: Option<Family> = None;
    for segment in raw.split(';') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        // A case citation's own reporter cite ("551 U.S. 205 (2007)") is
        // comma-separated from the case name — never split it up.
        if looks_like_case_citation(segment) {
            out.push(parse_one(segment, last_family, forum));
            continue;
        }
        for piece in split_top_level_commas(segment) {
            let piece = piece.trim();
            if piece.is_empty() {
                continue;
            }
            if let Some(rest) = piece.strip_prefix('(') {
                // A bare parenthetical continuation ("(c)" in "§ 3142(b), (c)")
                // extends the previous CiteRef's subsection chain.
                if let Some(prev) = out.last_mut() {
                    let inner = rest.trim_end_matches(')').trim();
                    if !inner.is_empty() {
                        let cref: &mut CiteRef = prev;
                        cref.subsections.push(inner.to_string());
                    }
                    continue;
                }
            }
            let cref = parse_one(piece, last_family, forum);
            if !cref.is_out_of_scope() {
                last_family = Some(cref.family);
            }
            out.push(cref);
        }
    }
    out
}

/// Split on commas that are not inside `(...)` (so `"§ 3142(b), (c)"` splits
/// into `["§ 3142(b)", "(c)"]`, not something that severs the parenthetical).
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut out = vec![];
    let mut depth = 0i32;
    let mut start = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth <= 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

fn parse_one(piece: &str, last_family: Option<Family>, forum: Option<&str>) -> CiteRef {
    if let Some(cref) = try_rule_family(piece) {
        return cref;
    }
    if let Some(cref) = try_usc_or_cfr(piece) {
        return cref;
    }
    if let Some(cref) = try_itc_cfr_slug(piece, forum) {
        return cref;
    }
    if let Some(cref) = try_mpep(piece) {
        return cref;
    }
    if looks_like_case_citation(piece) {
        return CiteRef {
            family: Family::Case,
            section: piece.to_string(),
            subsections: vec![],
            raw: piece.to_string(),
        };
    }
    if let Some(family) = last_family.filter(|f| !f.is_carry_forward_blocked()) {
        if let Some((section, subsections)) = bare_number(piece) {
            return CiteRef {
                family,
                section,
                subsections,
                raw: piece.to_string(),
            };
        }
    }
    CiteRef {
        family: Family::Other,
        section: piece.to_string(),
        subsections: vec![],
        raw: piece.to_string(),
    }
}

impl Family {
    /// `Case`/`Other` never seed a "bare trailing number" carry-forward —
    /// a stray "10" after a case name isn't a page pointing back at the
    /// last real rule/statute family.
    fn is_carry_forward_blocked(self) -> bool {
        matches!(self, Family::Case | Family::Other)
    }
}

const FRCP_PREFIXES: &[&str] = &["FRCP", "Fed. R. Civ. P.", "Fed R Civ P", "F.R.C.P."];
const FRAP_PREFIXES: &[&str] = &["FRAP", "Fed. R. App. P.", "Fed R App P", "F.R.A.P."];
const FRCRIMP_PREFIXES: &[&str] = &["FRCrimP", "Fed. R. Crim. P.", "Fed R Crim P", "F.R.Crim.P."];
const RCFC_PREFIXES: &[&str] = &["RCFC"];
const FEDCIR_PREFIXES: &[&str] = &["Fed. Cir. R.", "Fed Cir R"];

fn try_rule_family(piece: &str) -> Option<CiteRef> {
    for (prefixes, family) in [
        (FRCP_PREFIXES, Family::Frcp),
        (FRAP_PREFIXES, Family::Frap),
        (FRCRIMP_PREFIXES, Family::Frcrimp),
        (RCFC_PREFIXES, Family::Rcfc),
        (FEDCIR_PREFIXES, Family::FedCirRule),
    ] {
        if let Some(rest) = strip_prefix_ci(piece, prefixes) {
            let rest = rest.trim_start_matches('§').trim();
            let (section, subsections) = bare_number(rest)?;
            return Some(CiteRef {
                family,
                section,
                subsections,
                raw: piece.to_string(),
            });
        }
    }
    None
}

fn try_mpep(piece: &str) -> Option<CiteRef> {
    let rest = strip_prefix_ci(piece, &["MPEP"])?;
    let rest = rest.trim_start_matches('§').trim();
    let (section, subsections) = bare_number(rest)?;
    Some(CiteRef {
        family: Family::Mpep,
        section,
        subsections,
        raw: piece.to_string(),
    })
}

/// `"28 U.S.C. § 1291"`, `"35 USC 315(e)"`, `"37 C.F.R. § 42.108"`, `"37 CFR
/// 1.104"` — a leading title number, a U.S.C./C.F.R. marker, an optional
/// §/§§, then a section number.
fn try_usc_or_cfr(piece: &str) -> Option<CiteRef> {
    let digit_end = piece.find(|c: char| !c.is_ascii_digit())?;
    if digit_end == 0 {
        return None;
    }
    let title: u16 = piece[..digit_end].parse().ok()?;
    let rest = piece[digit_end..].trim_start();
    let (make_family, rest): (fn(u16) -> Family, &str) =
        if let Some(r) = strip_prefix_ci(rest, &["U.S.C.", "USC"]) {
            (Family::Usc, r)
        } else {
            let r = strip_prefix_ci(rest, &["C.F.R.", "CFR"])?;
            (Family::Cfr, r)
        };
    let rest = rest
        .trim_start()
        .trim_start_matches("§§")
        .trim_start_matches('§')
        .trim_start();
    let (section, subsections) = bare_number(rest)?;
    Some(CiteRef {
        family: make_family(title),
        section,
        subsections,
        raw: piece.to_string(),
    })
}

/// `itc-337.json`'s own shorthand: `"cfr210.16.b.4"` = 19 C.F.R. §
/// 210.16(b)(4), title implied by the pack's `forum` (only resolved for
/// `forum == "itc"`, where the ITC's own part of 19 C.F.R. — Part 210 — is
/// the only body this shorthand is used for).
fn try_itc_cfr_slug(piece: &str, forum: Option<&str>) -> Option<CiteRef> {
    if forum != Some("itc") {
        return None;
    }
    let rest = strip_prefix_ci(piece, &["cfr"])?;
    let mut parts = rest.split('.');
    let part_no = parts.next()?;
    if part_no.is_empty() || !part_no.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let sec_no = parts.next().unwrap_or("");
    if sec_no.is_empty() {
        return None;
    }
    let subsections: Vec<String> = parts
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .collect();
    Some(CiteRef {
        family: Family::Cfr(19),
        section: format!("{part_no}.{sec_no}"),
        subsections,
        raw: piece.to_string(),
    })
}

/// Case-insensitive prefix match against any of `prefixes`, tolerant of runs
/// of whitespace in `s` where the prefix has a single space. Returns the
/// remainder. Byte-scans `s` directly (never slices by a prefix's byte
/// length), so it's safe on non-ASCII input like a following `§`.
fn strip_prefix_ci<'a>(s: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    for &p in prefixes {
        if let Some(off) = prefix_byte_len(s, p) {
            return Some(s[off..].trim_start());
        }
    }
    None
}

/// Case-insensitive, whitespace-tolerant match of literal `prefix` at the
/// start of `s`; returns how many bytes of `s` it consumed.
fn prefix_byte_len(s: &str, prefix: &str) -> Option<usize> {
    let sb = s.as_bytes();
    let pb = prefix.as_bytes();
    let mut si = 0usize;
    let mut pi = 0usize;
    while pi < pb.len() {
        if pb[pi] == b' ' {
            // one-or-more spaces in `s` for one space in `prefix`
            let mut n = 0;
            while si < sb.len() && sb[si] == b' ' {
                si += 1;
                n += 1;
            }
            if n == 0 {
                return None;
            }
            pi += 1;
            continue;
        }
        if si >= sb.len() || !sb[si].eq_ignore_ascii_case(&pb[pi]) {
            return None;
        }
        si += 1;
        pi += 1;
    }
    Some(si)
}

/// Parse `"12(b)(6)"` → `("12", ["b", "6"])`, `"1441"` → `("1441", [])`.
fn bare_number(s: &str) -> Option<(String, Vec<String>)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let first = s.chars().next()?;
    if !(first.is_ascii_digit() || s.starts_with('(')) {
        return None;
    }
    if let Some(idx) = s.find('(') {
        let section = s[..idx].trim().to_string();
        if section.is_empty() {
            return None;
        }
        let rest = s[idx..].trim();
        let inner = rest
            .strip_prefix('(')
            .and_then(|r| r.strip_suffix(')'))
            .unwrap_or(rest.trim_start_matches('(').trim_end_matches(')'));
        let subs: Vec<String> = inner
            .split(")(")
            .map(str::trim)
            .filter(|x| !x.is_empty())
            .map(str::to_string)
            .collect();
        Some((section, subs))
    } else {
        Some((s.to_string(), vec![]))
    }
}

fn looks_like_case_citation(s: &str) -> bool {
    s.contains(" v. ") || s.contains(" v ")
}

#[cfg(test)]
#[path = "normalize_tests.rs"]
mod tests;
