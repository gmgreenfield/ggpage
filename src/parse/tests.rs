use super::*;
use crate::table::Cell;

fn txt(s: &str) -> Cell {
    Cell::Text(s.to_string())
}

fn tables(input: &str) -> Vec<Table> {
    collect_tables(&parse_str(input))
}

// --- boxed format ----------------------------------------------------------

#[test]
fn boxed_basic() {
    let input = "\
+----+------+
| id | name |
+----+------+
|  1 | a    |
|  2 | b    |
+----+------+
2 rows in set (0.00 sec)
";
    let ts = tables(input);
    assert_eq!(ts.len(), 1);
    let t = &ts[0];
    assert_eq!(t.columns, vec!["id", "name"]);
    assert_eq!(
        t.rows,
        vec![vec![txt("1"), txt("a")], vec![txt("2"), txt("b")]]
    );
    assert_eq!(t.footer.as_deref(), Some("2 rows in set (0.00 sec)"));
}

#[test]
fn boxed_null_is_distinct_from_empty() {
    let input = "\
+------+-------+
| a    | b     |
+------+-------+
| NULL |       |
+------+-------+
1 row in set (0.00 sec)
";
    let t = &tables(input)[0];
    // NULL token → Cell::Null; empty cell → empty Text.
    assert_eq!(t.rows[0][0], Cell::Null);
    assert_eq!(t.rows[0][1], Cell::Text(String::new()));
    assert_ne!(t.rows[0][0], t.rows[0][1]);
}

#[test]
fn boxed_unicode_values() {
    let input = "\
+------+--------+
| id   | name   |
+------+--------+
| 1    | 日本語 |
+------+--------+
1 row in set
";
    let t = &tables(input)[0];
    assert_eq!(t.rows[0][1], txt("日本語"));
}

#[test]
fn mariadb_unicode_box_borders() {
    // MariaDB can render with Unicode box-drawing characters.
    let input = "\
┌────┬──────┐
│ id │ name │
├────┼──────┤
│  1 │ a    │
└────┴──────┘
1 row in set (0.001 sec)
";
    let t = &tables(input)[0];
    assert_eq!(t.columns, vec!["id", "name"]);
    assert_eq!(t.rows, vec![vec![txt("1"), txt("a")]]);
    assert_eq!(t.footer.as_deref(), Some("1 row in set (0.001 sec)"));
}

#[test]
fn multiple_result_sets() {
    let input = "\
+----+
| a  |
+----+
|  1 |
+----+
1 row in set (0.00 sec)

+----+
| b  |
+----+
|  2 |
+----+
1 row in set (0.00 sec)
";
    let ts = tables(input);
    assert_eq!(ts.len(), 2);
    assert_eq!(ts[0].columns, vec!["a"]);
    assert_eq!(ts[1].columns, vec!["b"]);
    assert_eq!(ts[1].rows, vec![vec![txt("2")]]);
}

#[test]
fn query_ok_and_empty_set_footers() {
    assert!(is_footer("Query OK, 0 rows affected (0.01 sec)"));
    assert!(is_footer("Empty set (0.00 sec)"));
    assert!(is_footer("5 rows in set (0.00 sec)"));
    assert!(is_footer("1 row in set"));
    assert!(is_footer("3 rows affected (0.02 sec)"));
    assert!(!is_footer("this is not a footer"));
    assert!(!is_footer("| 1 | a |"));
}

// --- vertical \G format ----------------------------------------------------

#[test]
fn vertical_basic() {
    let input = "\
*************************** 1. row ***************************
  id: 1
name: alice
*************************** 2. row ***************************
  id: 2
name: bob
2 rows in set (0.00 sec)
";
    let ts = tables(input);
    assert_eq!(ts.len(), 1);
    let t = &ts[0];
    assert_eq!(t.columns, vec!["id", "name"]);
    assert_eq!(
        t.rows,
        vec![vec![txt("1"), txt("alice")], vec![txt("2"), txt("bob")]]
    );
    assert_eq!(t.footer.as_deref(), Some("2 rows in set (0.00 sec)"));
}

