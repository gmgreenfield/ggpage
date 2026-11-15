use super::*;
use crate::table::{Cell, Table};

fn sample() -> Table {
    // 4 columns, 5 rows. Column names short; values varying widths.
    let mut t = Table::new(vec![
        "id".into(),
        "name".into(),
        "city".into(),
        "note".into(),
    ]);
    for i in 1..=5 {
        t.push_row(vec![
            Cell::Text(i.to_string()),
            Cell::Text(format!("name{i}")),
            Cell::Text(format!("city{i}")),
            Cell::Text(format!("note number {i}")),
        ]);
    }
    t
}

fn widths_of(t: &Table) -> Widths {
    let mut w = Widths::new(t.ncols());
    w.observe_header(&t.columns);
    for r in &t.rows {
        w.observe_row(r);
    }
    w
}

// --- width accumulation ----------------------------------------------------

#[test]
fn widths_track_max_of_header_and_cells() {
    let t = sample();
    let w = widths_of(&t).effective(false, 999);
    // "id" header=2, values "1".."5"=1 → 2
    assert_eq!(w[0], 2);
    // "name" header=4, "name1"=5 → 5
    assert_eq!(w[1], 5);
    // "note" header=4, "note number 1"=13 → 13
    assert_eq!(w[3], 13);
}

#[test]
fn chop_caps_column_width() {
    let t = sample();
    let w = widths_of(&t).effective(true, 6);
    assert_eq!(w[3], 6); // "note" natural 13, capped to 6
    assert_eq!(w[1], 5); // "name1" under cap, unchanged
}

#[test]
fn widths_are_unicode_aware() {
    let mut t = Table::new(vec!["x".into()]);
    t.push_row(vec![Cell::Text("日本".into())]); // width 4
    let w = widths_of(&t).effective(false, 999);
    assert_eq!(w[0], 4);
}

// --- viewport navigation ---------------------------------------------------

#[test]
fn vertical_scroll_clamps_at_top_and_bottom() {
    let mut vp = Viewport::default();
    let (nrows, page) = (10, 4);
    vp.scroll_v(-5, nrows, page); // already at top
    assert_eq!(vp.top, 0);
    vp.scroll_v(3, nrows, page);
    assert_eq!(vp.top, 3);
    vp.scroll_v(100, nrows, page); // clamp: last page => top = 10-4 = 6
    assert_eq!(vp.top, 6);
    vp.goto_top();
    assert_eq!(vp.top, 0);
    vp.goto_bottom(nrows, page);
    assert_eq!(vp.top, 6);
}

#[test]
fn horizontal_scroll_keeps_one_scrollable_column() {
    let mut vp = Viewport::default(); // frozen = 1
    let ncols = 4;
    // scrollable columns = 3 (indices 1..4); max hscroll = 4-1-1 = 2
    vp.scroll_h(100, ncols);
    assert_eq!(vp.hscroll, 2);
    vp.scroll_h(-1, ncols);
    assert_eq!(vp.hscroll, 1);
    vp.h_home();
    assert_eq!(vp.hscroll, 0);
    vp.h_end(ncols);
    assert_eq!(vp.hscroll, 2);
}

#[test]
fn reveal_row_scrolls_minimally() {
    let mut vp = Viewport::default();
    let (nrows, page) = (100, 10);
    vp.reveal_row(50, nrows, page);
    assert_eq!(vp.top, 41); // 50 at bottom of a 10-row page => top 41
    vp.reveal_row(45, nrows, page); // already visible, no change
    assert_eq!(vp.top, 41);
    vp.reveal_row(5, nrows, page); // above => top = 5
    assert_eq!(vp.top, 5);
}

// --- rendering -------------------------------------------------------------

#[test]
fn header_is_pinned_and_rows_follow() {
    let t = sample();
    let w = widths_of(&t).effective(true, 48);
    let vp = Viewport::default();
    let r = render(&t, &w, &vp, 200, 4, None, None); // header + 3 data rows
    assert!(r.header.plain().starts_with("id"));
    assert_eq!(r.rows.len(), 3);
    assert!(r.rows[0].plain().starts_with("1 "));
}

#[test]
fn render_respects_vertical_scroll() {
    let t = sample();
    let w = widths_of(&t).effective(true, 48);
    let vp = Viewport {
        top: 2,
        ..Viewport::default()
    };
    let r = render(&t, &w, &vp, 200, 10, None, None);
    // data starts at row index 2 => id "3"
    assert!(r.rows[0].plain().starts_with("3 "));
    assert_eq!(r.rows.len(), 3); // only 5 rows total, from index 2
}

#[test]
fn frozen_column_stays_when_scrolled_right() {
    let t = sample();
    let w = widths_of(&t).effective(true, 48);
    let mut vp = Viewport::default(); // frozen = 1 (id)
    vp.scroll_h(1, t.ncols()); // skip "name"; show id | city | note
    let r = render(&t, &w, &vp, 200, 2, None, None);
    let header = r.header.plain();
    assert!(header.contains("id"));
    assert!(header.contains("city"));
    assert!(!header.contains("name")); // scrolled past
    // frozen separator appears right after the id column
    assert!(r.header.spans.iter().any(|s| s.kind == SpanKind::FrozenSep));
}

#[test]
fn narrow_area_truncates_last_column() {
    let t = sample();
    let w = widths_of(&t).effective(false, 999);
    let vp = Viewport::default();
    // Very narrow: only part of the table fits.
    let r = render(&t, &w, &vp, 10, 2, None, None);
    assert!(display_width(&r.header.plain()) <= 10);
}

#[test]
fn null_cells_get_null_kind() {
    let mut t = Table::new(vec!["a".into()]);
    t.push_row(vec![Cell::Null]);
    t.push_row(vec![Cell::Text(String::new())]);
    let w = widths_of(&t).effective(true, 48);
    let vp = Viewport::default();
    let r = render(&t, &w, &vp, 50, 4, None, None);
    assert_eq!(r.rows[0].spans[0].kind, SpanKind::Null);
    assert!(r.rows[0].plain().starts_with("NULL"));
    assert_eq!(r.rows[1].spans[0].kind, SpanKind::Data);
    assert!(!r.rows[1].plain().starts_with("NULL"));
}

#[test]
fn highlight_row_is_marked() {
    let t = sample();
    let w = widths_of(&t).effective(true, 48);
    let vp = Viewport::default();
    let r = render(&t, &w, &vp, 200, 10, None, None);
    // no highlight by default
    assert!(r.rows.iter().all(|l| !l.highlight));
}
