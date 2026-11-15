//! Input plumbing: a background reader that streams parse events over a
//! channel (so the UI never blocks on EOF), plus the non-TTY raw copy path.

use std::io::{BufRead, Read, Write};
use std::sync::mpsc::{channel, Receiver};
use std::thread;

use crate::parse::{Event, Parser};

/// A message from the reader thread to the UI.
pub enum Msg {
    /// A parse event produced from the stream.
    Event(Event),
    /// End of input reached; no more events will arrive.
    Done,
}

/// Spawn a thread that reads `reader` line by line, parses incrementally, and
/// sends [`Msg`]s over the returned channel. Returns immediately; the UI drains
/// the channel as data arrives.
pub fn spawn_reader<R: BufRead + Send + 'static>(mut reader: R) -> Receiver<Msg> {
    let (tx, rx) = channel();
    thread::spawn(move || {
        let mut parser = Parser::new();
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    let trimmed = line.strip_suffix('\n').unwrap_or(&line);
                    let trimmed = trimmed.strip_suffix('\r').unwrap_or(trimmed);
                    for ev in parser.push_line(trimmed) {
                        if tx.send(Msg::Event(ev)).is_err() {
                            return; // receiver gone; stop
                        }
                    }
                }
                Err(_) => break,
            }
        }
        for ev in parser.finish() {
            let _ = tx.send(Msg::Event(ev));
        }
        let _ = tx.send(Msg::Done);
    });
    rx
}

/// Copy `reader` to `writer` verbatim — used when stdout is not a TTY so the
/// tool is transparent in pipelines.
pub fn raw_copy(mut reader: impl Read, mut writer: impl Write) -> std::io::Result<()> {
    std::io::copy(&mut reader, &mut writer)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn collect(rx: Receiver<Msg>) -> Vec<Event> {
        let mut evs = Vec::new();
        for msg in rx {
            match msg {
                Msg::Event(e) => evs.push(e),
                Msg::Done => break,
            }
        }
        evs
    }

    #[test]
    fn reader_streams_parse_events_then_done() {
        let input = "\
+----+
| a  |
+----+
|  1 |
+----+
1 row in set
";
        let rx = spawn_reader(Cursor::new(input.as_bytes().to_vec()));
        let evs = collect(rx);
        let tables = crate::parse::collect_tables(&evs);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].columns, vec!["a"]);
        assert_eq!(tables[0].nrows(), 1);
    }

    #[test]
    fn reader_handles_crlf_line_endings() {
        let input = "x\ty\r\n1\t2\r\n";
        let rx = spawn_reader(Cursor::new(input.as_bytes().to_vec()));
        let tables = crate::parse::collect_tables(&collect(rx));
        assert_eq!(tables[0].columns, vec!["x", "y"]);
    }

    #[test]
    fn raw_copy_is_verbatim() {
        let input = b"hello\nworld\n";
        let mut out = Vec::new();
        raw_copy(Cursor::new(&input[..]), &mut out).unwrap();
        assert_eq!(out, input);
    }
}
