//! Streaming parsers for SQL-client output.
//!
//! The [`Parser`] is a line-at-a-time state machine: feed it one line at a time
//! with [`Parser::push_line`] and it returns zero or more [`Event`]s as soon as
//! it can. This lets the UI render a result set while it is still streaming in,
//! and never requires buffering the whole input. Call [`Parser::finish`] at EOF
//! to flush any table still open.
//!
//! Three formats are recognized, interleaved freely with non-table lines:
//!   * **boxed**   — `+----+----+` / `| a | b |` (mysql/MariaDB default),
//!   * **vertical**— `\G` output delimited by `*** N. row ***`,
//!   * **TSV**     — tab-separated batch output (`mysql -B`).
//!
//! Anything not recognized as part of a table is emitted as [`Event::Passthrough`]
//! verbatim, so warnings, prompts and prose behave like plain `less`.
//!
//! Known limitations (inherent to parsing *rendered* text, not the wire
//! protocol): a literal value equal to `NULL` is indistinguishable from SQL
//! NULL and is treated as NULL; a value containing the column delimiter (`|` in
//! boxed, tab in TSV) can confuse splitting; leading/trailing spaces inside a
//! boxed cell are lost to padding.

use crate::table::{Cell, Row, Table};

#[cfg(test)]
mod tests;

/// An incremental parse result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A new result set began; carries its column names.
    StartTable { columns: Vec<String> },
    /// A data row for the currently open table.
    Row(Row),
    /// The open table ended; carries the verbatim footer line if present.
    EndTable { footer: Option<String> },
    /// A line that is not part of any table — emit it verbatim.
    Passthrough(String),
}

#[derive(Debug, PartialEq, Eq)]
enum State {
    Idle,
    /// Saw a top border; expecting the header row next.
    BoxTop,
    /// Saw top border + header row; expecting the separator border.
    BoxHeader,
    /// Inside a boxed table body, reading rows until the closing border.
    BoxRows,
    /// Saw the closing border; a footer line may follow.
    BoxClosed,
    /// Inside `\G` vertical output.
    Vertical,
    /// Inside tab-separated batch output.
    Tsv,
}

