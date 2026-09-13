//! `tui::slash_menu` — floating slash-command menu.
//!
//! When the user's input starts with `/`, the event loop opens this
//! overlay ABOVE the input box. Renders up to 8 matching commands with
//! their one-line descriptions; the top row is highlighted and can be
//! moved with Up/Down. Enter completes-and-submits, Tab completes into
//! the buffer, Esc closes.

use commands::{slash_command_specs, SlashCommandSpec};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};
use ratatui::Frame;

use super::fuzzy;
use super::input_box::{ACCENT, DIM};

/// Max rows the menu ever renders. Wide-open list of every registered
/// slash command; scrolls on overflow so the pinned viewport stays a
/// consistent size.
pub const MENU_MAX_ROWS: usize = 12;

/// Immutable snapshot of the menu's active filter + selection. Owned by
/// the event loop; passed to `render_menu` each frame.
#[derive(Debug, Clone)]
pub struct SlashMenuState {
    /// The query typed after the leading `/`.
    pub query: String,
    /// Selected row within the filtered list (0-based).
    pub selected: usize,
    /// Cached filtered slice (indexes into curated commands, in display
    /// order). Refreshed by `apply_query`.
    filtered: Vec<&'static SlashCommandSpec>,
}

impl SlashMenuState {
    #[must_use]
    pub fn new() -> Self {
        let mut state = Self {
            query: String::new(),
            selected: 0,
            filtered: Vec::new(),
        };
        state.apply_query();
        state
    }

    /// Rebuild `filtered` from `query` against EVERY registered slash
    /// command (from `commands::slash_command_specs`). Empty query
    /// returns all commands in their canonical order; non-empty runs
    /// them through the fuzzy scorer.
    pub fn apply_query(&mut self) {
        let all: Vec<&'static SlashCommandSpec> = slash_command_specs().iter().collect();
        self.filtered = if self.query.is_empty() {
            all
        } else {
            let names: Vec<&'static str> = all.iter().map(|s| s.name).collect();
            fuzzy::rank(&self.query, names.iter().copied())
                .into_iter()
                .map(|(idx, _)| all[idx])
                .collect()
        };
        if self.filtered.is_empty() {
            self.selected = 0;
        } else if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len() - 1;
        }
    }

    /// Currently selected spec, or `None` when the filter matched nothing.
    #[must_use]
    pub fn current(&self) -> Option<&'static SlashCommandSpec> {
        self.filtered.get(self.selected).copied()
    }

    /// Move selection up (with wrap-around).
    pub fn select_prev(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected = if self.selected == 0 {
            self.filtered.len() - 1
        } else {
            self.selected - 1
        };
    }

    /// Move selection down (with wrap-around).
    pub fn select_next(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.selected = (self.selected + 1) % self.filtered.len();
    }

    /// Visible rows (bounded by `MENU_MAX_ROWS`).
    #[must_use]
    pub fn visible_len(&self) -> usize {
        self.filtered.len().min(MENU_MAX_ROWS)
    }

    /// Total rows the menu widget occupies. No borders — matches
    /// real Claude Code's minimal inline list overlay above the input.
    #[must_use]
    pub fn visible_rows(&self) -> u16 {
        u16::try_from(self.visible_len()).unwrap_or(1).max(1)
    }
}

impl Default for SlashMenuState {
    fn default() -> Self {
        Self::new()
    }
}

/// Renders the borderless floating menu into `area`. Caller carves out
/// the right `Rect` — typically directly above the input box. No block
/// / border is drawn (that was reading as a "second box" nested against
/// the input box's own border).
pub fn render_menu(frame: &mut Frame<'_>, area: Rect, state: &SlashMenuState) {
    let visible = state.visible_len();
    let total = state.filtered.len();
    let start = if total <= visible {
        0
    } else if state.selected < visible {
        0
    } else {
        (state.selected + 1).saturating_sub(visible)
    };

    let items: Vec<ListItem<'_>> = state
        .filtered
        .iter()
        .enumerate()
        .skip(start)
        .take(visible)
        .map(|(idx, spec)| build_row(spec, idx == state.selected))
        .collect();

    let mut list_state = ListState::default();
    if !state.filtered.is_empty() {
        list_state.select(Some(state.selected.saturating_sub(start)));
    }
    let list = List::new(items);
    frame.render_stateful_widget(list, area, &mut list_state);
}

fn build_row(spec: &SlashCommandSpec, selected: bool) -> ListItem<'static> {
    let marker = if selected { "▶ " } else { "  " };
    let marker_style = if selected {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DIM)
    };
    let name_style = if selected {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let name = format!("/{:<14}", spec.name);
    let summary = spec.summary.to_string();
    let line = Line::from(vec![
        Span::styled(marker.to_string(), marker_style),
        Span::styled(name, name_style),
        Span::styled(summary, Style::default().fg(DIM)),
    ]);
    ListItem::new(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn menu_starts_populated_when_query_empty() {
        let state = SlashMenuState::new();
        assert!(!state.filtered.is_empty());
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn apply_query_filters_and_clamps_selection() {
        let mut state = SlashMenuState::new();
        state.selected = 20; // Off the end.
        state.query = "mod".to_string();
        state.apply_query();
        assert!(!state.filtered.is_empty());
        assert!(state.selected < state.filtered.len());
        // Top hit should be `/model`.
        assert_eq!(state.current().map(|s| s.name), Some("model"));
    }

    #[test]
    fn select_next_wraps() {
        let mut state = SlashMenuState::new();
        let len = state.filtered.len();
        for _ in 0..len {
            state.select_next();
        }
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn select_prev_from_zero_wraps_to_end() {
        let mut state = SlashMenuState::new();
        state.select_prev();
        assert_eq!(state.selected, state.filtered.len() - 1);
    }

    #[test]
    fn render_menu_snapshot_default_query() {
        let state = SlashMenuState::new();
        let mut term = Terminal::new(TestBackend::new(60, state.visible_rows())).expect("backend");
        term.draw(|frame| render_menu(frame, frame.area(), &state))
            .expect("draw");
        let buf = term.backend().buffer();
        let dump: String = (0..buf.area.height)
            .map(|y| {
                let mut row: String = (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect();
                row = row.trim_end().to_string();
                row.push('\n');
                row
            })
            .collect();
        insta::assert_snapshot!("slash_menu_default", dump.trim_end());
    }

    #[test]
    fn render_menu_snapshot_filtered_query() {
        let mut state = SlashMenuState::new();
        state.query = "mod".to_string();
        state.apply_query();
        let mut term = Terminal::new(TestBackend::new(60, state.visible_rows())).expect("backend");
        term.draw(|frame| render_menu(frame, frame.area(), &state))
            .expect("draw");
        let buf = term.backend().buffer();
        let dump: String = (0..buf.area.height)
            .map(|y| {
                let mut row: String = (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect();
                row = row.trim_end().to_string();
                row.push('\n');
                row
            })
            .collect();
        insta::assert_snapshot!("slash_menu_filtered_mod", dump.trim_end());
    }
}
