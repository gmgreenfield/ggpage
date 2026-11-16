//! The interactive application state, kept pure so the whole flow (streaming
//! input, scrolling, searching, block navigation) is testable without a TTY.
//!
//! [`App`] consumes [`parse::Event`]s as they stream in, maps key events to
//! actions via [`keys`], and produces a [`Frame`] of styled lines for the thin
//! `tui` layer to draw. It never performs I/O.

use crate::cell;
use crate::diff;
use crate::export::{self, Format};
use crate::keys::{self, Action, Cmd, SearchKey};
use crate::parse::Event;
use crate::stats;
use crate::table::Table;
use crate::view::{self, DEFAULT_CHOP_CAP, RenderLine, Span, SpanKind, Viewport, Widths};
use crate::width::{display_width, pad_to_width, slice_width};
use crossterm::event::{KeyCode, KeyEvent};

/// A contiguous run of input: either a parsed table or plain passthrough text.
enum Block {
    Table { table: Table, widths: Widths },
    Text(Vec<String>),
}

impl Block {
    fn nrows(&self) -> usize {
        match self {
            Block::Table { table, .. } => table.nrows(),
            Block::Text(lines) => lines.len(),
        }
    }
    fn ncols(&self) -> usize {
        match self {
            Block::Table { table, .. } => table.ncols(),
            Block::Text(lines) => lines.iter().map(|l| display_width(l)).max().unwrap_or(0),
        }
    }
}

/// Interaction mode for incoming key events.
enum Mode {
    Normal,
    /// Editing the search prompt; `forward` is `/` vs `?`.
    Search {
        buf: String,
        forward: bool,
    },
    /// A `-` was pressed; the next key is the option letter.
    Option,
    /// `e` was pressed; the next key chooses the export format/destination.
    Export,
}

/// A side effect the pure app requests of the `tui` I/O layer.
pub enum Effect {
    /// Write `contents` to `path` on disk.
    WriteFile { path: String, contents: String },
    /// Copy `contents` to the system clipboard (OSC 52).
    Clipboard { contents: String },
}

/// A centered overlay (stats, cell view, diff, …).
pub struct Popup {
    pub title: String,
    pub lines: Vec<String>,
}

/// A frame ready to be drawn: a pinned header (tables only), body lines, a
/// one-line status, an optional prompt (shown instead of status), and an
/// optional centered popup overlay.
pub struct Frame {
    pub header: Option<RenderLine>,
    pub body: Vec<RenderLine>,
    pub status: String,
    pub prompt: Option<String>,
    pub popup: Option<Popup>,
}

pub struct App {
    blocks: Vec<Block>,
    cur: usize,
    vp: Viewport,
    mode: Mode,
    /// Terminal size in cells.
    width: usize,
    height: usize,
    /// Last submitted search query and direction.
    query: String,
    search_forward: bool,
    /// Current row (cursor) within the current block; highlighted and used by
    /// cell view, mark/diff, and search landing.
    cursor_row: usize,
    /// Rows marked for diffing (at most two).
    marks: Vec<usize>,
    /// Transient message (e.g. "pattern not found").
    message: Option<String>,
    cap: usize,
    /// A side effect awaiting execution by the `tui` layer.
    pending_effect: Option<Effect>,
    /// Current column (cursor) within a table block.
    cursor_col: usize,
    /// An open overlay, shown until the next key dismisses it.
    popup: Option<Popup>,
}

impl Default for App {
    fn default() -> Self {
        App::new(80, 24)
    }
}

impl App {
    pub fn new(width: usize, height: usize) -> Self {
        App {
            blocks: Vec::new(),
            cur: 0,
            vp: Viewport::default(),
            mode: Mode::Normal,
            width,
            height,
            query: String::new(),
            search_forward: true,
            cursor_row: 0,
            marks: Vec::new(),
            message: None,
            cap: DEFAULT_CHOP_CAP,
            pending_effect: None,
            cursor_col: 0,
            popup: None,
        }
    }

    /// Take the pending side effect, if any, for the I/O layer to perform.
    pub fn take_effect(&mut self) -> Option<Effect> {
        self.pending_effect.take()
    }

    /// Set a transient status message (e.g. an export result from the tui).
    pub fn set_message(&mut self, msg: impl Into<String>) {
        self.message = Some(msg.into());
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        self.width = width;
        self.height = height;
    }

