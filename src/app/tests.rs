use super::*;
use crate::parse::parse_str;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn feed(app: &mut App, input: &str) {
    for ev in parse_str(input) {
        app.apply_event(ev);
    }
}

fn key(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
}
fn code(k: KeyCode) -> KeyEvent {
    KeyEvent::new(k, KeyModifiers::NONE)
}

const TABLE: &str = "\
+----+-------+
| id | name  |
+----+-------+
|  1 | alice |
|  2 | bob   |
|  3 | carol |
|  4 | dave  |
|  5 | erin  |
+----+-------+
5 rows in set (0.00 sec)
";

#[test]
fn streaming_builds_table_block() {
    let mut app = App::new(80, 10);
    assert!(!app.has_content());
    feed(&mut app, TABLE);
    assert!(app.has_content());
    let f = app.frame();
    assert!(f.header.is_some());
    assert!(f.header.unwrap().plain().contains("id"));
    assert!(f.body[0].plain().starts_with("1 "));
}

#[test]
fn rows_surface_incrementally() {
    // After StartTable + one Row, the frame should already show that row,
    // before EndTable arrives.
    let mut app = App::new(80, 10);
    let mut p = crate::parse::Parser::new();
    for line in ["+----+", "| a  |", "+----+", "|  1 |"] {
        for ev in p.push_line(line) {
            app.apply_event(ev);
        }
    }
    let f = app.frame();
    assert_eq!(f.body.len(), 1);
    assert!(f.body[0].plain().starts_with("1"));
}

#[test]
fn vertical_navigation() {
    let mut app = App::new(80, 5); // body_h=4, page=3 (header pinned)
    feed(&mut app, TABLE);
    app.on_key(key('j'));
    assert!(app.frame().body[0].plain().starts_with("2 "));
    app.on_key(key('G'));
    // last page: top = 5-3 = 2 => first visible row id 3
    assert!(app.frame().body[0].plain().starts_with("3 "));
    app.on_key(key('g'));
    assert!(app.frame().body[0].plain().starts_with("1 "));
}

#[test]
fn quit_key_reports_quit() {
    let mut app = App::new(80, 10);
    feed(&mut app, TABLE);
    assert!(app.on_key(key('q')));
    assert!(!app.on_key(key('j')));
}

#[test]
fn chop_toggle_via_dash_s() {
    let mut app = App::new(80, 10);
    feed(&mut app, TABLE);
    assert!(app.frame().status.contains("[chop]"));
    app.on_key(key('-'));
    app.on_key(key('S'));
    assert!(!app.frame().status.contains("[chop]"));
}

#[test]
fn cursor_reveals_column_and_keeps_frozen() {
    // Narrow width so not every column fits; moving the cursor to the last
    // column scrolls earlier scrollable columns off, but the frozen first
    // column stays put.
    let input = "\
+----+------+------+------+
| id | aaaa | bbbb | cccc |
+----+------+------+------+
|  1 | w    | x    | y    |
+----+------+------+------+
1 row in set
";
    let mut app = App::new(16, 6);
    feed(&mut app, input);
    app.on_key(key('>')); // cursor to last column, revealing it
    let header = app.frame().header.unwrap().plain();
    assert!(header.contains("id")); // frozen column still present
    assert!(header.contains("cccc")); // cursor column revealed
    assert!(!header.contains("aaaa")); // an earlier scrollable column scrolled off
}

#[test]
fn cursor_column_header_is_marked() {
    let mut app = App::new(80, 6);
    feed(&mut app, TABLE);
    app.on_key(code(KeyCode::Right)); // cursor id -> name
    let header = app.frame().header.unwrap();
    assert!(
        header
            .spans
            .iter()
            .any(|s| s.kind == crate::view::SpanKind::HeaderCursor && s.text.contains("name"))
    );
}

#[test]
fn stats_popup_for_current_column() {
    let mut app = App::new(80, 10);
    feed(&mut app, TABLE);
    app.on_key(code(KeyCode::Right)); // cursor on "name"
    app.on_key(key('s'));
    let f = app.frame();
    let popup = f.popup.expect("stats popup");
    assert!(popup.title.contains("name"));
    assert!(popup.lines.iter().any(|l| l.contains("distinct")));
    // any key dismisses it
    app.on_key(key('q'));
    assert!(app.frame().popup.is_none());
}

