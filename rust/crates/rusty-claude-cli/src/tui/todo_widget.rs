//! `tui::todo_widget` — live-rerendering task list block. Matches real
//! Claude Code's inline todo widget:
//!
//! ```text
//! ⏺ Update Todos
//!   ⎿  ☒ Explore codebase structure
//!      ☒ Read main.rs
//!      ▶ Rewrite banner
//!      ☐ Add ratatui dependency
//! ```
//!
//! Renders styled `Line`s that `Terminal::insert_before` can stamp into
//! scrollback. Callers own the list of `TodoItem`s and their state
//! transitions; this module is pure formatting.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use super::input_box::ACCENT;

/// Status of a single todo item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

/// One task in the todo widget.
#[derive(Debug, Clone)]
pub struct TodoItem {
    pub status: TodoStatus,
    pub text: String,
}

impl TodoItem {
    #[must_use]
    pub fn pending(text: impl Into<String>) -> Self {
        Self {
            status: TodoStatus::Pending,
            text: text.into(),
        }
    }

    #[must_use]
    pub fn in_progress(text: impl Into<String>) -> Self {
        Self {
            status: TodoStatus::InProgress,
            text: text.into(),
        }
    }

    #[must_use]
    pub fn completed(text: impl Into<String>) -> Self {
        Self {
            status: TodoStatus::Completed,
            text: text.into(),
        }
    }
}

/// Render a todo list as styled lines. First line is the header (`⏺
/// Update Todos`), each subsequent line is `  ⎿  <glyph> <text>` (or
/// just `     <glyph> <text>` for subsequent items — matching real
/// Claude Code's continuation gutter).
#[must_use]
pub fn render_todos(items: &[TodoItem]) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = Vec::with_capacity(items.len() + 1);
    out.push(Line::from(vec![
        Span::styled(
            "⏺ ".to_string(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Update Todos".to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]));
    for (idx, item) in items.iter().enumerate() {
        let gutter = if idx == 0 { "  ⎿  " } else { "     " };
        let (glyph, glyph_style, text_style) = match item.status {
            TodoStatus::Pending => (
                "☐",
                Style::default().fg(Color::DarkGray),
                Style::default().fg(Color::DarkGray),
            ),
            TodoStatus::InProgress => (
                "▶",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                Style::default().fg(ACCENT),
            ),
            TodoStatus::Completed => (
                "☒",
                Style::default().fg(Color::Green),
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::CROSSED_OUT),
            ),
        };
        out.push(Line::from(vec![
            Span::styled(gutter.to_string(), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{glyph} "), glyph_style),
            Span::styled(item.text.clone(), text_style),
        ]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|s| s.content.clone().into_owned())
            .collect::<String>()
    }

    #[test]
    fn header_line_says_update_todos() {
        let items = vec![TodoItem::pending("first")];
        let lines = render_todos(&items);
        assert!(text_of(&lines[0]).contains("Update Todos"));
    }

    #[test]
    fn each_status_uses_correct_glyph() {
        let items = vec![
            TodoItem::completed("done thing"),
            TodoItem::in_progress("working on this"),
            TodoItem::pending("upcoming"),
        ];
        let lines = render_todos(&items);
        assert!(text_of(&lines[1]).contains("☒"));
        assert!(text_of(&lines[2]).contains("▶"));
        assert!(text_of(&lines[3]).contains("☐"));
    }

    #[test]
    fn first_item_uses_arrow_gutter_others_use_indent() {
        let items = vec![
            TodoItem::pending("one"),
            TodoItem::pending("two"),
            TodoItem::pending("three"),
        ];
        let lines = render_todos(&items);
        assert!(text_of(&lines[1]).starts_with("  ⎿"));
        assert!(text_of(&lines[2]).starts_with("     "));
        assert!(text_of(&lines[3]).starts_with("     "));
    }

    #[test]
    fn snapshot_mixed_list() {
        let items = vec![
            TodoItem::completed("Explore codebase structure"),
            TodoItem::completed("Read main.rs"),
            TodoItem::in_progress("Rewrite banner"),
            TodoItem::pending("Add ratatui dependency"),
            TodoItem::pending("Wire status bar"),
        ];
        let lines = render_todos(&items);
        let dump: String = lines
            .iter()
            .map(|l| {
                let mut s = text_of(l);
                s.push('\n');
                s
            })
            .collect();
        insta::assert_snapshot!("todo_widget_mixed", dump.trim_end());
    }
}