/// The streaming parser. Construct with [`Parser::new`].
pub struct Parser {
    state: State,
    /// Raw top border held until the table is confirmed (so it can be flushed
    /// as passthrough if the following lines don't actually form a table).
    pending_border: String,
    /// Header row held between seeing it and confirming the separator border.
    pending_header: String,
    /// Column names of the currently open table.
    columns: Vec<String>,
    /// Vertical mode: whether `StartTable` has been emitted yet.
    vert_started: bool,
    /// Vertical mode: fields of the record currently being accumulated.
    vert_fields: Vec<(String, Cell)>,
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser {
    pub fn new() -> Self {
        Parser {
            state: State::Idle,
            pending_border: String::new(),
            pending_header: String::new(),
            columns: Vec::new(),
            vert_started: false,
            vert_fields: Vec::new(),
        }
    }

    /// Feed one line (without its trailing newline). Returns the events it
    /// produced, if any.
    pub fn push_line(&mut self, line: &str) -> Vec<Event> {
        let mut out = Vec::new();
        self.process(line, &mut out);
        out
    }

    /// Flush any open table at end of input.
    pub fn finish(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        match self.state {
            State::BoxRows | State::BoxClosed | State::Tsv => {
                out.push(Event::EndTable { footer: None });
            }
            State::Vertical => {
                self.finalize_vertical_record(&mut out);
                out.push(Event::EndTable { footer: None });
            }
            State::BoxTop => {
                // Unconfirmed table: the lone border was just text.
                out.push(Event::Passthrough(std::mem::take(&mut self.pending_border)));
            }
            State::BoxHeader => {
                out.push(Event::Passthrough(std::mem::take(&mut self.pending_border)));
                out.push(Event::Passthrough(std::mem::take(&mut self.pending_header)));
            }
            State::Idle => {}
        }
        self.state = State::Idle;
        out
    }

    fn process(&mut self, line: &str, out: &mut Vec<Event>) {
        match self.state {
            State::Idle => self.process_idle(line, out),
            State::BoxTop => self.process_box_top(line, out),
            State::BoxHeader => self.process_box_header(line, out),
            State::BoxRows => self.process_box_rows(line, out),
            State::BoxClosed => self.process_box_closed(line, out),
            State::Vertical => self.process_vertical(line, out),
            State::Tsv => self.process_tsv(line, out),
        }
    }

    fn process_idle(&mut self, line: &str, out: &mut Vec<Event>) {
        if is_box_border(line) {
            self.pending_border = line.to_string();
            self.state = State::BoxTop;
        } else if is_vertical_marker(line) {
            self.vert_started = false;
            self.vert_fields.clear();
            self.state = State::Vertical;
            // The marker itself starts the first record; nothing to store yet.
        } else if looks_like_tsv_header(line) {
            self.columns = split_tsv(line);
            out.push(Event::StartTable {
                columns: self.columns.clone(),
            });
            self.state = State::Tsv;
        } else {
            out.push(Event::Passthrough(line.to_string()));
        }
    }

    fn process_box_top(&mut self, line: &str, out: &mut Vec<Event>) {
        if is_box_row(line) {
            self.pending_header = line.to_string();
            self.state = State::BoxHeader;
        } else {
            // Not a table after all; flush the border and reprocess this line.
            out.push(Event::Passthrough(std::mem::take(&mut self.pending_border)));
            self.state = State::Idle;
            self.process(line, out);
        }
    }

    fn process_box_header(&mut self, line: &str, out: &mut Vec<Event>) {
        if is_box_border(line) {
            // Confirmed: top border + header + separator.
            self.columns = split_box_row(&self.pending_header);
            self.pending_border.clear();
            self.pending_header.clear();
            out.push(Event::StartTable {
                columns: self.columns.clone(),
            });
            self.state = State::BoxRows;
        } else {
            out.push(Event::Passthrough(std::mem::take(&mut self.pending_border)));
            out.push(Event::Passthrough(std::mem::take(&mut self.pending_header)));
            self.state = State::Idle;
            self.process(line, out);
        }
    }

    fn process_box_rows(&mut self, line: &str, out: &mut Vec<Event>) {
        if is_box_border(line) {
            // Closing border; wait to see whether a footer follows.
            self.state = State::BoxClosed;
        } else if is_box_row(line) {
            let cells = split_box_row(line).into_iter().map(cell_from_token).collect();
            out.push(Event::Row(cells));
        } else {
            // Malformed/short table; close it and reprocess the stray line.
            out.push(Event::EndTable { footer: None });
            self.state = State::Idle;
            self.process(line, out);
        }
    }

    fn process_box_closed(&mut self, line: &str, out: &mut Vec<Event>) {
        if is_footer(line) {
            out.push(Event::EndTable {
                footer: Some(line.to_string()),
            });
            self.state = State::Idle;
        } else if is_box_border(line) {
            // A new table starts immediately after the previous one.
            out.push(Event::EndTable { footer: None });
            self.pending_border = line.to_string();
            self.state = State::BoxTop;
        } else {
            out.push(Event::EndTable { footer: None });
            self.state = State::Idle;
            self.process(line, out);
        }
    }

    fn process_vertical(&mut self, line: &str, out: &mut Vec<Event>) {
        if is_vertical_marker(line) {
            // Boundary between records: finalize the previous one.
            self.finalize_vertical_record(out);
        } else if is_footer(line) {
            self.finalize_vertical_record(out);
            out.push(Event::EndTable {
                footer: Some(line.to_string()),
            });
            self.state = State::Idle;
        } else if let Some((key, val)) = split_vertical_field(line) {
            self.vert_fields.push((key, cell_from_token(val)));
        } else if !self.vert_fields.is_empty() && !line.trim().is_empty() {
            // Continuation of the previous field's value (multi-line value).
            if let Some(last) = self.vert_fields.last_mut() {
                let mut text = last.1.text().to_string();
                text.push('\n');
                text.push_str(line);
                last.1 = Cell::Text(text);
            }
        } else {
            // Blank or unrecognized line ends the vertical block.
            self.finalize_vertical_record(out);
            out.push(Event::EndTable { footer: None });
            self.state = State::Idle;
            self.process(line, out);
        }
    }

    fn finalize_vertical_record(&mut self, out: &mut Vec<Event>) {
        if self.vert_fields.is_empty() {
            return;
        }
        let fields = std::mem::take(&mut self.vert_fields);
        if !self.vert_started {
            self.columns = fields.iter().map(|(k, _)| k.clone()).collect();
            out.push(Event::StartTable {
                columns: self.columns.clone(),
            });
            self.vert_started = true;
            out.push(Event::Row(fields.into_iter().map(|(_, v)| v).collect()));
        } else {
            // Align fields to the established column order.
            let mut row: Row = vec![Cell::Null; self.columns.len()];
            for (k, v) in fields {
                if let Some(i) = self.columns.iter().position(|c| *c == k) {
                    row[i] = v;
                }
            }
            out.push(Event::Row(row));
        }
    }

    fn process_tsv(&mut self, line: &str, out: &mut Vec<Event>) {
        if is_footer(line) {
            out.push(Event::EndTable {
                footer: Some(line.to_string()),
            });
            self.state = State::Idle;
        } else if line.contains('\t') {
            let cells = split_tsv(line).into_iter().map(cell_from_token).collect();
            out.push(Event::Row(cells));
        } else {
            // No tab: the batch block is over.
            out.push(Event::EndTable { footer: None });
            self.state = State::Idle;
            self.process(line, out);
        }
    }
}

// ---------------------------------------------------------------------------
// Line classification helpers
// ---------------------------------------------------------------------------

const HORIZ: &[char] = &['-', '─', '━', '═'];
const JUNCT: &[char] = &[
    '+', '┼', '┿', '╋', '┬', '┴', '├', '┤', '┌', '┐', '└', '┘', '╬', '╪', '╞', '╡', '╔', '╗',
    '╚', '╝', '╤', '╧', '╨', '╥',
];
const VBAR: &[char] = &['|', '│', '┃', '║'];

/// A horizontal border line like `+----+----+` (ASCII) or its Unicode
/// box-drawing equivalents (MariaDB). Every char must be a border/junction/
/// space and at least one must be a horizontal rule.
fn is_box_border(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    let mut has_horiz = false;
    for c in t.chars() {
        if HORIZ.contains(&c) {
            has_horiz = true;
        } else if JUNCT.contains(&c) || c == ' ' {
            // allowed
        } else {
            return false;
        }
    }
    has_horiz
}

/// A data/header row like `| a | b |`.
fn is_box_row(line: &str) -> bool {
    line.trim_start()
        .chars()
        .next()
        .is_some_and(|c| VBAR.contains(&c))
}

/// Split a boxed row into trimmed cell tokens, dropping the empty pieces
/// outside the outer bars.
fn split_box_row(line: &str) -> Vec<String> {
    let parts: Vec<&str> = line.trim().split(|c| VBAR.contains(&c)).collect();
    // The first and last pieces are outside the bounding bars.
    let inner = if parts.len() >= 2 {
        &parts[1..parts.len() - 1]
    } else {
        &parts[..]
    };
    inner.iter().map(|p| p.trim().to_string()).collect()
}

/// A `\G` record marker: `*************************** 1. row ***************************`.
fn is_vertical_marker(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('*') && t.ends_with('*') && t.contains(". row")
}

/// Split a `\G` field line `  name: value` into (key, value). The key is the
/// right-aligned field name (we left-trim it); the split is on the first
/// `": "`. Returns `None` if the line has no `": "`.
fn split_vertical_field(line: &str) -> Option<(String, &str)> {
    let idx = line.find(": ")?;
    let key = line[..idx].trim_start().to_string();
    let val = &line[idx + 2..];
    Some((key, val))
}

/// Heuristic: an Idle line that is tab-separated is treated as a TSV header.
/// Requires at least one tab so we don't eat ordinary prose.
fn looks_like_tsv_header(line: &str) -> bool {
    line.contains('\t')
}

fn split_tsv(line: &str) -> Vec<String> {
    line.split('\t').map(|s| s.to_string()).collect()
}

/// Match mysql/MariaDB status/footer lines kept verbatim as a table footer.
fn is_footer(line: &str) -> bool {
    let t = line.trim();
    if t.starts_with("Query OK") || t.starts_with("Empty set") {
        return true;
    }
    // "N row(s) in set ..." / "N row(s) affected ...".
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return false;
    }
    let rest = &t[digits..];
    rest.starts_with(" row") && (rest.contains("in set") || rest.contains("affected"))
}

