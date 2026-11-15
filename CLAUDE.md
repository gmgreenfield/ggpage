# CLAUDE.md — ggpage

## What this is
`ggpage` is a deluxe terminal pager for SQL client output, built to be a drop-in
replacement for `less`. Primary target is the `mysql` CLI, but it also handles
`psql`, `sqlite3`, and `mycli`. It parses tabular output into a real table model
and renders an interactive, scrollable view with a pinned header and frozen
columns. Non-tabular input behaves like plain `less`; if stdout is not a TTY it
copies input to output unchanged.

**Language:** Rust (edition 2024). **TUI:** Ratatui + crossterm.
Binary/crate name: `ggpage`.

## Architecture (data flow)
```
stdin/file ─▶ source (streaming reader thread)
            ─▶ parse (detect format, emit Table rows as they stream in)
            ─▶ table (bounded, append-driven model; distinguishes NULL vs "")
            ─▶ view  (pure layout: scroll offsets, pinned header, frozen cols,
                      render-to-buffer cell grid)
            ─▶ tui   (thin Ratatui/crossterm layer; TestBackend in tests)
```
If the input is not a recognized table, or stdout is not a TTY, we bypass the
table path and use `passthrough` (less-like behavior / raw copy).

## Module layout (`src/`)
- `main.rs`        — entry point: arg parsing, TTY detection, wiring.
- `source.rs`      — streaming line reader; never blocks on EOF.
- `parse/`         — format detection + parsers (boxed, `\G`, TSV batch).
                     Streaming: emits rows as they arrive. Pure, no TTY.
- `table.rs`       — Table/Column/Cell model; bounded storage. Pure.
- `width.rs`       — Unicode width + ANSI-escape-aware measure/truncate.
- `view.rs`        — viewport/layout math + render-to-buffer (cell grid).
                     Pure and fully testable without a terminal.
- `keys.rs`        — key event → Action mapping (less keybindings). Pure.
- `tui.rs`         — Ratatui terminal bootstrap + event loop. Kept THIN.
- `passthrough.rs` — less-like pager / raw copy for non-table input.

## Design rules
- **Keep the terminal layer thin.** All layout, scrolling, search, and key logic
  lives in pure modules (`view`, `keys`, `table`, `parse`) and is unit-tested.
  `tui` only translates crossterm events ↔ pure Actions and draws widgets.
- **Test-driven** for parsers, layout, and key handling: write the test first.
- **Streaming first.** Render while reading (reader thread → channel → model).
  Do not block on EOF. Keep memory bounded/compact for large result sets.
- **Unicode + ANSI correctness** everywhere widths matter — always go through
  `width`, never `str::len()` / `.chars().count()` for display width.
- Pure modules must not depend on ratatui/crossterm (except `view` may use
  ratatui's `Buffer`/`Rect` value types for the render target — no I/O).

## Build / test / run
```
cargo build                       # debug build
cargo test                        # run all tests
cargo clippy --all-targets        # lint
cargo fmt                         # format
cargo build --release             # optimized static-ish binary → target/release/ggpage
```
Run interactively:
```
mysql -e 'select * from t' | cargo run            # pipe
cargo run -- file.txt                              # file arg
PAGER='ggpage' mysql                               # via $PAGER
# inside mysql:  pager ggpage
```

## Conventions
- Small, incremental commits — one logical step each.
- Parsers return structured results + the trailing footer text verbatim.
- NULL is a distinct `Cell` variant, never conflated with the empty string.
- Tests live in `#[cfg(test)] mod tests` blocks next to the code (plus
  `tests/` for end-to-end).

## Known gotchas (fill in as discovered)
- MySQL vs MariaDB: MariaDB may use box-drawing characters and different footer
  wording; detection must tolerate both ASCII `+-|` and Unicode box borders.
- `\G` vertical output has no reliable column set until the first record block;
  columns are the union of field names seen across records.
- TSV batch output has no borders — detect via consistent tab counts and the
  absence of boxed borders.
- Restore the terminal (leave alt screen, disable raw mode) even on panic —
  `ratatui::init()` installs a restoring panic hook; we call `ratatui::restore()`.
- Key input must come from `/dev/tty`, not stdin, because stdin is the data
  pipe. This is why crossterm is built with the `use-dev-tty` feature.
- `app` is the pure integration layer: it turns parse events into table/text
  "blocks", owns the viewport + search + modes, and emits a `Frame`. `tui` only
  does terminal I/O + style mapping, so the whole interaction is unit-tested
  (see `app/tests.rs`) and rendered via ratatui's `TestBackend` (`tui` tests).
- NULL vs "": a *parsed* `NULL` token becomes `Cell::Null`; a literal string
  `"NULL"` is indistinguishable in rendered output and also treated as NULL.
