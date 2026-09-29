// SPDX-License-Identifier: GPL-3.0-or-later
//! Fuzzy substring matching for quoted phrases against vendored source
//! text. No external dependency: normalization + an exact-substring fast
//! path, falling back to a token-overlap (Dice coefficient) sliding-window
//! search so minor whitespace/punctuation drift between a pack's quoted
//! `note` and the vendored rule text doesn't read as unverifiable.

/// Result of a fuzzy match attempt.
#[derive(Debug, Clone, PartialEq)]
pub struct FuzzyMatch {
    /// 1.0 for an exact (post-normalization) substring match, otherwise the
    /// token-overlap score of the best window found.
    pub score: f64,
    /// The matching window of `haystack`, in its original (non-normalized)
    /// text, for a diagnostic message.
    pub snippet: String,
}

/// Below this token-overlap score, a window is not considered a match.
pub const DEFAULT_THRESHOLD: f64 = 0.7;

/// Lowercase, strip punctuation (keeping `§` and word characters), and
/// collapse whitespace — the shared normalization for both sides of a
/// fuzzy match so `"shall,"` and `"shall"` or a curly vs. straight quote
/// don't cost a match.
#[must_use]
pub fn normalize_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_was_space = true; // dedupe leading space too
    for c in s.chars() {
        if c.is_alphanumeric() || c == '§' {
            out.push(c.to_ascii_lowercase());
            last_was_space = false;
        } else if !last_was_space {
            // Every other character (whitespace, quotes, dashes, parens,
            // commas, ...) is a word separator, never dropped outright —
            // dropping "(" in "1498(a)" would glue two words into "1498a".
            out.push(' ');
            last_was_space = true;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

/// Search `haystack` for the best fuzzy match of `needle`. Returns `None`
/// if `needle` is empty or the best window scores below `threshold`.
#[must_use]
pub fn fuzzy_find(haystack: &str, needle: &str, threshold: f64) -> Option<FuzzyMatch> {
    let norm_needle = normalize_text(needle);
    if norm_needle.is_empty() {
        return None;
    }
    let norm_haystack = normalize_text(haystack);
    if let Some(pos) = norm_haystack.find(&norm_needle) {
        let snippet = extract_snippet(haystack, pos, norm_needle.len());
        return Some(FuzzyMatch {
            score: 1.0,
            snippet,
        });
    }

    let needle_words: Vec<&str> = norm_needle.split(' ').collect();
    let hay_words: Vec<&str> = norm_haystack.split(' ').collect();
    if needle_words.is_empty() || hay_words.is_empty() {
        return None;
    }
    let win = needle_words.len();
    if hay_words.len() < win {
        return score_bag(&needle_words, &hay_words)
            .filter(|&s| s >= threshold)
            .map(|score| FuzzyMatch {
                score,
                snippet: hay_words.join(" "),
            });
    }

    let mut best_score = 0.0;
    let mut best_start = 0usize;
    for start in 0..=(hay_words.len() - win) {
        let window = &hay_words[start..start + win];
        let score = score_bag(&needle_words, window).unwrap_or(0.0);
        if score > best_score {
            best_score = score;
            best_start = start;
        }
    }
    if best_score >= threshold {
        Some(FuzzyMatch {
            score: best_score,
            snippet: hay_words[best_start..best_start + win].join(" "),
        })
    } else {
        None
    }
}

/// Dice coefficient over word multisets: `2*|shared| / (|a| + |b|)`.
fn score_bag(a: &[&str], b: &[&str]) -> Option<f64> {
    if a.is_empty() || b.is_empty() {
        return None;
    }
    let mut b_remaining: Vec<&str> = b.to_vec();
    let mut shared = 0usize;
    for &w in a {
        if let Some(idx) = b_remaining.iter().position(|&x| x == w) {
            b_remaining.remove(idx);
            shared += 1;
        }
    }
    Some(2.0 * shared as f64 / (a.len() + b.len()) as f64)
}

/// Best-effort mapping of a normalized-text match position back onto a
/// short excerpt of the original `haystack`, for error messages. Not exact
/// (normalization changes byte offsets); walks by normalized-word-count
/// starting near `pos` instead of trying to invert the byte offset.
fn extract_snippet(original: &str, norm_pos: usize, norm_len: usize) -> String {
    // Cheap approximation, integer-only: normalized text is never longer
    // than the original per character, so a same-length window starting at
    // the same fractional offset (computed in integer arithmetic, not
    // float, to sidestep truncation/sign-loss casts) is a reasonable (if
    // approximate) excerpt.
    let norm_total = normalize_text(original).len().max(1);
    let start = original.len() * norm_pos / norm_total;
    let start = start.min(original.len());
    let end = (start + norm_len.max(1) * 2).min(original.len());
    // Snap to char boundaries.
    let start = ceil_char_boundary(original, start);
    let end = ceil_char_boundary(original, end.max(start));
    original[start..end].trim().to_string()
}

fn ceil_char_boundary(s: &str, mut i: usize) -> usize {
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i.min(s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_collapses_whitespace_and_punctuation() {
        assert_eq!(normalize_text("  Hello,   World!  "), "hello world");
        assert_eq!(normalize_text("shall\u{2014}not"), "shall not");
    }

    #[test]
    fn normalize_keeps_section_symbol() {
        assert_eq!(normalize_text("§ 1498(a)"), "§ 1498 a");
    }

    #[test]
    fn exact_match_scores_one() {
        let hay = "The court may, for good cause, extend the time.";
        let m = fuzzy_find(hay, "for good cause, extend the time", DEFAULT_THRESHOLD).unwrap();
        assert_eq!(m.score, 1.0);
    }

    #[test]
    fn near_exact_with_curly_quotes_and_extra_space_still_matches() {
        let hay = "the reply must be \u{201c}complete\u{201d} and filed within 30 days";
        let needle = "the reply must be complete and filed within 30 days";
        let m = fuzzy_find(hay, needle, DEFAULT_THRESHOLD).unwrap();
        assert_eq!(m.score, 1.0);
    }

    #[test]
    fn word_reorder_or_drift_falls_back_to_token_overlap() {
        let hay = "reasonable and entire compensation for the use of a patented invention";
        let needle = "reasonable and entire compensation for use of the patented invention";
        let m = fuzzy_find(hay, needle, DEFAULT_THRESHOLD).unwrap();
        assert!(
            m.score < 1.0,
            "expected a fuzzy (non-exact) match, got {}",
            m.score
        );
        assert!(m.score >= DEFAULT_THRESHOLD);
    }

    #[test]
    fn unrelated_text_does_not_match() {
        let hay = "the plaintiff must file a complaint stating the claim";
        let needle = "the defendant may remove the action to federal court";
        assert!(fuzzy_find(hay, needle, DEFAULT_THRESHOLD).is_none());
    }

    #[test]
    fn empty_needle_never_matches() {
        assert!(fuzzy_find("anything here", "", DEFAULT_THRESHOLD).is_none());
        assert!(fuzzy_find("anything here", "   ", DEFAULT_THRESHOLD).is_none());
    }

    #[test]
    fn needle_longer_than_haystack_is_handled() {
        let hay = "short text";
        let needle = "this needle has many more words than the haystack does at all";
        assert!(fuzzy_find(hay, needle, DEFAULT_THRESHOLD).is_none());
    }

    /// A needle longer than the haystack can still score above threshold if
    /// most of its words are present (the haystack is treated as one
    /// short window rather than never matching just because it's shorter).
    #[test]
    fn needle_longer_than_haystack_can_still_match() {
        let hay = "alpha beta gamma";
        let needle = "alpha beta gamma delta";
        let m = fuzzy_find(hay, needle, DEFAULT_THRESHOLD).unwrap();
        assert!(m.score >= DEFAULT_THRESHOLD);
        assert_eq!(m.snippet, "alpha beta gamma");
    }
}
