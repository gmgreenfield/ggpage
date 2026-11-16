//! Rich rendering of a single cell value for the inspection popup:
//! pretty-printed JSON, a hex dump for binary-ish values, or wrapped plain
//! text. Pure and unit-tested.

/// Produce `(kind, lines)` for viewing `value`, where `kind` is a short label
/// ("json" / "hex" / "text"). `wrap` is the max line width for plain text.
pub fn cell_view(value: &str, wrap: usize) -> (&'static str, Vec<String>) {
    if looks_binary(value) {
        ("hex", hex_dump(value.as_bytes()))
    } else if let Some(pretty) = pretty_json(value) {
        ("json", pretty.lines().map(String::from).collect())
    } else {
        ("text", wrap_text(value, wrap.max(8)))
    }
}

/// True if the value contains control characters other than tab/newline/CR,
/// or the Unicode replacement char — i.e. it's better shown as hex.
fn looks_binary(s: &str) -> bool {
    s.chars()
        .any(|c| c == '\u{FFFD}' || ((c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r'))
}

/// Reformat `s` as indented JSON if it looks like a JSON object/array. This is
/// a structural reflow (string-aware), not a validator — good enough for a
/// readable popup. Returns `None` if it doesn't look like JSON.
pub fn pretty_json(s: &str) -> Option<String> {
    let t = s.trim();
    let looks_json =
        (t.starts_with('{') && t.ends_with('}')) || (t.starts_with('[') && t.ends_with(']'));
    if !looks_json || t.len() < 2 {
        return None;
    }

    let mut out = String::new();
    let mut indent = 0usize;
    let mut in_str = false;
    let mut esc = false;
    let mut chars = t.chars().peekable();

    let newline = |out: &mut String, indent: usize| {
        out.push('\n');
        out.push_str(&"  ".repeat(indent));
    };

    while let Some(c) = chars.next() {
        if in_str {
            out.push(c);
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
            }
            '{' | '[' => {
                out.push(c);
                // Keep empty containers inline.
                let mut look = chars.clone();
                while matches!(look.peek(), Some(w) if w.is_whitespace()) {
                    look.next();
                }
                if matches!(look.peek(), Some('}') | Some(']')) {
                    // consume whitespace + the closer and emit inline
                    while matches!(chars.peek(), Some(w) if w.is_whitespace()) {
                        chars.next();
                    }
                    if let Some(close) = chars.next() {
                        out.push(close);
                    }
                } else {
                    indent += 1;
                    newline(&mut out, indent);
                }
            }
            '}' | ']' => {
                indent = indent.saturating_sub(1);
                newline(&mut out, indent);
                out.push(c);
            }
            ',' => {
                out.push(c);
                newline(&mut out, indent);
            }
            ':' => out.push_str(": "),
            c if c.is_whitespace() => {} // collapse insignificant whitespace
            c => out.push(c),
        }
    }
    Some(out)
}

/// An `xxd`-style hex dump.
fn hex_dump(bytes: &[u8]) -> Vec<String> {
    let mut lines = Vec::new();
    for (i, chunk) in bytes.chunks(16).enumerate() {
        let mut hex = String::new();
        for (j, b) in chunk.iter().enumerate() {
            hex.push_str(&format!("{b:02x} "));
            if j == 7 {
                hex.push(' ');
            }
        }
        let ascii: String = chunk
            .iter()
            .map(|&b| {
                if (0x20..0x7f).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        lines.push(format!("{:08x}  {:<49} {}", i * 16, hex, ascii));
    }
    if lines.is_empty() {
        lines.push("(empty)".to_string());
    }
    lines
}

/// Break `s` into lines no wider than `width` (by char count), preserving any
/// existing newlines.
fn wrap_text(s: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in s.split('\n') {
        if line.is_empty() {
            out.push(String::new());
            continue;
        }
        let mut cur = String::new();
        let mut n = 0;
        for c in line.chars() {
            cur.push(c);
            n += 1;
            if n >= width {
                out.push(std::mem::take(&mut cur));
                n = 0;
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_json_indents_objects() {
        let (kind, lines) = cell_view(r#"{"a":1,"b":[2,3]}"#, 80);
        assert_eq!(kind, "json");
        let joined = lines.join("\n");
        assert_eq!(joined, "{\n  \"a\": 1,\n  \"b\": [\n    2,\n    3\n  ]\n}");
    }

    #[test]
    fn pretty_json_keeps_empty_containers_inline() {
        let out = pretty_json("{\"a\":{},\"b\":[]}").unwrap();
        assert_eq!(out, "{\n  \"a\": {},\n  \"b\": []\n}");
    }

    #[test]
    fn json_detection_ignores_non_json() {
        assert!(pretty_json("hello world").is_none());
        assert!(pretty_json("123").is_none());
        let (kind, _) = cell_view("just text", 80);
        assert_eq!(kind, "text");
    }

    #[test]
    fn json_string_with_braces_is_not_split() {
        // Braces inside a string must not affect indentation.
        let out = pretty_json(r#"{"k":"a{b}c"}"#).unwrap();
        assert_eq!(out, "{\n  \"k\": \"a{b}c\"\n}");
    }

    #[test]
    fn binary_value_renders_as_hex() {
        let (kind, lines) = cell_view("A\u{0}B", 80);
        assert_eq!(kind, "hex");
        assert!(lines[0].starts_with("00000000"));
        assert!(lines[0].contains("41 00 42"));
        assert!(lines[0].trim_end().ends_with("A.B"));
    }

    #[test]
    fn long_text_wraps() {
        let (kind, lines) = cell_view(&"x".repeat(25), 10);
        assert_eq!(kind, "text");
        assert_eq!(lines.len(), 3); // 10 + 10 + 5
        assert_eq!(lines[0], "x".repeat(10));
    }
}
