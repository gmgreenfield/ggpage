//! Field-by-field diff of two rows in a table. Pure and unit-tested.

use crate::table::{Cell, Table};
use crate::width::display_width;

fn show(cell: Option<&Cell>) -> String {
    match cell {
        Some(Cell::Null) | None => "NULL".to_string(),
        Some(Cell::Text(s)) => s.clone(),
    }
}

/// Produce human-readable lines comparing rows `a` and `b` of `table`,
/// column by column, marking the ones that differ with `*`.
pub fn diff_rows(table: &Table, a: usize, b: usize) -> Vec<String> {
    let row_a = table.rows.get(a);
    let row_b = table.rows.get(b);
    if row_a.is_none() || row_b.is_none() {
        return vec!["(invalid rows)".to_string()];
    }
    let row_a = row_a.unwrap();
    let row_b = row_b.unwrap();

    let col_w = table
        .columns
        .iter()
        .map(|c| display_width(c))
        .max()
        .unwrap_or(0)
        .max(6);

    let mut lines = vec![
        format!(
            "{:<cw$}   row {}      row {}",
            "column",
            a + 1,
            b + 1,
            cw = col_w
        ),
        String::new(),
    ];
    let mut differ = 0;
    for (i, col) in table.columns.iter().enumerate() {
        let va = show(row_a.get(i));
        let vb = show(row_b.get(i));
        let mark = if va != vb {
            differ += 1;
            '*'
        } else {
            ' '
        };
        lines.push(format!("{mark} {col:<col_w$}  {va}  →  {vb}"));
    }
    lines.push(String::new());
    lines.push(format!(
        "{differ} of {} columns differ",
        table.columns.len()
    ));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tbl() -> Table {
        let mut t = Table::new(vec!["id".into(), "name".into(), "note".into()]);
        t.push_row(vec![
            Cell::Text("1".into()),
            Cell::Text("alice".into()),
            Cell::Null,
        ]);
        t.push_row(vec![
            Cell::Text("2".into()),
            Cell::Text("alice".into()),
            Cell::Text("hi".into()),
        ]);
        t
    }

    #[test]
    fn marks_differing_columns() {
        let lines = diff_rows(&tbl(), 0, 1);
        let joined = lines.join("\n");
        // id differs, name same, note differs (NULL vs hi)
        assert!(joined.contains("* id"));
        assert!(joined.contains("  name")); // not marked
        assert!(joined.contains("* note"));
        assert!(joined.contains("NULL  →  hi"));
        assert!(joined.contains("2 of 3 columns differ"));
    }

    #[test]
    fn invalid_rows_reported() {
        let lines = diff_rows(&tbl(), 0, 9);
        assert_eq!(lines, vec!["(invalid rows)".to_string()]);
    }
}
