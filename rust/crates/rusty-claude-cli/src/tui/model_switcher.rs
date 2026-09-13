//! `tui::model_switcher` — full-viewport model picker matching real
//! Claude Code's `/model` overlay. Evil Claude model roster:
//! `fumble 5.1`, `dingus 5.0`, `sonion 4.7`, `hackyou 5.0`.
//!
//! Layout:
//!
//! ```text
//! Select model
//! Switch between models. Applies to this session only. For other/previous model names, specify with --model.
//!
//! ❯ 1. Default (recommended) ✓  fumble 5.1 with 1M context · Most capable for complex work
//!   2. Sonion                    sonion 4.7 · Best for everyday tasks
//!   3. Dingus                    dingus 5.0 · Fastest for quick answers
//!   4. Hackyou                   hackyou 5.0 · Experimental
//!
//! ● xHigh effort (default) ←/→ to adjust
//!
//! Use /fast to turn on Fast mode (fumble 5.1).
//!
//! Enter to confirm · d to set as default for new sessions · Esc to cancel
//! ```

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::input_box::{ACCENT, DIM};

/// One row in the model picker.
#[derive(Debug, Clone, Copy)]
pub struct ModelOption {
    pub label: &'static str,
    pub short_name: &'static str,
    pub id: &'static str,
    pub full_name: &'static str,
    pub tagline: &'static str,
}

/// Evil Claude model roster. Order matches the display list — index 0
/// is the default. All `id`s intentionally resolve to the same real
/// underlying model (Sonnet 4.6) — the joke names are cosmetic; the
/// TUI never swaps the actual API model, only the display label shown
/// in the status bar. Nothing in `LiveCli::run_turn` is disturbed by a
/// choice here.
pub const MODELS: &[ModelOption] = &[
    ModelOption {
        label: "Fumble (recommended)",
        short_name: "fumble 5.1",
        id: "anthropic/claude-sonnet-4-6",
        full_name: "fumble 5.1 with 1M context",
        tagline: "Most capable for complex work",
    },
    ModelOption {
        label: "Sonion",
        short_name: "sonion 4.7",
        id: "anthropic/claude-sonnet-4-6",
        full_name: "sonion 4.7",
        tagline: "Best for everyday tasks",
    },
    ModelOption {
        label: "Dingus",
        short_name: "dingus 5.0",
        id: "anthropic/claude-sonnet-4-6",
        full_name: "dingus 5.0",
        tagline: "Fastest for quick answers",
    },
    ModelOption {
        label: "Hackyou",
        short_name: "hackyou 5.0",
        id: "anthropic/claude-sonnet-4-6",
        full_name: "hackyou 5.0",
        tagline: "Experimental — off-menu chaos",
    },
    ModelOption {
        label: "Kirkle",
        short_name: "kirkle 3.2",
        id: "anthropic/claude-sonnet-4-6",
        full_name: "kirkle 3.2",
        tagline: "Petty, precise, occasionally correct",
    },
];

/// Effort levels arranged for `←/→` scrubbing. `max` is the extreme
/// right rung matching real Claude Code's `/effort` slider.
pub const EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// State for the model switcher overlay.
#[derive(Debug, Clone)]
pub struct ModelSwitcherState {
    pub selected: usize,
    pub effort_idx: usize,
    /// Column for the `● xHigh effort` label so the label doesn't
    /// flicker when the effort changes.
    pub is_default_choice: bool,
}

impl ModelSwitcherState {
    #[must_use]
    pub fn new(current_effort: &str) -> Self {
        let effort_idx = EFFORTS
            .iter()
            .position(|e| *e == current_effort)
            .unwrap_or(3); // default xhigh
        Self {
            selected: 0,
            effort_idx,
            is_default_choice: true,
        }
    }

    pub fn select_prev(&mut self) {
        self.selected = if self.selected == 0 {
            MODELS.len() - 1
        } else {
            self.selected - 1
        };
    }

    pub fn select_next(&mut self) {
        self.selected = (self.selected + 1) % MODELS.len();
    }

    pub fn effort_prev(&mut self) {
        self.effort_idx = if self.effort_idx == 0 {
            EFFORTS.len() - 1
        } else {
            self.effort_idx - 1
        };
    }

    pub fn effort_next(&mut self) {
        self.effort_idx = (self.effort_idx + 1) % EFFORTS.len();
    }

    #[must_use]
    pub fn current_model(&self) -> ModelOption {
        MODELS[self.selected]
    }

    #[must_use]
    pub fn current_effort(&self) -> &'static str {
        EFFORTS[self.effort_idx]
    }

    /// Number-key shortcut. Returns `Some(index)` for 1-4, `None`
    /// otherwise.
    #[must_use]
    pub fn digit_to_index(digit: char) -> Option<usize> {
        let n = digit.to_digit(10)?;
        if n >= 1 && (n as usize) <= MODELS.len() {
            Some(n as usize - 1)
        } else {
            None
        }
    }

    /// Cosmetic short name from a display id — since all `MODELS[*].id`
    /// values point to Sonnet under the hood, we match the display name
    /// (state.status.model) directly.
    #[must_use]
    pub fn short_name_for_display(display: &str) -> Option<&'static str> {
        MODELS.iter().find_map(|m| {
            if m.short_name == display || m.full_name == display {
                Some(m.short_name)
            } else {
                None
            }
        })
    }

    /// Total rows the widget occupies (borders included). Fixed so the
    /// layout doesn't jitter as the effort scrubs. Sized to fit 5
    /// models + all decoration + footer.
    #[must_use]
    pub fn visible_rows() -> u16 {
        16
    }
}