    /// Set the initial chop (column-width cap) state.
    pub fn set_chop(&mut self, chop: bool) {
        self.vp.chop = chop;
    }

    /// True once any content has been received.
    pub fn has_content(&self) -> bool {
        !self.blocks.is_empty()
    }

    // --- streaming input ---------------------------------------------------

    pub fn apply_event(&mut self, ev: Event) {
        match ev {
            Event::StartTable { columns } => {
                let mut widths = Widths::new(columns.len());
                widths.observe_header(&columns);
                self.blocks.push(Block::Table {
                    table: Table::new(columns),
                    widths,
                });
            }
            Event::Row(row) => {
                if let Some(Block::Table { table, widths }) = self.blocks.last_mut() {
                    widths.observe_row(&row);
                    table.push_row(row);
                }
            }
            Event::EndTable { footer } => {
                if let Some(Block::Table { table, .. }) = self.blocks.last_mut() {
                    table.footer = footer;
                }
            }
            Event::Passthrough(line) => {
                if let Some(Block::Text(lines)) = self.blocks.last_mut() {
                    lines.push(line);
                } else {
                    self.blocks.push(Block::Text(vec![line]));
                }
            }
        }
    }

    // --- geometry ----------------------------------------------------------

    fn is_table(&self) -> bool {
        matches!(self.blocks.get(self.cur), Some(Block::Table { .. }))
    }

    /// Height available for the body (everything but the status line).
    fn body_h(&self) -> usize {
        self.height.saturating_sub(1)
    }

    /// Number of data rows shown at once in the current block.
    fn page(&self) -> usize {
        if self.is_table() {
            self.body_h().saturating_sub(1) // header is pinned
        } else {
            self.body_h()
        }
    }

    fn extent(&self) -> (usize, usize) {
        match self.blocks.get(self.cur) {
            Some(b) => (b.nrows(), b.ncols()),
            None => (0, 0),
        }
    }

    // --- key handling ------------------------------------------------------

    /// Handle a key event. Returns `true` if the app should quit.
    pub fn on_key(&mut self, ev: KeyEvent) -> bool {
        self.message = None;
        // Any key dismisses an open popup.
        if self.popup.is_some() {
            self.popup = None;
            return false;
        }
        match &mut self.mode {
            Mode::Normal => return self.on_normal_key(ev),
            Mode::Search { .. } => self.on_search_key(ev),
            Mode::Option => self.on_option_key(ev),
            Mode::Export => self.on_export_key(ev),
        }
        false
    }

    fn on_normal_key(&mut self, ev: KeyEvent) -> bool {
        match keys::normal(ev) {
            Some(Cmd::Do(a)) => return self.apply_action(a),
            Some(Cmd::BeginSearch { forward }) => {
                self.mode = Mode::Search {
                    buf: String::new(),
                    forward,
                };
            }
            Some(Cmd::BeginOption) => self.mode = Mode::Option,
            Some(Cmd::BeginExport) => {
                if self.current_table().is_some() {
                    self.mode = Mode::Export;
                } else {
                    self.message = Some("export: current block is not a table".into());
                }
            }
            Some(Cmd::ShowStats) => self.show_stats(),
            Some(Cmd::ViewCell) => self.view_cell(),
            Some(Cmd::MarkRow) => self.mark_row(),
            None => {}
        }
        false
    }

    fn view_cell(&mut self) {
        let (r, c) = (self.cursor_row, self.cursor_col);
        let Some(table) = self.current_table() else {
            self.message = Some("view: current block is not a table".into());
            return;
        };
        let col = table.columns.get(c).cloned().unwrap_or_default();
        let (kind, lines) = match table.rows.get(r).and_then(|row| row.get(c)) {
            Some(crate::table::Cell::Null) | None => ("null", vec!["NULL".to_string()]),
            Some(crate::table::Cell::Text(s)) => {
                cell::cell_view(s, self.width.saturating_sub(4).max(16))
            }
        };
        self.popup = Some(Popup {
            title: format!("cell · {col} [{kind}]"),
            lines,
        });
    }

