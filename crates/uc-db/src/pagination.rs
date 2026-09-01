//! Cursor pagination shared by every `list` in the repo layer.
//!
//! The logic was duplicated eighteen times — once per list function per backend
//! — and every copy carried the same underflow.

/// Split an over-fetched page into the rows to return and the next page token.
///
/// Callers fetch `max_results + 1` rows. If more than `max_results` came back
/// there is another page, and the token is the key of the last row returned, so
/// the next request's `WHERE key > token` resumes exactly after it.
pub fn page<T>(
    mut rows: Vec<T>,
    max_results: i64,
    key: impl Fn(&T) -> String,
) -> (Vec<T>, Option<String>) {
    // A non-positive limit has no page to describe. Guarding here is the point:
    // the previous `rows.get(max_results as usize - 1)` underflowed at
    // max_results = 0 — a panic in debug, and in release it wrapped to
    // usize::MAX so `get` returned None and pagination dead-ended with an empty
    // page and no token, which no caller could distinguish from "no more rows".
    let limit = usize::try_from(max_results.max(0)).unwrap_or(usize::MAX);
    if limit == 0 {
        return (Vec::new(), None);
    }

    let has_more = rows.len() > limit;
    rows.truncate(limit);
    let next = if has_more { rows.last().map(&key) } else { None };
    (rows, next)
}

/// How many rows to fetch for a page of `max_results`: one extra, so the caller
/// can tell whether another page exists.
///
/// Saturating, because `max_results as usize + 1` overflows for a negative
/// limit — `(-1i64) as usize` is usize::MAX, and adding one wraps. The SQL path
/// never hit this (it binds an i64 straight into LIMIT), so it is specific to
/// the in-memory scans.
pub fn over_fetch(max_results: i64) -> usize {
    usize::try_from(max_results.max(0))
        .unwrap_or(usize::MAX)
        .saturating_add(1)
}

#[cfg(test)]
mod tests {
    // Tests panic on purpose; see the note in the crate-level modules.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
    use super::*;

    fn names(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("row{i}")).collect()
    }

    #[test]
    fn a_full_page_yields_a_token_pointing_at_its_last_row() {
        // Caller asked for 2 and over-fetched 3.
        let (rows, next) = page(names(3), 2, |s| s.clone());
        assert_eq!(rows, vec!["row0", "row1"]);
        assert_eq!(next.as_deref(), Some("row1"), "token resumes after row1");
    }

    #[test]
    fn a_short_page_has_no_token() {
        let (rows, next) = page(names(2), 5, |s| s.clone());
        assert_eq!(rows.len(), 2);
        assert_eq!(next, None);
    }

    #[test]
    fn an_exactly_full_page_has_no_token() {
        // Over-fetch returned exactly max_results, so nothing follows.
        let (rows, next) = page(names(2), 2, |s| s.clone());
        assert_eq!(rows.len(), 2);
        assert_eq!(next, None);
    }

    #[test]
    fn a_non_positive_limit_does_not_underflow() {
        // This is the bug: `max_results as usize - 1` at 0.
        for limit in [0i64, -1, i64::MIN] {
            let (rows, next) = page(names(3), limit, |s| s.clone());
            assert!(rows.is_empty(), "limit {limit} must yield no rows");
            assert_eq!(next, None, "limit {limit} must yield no token");
        }
    }

    #[test]
    fn over_fetch_asks_for_one_more_and_never_overflows() {
        assert_eq!(over_fetch(50), 51);
        assert_eq!(over_fetch(0), 1);
        // `max_results as usize + 1` wrapped here.
        assert_eq!(over_fetch(-1), 1);
        assert_eq!(over_fetch(i64::MIN), 1);
        // The point is that it neither panics nor wraps to a small number;
        // i64::MAX + 1 fits in a 64-bit usize and only saturates on 32-bit.
        assert!(over_fetch(i64::MAX) >= usize::try_from(i64::MAX).unwrap_or(usize::MAX));
    }

    #[test]
    fn empty_input_is_empty_output() {
        let (rows, next) = page(Vec::<String>::new(), 10, |s: &String| s.clone());
        assert!(rows.is_empty());
        assert_eq!(next, None);
    }
}
