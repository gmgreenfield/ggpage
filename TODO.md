# ggpage — TODO

A deluxe terminal pager for SQL client output (mysql first; also psql / sqlite3 /
mycli). Drop-in replacement for `less`. Rust + Ratatui.

Legend: `[ ]` todo · `[x]` done · `[~]` in progress

## Project setup
- [x] `git init`, `cargo init`
- [x] Package/module layout
- [x] TODO.md
- [x] CLAUDE.md
- [x] .gitignore
- [x] Add deps: ratatui, crossterm, unicode-width
- [x] CI (GitHub Actions: build, clippy, test, fmt --check)
- [x] README (setup instructions)

## Phase 1 — must-haves

### Input / source layer
- [x] Non-TTY stdout → copy stdin to stdout unchanged (`passthrough`)
- [x] Streaming line reader that does not block on EOF (reader thread + channel)
- [x] CLI flags: `-S`, file arg, `-V`/`-h`, forgiving `+`/`-` less-compat subset
- [x] Read from stdin pipe; read from file argument

### Parsing (TDD — tests first)
- [x] Format detection (boxed / vertical `\G` / TSV batch / plain)
- [x] Parse mysql boxed tables (`+---+---+`)
- [x] Parse `\G` vertical output
- [x] Parse tab-separated batch output
- [x] MariaDB quirks (box-drawing chars, differing footers)
- [x] Multiple result sets in one stream
- [x] `N rows in set` footer handling (+ `Query OK`, `Empty set`)
- [x] NULL token distinct from empty string
- [x] Streaming parse: emit rows as they arrive, bounded memory

### Table model
- [x] Column/Row/Cell model, append-driven (NULL distinct from "")
- [ ] Bounded/compact storage for large result sets
- [x] Width measurement: Unicode width + ANSI escape aware (`width` module)

### View / layout (TDD — tests first)
- [x] Viewport model: vertical scroll offset, row window
- [x] Pinned header row(s)
- [x] Frozen first column(s) during horizontal scroll
- [x] Horizontal scroll offset + `-S` chop toggle (chop = cap column width)
- [x] Render-to-structured-grid (pure) for tests; tui blits it
- [x] Terminal resize handling (tui layer: Resize event → app.resize)
- [x] Column width computation (sampled, streaming-friendly — `Widths`)

### Key handling (TDD — tests first)
- [x] less keys: `j`/`k`, space/`b`, `d`/`u`, `g`/`G`, `q`
- [x] Arrow keys (up/down/left/right), PageUp/PageDown, Home/End
- [x] `-S` chop toggle key (`-` then `S`)
- [x] Search: `/`, `?`, `n`, `N` (begin-search + input editing)
- [x] Horizontal scroll keys (h/l, arrows, `<`/`>`)

### TUI integration (thin — Ratatui + crossterm)
- [x] Terminal bootstrap (raw mode, alt screen) + restore on panic/exit
      (ratatui::init/restore; key input via /dev/tty so stdin stays the pipe)
- [x] Event loop; translate crossterm events → pure key actions
- [x] TestBackend-based render tests
- [x] Status/prompt line (less-style status + `/`/`?` search input)
- [x] Wire parser stream → model → view → widget
- [x] Render while reading (reader thread feeding model, redraw on update)

### Integration
- [x] End-to-end: `mysql` boxed output → interactive view (TestBackend render)
- [x] `pager ggpage` usage documented (README/help)
- [x] Works via `$PAGER` (reads stdin pipe; /dev/tty for keys)
- [ ] Manual smoke test against real mysql/psql/sqlite3/mycli

## Phase 2 — differentiators (DO NOT START until phase 1 done + tested)
- [x] Column cursor (h/l/←/→) + reveal-scroll, highlighted header (foundation)
- [ ] Column ops: hide / reorder / resize
- [ ] Search within a single column
- [ ] Pretty JSON popup for a cell
- [x] NULL vs '' distinct rendering (model + view distinguish)
- [ ] BLOB / hex display
- [x] Export view: CSV / Markdown / JSON / INSERT → file (`x`)
- [x] Export to clipboard via OSC 52 (shift + format letter)
- [ ] Mark two rows, field-by-field diff
- [x] Column stats: min/max, distinct, null count (`s` popup, current column)
- [ ] Detect EXPLAIN / EXPLAIN ANALYZE → tree render, highlight costly steps

## Release polish (last)
- [ ] Single static binary (release profile already set: lto, strip)
- [ ] goreleaser-equivalent: cargo-dist or release workflow
- [ ] Homebrew tap
- [ ] README demo GIF (vhs)
- [ ] One-line setup for mysql / psql / mycli
