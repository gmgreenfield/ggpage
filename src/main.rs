// Several pure-module APIs are currently exercised only by unit tests, or are
// reserved for phase-2 features (column ops, export, stats). They are
// intentionally kept; revisit and tighten before the 1.0 release.
#![allow(dead_code)]

mod app;
mod cell;
mod clip;
mod diff;
mod export;
mod keys;
mod parse;
mod source;
mod stats;
mod table;
mod tui;
mod view;
mod width;

use std::fs::File;
use std::io::{self, BufReader, IsTerminal, Read};

use app::App;

/// Parsed command-line arguments (a small `less`-compatible subset).
struct Args {
    /// First file argument, if any (otherwise read stdin).
    file: Option<String>,
    /// `-S`: chop long lines / cap column width (default on for tables).
    chop: bool,
    /// Show version and exit.
    version: bool,
    /// Show help and exit.
    help: bool,
}

fn parse_args<I: Iterator<Item = String>>(args: I) -> Args {
    let mut out = Args {
        file: None,
        chop: true,
        help: false,
        version: false,
    };
    for arg in args {
        match arg.as_str() {
            "-V" | "--version" => out.version = true,
            "-h" | "--help" => out.help = true,
            "-S" => out.chop = true,
            // Ignore other single-letter less flags and `+commands` gracefully
            // so ggpage is a forgiving drop-in for `less`/`$PAGER`.
            s if s.starts_with('+') => {}
            s if s.starts_with('-') && s.len() > 1 => {}
            s => {
                if out.file.is_none() {
                    out.file = Some(s.to_string());
                }
            }
        }
    }
    out
}

const HELP: &str = "\
ggpage — a deluxe terminal pager for SQL client output.

USAGE:
    ggpage [FILE]
    <sql-client> | ggpage

OPTIONS:
    -S            chop long lines / cap column width (default)
    -h, --help    show this help
    -V, --version show version

Inside mysql:   pager ggpage
Via env:        export PAGER=ggpage
";

fn main() -> io::Result<()> {
    let args = parse_args(std::env::args().skip(1));

    if args.help {
        print!("{HELP}");
        return Ok(());
    }
    if args.version {
        println!("ggpage {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    // Open the input source: a file argument, or stdin.
    let input: Box<dyn Read + Send> = match &args.file {
        Some(path) => Box::new(File::open(path)?),
        None => Box::new(io::stdin()),
    };

    // If stdout is not a terminal, be transparent: copy input to output.
    if !io::stdout().is_terminal() {
        return source::raw_copy(input, io::stdout().lock());
    }

    // Interactive, but nothing is piped in and no file given: nothing to page.
    if args.file.is_none() && io::stdin().is_terminal() {
        eprintln!("ggpage: no input — pipe data in or pass a file (see --help)");
        return Ok(());
    }

    let rx = source::spawn_reader(BufReader::new(input));
    let mut app = App::new(80, 24);
    app.set_chop(args.chop);
    tui::run(&mut app, rx)
}