#[test]
fn vertical_null_value() {
    let input = "\
*************************** 1. row ***************************
  id: 1
name: NULL
1 row in set (0.00 sec)
";
    let t = &tables(input)[0];
    assert_eq!(t.rows[0][1], Cell::Null);
}

#[test]
fn vertical_multiline_value() {
    let input = "\
*************************** 1. row ***************************
   id: 1
 note: line one
line two
1 row in set
";
    let t = &tables(input)[0];
    assert_eq!(t.columns, vec!["id", "note"]);
    assert_eq!(t.rows[0][1], txt("line one\nline two"));
}

// --- TSV batch format ------------------------------------------------------

#[test]
fn tsv_basic() {
    let input = "id\tname\n1\talice\n2\tbob\n";
    let ts = tables(input);
    assert_eq!(ts.len(), 1);
    let t = &ts[0];
    assert_eq!(t.columns, vec!["id", "name"]);
    assert_eq!(
        t.rows,
        vec![vec![txt("1"), txt("alice")], vec![txt("2"), txt("bob")]]
    );
}

#[test]
fn tsv_null_token() {
    let input = "id\tname\n1\tNULL\n";
    let t = &tables(input)[0];
    assert_eq!(t.rows[0][1], Cell::Null);
}

#[test]
fn tsv_ends_at_blank_line() {
    let input = "id\tname\n1\ta\n\nnot a table";
    let ts = tables(input);
    assert_eq!(ts.len(), 1);
    assert_eq!(ts[0].rows.len(), 1);
}

// --- passthrough & mixed ---------------------------------------------------

#[test]
fn plain_text_passes_through() {
    let events = parse_str("just some text\nanother line\n");
    assert_eq!(
        events,
        vec![
            Event::Passthrough("just some text".to_string()),
            Event::Passthrough("another line".to_string()),
        ]
    );
    assert!(collect_tables(&events).is_empty());
}

#[test]
fn warning_line_before_table_passes_through() {
    let input = "\
Warning: something
+----+
| a  |
+----+
|  1 |
+----+
1 row in set
";
    let events = parse_str(input);
    assert_eq!(
        events[0],
        Event::Passthrough("Warning: something".to_string())
    );
    let ts = collect_tables(&events);
    assert_eq!(ts.len(), 1);
    assert_eq!(ts[0].rows, vec![vec![txt("1")]]);
}

#[test]
fn lone_border_like_line_is_passthrough() {
    // A separator that isn't followed by a table body should not be eaten.
    let events = parse_str("+------+\nnot a header\n");
    assert_eq!(
        events,
        vec![
            Event::Passthrough("+------+".to_string()),
            Event::Passthrough("not a header".to_string()),
        ]
    );
}

#[test]
fn empty_input_yields_nothing() {
    assert!(parse_str("").is_empty());
}

// --- streaming behavior ----------------------------------------------------

#[test]
fn streaming_emits_rows_before_eof() {
    // Rows must surface as they arrive, without waiting for the closing border.
    let mut p = Parser::new();
    assert!(p.push_line("+----+").is_empty()); // top border held
    assert!(p.push_line("| a  |").is_empty()); // header held
    let ev = p.push_line("+----+"); // separator confirms table
    assert_eq!(
        ev,
        vec![Event::StartTable {
            columns: vec!["a".into()]
        }]
    );
    let ev = p.push_line("|  1 |");
    assert_eq!(ev, vec![Event::Row(vec![txt("1")])]);
    let ev = p.push_line("|  2 |");
    assert_eq!(ev, vec![Event::Row(vec![txt("2")])]);
    // EOF before a closing border still closes the table cleanly.
    assert!(p.push_line("+----+").is_empty());
    let ev = p.finish();
    assert_eq!(ev, vec![Event::EndTable { footer: None }]);
}

#[test]
fn finish_flushes_open_table_without_footer() {
    let input = "\
+----+
| a  |
+----+
|  1 |";
    // No closing border, no footer, no trailing newline.
    let t = &tables(input)[0];
    assert_eq!(t.rows, vec![vec![txt("1")]]);
    assert_eq!(t.footer, None);
}
