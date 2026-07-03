/// Unit tests for pure logic functions in uc-api (no DB, no HTTP).
/// Tests helpers.rs: split2, split3, validate_sql_name, now_ms
use uc_api::catalog_api::helpers::*;

// ── split2 ────────────────────────────────────────────────────────────────────

#[test]
fn split2_valid() {
    let (a, b) = split2("catalog.schema").unwrap();
    assert_eq!(a, "catalog");
    assert_eq!(b, "schema");
}

#[test]
fn split2_no_dot_returns_error() {
    assert!(split2("nodot").is_err());
}

#[test]
fn split2_multiple_dots_splits_on_first() {
    // splitn(2) → splits at first dot only
    let (a, b) = split2("a.b.c").unwrap();
    assert_eq!(a, "a");
    assert_eq!(b, "b.c");
}

#[test]
fn split2_leading_dot() {
    // empty first part
    let (a, b) = split2(".b").unwrap();
    assert_eq!(a, "");
    assert_eq!(b, "b");
}

// ── split3 ────────────────────────────────────────────────────────────────────

#[test]
fn split3_valid() {
    let (a, b, c) = split3("cat.sch.tbl").unwrap();
    assert_eq!(a, "cat");
    assert_eq!(b, "sch");
    assert_eq!(c, "tbl");
}

#[test]
fn split3_only_two_parts_errors() {
    assert!(split3("cat.sch").is_err());
}

#[test]
fn split3_four_parts_keeps_third_as_last() {
    // splitn(3) on "a.b.c.d" → ["a", "b", "c.d"]
    let (a, b, c) = split3("a.b.c.d").unwrap();
    assert_eq!(a, "a");
    assert_eq!(b, "b");
    assert_eq!(c, "c.d");
}

#[test]
fn split3_no_dots_errors() {
    assert!(split3("nodots").is_err());
}

// ── validate_sql_name ─────────────────────────────────────────────────────────

#[test]
fn valid_names_accepted() {
    for name in [
        "catalog",
        "my_schema",
        "Table123",
        "a",
        "CamelCase",
        "with-dash",
    ] {
        assert!(validate_sql_name(name).is_ok(), "Expected valid: {name}");
    }
}

#[test]
fn empty_name_rejected() {
    assert!(validate_sql_name("").is_err());
}

#[test]
fn name_with_dot_rejected() {
    assert!(validate_sql_name("cat.schema").is_err());
}

#[test]
fn name_with_slash_rejected() {
    assert!(validate_sql_name("bad/name").is_err());
}

#[test]
fn name_with_space_rejected() {
    assert!(validate_sql_name("has space").is_err());
}

#[test]
fn name_with_tab_rejected() {
    assert!(validate_sql_name("has\ttab").is_err());
}

#[test]
fn name_with_newline_rejected() {
    assert!(validate_sql_name("has\nnewline").is_err());
}

#[test]
fn name_exactly_255_chars_accepted() {
    let name = "a".repeat(255);
    assert!(validate_sql_name(&name).is_ok());
}

#[test]
fn name_256_chars_rejected() {
    let name = "a".repeat(256);
    assert!(validate_sql_name(&name).is_err());
}

#[test]
fn name_with_control_char_rejected() {
    assert!(validate_sql_name("bad\x01name").is_err());
}

// ── now_ms ────────────────────────────────────────────────────────────────────

#[test]
fn now_ms_returns_reasonable_timestamp() {
    let t = now_ms();
    // Should be after 2024-01-01 and before 2100-01-01
    assert!(t > 1_700_000_000_000, "timestamp too old: {t}");
    assert!(t < 4_000_000_000_000, "timestamp too far future: {t}");
}

#[test]
fn now_ms_monotonic() {
    let t1 = now_ms();
    let t2 = now_ms();
    assert!(t2 >= t1);
}