#[test]
fn search_finds_and_reveals_row() {
    let mut app = App::new(80, 4); // small page forces scrolling
    feed(&mut app, TABLE);
    // search for "carol" (row index 2)
    app.on_key(key('/'));
    for c in "carol".chars() {
        app.on_key(key(c));
    }
    app.on_key(code(KeyCode::Enter));
    let f = app.frame();
    // the matching row should be visible and highlighted
    assert!(
        f.body
            .iter()
            .any(|l| l.highlight && l.plain().contains("carol"))
    );
}

#[test]
fn search_not_found_sets_message() {
    let mut app = App::new(80, 10);
    feed(&mut app, TABLE);
    app.on_key(key('/'));
    for c in "zzz".chars() {
        app.on_key(key(c));
    }
    app.on_key(code(KeyCode::Enter));
    assert!(app.frame().status.contains("not found"));
}

#[test]
fn next_and_prev_match_cycle() {
    // Two rows contain "a": alice (0), carol (2), dave (3 has 'a'), erin no, bob no.
    let mut app = App::new(80, 10);
    feed(&mut app, TABLE);
    app.on_key(key('/'));
    app.on_key(key('a'));
    app.on_key(code(KeyCode::Enter));
    let first = app.match_row_for_test();
    app.on_key(key('n'));
    let second = app.match_row_for_test();
    assert_ne!(first, second);
    app.on_key(key('N'));
    assert_eq!(app.match_row_for_test(), first);
}

#[test]
fn search_prompt_shows_typed_query() {
    let mut app = App::new(80, 10);
    feed(&mut app, TABLE);
    app.on_key(key('/'));
    app.on_key(key('a'));
    app.on_key(key('b'));
    assert_eq!(app.frame().prompt.as_deref(), Some("/ab"));
}

#[test]
fn passthrough_text_is_a_text_block() {
    let mut app = App::new(80, 10);
    feed(&mut app, "just some log line\nanother line\n");
    let f = app.frame();
    assert!(f.header.is_none()); // text block has no pinned header
    assert_eq!(f.body[0].plain(), "just some log line");
    assert!(f.status.contains("lines"));
}

#[test]
fn multiple_blocks_counted_in_status() {
    let mut app = App::new(80, 10);
    feed(&mut app, "preamble\n");
    feed(&mut app, TABLE);
    assert!(app.frame().status.contains("block 1/2"));
}

#[test]
fn export_to_file_produces_write_effect() {
    let mut app = App::new(80, 10);
    feed(&mut app, TABLE);
    app.on_key(key('x'));
    app.on_key(key('c')); // CSV to file
    match app.take_effect() {
        Some(Effect::WriteFile { path, contents }) => {
            assert_eq!(path, "ggpage-export.csv");
            assert!(contents.starts_with("id,name\n"));
            assert!(contents.contains("alice"));
        }
        _ => panic!("expected a WriteFile effect"),
    }
    assert!(app.frame().status.contains("exported"));
}

#[test]
fn export_to_clipboard_produces_clipboard_effect() {
    let mut app = App::new(80, 10);
    feed(&mut app, TABLE);
    app.on_key(key('x'));
    app.on_key(key('J')); // JSON to clipboard (shift)
    match app.take_effect() {
        Some(Effect::Clipboard { contents }) => assert!(contents.starts_with("[\n")),
        _ => panic!("expected a Clipboard effect"),
    }
    assert!(app.frame().status.contains("clipboard"));
}

#[test]
fn export_prompt_is_shown_then_cancelled() {
    let mut app = App::new(80, 10);
    feed(&mut app, TABLE);
    app.on_key(key('x'));
    assert!(app.frame().prompt.unwrap().contains("export:"));
    app.on_key(code(KeyCode::Esc));
    assert!(app.frame().prompt.is_none());
    assert!(app.take_effect().is_none());
}

#[test]
fn export_refused_on_text_block() {
    let mut app = App::new(80, 10);
    feed(&mut app, "just text\n");
    app.on_key(key('x'));
    assert!(app.take_effect().is_none());
    assert!(app.frame().status.contains("not a table"));
}

#[test]
fn resize_changes_page_size() {
    let mut app = App::new(80, 24);
    feed(&mut app, TABLE);
    app.on_key(key('G'));
    let tall_top = app.frame().body[0].plain().chars().next().unwrap();
    app.resize(80, 4);
    app.on_key(key('G'));
    let short_top = app.frame().body[0].plain().chars().next().unwrap();
    // Smaller window => bottom page starts lower in the data.
    assert_ne!(tall_top, short_top);
}
