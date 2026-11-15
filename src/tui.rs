//! The thin terminal layer: it owns the ratatui terminal and the event loop,
//! translating crossterm events into [`App`] calls and blitting the [`Frame`]
//! the app produces. All layout/logic lives in `app`/`view`; this file only
//! does I/O and style mapping.

use std::io;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use crossterm::event::{self, Event as CEvent, KeyEventKind};
use ratatui::Frame as RFrame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span as RSpan, Text};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::source::Msg;
use crate::view::{RenderLine, SpanKind};

/// Set up the terminal, run the event loop until the user quits, and restore
/// the terminal afterwards (ratatui installs a panic hook that also restores).
pub fn run(app: &mut App, rx: Receiver<Msg>) -> io::Result<()> {
    let mut terminal = ratatui::init();
    let size = terminal.size()?;
    app.resize(size.width as usize, size.height as usize);
    let result = event_loop(&mut terminal, app, rx);
    ratatui::restore();
    result
}

fn event_loop(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    rx: Receiver<Msg>,
) -> io::Result<()> {
    let mut streaming = true;
    loop {
        // Drain any input that has streamed in since the last tick.
        if streaming {
            loop {
                match rx.try_recv() {
                    Ok(Msg::Event(ev)) => app.apply_event(ev),
                    Ok(Msg::Done) => streaming = false,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        streaming = false;
                        break;
                    }
                }
            }
        }

        terminal.draw(|f| draw(f, app))?;

        // Poll briefly so we stay responsive to a growing stream; once input is
        // complete we can afford to block longer waiting for a keypress.
        let timeout = if streaming {
            Duration::from_millis(30)
        } else {
            Duration::from_millis(200)
        };
        if event::poll(timeout)? {
            match event::read()? {
                CEvent::Key(k) if k.kind != KeyEventKind::Release => {
                    if app.on_key(k) {
                        return Ok(());
                    }
                }
                CEvent::Resize(w, h) => app.resize(w as usize, h as usize),
                _ => {}
            }
        }
    }
}

fn draw(f: &mut RFrame, app: &App) {
    let frame = app.frame();
    let area = f.area();

    let has_header = frame.header.is_some();
    let chunks = Layout::vertical([
        Constraint::Length(if has_header { 1 } else { 0 }),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area);

    if let Some(header) = &frame.header {
        f.render_widget(Paragraph::new(to_line(header)), chunks[0]);
    }

    let body: Vec<Line> = frame.body.iter().map(to_line).collect();
    f.render_widget(Paragraph::new(Text::from(body)), chunks[1]);

    let status = frame.prompt.as_deref().unwrap_or(&frame.status);
    let status_style = Style::default()
        .fg(Color::Black)
        .bg(Color::Gray)
        .add_modifier(Modifier::BOLD);
    f.render_widget(
        Paragraph::new(Line::from(status.to_string())).style(status_style),
        chunks[2],
    );
}

/// Map a pure [`RenderLine`] to a styled ratatui [`Line`].
fn to_line(line: &RenderLine) -> Line<'static> {
    let spans: Vec<RSpan> = line
        .spans
        .iter()
        .map(|s| RSpan::styled(s.text.clone(), style_for(s.kind)))
        .collect();
    let mut l = Line::from(spans);
    if line.highlight {
        l = l.style(Style::default().add_modifier(Modifier::REVERSED));
    }
    l
}

fn style_for(kind: SpanKind) -> Style {
    match kind {
        SpanKind::Header => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        SpanKind::Data => Style::default(),
        SpanKind::Null => Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
        SpanKind::Separator => Style::default().fg(Color::DarkGray),
        SpanKind::FrozenSep => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_str;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    fn row_text(buf: &Buffer, y: u16) -> String {
        let w = buf.area().width;
        (0..w).map(|x| buf[(x, y)].symbol()).collect()
    }

    fn render_app(app: &App, w: u16, h: u16) -> Buffer {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn draws_pinned_header_rows_and_status() {
        let mut app = App::new(40, 6);
        app.resize(40, 6);
        for ev in parse_str(
            "+----+-------+\n| id | name  |\n+----+-------+\n|  1 | alice |\n|  2 | bob   |\n+----+-------+\n2 rows in set\n",
        ) {
            app.apply_event(ev);
        }
        let buf = render_app(&app, 40, 6);
        // Row 0: header; last row: status; data rows between.
        assert!(row_text(&buf, 0).contains("id"));
        assert!(row_text(&buf, 0).contains("name"));
        assert!(row_text(&buf, 1).contains("alice"));
        assert!(row_text(&buf, 2).contains("bob"));
        assert!(row_text(&buf, 5).contains("rows 1-2/2"));
    }

    #[test]
    fn text_block_has_no_header_row() {
        let mut app = App::new(40, 5);
        app.resize(40, 5);
        for ev in parse_str("hello world\nsecond line\n") {
            app.apply_event(ev);
        }
        let buf = render_app(&app, 40, 5);
        assert!(row_text(&buf, 0).contains("hello world"));
        assert!(row_text(&buf, 1).contains("second line"));
        assert!(row_text(&buf, 4).contains("lines 1-2/2"));
    }

    #[test]
    fn search_prompt_is_drawn_in_status_area() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = App::new(40, 5);
        app.resize(40, 5);
        for ev in parse_str("a\tb\n1\t2\n") {
            app.apply_event(ev);
        }
        app.on_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        app.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        let buf = render_app(&app, 40, 5);
        assert!(row_text(&buf, 4).contains("/x"));
    }
}