    fn mark_row(&mut self) {
        if self.current_table().is_none() {
            self.message = Some("mark: current block is not a table".into());
            return;
        }
        let r = self.cursor_row;
        if let Some(pos) = self.marks.iter().position(|&m| m == r) {
            self.marks.remove(pos);
            self.message = Some(format!("unmarked row {}", r + 1));
            return;
        }
        self.marks.push(r);
        if self.marks.len() == 2 {
            let (a, b) = (self.marks[0], self.marks[1]);
            let lines = match self.current_table() {
                Some(t) => diff::diff_rows(t, a, b),
                None => return,
            };
            self.popup = Some(Popup {
                title: format!("diff · rows {} & {}", a + 1, b + 1),
                lines,
            });
            self.marks.clear();
        } else {
            self.message = Some(format!("marked row {} — mark another to diff", r + 1));
        }
    }

    fn show_stats(&mut self) {
        let col = self.cursor_col;
        let Some(table) = self.current_table() else {
            self.message = Some("stats: current block is not a table".into());
            return;
        };
        let st = stats::column_stats(table, col);
        self.popup = Some(Popup {
            title: format!("column stats · {}", st.column),
            lines: st.lines(),
        });
    }

    /// Map an export-mode key to (format, to-clipboard?). Uppercase = clipboard.
    fn on_export_key(&mut self, ev: KeyEvent) {
        let KeyCode::Char(c) = ev.code else {
            self.mode = Mode::Normal;
            return;
        };
        let choice = match c {
            'c' => Some((Format::Csv, false)),
            'C' => Some((Format::Csv, true)),
            'm' => Some((Format::Markdown, false)),
            'M' => Some((Format::Markdown, true)),
            'j' => Some((Format::Json, false)),
            'J' => Some((Format::Json, true)),
            'i' => Some((Format::Insert, false)),
            'I' => Some((Format::Insert, true)),
            _ => None,
        };
        self.mode = Mode::Normal;
        if let Some((format, clipboard)) = choice {
            self.do_export(format, clipboard);
        }
    }

    fn do_export(&mut self, format: Format, clipboard: bool) {
        let Some(table) = self.current_table() else {
            return;
        };
        let rows = table.nrows();
        let contents = export::export(table, format, "exported");
        if clipboard {
            self.message = Some(format!(
                "copied {rows} rows as {} to clipboard",
                format.label()
            ));
            self.pending_effect = Some(Effect::Clipboard { contents });
        } else {
            let path = format!("ggpage-export.{}", format.extension());
            self.message = Some(format!(
                "exported {rows} rows as {} → {path}",
                format.label()
            ));
            self.pending_effect = Some(Effect::WriteFile { path, contents });
        }
    }

    fn current_table(&self) -> Option<&Table> {
        match self.blocks.get(self.cur) {
            Some(Block::Table { table, .. }) => Some(table),
            _ => None,
        }
    }

    fn on_search_key(&mut self, ev: KeyEvent) {
        match keys::search_input(ev) {
            Some(SearchKey::Insert(c)) => {
                if let Mode::Search { buf, .. } = &mut self.mode {
                    buf.push(c);
                }
            }
            Some(SearchKey::Backspace) => {
                if let Mode::Search { buf, .. } = &mut self.mode {
                    buf.pop();
                }
            }
            Some(SearchKey::Submit) => {
                // Extract the query, ending the borrow before mutating mode.
                let submitted = match &self.mode {
                    Mode::Search { buf, forward } => Some((buf.clone(), *forward)),
                    _ => None,
                };
                self.mode = Mode::Normal;
                if let Some((q, fwd)) = submitted
                    && !q.is_empty()
                {
                    self.query = q;
                    self.search_forward = fwd;
                    self.search_from(self.cursor_row, fwd);
                }
            }
            Some(SearchKey::Cancel) => self.mode = Mode::Normal,
            None => {}
        }
    }

    fn on_option_key(&mut self, ev: KeyEvent) {
        if let KeyCode::Char(c) = ev.code
            && let Some(a) = keys::option(c)
        {
            self.apply_action(a);
        }
        self.mode = Mode::Normal;
    }

