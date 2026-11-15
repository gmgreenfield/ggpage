//! Pure layout and rendering for the table view. No terminal I/O lives here:
//! [`render`] turns a [`Table`] plus a [`Viewport`] into a structured grid of
//! styled spans, which the thin `tui` layer blits to the screen. Keeping this
//! pure makes the pinned header, frozen columns, horizontal scrolling and chop
//! behavior fully unit-testable.

use crate::table::{Cell, Table};
use crate::width::{display_width, pad_to_width};

/// How a span should be styled by the terminal layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    /// A pinned header cell.
    Header,
    /// The header cell of the current (cursor) column.
    HeaderCursor,
    /// An ordinary data cell.
    Data,
    /// A NULL value (rendered distinctly from an empty string).
    Null,
    /// The separator between two columns.
    Separator,
    /// The separator right after the frozen column block.
    FrozenSep,
}

/// A run of text with a single style.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub kind: SpanKind,
}

impl Span {
    fn new(text: impl Into<String>, kind: SpanKind) -> Self {
        Span {
            text: text.into(),
            kind,
        }
    }
}

/// One rendered line (header or data row).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RenderLine {
    pub spans: Vec<Span>,
    /// True when this data line is the current search match (tui highlights it).
    pub highlight: bool,
}

impl RenderLine {
    /// Flatten to plain text (for tests / passthrough).
    pub fn plain(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }
}

/// The result of laying out one frame.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Render {
    pub header: RenderLine,
    pub rows: Vec<RenderLine>,
}

/// Running maximum display width per column, updated as rows stream in so we
/// never rescan the whole result set. Observed widths are clamped to a ceiling
/// so a single huge cell can't blow up the layout math.
#[derive(Debug, Clone, Default)]
pub struct Widths {
    natural: Vec<usize>,
}

/// Hard ceiling on an observed column's natural width.
const WIDTH_CEIL: usize = 500;
/// Default cap applied to each column when chop (truncation) is on.
pub const DEFAULT_CHOP_CAP: usize = 48;

impl Widths {
    pub fn new(ncols: usize) -> Self {
        Widths {
            natural: vec![0; ncols],
        }
    }

    fn grow_to(&mut self, n: usize) {
        if self.natural.len() < n {
            self.natural.resize(n, 0);
        }
    }

    pub fn observe_header(&mut self, columns: &[String]) {
        self.grow_to(columns.len());
        for (i, c) in columns.iter().enumerate() {
            self.natural[i] = self.natural[i].max(display_width(c).min(WIDTH_CEIL));
        }
    }

    pub fn observe_row(&mut self, row: &[Cell]) {
        self.grow_to(row.len());
        for (i, cell) in row.iter().enumerate() {
            let w = display_width(render_cell_text(cell));
            self.natural[i] = self.natural[i].max(w.min(WIDTH_CEIL));
        }
    }

    /// Effective per-column widths for rendering: capped when `chop` is on.
    pub fn effective(&self, chop: bool, cap: usize) -> Vec<usize> {
        self.natural
            .iter()
            .map(|&w| if chop { w.min(cap) } else { w })
            .collect()
    }
}

/// The scroll/display state of the table view. Pure data; the app owns one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewport {
    /// Index of the first visible data row.
    pub top: usize,
    /// Number of *scrollable* columns scrolled past (frozen cols excluded).
    pub hscroll: usize,
    /// Number of frozen leading columns, always visible.
    pub frozen: usize,
    /// `-S`: when true, long cells are truncated to the cap width.
    pub chop: bool,
}

impl Default for Viewport {
    fn default() -> Self {
        Viewport {
            top: 0,
            hscroll: 0,
            frozen: 1,
            chop: true,
        }
    }
}

impl Viewport {
    /// Largest valid `top` given the data row count and how many data rows fit.
    fn max_top(nrows: usize, page: usize) -> usize {
        nrows.saturating_sub(page.max(1))
    }

    /// Largest valid `hscroll`: keep at least one scrollable column on screen.
    fn max_hscroll(&self, ncols: usize) -> usize {
        ncols.saturating_sub(self.frozen).saturating_sub(1)
    }

    /// Move vertically by `delta` rows (negative = up), clamped.
    pub fn scroll_v(&mut self, delta: isize, nrows: usize, page: usize) {
        let new = self.top as isize + delta;
        let max = Self::max_top(nrows, page) as isize;
        self.top = new.clamp(0, max.max(0)) as usize;
    }

    pub fn goto_top(&mut self) {
        self.top = 0;
    }

    pub fn goto_bottom(&mut self, nrows: usize, page: usize) {
        self.top = Self::max_top(nrows, page);
    }

    /// Move horizontally by `delta` columns (negative = left), clamped.
    pub fn scroll_h(&mut self, delta: isize, ncols: usize) {
        let new = self.hscroll as isize + delta;
        let max = self.max_hscroll(ncols) as isize;
        self.hscroll = new.clamp(0, max.max(0)) as usize;
    }

