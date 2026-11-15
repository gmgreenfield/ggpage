//! Key-event → action mapping for the interactive table view, following `less`
//! conventions. This module is pure: it maps crossterm key *values* to
//! [`Cmd`]s and never touches the terminal, so every binding is unit-tested.
//!
//! Multi-key modes (search text entry after `/`, option toggle after `-`) are
//! owned by the app/tui layer; this module only reports when one begins
//! ([`Cmd::BeginSearch`], [`Cmd::BeginOption`]) and resolves the option key
//! ([`option`]).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// A concrete navigation/command action applied to the view state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Quit,
    LineDown,
    LineUp,
    HalfPageDown,
    HalfPageUp,
    PageDown,
    PageUp,
    GotoTop,
    GotoBottom,
    ColLeft,
    ColRight,
    ColHome,
    ColEnd,
    ToggleChop,
    NextMatch,
    PrevMatch,
}

/// The outcome of a normal-mode key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cmd {
    /// Apply an action immediately.
    Do(Action),
    /// Enter search-input mode; `forward` is `/` vs `?`.
    BeginSearch { forward: bool },
    /// Enter option mode (a `-` was pressed, as in `less -S`).
    BeginOption,
    /// Enter export mode (`x`): the next key chooses the format/destination.
    BeginExport,
    /// Show column statistics for the current column (`s`).
    ShowStats,
}

/// Map a normal-mode key event to a command, or `None` if unbound.
pub fn normal(ev: KeyEvent) -> Option<Cmd> {
    use Action as A;
    use KeyCode::*;
    let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
    let cmd = match ev.code {
        Char('q') => Cmd::Do(A::Quit),
        Char('c') if ctrl => Cmd::Do(A::Quit),

        Char('j') | Char('e') | Down | Enter => Cmd::Do(A::LineDown),
        Char('k') | Char('y') | Up => Cmd::Do(A::LineUp),

        Char(' ') | Char('f') | PageDown => Cmd::Do(A::PageDown),
        Char('b') | PageUp => Cmd::Do(A::PageUp),
        Char('d') => Cmd::Do(A::HalfPageDown),
        Char('u') => Cmd::Do(A::HalfPageUp),

        Char('g') | Home => Cmd::Do(A::GotoTop),
        Char('G') | End => Cmd::Do(A::GotoBottom),

        Char('h') | Left => Cmd::Do(A::ColLeft),
        Char('l') | Right => Cmd::Do(A::ColRight),
        Char('<') => Cmd::Do(A::ColHome),
        Char('>') => Cmd::Do(A::ColEnd),

        Char('/') => Cmd::BeginSearch { forward: true },
        Char('?') => Cmd::BeginSearch { forward: false },
        Char('n') => Cmd::Do(A::NextMatch),
        Char('N') => Cmd::Do(A::PrevMatch),

        Char('-') => Cmd::BeginOption,
        Char('x') => Cmd::BeginExport,
        Char('s') => Cmd::ShowStats,

        _ => return None,
    };
    Some(cmd)
}

/// Resolve the option character typed after `-` (e.g. `-S`).
pub fn option(c: char) -> Option<Action> {
    match c {
        'S' => Some(Action::ToggleChop),
        _ => None,
    }
}

/// What a key means while editing the search prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchKey {
    Insert(char),
    Backspace,
    Submit,
    Cancel,
}