    fn apply_action(&mut self, a: Action) -> bool {
        let (nrows, ncols) = self.extent();
        let page = self.page();
        match a {
            Action::Quit => return true,
            Action::LineDown => self.move_row(1, nrows),
            Action::LineUp => self.move_row(-1, nrows),
            Action::HalfPageDown => self.move_row((page / 2).max(1) as isize, nrows),
            Action::HalfPageUp => self.move_row(-((page / 2).max(1) as isize), nrows),
            Action::PageDown => self.move_row(page as isize, nrows),
            Action::PageUp => self.move_row(-(page as isize), nrows),
            Action::GotoTop => {
                self.cursor_row = 0;
                self.vp.goto_top();
            }
            Action::GotoBottom => {
                self.cursor_row = nrows.saturating_sub(1);
                self.vp.goto_bottom(nrows, page);
            }
            Action::ColLeft => self.cursor_move(-1, ncols),
            Action::ColRight => self.cursor_move(1, ncols),
            Action::ColHome => self.cursor_home(),
            Action::ColEnd => self.cursor_end(ncols),
            Action::ToggleChop => self.vp.chop = !self.vp.chop,
            Action::NextMatch => self.repeat_search(self.search_forward),
            Action::PrevMatch => self.repeat_search(!self.search_forward),
        }
        false
    }

    /// Move the row cursor by `delta`, clamped, and scroll to keep it visible.
    fn move_row(&mut self, delta: isize, nrows: usize) {
        if nrows == 0 {
            return;
        }
        let new = (self.cursor_row as isize + delta).clamp(0, nrows as isize - 1) as usize;
        self.cursor_row = new;
        let page = self.page();
        self.vp.reveal_row(new, nrows, page);
    }

    /// Move the column cursor (tables) or scroll text horizontally.
    fn cursor_move(&mut self, dir: isize, ncols: usize) {
        if !self.is_table() {
            let step = 8 * dir;
            let new = (self.vp.hscroll as isize + step).max(0) as usize;
            self.vp.hscroll = new.min(ncols.saturating_sub(1));
            return;
        }
        if ncols == 0 {
            return;
        }
        let new = (self.cursor_col as isize + dir).clamp(0, ncols as isize - 1) as usize;
        self.cursor_col = new;
        self.reveal_col(new);
    }

    fn cursor_home(&mut self) {
        if self.is_table() {
            self.cursor_col = 0;
            self.vp.h_home();
        } else {
            self.vp.h_home();
        }
    }

    fn cursor_end(&mut self, ncols: usize) {
        if self.is_table() {
            self.cursor_col = ncols.saturating_sub(1);
            self.reveal_col(self.cursor_col);
        } else {
            self.vp.hscroll = ncols.saturating_sub(self.width.max(1));
        }
    }

    /// Adjust horizontal scroll so the given column is visible. Frozen columns
    /// are always visible and need no scrolling.
    fn reveal_col(&mut self, col: usize) {
        let (_, ncols) = self.extent();
        let frozen = self.vp.frozen.min(ncols);
        if col < frozen {
            return;
        }
        let scroll_idx = col - frozen;
        if scroll_idx < self.vp.hscroll {
            self.vp.hscroll = scroll_idx;
            return;
        }
        let eff = self.current_widths_eff();
        while self.vp.hscroll < scroll_idx {
            let visible = view::visible_column_indices(&eff, frozen, self.vp.hscroll, self.width);
            if visible.contains(&col) {
                break;
            }
            self.vp.hscroll += 1;
        }
    }

    fn current_widths_eff(&self) -> Vec<usize> {
        match self.blocks.get(self.cur) {
            Some(Block::Table { widths, .. }) => widths.effective(self.vp.chop, self.cap),
            _ => Vec::new(),
        }
    }

    // --- search ------------------------------------------------------------

    fn repeat_search(&mut self, forward: bool) {
        if self.query.is_empty() {
            return;
        }
        let (nrows, _) = self.extent();
        if nrows == 0 {
            return;
        }
        // Start strictly after/before the current row (wrapping).
        let start = if forward {
            (self.cursor_row + 1) % nrows
        } else {
            (self.cursor_row + nrows - 1) % nrows
        };
        self.search_from(start, forward);
    }

    fn search_from(&mut self, start: usize, forward: bool) {
        let (nrows, _) = self.extent();
        if nrows == 0 {
            return;
        }
        let found = (0..nrows).find_map(|off| {
            let idx = if forward {
                (start + off) % nrows
            } else {
                (start + nrows - (off % nrows)) % nrows
            };
            if self.row_matches(idx) {
                Some(idx)
            } else {
                None
            }
        });
        match found {
            Some(idx) => {
                self.cursor_row = idx;
                let page = self.page();
                self.vp.reveal_row(idx, nrows, page);
            }
            None => {
                self.message = Some(format!("pattern not found: {}", self.query));
            }
        }
    }

