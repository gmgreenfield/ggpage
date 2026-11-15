//! Export a [`Table`] to CSV, Markdown, JSON, or SQL `INSERT` statements.
//! Pure string transforms — no I/O — so each format is unit-tested directly.
//!
//! NULL handling differs by format to match each one's conventions: CSV and
//! Markdown have no null literal (rendered empty / as `NULL` text), whereas JSON
//! (`null`) and `INSERT` (`NULL`) represent it faithfully.

use crate::table::{Cell, Table};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Csv,
    Markdown,
    Json,
    Insert,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Csv => "csv",
            Format::Markdown => "md",
            Format::Json => "json",
            Format::Insert => "sql",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Csv => "CSV",
            Format::Markdown => "Markdown",
            Format::Json => "JSON",
            Format::Insert => "INSERT",
        }
    }
}

/// Render `table` in `format`. `name` is the table name used for `INSERT`.
pub fn export(table: &Table, format: Format, name: &str) -> String {
    match format {
        Format::Csv => to_csv(table),
        Format::Markdown => to_markdown(table),
        Format::Json => to_json(table),
        Format::Insert => to_insert(table, name),
    }
}

// --- CSV -------------------------------------------------------------------

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn to_csv(table: &Table) -> String {
    let mut out = String::new();
    let header: Vec<String> = table.columns.iter().map(|c| csv_field(c)).collect();
    out.push_str(&header.join(","));
    out.push('\n');
    for row in &table.rows {
        let fields: Vec<String> = row
            .iter()
            .map(|c| match c {
                Cell::Null => String::new(),
                Cell::Text(s) => csv_field(s),
            })
            .collect();
        out.push_str(&fields.join(","));
        out.push('\n');
    }
    out
}

// --- Markdown --------------------------------------------------------------

fn md_field(s: &str) -> String {
    s.replace('|', "\\|").replace(['\n', '\r'], " ")
}

fn to_markdown(table: &Table) -> String {
    let mut out = String::new();
    let cols: Vec<String> = table.columns.iter().map(|c| md_field(c)).collect();
    out.push_str(&format!("| {} |\n", cols.join(" | ")));
    let sep: Vec<&str> = table.columns.iter().map(|_| "---").collect();
    out.push_str(&format!("| {} |\n", sep.join(" | ")));
    for row in &table.rows {
        let fields: Vec<String> = row
            .iter()
            .map(|c| match c {
                Cell::Null => String::new(),
                Cell::Text(s) => md_field(s),
            })
            .collect();
        out.push_str(&format!("| {} |\n", fields.join(" | ")));
    }
    out
}

// --- JSON ------------------------------------------------------------------

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn to_json(table: &Table) -> String {
    let mut out = String::from("[\n");
    for (r, row) in table.rows.iter().enumerate() {
        out.push_str("  {\n");
        for (i, col) in table.columns.iter().enumerate() {
            let value = match row.get(i) {
                Some(Cell::Null) | None => "null".to_string(),
                Some(Cell::Text(s)) => json_string(s),
            };
            let comma = if i + 1 < table.columns.len() { "," } else { "" };
            out.push_str(&format!("    {}: {}{}\n", json_string(col), value, comma));
        }
        let comma = if r + 1 < table.rows.len() { "," } else { "" };
        out.push_str(&format!("  }}{}\n", comma));
    }
    out.push(']');
    out
}

// --- INSERT ----------------------------------------------------------------

/// True if `s` is a bare numeric literal that can be emitted unquoted.
fn is_numeric_literal(s: &str) -> bool {
    let t = s.strip_prefix('-').unwrap_or(s);
    if t.is_empty() {
        return false;
    }
    let mut seen_dot = false;
    let mut digits = 0;
    for c in t.chars() {
        match c {
            '0'..='9' => digits += 1,
            '.' if !seen_dot => seen_dot = true,
            _ => return false,
        }
    }
    digits > 0
}

fn sql_value(cell: &Cell) -> String {
    match cell {
        Cell::Null => "NULL".to_string(),
        Cell::Text(s) if is_numeric_literal(s) => s.clone(),
        Cell::Text(s) => format!("'{}'", s.replace('\'', "''")),
    }
}

fn ident(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

fn to_insert(table: &Table, name: &str) -> String {
    if table.rows.is_empty() {
        return format!("-- no rows to insert into {}\n", ident(name));
    }
    let cols: Vec<String> = table.columns.iter().map(|c| ident(c)).collect();
    let mut out = format!("INSERT INTO {} ({}) VALUES\n", ident(name), cols.join(", "));
    for (r, row) in table.rows.iter().enumerate() {
        let vals: Vec<String> = row.iter().map(sql_value).collect();
        let terminator = if r + 1 < table.rows.len() { "," } else { ";" };
        out.push_str(&format!("  ({}){}\n", vals.join(", "), terminator));
    }
    out
}

#[cfg(test)]
mod tests;
