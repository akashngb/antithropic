//! `tui::effort_switcher` — horizontal effort slider shown by `/effort`.
//! Matches real Claude Code:
//!
//! ```text
//! Effort
//!
//!                 Speed                        Intelligence
//!                 ─────────────────────────────────────────
//!                                                       ▲
//!                 low   medium   high   xhigh          max
//!
//! ←/→ to adjust · Enter to confirm · Esc to cancel
//! ```
//!
//! Displays the 5 effort rungs `low → medium → high → xhigh → max`,
//! with a `▲` marker above the current selection. The rightmost rung
//! (`max`) renders in the accent color to signal it as the "most
//! intelligence" pole.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::input_box::{ACCENT, DIM};
use super::model_switcher::EFFORTS;

/// State for the effort slider overlay.
#[derive(Debug, Clone)]
pub struct EffortSwitcherState {
    pub selected: usize,
}

impl EffortSwitcherState {
    #[must_use]
    pub fn new(current: &str) -> Self {
        let selected = EFFORTS.iter().position(|e| *e == current).unwrap_or(3);
        Self { selected }
    }

    pub fn select_prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn select_next(&mut self) {
        if self.selected + 1 < EFFORTS.len() {
            self.selected += 1;
        }
    }

    #[must_use]
    pub fn current(&self) -> &'static str {
        EFFORTS[self.selected]
    }

    /// Total rows occupied by the widget.
    #[must_use]
    pub fn visible_rows() -> u16 {
        8
    }
}

/// Render the horizontal slider.
pub fn render_effort(frame: &mut Frame<'_>, area: Rect, state: &EffortSwitcherState) {
    // Compose the option row + compute the column each option starts at
    // so the pole labels + arrow marker line up.
    let mut option_row = String::new();
    let mut column_offsets: Vec<usize> = Vec::with_capacity(EFFORTS.len());
    for (idx, e) in EFFORTS.iter().enumerate() {
        if idx > 0 {
            option_row.push_str("   ");
        }
        column_offsets.push(option_row.chars().count());
        option_row.push_str(e);
    }

    let indent = 16_usize;
    let total_span = option_row.chars().count();

    // Line 1: header
    let header = Line::from(vec![Span::styled(
        "Effort".to_string(),
        Style::default().add_modifier(Modifier::BOLD),
    )]);

    // Line 2: blank
    let blank = Line::from(vec![]);

    // Line 3: pole labels ("Speed" and "Intelligence") — Speed lines up
    // with `low`, Intelligence with `max`.
    let mut pole_str = String::new();
    pole_str.push_str(&" ".repeat(indent));
    pole_str.push_str("Speed");
    let intelligence_target =
        indent + column_offsets[EFFORTS.len() - 1] + EFFORTS[EFFORTS.len() - 1].chars().count();
    while pole_str.chars().count() < intelligence_target.saturating_sub("Intelligence".len()) {
        pole_str.push(' ');
    }
    pole_str.push_str("Intelligence");
    let poles = Line::from(vec![Span::styled(
        pole_str,
        Style::default()
            .add_modifier(Modifier::BOLD)
            .fg(Color::White),
    )]);

    // Line 4: horizontal rule beneath the labels, sized to span from Speed to Intelligence.
    let rule_left = indent;
    let rule_right = indent + total_span;
    let mut rule = String::new();
    rule.push_str(&" ".repeat(rule_left));
    rule.push_str(&"─".repeat(rule_right.saturating_sub(rule_left)));
    let rule_line = Line::from(vec![Span::styled(rule, Style::default().fg(DIM))]);

    // Line 5: arrow row (▲ marker above current selection)
    let arrow_col =
        indent + column_offsets[state.selected] + EFFORTS[state.selected].chars().count() / 2;
    let mut arrow_row = String::new();
    for _ in 0..arrow_col {
        arrow_row.push(' ');
    }
    arrow_row.push('▲');
    let arrow_line = Line::from(vec![Span::styled(
        arrow_row,
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    )]);

    // Line 6: options row itself, with the current selection styled in accent.
    let mut option_spans: Vec<Span<'static>> = Vec::new();
    option_spans.push(Span::raw(" ".repeat(indent)));
    for (idx, e) in EFFORTS.iter().enumerate() {
        if idx > 0 {
            option_spans.push(Span::styled("   ".to_string(), Style::default().fg(DIM)));
        }
        let is_selected = idx == state.selected;
        let is_extreme_max = idx == EFFORTS.len() - 1;
        let style = if is_selected {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else if is_extreme_max {
            Style::default()
                .fg(Color::Rgb(255, 100, 100))
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(DIM)
        };
        option_spans.push(Span::styled((*e).to_string(), style));
    }
    let option_line = Line::from(option_spans);

    // Line 7: blank
    // Line 8: footer
    let footer = Line::from(vec![Span::styled(
        "←/→ to adjust · Enter to confirm · Esc to cancel".to_string(),
        Style::default().fg(DIM),
    )]);

    let lines = vec![
        header,
        blank.clone(),
        poles,
        rule_line,
        arrow_line,
        option_line,
        blank,
        footer,
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn nav_clamps_at_bounds() {
        let mut state = EffortSwitcherState::new("low");
        assert_eq!(state.current(), "low");
        state.select_prev();
        assert_eq!(state.current(), "low"); // clamped
        for _ in 0..10 {
            state.select_next();
        }
        assert_eq!(state.current(), "max"); // clamped
    }

    #[test]
    fn new_seeds_selection_from_current() {
        assert_eq!(EffortSwitcherState::new("medium").selected, 1);
        assert_eq!(EffortSwitcherState::new("max").selected, 4);
        assert_eq!(EffortSwitcherState::new("nonsense").selected, 3); // default xhigh
    }

    #[test]
    fn snapshot_effort_slider_max() {
        let state = EffortSwitcherState::new("max");
        let mut term = Terminal::new(TestBackend::new(80, EffortSwitcherState::visible_rows()))
            .expect("backend");
        term.draw(|frame| render_effort(frame, frame.area(), &state))
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
        insta::assert_snapshot!("effort_slider_max", dump.trim_end());
    }

    #[test]
    fn snapshot_effort_slider_medium() {
        let state = EffortSwitcherState::new("medium");
        let mut term = Terminal::new(TestBackend::new(80, EffortSwitcherState::visible_rows()))
            .expect("backend");
        term.draw(|frame| render_effort(frame, frame.area(), &state))
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
        insta::assert_snapshot!("effort_slider_medium", dump.trim_end());
    }
}