/// Interpret a key while the search prompt is active.
pub fn search_input(ev: KeyEvent) -> Option<SearchKey> {
    let ctrl = ev.modifiers.contains(KeyModifiers::CONTROL);
    match ev.code {
        KeyCode::Enter => Some(SearchKey::Submit),
        KeyCode::Esc => Some(SearchKey::Cancel),
        KeyCode::Char('c') if ctrl => Some(SearchKey::Cancel),
        KeyCode::Backspace => Some(SearchKey::Backspace),
        KeyCode::Char(c) if !ctrl => Some(SearchKey::Insert(c)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }
    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }
    fn code(k: KeyCode) -> KeyEvent {
        KeyEvent::new(k, KeyModifiers::NONE)
    }

    #[test]
    fn vertical_movement_keys() {
        assert_eq!(normal(key('j')), Some(Cmd::Do(Action::LineDown)));
        assert_eq!(normal(key('k')), Some(Cmd::Do(Action::LineUp)));
        assert_eq!(normal(code(KeyCode::Down)), Some(Cmd::Do(Action::LineDown)));
        assert_eq!(normal(code(KeyCode::Up)), Some(Cmd::Do(Action::LineUp)));
    }

    #[test]
    fn paging_keys() {
        assert_eq!(normal(key(' ')), Some(Cmd::Do(Action::PageDown)));
        assert_eq!(normal(key('f')), Some(Cmd::Do(Action::PageDown)));
        assert_eq!(normal(ctrl('f')), Some(Cmd::Do(Action::PageDown)));
        assert_eq!(normal(key('b')), Some(Cmd::Do(Action::PageUp)));
        assert_eq!(normal(key('d')), Some(Cmd::Do(Action::HalfPageDown)));
        assert_eq!(normal(key('u')), Some(Cmd::Do(Action::HalfPageUp)));
        assert_eq!(
            normal(code(KeyCode::PageDown)),
            Some(Cmd::Do(Action::PageDown))
        );
        assert_eq!(normal(code(KeyCode::PageUp)), Some(Cmd::Do(Action::PageUp)));
    }

    #[test]
    fn goto_top_and_bottom() {
        assert_eq!(normal(key('g')), Some(Cmd::Do(Action::GotoTop)));
        assert_eq!(normal(key('G')), Some(Cmd::Do(Action::GotoBottom)));
        assert_eq!(normal(code(KeyCode::Home)), Some(Cmd::Do(Action::GotoTop)));
        assert_eq!(
            normal(code(KeyCode::End)),
            Some(Cmd::Do(Action::GotoBottom))
        );
    }

    #[test]
    fn horizontal_keys() {
        assert_eq!(normal(key('h')), Some(Cmd::Do(Action::ColLeft)));
        assert_eq!(normal(key('l')), Some(Cmd::Do(Action::ColRight)));
        assert_eq!(normal(code(KeyCode::Left)), Some(Cmd::Do(Action::ColLeft)));
        assert_eq!(
            normal(code(KeyCode::Right)),
            Some(Cmd::Do(Action::ColRight))
        );
        assert_eq!(normal(key('<')), Some(Cmd::Do(Action::ColHome)));
        assert_eq!(normal(key('>')), Some(Cmd::Do(Action::ColEnd)));
    }

    #[test]
    fn quit_keys() {
        assert_eq!(normal(key('q')), Some(Cmd::Do(Action::Quit)));
        assert_eq!(normal(ctrl('c')), Some(Cmd::Do(Action::Quit)));
    }

    #[test]
    fn search_and_option_begin() {
        assert_eq!(normal(key('/')), Some(Cmd::BeginSearch { forward: true }));
        assert_eq!(normal(key('?')), Some(Cmd::BeginSearch { forward: false }));
        assert_eq!(normal(key('n')), Some(Cmd::Do(Action::NextMatch)));
        assert_eq!(normal(key('N')), Some(Cmd::Do(Action::PrevMatch)));
        assert_eq!(normal(key('-')), Some(Cmd::BeginOption));
        assert_eq!(normal(key('x')), Some(Cmd::BeginExport));
        assert_eq!(normal(key('s')), Some(Cmd::ShowStats));
    }

    #[test]
    fn option_s_toggles_chop() {
        assert_eq!(option('S'), Some(Action::ToggleChop));
        assert_eq!(option('x'), None);
    }

    #[test]
    fn unbound_key_is_none() {
        assert_eq!(normal(key('z')), None);
    }

    #[test]
    fn search_input_editing() {
        assert_eq!(search_input(key('a')), Some(SearchKey::Insert('a')));
        assert_eq!(
            search_input(code(KeyCode::Backspace)),
            Some(SearchKey::Backspace)
        );
        assert_eq!(search_input(code(KeyCode::Enter)), Some(SearchKey::Submit));
        assert_eq!(search_input(code(KeyCode::Esc)), Some(SearchKey::Cancel));
        assert_eq!(search_input(ctrl('c')), Some(SearchKey::Cancel));
    }
}
