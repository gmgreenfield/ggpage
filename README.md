# ggpage

A deluxe terminal pager for SQL client output — a drop-in replacement for
`less` that understands tables. It parses `mysql`/MariaDB, `psql`, `sqlite3` and
`mycli` output into a real grid with a **pinned header**, **frozen columns**,
horizontal scrolling, and search — while streaming large result sets and staying
instant to start. Non-table input behaves like plain `less`; when stdout is not
a terminal, input is copied through unchanged so it stays invisible in pipelines.

> Status: **Phase 1 complete** (the must-haves below). Phase 2 differentiators
> are in progress — see [TODO.md](TODO.md).

## Features (Phase 1)

- **Parses the formats SQL clients actually emit:**
  - boxed tables (`+----+----+`), including MariaDB's Unicode box-drawing borders
  - `\G` vertical output
  - tab-separated batch output (`mysql -B`)
  - multiple result sets in one stream, with `N rows in set` / `Query OK` /
    `Empty set` footers preserved
- **Pinned header & frozen first column(s)** that stay put while you scroll.
- **`less` keybindings:** `j`/`k`, `space`/`b`, `d`/`u`, `g`/`G`, `/` `?` `n`
  `N`, arrow keys, `PageUp`/`PageDown`, `Home`/`End`, `-S` to toggle column
  chopping, `q` to quit.
- **Streaming:** renders while reading, never blocks on EOF, bounded memory.
- **Correct Unicode width, ANSI-escape handling, and live terminal resize.**
- **NULL is shown distinctly from the empty string.**

## Install

```sh
# From source (requires a recent Rust toolchain)
cargo install --path .
# or
cargo build --release   # binary at target/release/ggpage
```

## Usage

```sh
# Pipe any SQL client's output in:
mysql -e 'SELECT * FROM users' | ggpage

# Or open a saved result:
ggpage result.txt
```

### One-line setup

**mysql** — inside the client:
```
pager ggpage
```
or persist it in `~/.my.cnf`:
```ini
[mysql]
pager = ggpage
```

**psql** — inside the client:
```
\setenv PAGER ggpage
\pset pager always
```
or in `~/.psqlrc`:
```
\setenv PAGER ggpage
```

**mycli** — in `~/.myclirc`:
```ini
pager = ggpage
```

**Anything via `$PAGER`:**
```sh
export PAGER=ggpage
```

## Keybindings

| Keys | Action |
|------|--------|
| `j` / `k`, `↓` / `↑` | line down / up |
| `space` / `b`, `PgDn` / `PgUp` | page down / up |
| `d` / `u` | half-page down / up |
| `g` / `G`, `Home` / `End` | top / bottom |
| `h` / `l`, `←` / `→` | move column cursor left / right |
| `<` / `>` | first / last column |
| `s` | column stats popup (rows, nulls, distinct, min/max) |
| `v` | view current cell (pretty JSON, hex for binary, or text) |
| `m` | mark current row; mark a second row to diff them |
| `/` `?` | search forward / backward |
| `n` / `N` | next / previous match |
| `-S` | toggle column chopping |
| `x` | export (then `c`/`m`/`j`/`i` to a file; shift for clipboard) |
| `q`, `Ctrl-C` | quit |

### Export

Press `x`, then pick a format — `c` CSV, `m` Markdown, `j` JSON, `i` INSERT.
A lowercase letter writes `ggpage-export.<ext>` in the current directory; the
uppercase letter (`C`/`M`/`J`/`I`) copies to the system clipboard via OSC 52
(works over SSH). NULL is preserved as `null` in JSON and `NULL` in INSERT.

## Development

```sh
cargo test                     # unit + render tests (no TTY needed)
cargo clippy --all-targets -- -D warnings
cargo fmt
```

Architecture and conventions live in [CLAUDE.md](CLAUDE.md). The terminal layer
is deliberately thin: all parsing, layout, scrolling, search and key logic are
pure and unit-tested; rendering is verified with ratatui's `TestBackend`.

## License

MIT