    fn row_matches(&self, idx: usize) -> bool {
        let needle = &self.query;
        let smart_ci = needle.chars().all(|c| !c.is_uppercase());
        let hay_has = |hay: &str| {
            if smart_ci {
                hay.to_lowercase().contains(&needle.to_lowercase())
            } else {
                hay.contains(needle)
            }
        };
        match self.blocks.get(self.cur) {
            Some(Block::Table { table, .. }) => table
                .rows
                .get(idx)
                .is_some_and(|row| row.iter().any(|c| hay_has(c.text()))),
            Some(Block::Text(lines)) => lines.get(idx).is_some_and(|l| hay_has(l)),
            None => false,
        }
    }

    // --- rendering ---------------------------------------------------------

    pub fn frame(&self) -> Frame {
        let body_h = self.body_h();
        let hl = if self.extent().0 > 0 {
            Some(self.cursor_row)
        } else {
            None
        };
        let (header, body) = match self.blocks.get(self.cur) {
            Some(Block::Table { table, widths }) => {
                let eff = widths.effective(self.vp.chop, self.cap);
                let cursor = Some(self.cursor_col.min(table.ncols().saturating_sub(1)));
                let r = view::render(table, &eff, &self.vp, self.width, body_h, hl, cursor);
                (Some(r.header), r.rows)
            }
            Some(Block::Text(lines)) => (None, self.render_text(lines, body_h, hl)),
            None => (None, Vec::new()),
        };

        let prompt = match &self.mode {
            Mode::Search { buf, forward } => {
                Some(format!("{}{}", if *forward { '/' } else { '?' }, buf))
            }
            Mode::Export => {
                Some("export: [c]sv [m]d [j]son [i]nsert — SHIFT for clipboard, Esc cancels".into())
            }
            _ => None,
        };

        let popup = self.popup.as_ref().map(|p| Popup {
            title: p.title.clone(),
            lines: p.lines.clone(),
        });

        Frame {
            header,
            body,
            status: self.status_line(),
            prompt,
            popup,
        }
    }

    fn render_text(&self, lines: &[String], body_h: usize, hl: Option<usize>) -> Vec<RenderLine> {
        lines
            .iter()
            .enumerate()
            .skip(self.vp.top)
            .take(body_h)
            .map(|(i, line)| RenderLine {
                spans: vec![Span {
                    text: slice_width(line, self.vp.hscroll, self.width),
                    kind: SpanKind::Data,
                }],
                highlight: hl == Some(i),
            })
            .collect()
    }

    fn status_line(&self) -> String {
        if let Some(msg) = &self.message {
            return msg.clone();
        }
        let (nrows, ncols) = self.extent();
        let page = self.page();
        let first = if nrows == 0 { 0 } else { self.vp.top + 1 };
        let last = (self.vp.top + page).min(nrows);
        let at_end = last >= nrows;

        let mut s = String::new();
        if self.is_table() {
            let frozen = self.vp.frozen.min(ncols);
            let first_scroll = frozen + self.vp.hscroll + 1;
            s.push_str(&format!(
                "rows {first}-{last}/{nrows}  cols 1-{frozen},{first_scroll}../{ncols}",
            ));
            if self.vp.chop {
                s.push_str("  [chop]");
            }
        } else {
            s.push_str(&format!("lines {first}-{last}/{nrows}"));
        }
        if self.blocks.len() > 1 {
            s.push_str(&format!("  block {}/{}", self.cur + 1, self.blocks.len()));
        }
        if !self.query.is_empty() {
            s.push_str(&format!("  /{}", self.query));
        }
        if let Some(&m) = self.marks.first() {
            s.push_str(&format!("  mark:{}", m + 1));
        }
        s.push_str(if at_end { "  (END)" } else { "" });
        pad_to_width(&s, self.width)
    }
}

#[cfg(test)]
impl App {
    fn cursor_row_for_test(&self) -> usize {
        self.cursor_row
    }
}

#[cfg(test)]
mod tests;
