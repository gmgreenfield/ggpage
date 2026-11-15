//! Display-width measurement that is Unicode-aware and ignores ANSI escape
//! sequences. Everything that needs the on-screen width of a string must go
//! through here — never use `str::len()` or `chars().count()`.

use unicode_width::UnicodeWidthChar;

/// An iterator adapter that walks a string's `char`s while skipping ANSI
/// escape sequences (CSI `ESC [ ... final`, plus a lone `ESC x`). Yields
/// `(char, width)` for the visible characters only.
struct VisibleChars<'a> {
    inner: std::str::Chars<'a>,
}

impl<'a> VisibleChars<'a> {
    fn new(s: &'a str) -> Self {
        VisibleChars { inner: s.chars() }
    }
}

impl Iterator for VisibleChars<'_> {
    type Item = (char, usize);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let c = self.inner.next()?;
            if c == '\x1b' {
                // Escape sequence: consume and discard it.
                match self.inner.clone().next() {
                    Some('[') => {
                        // CSI: ESC [ params/intermediates ... final byte 0x40..=0x7e
                        self.inner.next(); // consume '['
                        for d in self.inner.by_ref() {
                            if ('\x40'..='\x7e').contains(&d) {
                                break;
                            }
                        }
                    }
                    Some(']') => {
                        // OSC: ESC ] ... terminated by BEL or ST (ESC \).
                        self.inner.next(); // consume ']'
                        loop {
                            match self.inner.next() {
                                None | Some('\x07') => break,
                                Some('\x1b') => {
                                    // possible ST: ESC \
                                    if self.inner.clone().next() == Some('\\') {
                                        self.inner.next();
                                    }
                                    break;
                                }
                                Some(_) => {}
                            }
                        }
                    }
                    Some(_) => {
                        // Two-char escape: ESC x — drop the following byte.
                        self.inner.next();
                    }
                    None => {}
                }
                continue;
            }
            let w = c.width().unwrap_or(0);
            return Some((c, w));
        }
    }
}

/// On-screen width of `s`, ignoring ANSI escapes and counting East-Asian
/// wide characters as 2 columns.
pub fn display_width(s: &str) -> usize {
    VisibleChars::new(s).map(|(_, w)| w).sum()
}

/// Truncate `s` so its display width is at most `max` columns. ANSI escapes are
/// dropped. Returns the truncated plain string and its actual display width
/// (which may be less than `max` if a wide char straddled the boundary).
pub fn truncate_to_width(s: &str, max: usize) -> (String, usize) {
    let mut out = String::new();
    let mut used = 0;
    for (c, w) in VisibleChars::new(s) {
        if used + w > max {
            break;
        }
        out.push(c);
        used += w;
    }
    (out, used)
}

/// Take the horizontal slice of `s` covering display columns
/// `[start, start + width)`, dropping ANSI escapes. A wide char straddling
/// either boundary is dropped rather than split.
pub fn slice_width(s: &str, start: usize, width: usize) -> String {
    let mut col = 0;
    let mut out = String::new();
    let mut taken = 0;
    for (c, w) in VisibleChars::new(s) {
        if col < start {
            col += w;
            continue;
        }
        if taken + w > width {
            break;
        }
        out.push(c);
        taken += w;
        col += w;
    }
    out
}

/// Pad `s` on the right with spaces until it reaches `width` display columns.
/// If `s` is already wider, it is truncated to `width`.
pub fn pad_to_width(s: &str, width: usize) -> String {
    let w = display_width(s);
    if w >= width {
        let (t, _) = truncate_to_width(s, width);
        t
    } else {
        let mut out = String::with_capacity(s.len() + (width - w));
        out.push_str(s);
        out.extend(std::iter::repeat_n(' ', width - w));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_width_is_byte_count() {
        assert_eq!(display_width("hello"), 5);
        assert_eq!(display_width(""), 0);
    }

    #[test]
    fn cjk_chars_are_double_width() {
        assert_eq!(display_width("中文"), 4);
        assert_eq!(display_width("a中b"), 4);
    }

    #[test]
    fn ansi_sgr_sequences_have_zero_width() {
        // red "hi" reset
        assert_eq!(display_width("\x1b[31mhi\x1b[0m"), 2);
    }

    #[test]
    fn osc_sequences_have_zero_width() {
        // OSC title set, BEL-terminated, then visible text
        assert_eq!(display_width("\x1b]0;title\x07ok"), 2);
    }

    #[test]
    fn truncate_respects_display_width() {
        let (s, w) = truncate_to_width("hello", 3);
        assert_eq!((s.as_str(), w), ("hel", 3));
    }

    #[test]
    fn truncate_does_not_split_wide_char() {
        // "中" is width 2; with max 1 it cannot fit.
        let (s, w) = truncate_to_width("中x", 1);
        assert_eq!((s.as_str(), w), ("", 0));
        // with max 2 it fits exactly.
        let (s, w) = truncate_to_width("中x", 2);
        assert_eq!((s.as_str(), w), ("中", 2));
    }

    #[test]
    fn truncate_strips_ansi() {
        let (s, w) = truncate_to_width("\x1b[31mhello\x1b[0m", 3);
        assert_eq!((s.as_str(), w), ("hel", 3));
    }

    #[test]
    fn pad_fills_to_width() {
        assert_eq!(pad_to_width("ab", 5), "ab   ");
        assert_eq!(pad_to_width("中", 5), "中   ");
    }

    #[test]
    fn pad_truncates_when_too_wide() {
        assert_eq!(pad_to_width("hello", 3), "hel");
    }

    #[test]
    fn slice_takes_middle_columns() {
        assert_eq!(slice_width("abcdef", 2, 3), "cde");
        assert_eq!(slice_width("abcdef", 4, 10), "ef");
        assert_eq!(slice_width("abcdef", 10, 3), "");
    }

    #[test]
    fn slice_drops_straddling_wide_char() {
        // "中" occupies cols 0-1, "x" col 2. Starting at col 1 drops the wide char.
        assert_eq!(slice_width("中x", 1, 5), "x");
    }
}
