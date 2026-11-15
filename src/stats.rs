//! Per-column summary statistics: row count, null count, distinct count, and
//! min/max (numeric when every non-null value parses as a number, otherwise
//! lexicographic). Pure and unit-tested.

use std::collections::HashSet;

use crate::table::{Cell, Table};

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnStats {
    pub column: String,
    pub count: usize,
    pub nulls: usize,
    pub distinct: usize,
    pub numeric: bool,
    pub min: Option<String>,
    pub max: Option<String>,
}

/// Compute statistics for column `col` of `table`.
pub fn column_stats(table: &Table, col: usize) -> ColumnStats {
    let column = table.columns.get(col).cloned().unwrap_or_default();
    let mut nulls = 0;
    let mut distinct: HashSet<&str> = HashSet::new();
    let mut str_min: Option<&str> = None;
    let mut str_max: Option<&str> = None;
    let mut num_min: Option<f64> = None;
    let mut num_max: Option<f64> = None;
    let mut num_min_s: Option<&str> = None;
    let mut num_max_s: Option<&str> = None;
    let mut non_null = 0;
    let mut all_numeric = true;

    for row in &table.rows {
        match row.get(col) {
            Some(Cell::Null) | None => nulls += 1,
            Some(Cell::Text(s)) => {
                non_null += 1;
                distinct.insert(s.as_str());
                if str_min.is_none_or(|m| s.as_str() < m) {
                    str_min = Some(s);
                }
                if str_max.is_none_or(|m| s.as_str() > m) {
                    str_max = Some(s);
                }
                match s.trim().parse::<f64>() {
                    Ok(n) if all_numeric => {
                        if num_min.is_none_or(|m| n < m) {
                            num_min = Some(n);
                            num_min_s = Some(s);
                        }
                        if num_max.is_none_or(|m| n > m) {
                            num_max = Some(n);
                            num_max_s = Some(s);
                        }
                    }
                    _ => all_numeric = false,
                }
            }
        }
    }

    let numeric = all_numeric && non_null > 0;
    let (min, max) = if numeric {
        (num_min_s.map(String::from), num_max_s.map(String::from))
    } else {
        (str_min.map(String::from), str_max.map(String::from))
    };

    ColumnStats {
        column,
        count: table.rows.len(),
        nulls,
        distinct: distinct.len(),
        numeric,
        min,
        max,
    }
}

impl ColumnStats {
    /// Render as human-readable lines for a popup.
    pub fn lines(&self) -> Vec<String> {
        vec![
            format!("column:   {}", self.column),
            format!("rows:     {}", self.count),
            format!("nulls:    {}", self.nulls),
            format!("distinct: {}", self.distinct),
            format!(
                "type:     {}",
                if self.numeric { "numeric" } else { "text" }
            ),
            format!("min:      {}", self.min.as_deref().unwrap_or("—")),
            format!("max:      {}", self.max.as_deref().unwrap_or("—")),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tbl() -> Table {
        let mut t = Table::new(vec!["n".into(), "s".into()]);
        for (n, s) in [
            (Cell::Text("3".into()), Cell::Text("banana".into())),
            (Cell::Text("1".into()), Cell::Text("apple".into())),
            (Cell::Text("10".into()), Cell::Null),
            (Cell::Null, Cell::Text("apple".into())),
        ] {
            t.push_row(vec![n, s]);
        }
        t
    }

    #[test]
    fn numeric_column_uses_numeric_min_max() {
        let st = column_stats(&tbl(), 0);
        assert_eq!(st.count, 4);
        assert_eq!(st.nulls, 1);
        assert_eq!(st.distinct, 3); // "3","1","10"
        assert!(st.numeric);
        // numeric, so 10 > 3 (not lexicographic where "3" > "10")
        assert_eq!(st.min.as_deref(), Some("1"));
        assert_eq!(st.max.as_deref(), Some("10"));
    }

    #[test]
    fn text_column_uses_lexicographic_min_max_and_distinct() {
        let st = column_stats(&tbl(), 1);
        assert_eq!(st.nulls, 1);
        assert_eq!(st.distinct, 2); // "banana","apple"
        assert!(!st.numeric);
        assert_eq!(st.min.as_deref(), Some("apple"));
        assert_eq!(st.max.as_deref(), Some("banana"));
    }

    #[test]
    fn all_null_column() {
        let mut t = Table::new(vec!["x".into()]);
        t.push_row(vec![Cell::Null]);
        t.push_row(vec![Cell::Null]);
        let st = column_stats(&t, 0);
        assert_eq!(st.nulls, 2);
        assert_eq!(st.distinct, 0);
        assert!(!st.numeric);
        assert_eq!(st.min, None);
        assert_eq!(st.lines().len(), 7);
    }
}