/// Map a raw cell token to a [`Cell`], treating the literal `NULL` as SQL NULL.
fn cell_from_token<S: AsRef<str>>(token: S) -> Cell {
    let s = token.as_ref();
    if s == "NULL" {
        Cell::Null
    } else {
        Cell::Text(s.to_string())
    }
}

// ---------------------------------------------------------------------------
// Convenience: non-streaming collection (used by tests and the non-TTY path)
// ---------------------------------------------------------------------------

/// Drive the parser over a whole string and return the event stream.
pub fn parse_str(input: &str) -> Vec<Event> {
    let mut parser = Parser::new();
    let mut out = Vec::new();
    for line in split_lines(input) {
        out.extend(parser.push_line(line));
    }
    out.extend(parser.finish());
    out
}

/// Split input into lines without a spurious trailing empty line.
fn split_lines(input: &str) -> impl Iterator<Item = &str> {
    let trimmed = input.strip_suffix('\n').unwrap_or(input);
    if trimmed.is_empty() && input.is_empty() {
        // Empty input → no lines.
        return either_lines(None);
    }
    either_lines(Some(trimmed.split('\n')))
}

fn either_lines(it: Option<std::str::Split<'_, char>>) -> impl Iterator<Item = &str> {
    it.into_iter().flatten()
}

/// Collect the event stream into concrete [`Table`]s (ignoring passthrough
/// lines). Useful for tests and batch rendering.
pub fn collect_tables(events: &[Event]) -> Vec<Table> {
    let mut tables = Vec::new();
    let mut cur: Option<Table> = None;
    for ev in events {
        match ev {
            Event::StartTable { columns } => {
                cur = Some(Table::new(columns.clone()));
            }
            Event::Row(row) => {
                if let Some(t) = cur.as_mut() {
                    t.push_row(row.clone());
                }
            }
            Event::EndTable { footer } => {
                if let Some(mut t) = cur.take() {
                    t.footer = footer.clone();
                    tables.push(t);
                }
            }
            Event::Passthrough(_) => {}
        }
    }
    if let Some(t) = cur.take() {
        tables.push(t);
    }
    tables
}