    pub fn h_home(&mut self) {
        self.hscroll = 0;
    }

    pub fn h_end(&mut self, ncols: usize) {
        self.hscroll = self.max_hscroll(ncols);
    }

    /// Ensure `row` is visible, scrolling the viewport minimally (used by search).
    pub fn reveal_row(&mut self, row: usize, nrows: usize, page: usize) {
        if row < self.top {
            self.top = row;
        } else if page > 0 && row >= self.top + page {
            self.top = row + 1 - page;
        }
        self.top = self.top.min(Self::max_top(nrows, page));
    }
}

/// Render text for a cell: NULL shows as the literal `NULL`.
fn render_cell_text(cell: &Cell) -> &str {
    match cell {
        Cell::Null => "NULL",
        Cell::Text(s) => s,
    }
}

/// Column separator (3 display columns wide).
const SEP: &str = " │ ";
const SEP_W: usize = 3;

/// Decide which columns are visible and how much width each gets, honoring
/// frozen columns, horizontal scroll, and the available width. The final
/// visible column may be allotted less than its full width (and thus truncated).
/// Returns `(column_index, allotted_width, is_last_frozen)`.
fn visible_columns(
    widths: &[usize],
    frozen: usize,
    hscroll: usize,
    total_w: usize,
) -> Vec<(usize, usize, bool)> {
    let n = widths.len();
    let frozen = frozen.min(n);
    let order: Vec<usize> = (0..frozen).chain((frozen + hscroll)..n).collect();

    let mut out = Vec::new();
    let mut used = 0usize;
    for (i, &col) in order.iter().enumerate() {
        let sep = if i == 0 { 0 } else { SEP_W };
        if used + sep >= total_w {
            break;
        }
        used += sep;
        let avail = total_w - used;
        let w = widths[col].min(avail);
        let is_last_frozen = frozen > 0 && i == frozen - 1;
        out.push((col, w, is_last_frozen));
        used += w;
        if w < widths[col] {
            break; // this column was truncated; no room for more
        }
    }
    out
}

fn build_line(
    cells: &[String],
    layout: &[(usize, usize, bool)],
    header: bool,
    cursor: Option<usize>,
) -> RenderLine {
    let mut spans = Vec::new();
    for (i, &(col, w, last_frozen)) in layout.iter().enumerate() {
        if i > 0 {
            let kind = if layout[i - 1].2 {
                SpanKind::FrozenSep
            } else {
                SpanKind::Separator
            };
            spans.push(Span::new(SEP, kind));
        }
        let raw = cells.get(col).map(String::as_str).unwrap_or("");
        let is_null = raw == NULL_SENTINEL;
        let text = pad_to_width(if is_null { "NULL" } else { raw }, w);
        let kind = if header {
            if cursor == Some(col) {
                SpanKind::HeaderCursor
            } else {
                SpanKind::Header
            }
        } else if is_null {
            SpanKind::Null
        } else {
            SpanKind::Data
        };
        spans.push(Span::new(text, kind));
        let _ = last_frozen;
    }
    RenderLine {
        spans,
        highlight: false,
    }
}

/// Sentinel used to pass "this cell is NULL" through the string-based builder.
const NULL_SENTINEL: &str = "\u{0}NULL\u{0}";

/// Lay out one frame of the table into a [`Render`].
///
/// `area_w`/`area_h` are the width and height (in cells) available to the table,
/// where the header consumes one of the `area_h` lines. `highlight_row` marks a
/// data row index (e.g. the current search match) for the tui to emphasize.
pub fn render(
    table: &Table,
    widths: &[usize],
    vp: &Viewport,
    area_w: usize,
    area_h: usize,
    highlight_row: Option<usize>,
    cursor_col: Option<usize>,
) -> Render {
    let layout = visible_columns(widths, vp.frozen, vp.hscroll, area_w);

    let header_cells = table.columns.clone();
    let header = build_line(&header_cells, &layout, true, cursor_col);

    let data_rows = area_h.saturating_sub(1);
    let mut rows = Vec::with_capacity(data_rows);
    for r in 0..data_rows {
        let idx = vp.top + r;
        if idx >= table.rows.len() {
            break;
        }
        let cells: Vec<String> = table.rows[idx]
            .iter()
            .map(|c| match c {
                Cell::Null => NULL_SENTINEL.to_string(),
                Cell::Text(s) => s.clone(),
            })
            .collect();
        let mut line = build_line(&cells, &layout, false, None);
        line.highlight = highlight_row == Some(idx);
        rows.push(line);
    }

    Render { header, rows }
}

/// The column indices currently visible for the given scroll state and width.
pub fn visible_column_indices(
    widths: &[usize],
    frozen: usize,
    hscroll: usize,
    area_w: usize,
) -> Vec<usize> {
    visible_columns(widths, frozen, hscroll, area_w)
        .into_iter()
        .map(|(col, _, _)| col)
        .collect()
}

#[cfg(test)]
mod tests;
