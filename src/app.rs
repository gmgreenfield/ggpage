//! The interactive application state, kept pure so the whole flow (streaming
//! input, scrolling, searching, block navigation) is testable without a TTY.
//!
//! [`App`] consumes [`parse::Event`]s as they stream in, maps key events to
//! actions via [`keys`], and produces a [`Frame`] of styled lines for the thin
//! `tui` layer to draw. It never performs I/O.

use crate::keys::{self, Action, Cmd, SearchKey};
use crate::parse::Event;
use crate::table::Table;
use crate::view::{self, RenderLine, Span, SpanKind, Viewport, Widths, DEFAULT_CHOP_CAP};
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
            Block::Text(lines) => lines
                .iter()
                .map(|l| display_width(l))
                .max()
                .unwrap_or(0),
        }
    }
}

/// Interaction mode for incoming key events.
enum Mode {
    Normal,
    /// Editing the search prompt; `forward` is `/` vs `?`.
    Search { buf: String, forward: bool },
    /// A `-` was pressed; the next key is the option letter.
    Option,
}

/// A frame ready to be drawn: a pinned header (tables only), body lines, a
/// one-line status, and an optional prompt (shown instead of status).
pub struct Frame {
    pub header: Option<RenderLine>,
    pub body: Vec<RenderLine>,
    pub status: String,
    pub prompt: Option<String>,
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
    /// Row index of the current match in the current block.
    match_row: Option<usize>,
    /// Transient message (e.g. "pattern not found").
    message: Option<String>,
    cap: usize,
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
            match_row: None,
            message: None,
            cap: DEFAULT_CHOP_CAP,
        }
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
        match &mut self.mode {
            Mode::Normal => return self.on_normal_key(ev),
            Mode::Search { .. } => self.on_search_key(ev),
            Mode::Option => self.on_option_key(ev),
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
            None => {}
        }
        false
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
                if let Some((q, fwd)) = submitted {
                    if !q.is_empty() {
                        self.query = q;
                        self.search_forward = fwd;
                        self.search_from(self.vp.top, fwd);
                    }
                }
            }
            Some(SearchKey::Cancel) => self.mode = Mode::Normal,
            None => {}
        }
    }

    fn on_option_key(&mut self, ev: KeyEvent) {
        if let KeyCode::Char(c) = ev.code {
            if let Some(a) = keys::option(c) {
                self.apply_action(a);
            }
        }
        self.mode = Mode::Normal;
    }

    fn apply_action(&mut self, a: Action) -> bool {
        let (nrows, ncols) = self.extent();
        let page = self.page();
        match a {
            Action::Quit => return true,
            Action::LineDown => self.vp.scroll_v(1, nrows, page),
            Action::LineUp => self.vp.scroll_v(-1, nrows, page),
            Action::HalfPageDown => self.vp.scroll_v((page / 2).max(1) as isize, nrows, page),
            Action::HalfPageUp => self.vp.scroll_v(-((page / 2).max(1) as isize), nrows, page),
            Action::PageDown => self.vp.scroll_v(page as isize, nrows, page),
            Action::PageUp => self.vp.scroll_v(-(page as isize), nrows, page),
            Action::GotoTop => self.vp.goto_top(),
            Action::GotoBottom => self.vp.goto_bottom(nrows, page),
            Action::ColLeft => self.col_scroll(-1, ncols),
            Action::ColRight => self.col_scroll(1, ncols),
            Action::ColHome => self.vp.h_home(),
            Action::ColEnd => self.col_end(ncols),
            Action::ToggleChop => self.vp.chop = !self.vp.chop,
            Action::NextMatch => self.repeat_search(self.search_forward),
            Action::PrevMatch => self.repeat_search(!self.search_forward),
        }
        false
    }

    /// Horizontal scroll: whole columns for tables, 8-column steps for text.
    fn col_scroll(&mut self, dir: isize, ncols: usize) {
        if self.is_table() {
            self.vp.scroll_h(dir, ncols);
        } else {
            let step = 8 * dir;
            let new = (self.vp.hscroll as isize + step).max(0) as usize;
            self.vp.hscroll = new.min(ncols.saturating_sub(1));
        }
    }

    fn col_end(&mut self, ncols: usize) {
        if self.is_table() {
            self.vp.h_end(ncols);
        } else {
            self.vp.hscroll = ncols.saturating_sub(self.width.max(1));
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
        // Start strictly after/before the current match (wrapping).
        let start = match self.match_row {
            Some(r) if forward => (r + 1) % nrows,
            Some(r) => (r + nrows - 1) % nrows,
            None => self.vp.top,
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
                self.match_row = Some(idx);
                let page = self.page();
                self.vp.reveal_row(idx, nrows, page);
            }
            None => {
                self.match_row = None;
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
        let (header, body) = match self.blocks.get(self.cur) {
            Some(Block::Table { table, widths }) => {
                let eff = widths.effective(self.vp.chop, self.cap);
                let r = view::render(table, &eff, &self.vp, self.width, body_h, self.match_row);
                (Some(r.header), r.rows)
            }
            Some(Block::Text(lines)) => (None, self.render_text(lines, body_h)),
            None => (None, Vec::new()),
        };

        let prompt = match &self.mode {
            Mode::Search { buf, forward } => {
                Some(format!("{}{}", if *forward { '/' } else { '?' }, buf))
            }
            _ => None,
        };

        Frame {
            header,
            body,
            status: self.status_line(),
            prompt,
        }
    }

    fn render_text(&self, lines: &[String], body_h: usize) -> Vec<RenderLine> {
        let mut out = Vec::with_capacity(body_h);
        for i in self.vp.top..(self.vp.top + body_h).min(lines.len()) {
            let sliced = slice_width(&lines[i], self.vp.hscroll, self.width);
            out.push(RenderLine {
                spans: vec![Span {
                    text: sliced,
                    kind: SpanKind::Data,
                }],
                highlight: self.match_row == Some(i),
            });
        }
        out
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
        s.push_str(if at_end { "  (END)" } else { "" });
        pad_to_width(&s, self.width)
    }
}

#[cfg(test)]
impl App {
    fn match_row_for_test(&self) -> Option<usize> {
        self.match_row
    }
}

#[cfg(test)]
mod tests;
