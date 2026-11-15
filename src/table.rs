//! The in-memory table model produced by the parsers and consumed by the view.
//!
//! A single SQL client invocation can emit several result sets (e.g. a script
//! with multiple statements). Each becomes a [`Table`]. The model keeps NULL
//! distinct from the empty string, which matters both for rendering and for the
//! phase-2 export/stats features.

/// One field value in a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cell {
    /// SQL NULL — rendered distinctly from an empty string.
    Null,
    /// A textual value exactly as it appeared in the client output.
    Text(String),
}

impl Cell {
    /// The raw text of this cell, or `""` for NULL. Prefer [`Cell::is_null`]
    /// plus explicit handling when the NULL/empty distinction matters.
    pub fn text(&self) -> &str {
        match self {
            Cell::Null => "",
            Cell::Text(s) => s,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Cell::Null)
    }
}

/// A row is just its cells; it is always the same length as the table's
/// column list (parsers pad short rows with NULL / empty as appropriate).
pub type Row = Vec<Cell>;

/// One result set: a header, zero or more rows, and the verbatim footer line
/// the client printed (e.g. `"5 rows in set (0.00 sec)"`), if any.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Table {
    pub columns: Vec<String>,
    pub rows: Vec<Row>,
    /// Trailing status line kept verbatim (`N rows in set`, `Query OK`, …).
    pub footer: Option<String>,
}

impl Table {
    pub fn new(columns: Vec<String>) -> Self {
        Table {
            columns,
            rows: Vec::new(),
            footer: None,
        }
    }

    pub fn ncols(&self) -> usize {
        self.columns.len()
    }

    pub fn nrows(&self) -> usize {
        self.rows.len()
    }

    /// Append a row, normalizing its length to the column count: short rows are
    /// padded with NULL, over-long rows are truncated.
    pub fn push_row(&mut self, mut row: Row) {
        let n = self.columns.len();
        if row.len() < n {
            row.resize(n, Cell::Null);
        } else if row.len() > n {
            row.truncate(n);
        }
        self.rows.push(row);
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Cell {
        Cell::Text(s.to_string())
    }

    #[test]
    fn null_is_distinct_from_empty_string() {
        assert_ne!(Cell::Null, Cell::Text(String::new()));
        assert!(Cell::Null.is_null());
        assert!(!Cell::Text(String::new()).is_null());
        assert_eq!(Cell::Null.text(), "");
        assert_eq!(Cell::Text(String::new()).text(), "");
    }

    #[test]
    fn push_row_pads_short_rows_with_null() {
        let mut table = Table::new(vec!["a".into(), "b".into(), "c".into()]);
        table.push_row(vec![t("1")]);
        assert_eq!(table.rows[0], vec![t("1"), Cell::Null, Cell::Null]);
    }

    #[test]
    fn push_row_truncates_long_rows() {
        let mut table = Table::new(vec!["a".into()]);
        table.push_row(vec![t("1"), t("2")]);
        assert_eq!(table.rows[0], vec![t("1")]);
    }

    #[test]
    fn dimensions() {
        let mut table = Table::new(vec!["a".into(), "b".into()]);
        table.push_row(vec![t("1"), t("2")]);
        assert_eq!(table.ncols(), 2);
        assert_eq!(table.nrows(), 1);
        assert!(!table.is_empty());
    }
}