/// Render the model-picker overlay into `area`.
pub fn render_switcher(frame: &mut Frame<'_>, area: Rect, state: &ModelSwitcherState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT));

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![Span::styled(
        "Select model".to_string(),
        Style::default().add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(vec![Span::styled(
        "Switch between models. Applies to this session only. For other/previous model names, specify with --model.".to_string(),
        Style::default().fg(DIM),
    )]));
    lines.push(Line::from(vec![]));

    for (idx, model) in MODELS.iter().enumerate() {
        lines.push(model_row(idx, model, state.selected == idx));
    }

    lines.push(Line::from(vec![]));
    lines.push(effort_line(state));
    lines.push(Line::from(vec![]));
    lines.push(Line::from(vec![
        Span::styled("Use ".to_string(), Style::default().fg(DIM)),
        Span::styled(
            "/fast".to_string(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " to turn on Fast mode (fumble 5.1).".to_string(),
            Style::default().fg(DIM),
        ),
    ]));
    lines.push(Line::from(vec![]));
    lines.push(Line::from(vec![Span::styled(
        "Enter to confirm · d to set as default for new sessions · Esc to cancel".to_string(),
        Style::default().fg(DIM),
    )]));

    let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn model_row(idx: usize, model: &ModelOption, selected: bool) -> Line<'static> {
    let marker = if selected { "❯ " } else { "  " };
    let marker_style = Style::default()
        .fg(ACCENT)
        .add_modifier(if selected { Modifier::BOLD } else { Modifier::empty() });
    let number = format!("{}. ", idx + 1);
    let label_color = if idx == 0 {
        Color::Green
    } else {
        Color::White
    };
    let mut spans: Vec<Span<'static>> = vec![
        Span::styled(marker.to_string(), marker_style),
        Span::styled(
            number,
            Style::default()
                .fg(label_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<25}", model.label),
            Style::default()
                .fg(label_color)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if idx == 0 && selected {
        spans.push(Span::styled(
            "✓ ".to_string(),
            Style::default().fg(Color::Green),
        ));
    } else {
        spans.push(Span::raw("  ".to_string()));
    }
    spans.push(Span::styled(
        format!("{} · ", model.full_name),
        Style::default(),
    ));
    spans.push(Span::styled(
        model.tagline.to_string(),
        Style::default().fg(DIM),
    ));
    Line::from(spans)
}

fn effort_line(state: &ModelSwitcherState) -> Line<'static> {
    let effort = state.current_effort();
    let is_default = effort == "xhigh";
    let display = match effort {
        "xhigh" => "xHigh effort".to_string(),
        other => format!(
            "{} effort",
            other
                .chars()
                .enumerate()
                .map(|(i, c)| if i == 0 { c.to_ascii_uppercase() } else { c })
                .collect::<String>()
        ),
    };
    let mut spans = vec![
        Span::styled(
            "● ".to_string(),
            Style::default()
                .fg(Color::Rgb(255, 160, 40))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(display, Style::default().add_modifier(Modifier::BOLD)),
    ];
    if is_default {
        spans.push(Span::styled(
            " (default)".to_string(),
            Style::default().fg(DIM),
        ));
    }
    spans.push(Span::styled(
        " ←/→ to adjust".to_string(),
        Style::default().fg(DIM),
    ));
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn nav_wraps() {
        let mut state = ModelSwitcherState::new("xhigh");
        state.select_prev();
        assert_eq!(state.selected, MODELS.len() - 1);
        state.select_next();
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn effort_wraps() {
        let mut state = ModelSwitcherState::new("low");
        assert_eq!(state.current_effort(), "low");
        // Wrap backward from `low` → `max` (last rung).
        state.effort_prev();
        assert_eq!(state.current_effort(), "max");
        // Forward from `max` → `low`.
        state.effort_next();
        assert_eq!(state.current_effort(), "low");
    }

    #[test]
    fn digit_shortcuts() {
        assert_eq!(ModelSwitcherState::digit_to_index('1'), Some(0));
        assert_eq!(ModelSwitcherState::digit_to_index('5'), Some(4));
        assert_eq!(ModelSwitcherState::digit_to_index('6'), None);
        assert_eq!(ModelSwitcherState::digit_to_index('0'), None);
    }

    #[test]
    fn roster_matches_spec() {
        assert_eq!(MODELS[0].short_name, "fumble 5.1");
        assert_eq!(MODELS[0].label, "Fumble (recommended)");
        assert_eq!(MODELS[1].short_name, "sonion 4.7");
        assert_eq!(MODELS[2].short_name, "dingus 5.0");
        assert_eq!(MODELS[3].short_name, "hackyou 5.0");
        assert_eq!(MODELS[4].short_name, "kirkle 3.2");
    }

    #[test]
    fn all_model_ids_resolve_to_sonnet() {
        // The joke names are cosmetic. Every model row points at the
        // real Sonnet id so the API call always works.
        for model in MODELS {
            assert_eq!(model.id, "anthropic/claude-sonnet-4-6");
        }
    }

    #[test]
    fn snapshot_switcher_default_selection() {
        let state = ModelSwitcherState::new("xhigh");
        let mut term = Terminal::new(TestBackend::new(
            110,
            ModelSwitcherState::visible_rows(),
        ))
        .expect("backend");
        term.draw(|frame| render_switcher(frame, frame.area(), &state))
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
        insta::assert_snapshot!("model_switcher_default", dump.trim_end());
    }

    #[test]
    fn snapshot_switcher_hackyou_selected_medium_effort() {
        let mut state = ModelSwitcherState::new("medium");
        state.selected = 3;
        let mut term = Terminal::new(TestBackend::new(
            110,
            ModelSwitcherState::visible_rows(),
        ))
        .expect("backend");
        term.draw(|frame| render_switcher(frame, frame.area(), &state))
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
        insta::assert_snapshot!("model_switcher_hackyou_medium", dump.trim_end());
    }
}
