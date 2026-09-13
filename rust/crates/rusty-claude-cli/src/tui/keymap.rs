//! `tui::keymap` — key event → semantic action mapping. Pure function
//! so we can unit-test the full binding matrix without a live REPL.
//!
//! Slice 6 additions:
//! - `Shift+Tab`  — cycle permission mode
//! - `Esc Esc`    — rewind one turn (dispatch to `/rewind` handler)
//! - `?`          — toggle shortcuts overlay
//! - `Ctrl+R`     — reverse-search prompt history

use crossterm::event::{KeyCode, KeyModifiers};

/// A semantic action extracted from a raw key event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Insert a character (input handling elsewhere).
    Char(char),
    Backspace,
    Enter,
    ShiftEnter,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    Tab,
    Escape,
    /// Second Escape within the rewind window.
    RewindTurn,
    /// Cycle to the next permission mode (`Shift+Tab`).
    CyclePermissionMode,
    /// Open/close the `?` shortcut overlay.
    ToggleShortcuts,
    /// Ctrl+R reverse-history search.
    ReverseSearch,
    /// Ctrl+C.
    Cancel,
    /// Ctrl+D.
    Eof,
    /// Nothing to do (unbound key).
    None,
}

/// Map a single key event to an action. Some actions (like `Esc Esc`)
/// require multi-key state — the caller keeps track of the last-seen
/// Escape and calls [`escalate_double_esc`] as needed.
#[must_use]
pub fn map(code: KeyCode, mods: KeyModifiers) -> Action {
    if mods.contains(KeyModifiers::CONTROL) {
        return match code {
            KeyCode::Char('c') => Action::Cancel,
            KeyCode::Char('d') => Action::Eof,
            KeyCode::Char('r') => Action::ReverseSearch,
            _ => Action::None,
        };
    }
    match code {
        KeyCode::BackTab => Action::CyclePermissionMode,
        KeyCode::Tab if mods.contains(KeyModifiers::SHIFT) => Action::CyclePermissionMode,
        KeyCode::Enter if mods.contains(KeyModifiers::SHIFT) => Action::ShiftEnter,
        KeyCode::Enter => Action::Enter,
        KeyCode::Char('?') => Action::ToggleShortcuts,
        KeyCode::Char(c) => Action::Char(c),
        KeyCode::Backspace => Action::Backspace,
        KeyCode::Left => Action::Left,
        KeyCode::Right => Action::Right,
        KeyCode::Up => Action::Up,
        KeyCode::Down => Action::Down,
        KeyCode::Home => Action::Home,
        KeyCode::End => Action::End,
        KeyCode::Tab => Action::Tab,
        KeyCode::Esc => Action::Escape,
        _ => Action::None,
    }
}

/// If the last action was `Escape` and now we see another `Escape`
/// within the rewind window, escalate to `RewindTurn`. Caller is
/// responsible for the timing window (typical: 500ms).
#[must_use]
pub fn escalate_double_esc(prev: Option<&Action>, current: Action) -> Action {
    if matches!(prev, Some(Action::Escape)) && matches!(current, Action::Escape) {
        Action::RewindTurn
    } else {
        current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_c_maps_to_cancel() {
        assert_eq!(
            map(KeyCode::Char('c'), KeyModifiers::CONTROL),
            Action::Cancel
        );
    }

    #[test]
    fn ctrl_r_maps_to_reverse_search() {
        assert_eq!(
            map(KeyCode::Char('r'), KeyModifiers::CONTROL),
            Action::ReverseSearch
        );
    }

    #[test]
    fn backtab_and_shift_tab_both_cycle_permission_mode() {
        assert_eq!(
            map(KeyCode::BackTab, KeyModifiers::NONE),
            Action::CyclePermissionMode
        );
        assert_eq!(
            map(KeyCode::Tab, KeyModifiers::SHIFT),
            Action::CyclePermissionMode
        );
    }

    #[test]
    fn question_mark_toggles_shortcuts() {
        assert_eq!(
            map(KeyCode::Char('?'), KeyModifiers::NONE),
            Action::ToggleShortcuts
        );
    }

    #[test]
    fn double_escape_escalates_to_rewind() {
        let first = map(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(first, Action::Escape);
        let second = map(KeyCode::Esc, KeyModifiers::NONE);
        let escalated = escalate_double_esc(Some(&first), second);
        assert_eq!(escalated, Action::RewindTurn);
    }

    #[test]
    fn single_escape_after_non_escape_does_not_rewind() {
        let escalated = escalate_double_esc(Some(&Action::Char('a')), Action::Escape);
        assert_eq!(escalated, Action::Escape);
    }

    #[test]
    fn shift_enter_maps_to_newline() {
        assert_eq!(
            map(KeyCode::Enter, KeyModifiers::SHIFT),
            Action::ShiftEnter
        );
    }
}
