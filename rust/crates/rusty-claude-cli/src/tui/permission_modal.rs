//! `tui::permission_modal` — inline modal shown when a gated tool
//! (Bash, WebFetch, etc.) needs user approval. Numbered 3-option
//! layout matching real Claude Code:
//!
//! ```text
//! ╭─────────────────────────────────────────────────╮
//! │ Bash command                                    │
//! │   rm -rf ./tmp                                  │
//! │                                                 │
//! │ Do you want to proceed?                         │
//! │ ▶ 1. Yes                                        │
//! │   2. Yes, and don't ask again for this session  │
//! │   3. No, and tell Claude what to do differently │
//! │      (esc)                                      │
//! ╰─────────────────────────────────────────────────╯
//! ```

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::input_box::{ACCENT, DIM};

/// Selected option in the modal. `Continue` is only ever used to represent
/// "user hasn't decided yet" during rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionChoice {
    Yes,
    YesRemember,
    No,
}

/// State for one permission prompt. The event loop pushes one of these
/// into `AppState` when the runtime asks for approval; user's response
/// resolves into a `PermissionChoice`.
#[derive(Debug, Clone)]
pub struct PermissionModalState {
    pub tool_name: String,
    pub summary: String,
    /// 0 = Yes, 1 = Yes-remember, 2 = No. Arrow keys wrap.
    pub selected: usize,
}

impl PermissionModalState {
    #[must_use]
    pub fn new(tool_name: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            summary: summary.into(),
            selected: 0,
        }
    }

    pub fn select_prev(&mut self) {
        self.selected = if self.selected == 0 {
            2
        } else {
            self.selected - 1
        };
    }

    pub fn select_next(&mut self) {
        self.selected = (self.selected + 1) % 3;
    }

    #[must_use]
    pub fn resolve(&self) -> PermissionChoice {
        match self.selected {
            0 => PermissionChoice::Yes,
            1 => PermissionChoice::YesRemember,
            _ => PermissionChoice::No,
        }
    }

    /// Number-key shortcut → choice. Returns `None` for unrelated keys.
    #[must_use]
    pub fn choice_for_digit(digit: char) -> Option<PermissionChoice> {
        match digit {
            '1' => Some(PermissionChoice::Yes),
            '2' => Some(PermissionChoice::YesRemember),
            '3' => Some(PermissionChoice::No),
            _ => None,
        }
    }

    /// Rows the modal occupies including borders (fixed at 10 lines to
    /// accommodate wrapping on the No option).
    #[must_use]
    pub fn visible_rows() -> u16 {
        10
    }
}

pub fn render_modal(frame: &mut Frame<'_>, area: Rect, state: &PermissionModalState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT));
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![Span::styled(
        format!("{} command", state.tool_name),
        Style::default().add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(state.summary.clone(), Style::default().fg(ACCENT)),
    ]));
    lines.push(Line::from(vec![]));
    lines.push(Line::from(vec![Span::styled(
        "Do you want to proceed?".to_string(),
        Style::default().add_modifier(Modifier::BOLD),
    )]));
    lines.push(option_line(0, "1. Yes", state.selected == 0));
    lines.push(option_line(
        1,
        "2. Yes, and don't ask again for this session",
        state.selected == 1,
    ));
    lines.push(option_line(
        2,
        "3. No, and tell Claude what to do differently (esc)",
        state.selected == 2,
    ));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn option_line(_idx: usize, label: &str, selected: bool) -> Line<'static> {
    let marker = if selected { "▶ " } else { "  " };
    let style = if selected {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(DIM)
    };
    Line::from(vec![
        Span::styled(marker.to_string(), style),
        Span::styled(label.to_string(), style),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn nav_wraps_around() {
        let mut state = PermissionModalState::new("Bash", "rm -rf /tmp");
        state.select_prev();
        assert_eq!(state.selected, 2);
        state.select_next();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn resolve_maps_selection() {
        let mut state = PermissionModalState::new("Bash", "rm");
        assert_eq!(state.resolve(), PermissionChoice::Yes);
        state.selected = 1;
        assert_eq!(state.resolve(), PermissionChoice::YesRemember);
        state.selected = 2;
        assert_eq!(state.resolve(), PermissionChoice::No);
    }

    #[test]
    fn digit_shortcut() {
        assert_eq!(
            PermissionModalState::choice_for_digit('1'),
            Some(PermissionChoice::Yes)
        );
        assert_eq!(
            PermissionModalState::choice_for_digit('3'),
            Some(PermissionChoice::No)
        );
        assert!(PermissionModalState::choice_for_digit('9').is_none());
    }

    #[test]
    fn snapshot_modal_for_bash() {
        let state = PermissionModalState::new("Bash", "rm -rf ./tmp");
        let rows = PermissionModalState::visible_rows();
        let mut term = Terminal::new(TestBackend::new(60, rows)).expect("backend");
        term.draw(|frame| render_modal(frame, frame.area(), &state))
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
        insta::assert_snapshot!("permission_modal_bash", dump.trim_end());
    }
}
