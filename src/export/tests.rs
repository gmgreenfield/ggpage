use super::*;
use crate::table::{Cell, Table};

fn sample() -> Table {
    let mut t = Table::new(vec!["id".into(), "name".into(), "note".into()]);
    t.push_row(vec![
        Cell::Text("1".into()),
        Cell::Text("alice".into()),
        Cell::Null,
    ]);
    t.push_row(vec![
        Cell::Text("2".into()),
        Cell::Text("b, \"quoted\"".into()),
        Cell::Text("line1\nline2".into()),
    ]);
    t
}

#[test]
fn csv_quotes_special_fields_and_blanks_null() {
    let out = export(&sample(), Format::Csv, "t");
    // Header, then NULL rendered as an empty field.
    assert!(out.starts_with("id,name,note\n1,alice,\n"));
    // Comma, quotes and newline force quoting; embedded quotes doubled; the
    // newline is preserved inside the quoted field.
    assert!(out.contains("2,\"b, \"\"quoted\"\"\",\"line1\nline2\"\n"));
}

#[test]
fn markdown_has_header_separator_and_escapes_pipes() {
    let mut t = Table::new(vec!["a".into(), "b".into()]);
    t.push_row(vec![Cell::Text("x|y".into()), Cell::Null]);
    let out = export(&t, Format::Markdown, "t");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "| a | b |");
    assert_eq!(lines[1], "| --- | --- |");
    assert_eq!(lines[2], "| x\\|y |  |");
}

#[test]
fn json_is_array_of_objects_with_real_null() {
    let out = export(&sample(), Format::Json, "t");
    assert!(out.starts_with("[\n"));
    assert!(out.contains("\"id\": \"1\""));
    assert!(out.contains("\"note\": null")); // NULL → JSON null
    assert!(out.contains("\\n")); // newline escaped in value
    assert!(out.trim_end().ends_with(']'));
}

#[test]
fn insert_quotes_strings_numbers_bare_and_null_keyword() {
    let out = export(&sample(), Format::Insert, "users");
    assert!(out.starts_with("INSERT INTO `users` (`id`, `name`, `note`) VALUES\n"));
    // id is numeric → unquoted; name quoted with doubled apostrophes if any; NULL keyword
    assert!(out.contains("  (1, 'alice', NULL),\n"));
    // apostrophes escaped; last row terminated with ';'
    assert!(out.contains("'b, \"quoted\"'"));
    assert!(out.trim_end().ends_with(';'));
}

#[test]
fn insert_escapes_single_quotes() {
    let mut t = Table::new(vec!["x".into()]);
    t.push_row(vec![Cell::Text("O'Brien".into())]);
    let out = export(&t, Format::Insert, "t");
    assert!(out.contains("('O''Brien');"));
}

#[test]
fn numeric_literal_detection() {
    assert!(is_numeric_literal("42"));
    assert!(is_numeric_literal("-3.14"));
    assert!(is_numeric_literal("0"));
    assert!(!is_numeric_literal(""));
    assert!(!is_numeric_literal("-"));
    assert!(!is_numeric_literal("1.2.3"));
    assert!(!is_numeric_literal("12a"));
    assert!(!is_numeric_literal("NULL"));
}

#[test]
fn empty_table_insert_is_a_comment() {
    let t = Table::new(vec!["a".into()]);
    let out = export(&t, Format::Insert, "t");
    assert!(out.starts_with("-- no rows"));
}

#[test]
fn format_metadata() {
    assert_eq!(Format::Csv.extension(), "csv");
    assert_eq!(Format::Json.label(), "JSON");
}
