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
- [ ] CI (GitHub Actions: build, clippy, test, fmt --check)
- [ ] README (setup instructions)

## Phase 1 — must-haves

### Input / source layer
- [ ] Non-TTY stdout → copy stdin to stdout unchanged (`passthrough`)
- [ ] Streaming line reader that does not block on EOF
- [ ] CLI flags: `-S` (chop long lines), file args, `+` commands (less compat subset)
- [ ] Read from stdin pipe; read from file argument

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
- [ ] Viewport model: vertical scroll offset, row window
- [ ] Pinned header row(s)
- [ ] Frozen first column(s) during horizontal scroll
- [ ] Horizontal scroll offset + `-S` chop toggle
- [ ] Render-to-buffer (cell grid) for tests via ratatui TestBackend
- [ ] Terminal resize handling
- [ ] Column width computation (sampled, streaming-friendly)

### Key handling (TDD — tests first)
- [ ] less keys: `j`/`k`, space/`b`, `d`/`u`, `g`/`G`, `q`
- [ ] Arrow keys (up/down/left/right), PageUp/PageDown, Home/End
- [ ] `-S` chop toggle key
- [ ] Search: `/`, `?`, `n`, `N`
- [ ] Horizontal scroll keys (left/right, `<`/`>`)

### TUI integration (thin — Ratatui + crossterm)
- [ ] Terminal bootstrap (raw mode, alt screen) + restore on panic/exit
- [ ] Event loop; translate crossterm events → pure key actions
- [ ] TestBackend-based render tests
- [ ] Status/prompt line (less-style `:` prompt, search input)
- [ ] Wire parser stream → model → view → widget
- [ ] Render while reading (reader thread feeding model, redraw on update)

### Integration
- [ ] End-to-end: `mysql` boxed output → interactive view
- [ ] `pager ggpage` usage documented + tested via env
- [ ] Works via `$PAGER`
- [ ] Manual smoke test against real mysql/psql/sqlite3/mycli

## Phase 2 — differentiators (DO NOT START until phase 1 done + tested)
- [ ] Column ops: hide / reorder / resize
- [ ] Search within a single column
- [ ] Pretty JSON popup for a cell
- [ ] NULL vs '' distinct rendering (model already distinguishes)
- [ ] BLOB / hex display
- [ ] Export view: CSV / Markdown / JSON / INSERT → file
- [ ] Export to clipboard via OSC 52
- [ ] Mark two rows, field-by-field diff
- [ ] Column stats: min/max, distinct, null count
- [ ] Detect EXPLAIN / EXPLAIN ANALYZE → tree render, highlight costly steps

## Release polish (last)
- [ ] Single static binary (release profile already set: lto, strip)
- [ ] goreleaser-equivalent: cargo-dist or release workflow
- [ ] Homebrew tap
- [ ] README demo GIF (vhs)
- [ ] One-line setup for mysql / psql / mycli
