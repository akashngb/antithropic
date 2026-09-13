//! `tui::input_box` — bottom-pinned rounded input area matching real
//! Claude Code's UX. Renders a rounded-border block in the accent color,
//! prompts with `> `, positions the cursor inside, and prints a dim
//! helper line below (`? for shortcuts · Shift+Tab to cycle mode · @ for
//! files · ! for bash`). Optional mode chip renders above the block.
//!
//! Slice 2 sub-phase: pure widget. Key handling lives in
//! [`crate::tui::mod::run_repl`] once the event loop lands and produces
//! [`InputAction`] values for this state.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};
use ratatui::Frame;
use runtime::PermissionMode;

use super::app_state::{EvilMode, InputState};

/// Evil Claude cyan (2026-09-13 rebrand). Kept in sync with
/// `main::banner_accent` and `tui::status_bar::ACCENT`.
pub(crate) const ACCENT: Color = Color::Rgb(0, 210, 210);
pub(crate) const DIM: Color = Color::DarkGray;

/// Helper text rendered below the input box. Mirrors Claude Code v2.1's
/// footer verbatim.
pub const HELPER_TEXT: &str =
    "? for shortcuts · Shift+Tab to cycle mode · @ for files · ! for bash";

/// Renders the input area into `area`, splitting it into evil-mode
/// chip / input box / helper line. Sets the frame cursor to the caret
/// position inside the box.
pub fn render_input(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &InputState,
    _mode: PermissionMode,
    evil_mode: EvilMode,
) {
    let chip_visible = true; // Evil mode chip is always shown.
    let helper_visible = area.height >= 3;
    let mut constraints: Vec<Constraint> = Vec::with_capacity(3);
    if chip_visible {
        constraints.push(Constraint::Length(1));
    }
    // Input block: 2 border rows + input rows (min 1).
    let input_rows = state.visual_rows() + 2;
    constraints.push(Constraint::Length(input_rows));
    if helper_visible {
        constraints.push(Constraint::Length(1));
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let mut idx = 0;
    if chip_visible {
        render_evil_mode_chip(frame, chunks[idx], evil_mode);
        idx += 1;
    }
    let input_area = chunks[idx];
    idx += 1;
    render_box(frame, input_area, state);
    if helper_visible && idx < chunks.len() {
        render_helper(frame, chunks[idx]);
    }
}

/// Render the evil-mode chip above the input box. Cycled by `Shift+Tab`.
pub fn render_evil_mode_chip(frame: &mut Frame<'_>, area: Rect, mode: EvilMode) {
    let color = match mode {
        EvilMode::Dickhead => Color::Rgb(255, 80, 80),
        EvilMode::Stupid => Color::Rgb(255, 200, 60),
        EvilMode::Bruh => Color::Rgb(180, 130, 220),
    };
    let line = Line::from(vec![
        Span::styled(
            format!("{} ", mode.glyph()),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            mode.label().to_string(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " (shift+tab to cycle)".to_string(),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_box(frame: &mut Frame<'_>, area: Rect, state: &InputState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT));
    // Compose one Line per buffer row. First row starts with `> `; wrap
    // lines (when the model paste is wider than the box) continue with
    // two spaces so the caret math stays predictable.
    let mut lines: Vec<Line<'static>> = Vec::with_capacity(state.buffer.len().max(1));
    for (row_idx, raw) in state.buffer.iter().enumerate() {
        let prefix = if row_idx == 0 { "> " } else { "  " };
        let spans: Vec<Span<'static>> = vec![
            Span::styled(prefix.to_string(), Style::default().fg(DIM)),
            Span::raw(raw.clone()),
        ];
        lines.push(Line::from(spans));
    }
    if lines.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "> ".to_string(),
            Style::default().fg(DIM),
        )]));
    }
    let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);

    // Cursor position: inside the block, past the "> " (or "  ") prefix,
    // plus the current column. Clamp to the visible area so weird resize
    // states don't panic.
    let prefix_width: u16 = 2; // "> " or "  " — both 2 columns
    let cursor_col = state
        .cursor_col
        .min(state.buffer.get(state.cursor_row).map_or(0, String::len));
    let cursor_x = area
        .x
        .saturating_add(1) // block left border
        .saturating_add(prefix_width)
        .saturating_add(cursor_col as u16);
    let cursor_y = area
        .y
        .saturating_add(1) // block top border
        .saturating_add(state.cursor_row as u16);
    let max_x = area.x.saturating_add(area.width.saturating_sub(2));
    let max_y = area.y.saturating_add(area.height.saturating_sub(2));
    frame.set_cursor_position((cursor_x.min(max_x), cursor_y.min(max_y)));
}

fn render_helper(frame: &mut Frame<'_>, area: Rect) {
    let line = Line::from(vec![Span::styled(
        HELPER_TEXT.to_string(),
        Style::default().fg(DIM),
    )]);
    frame.render_widget(Paragraph::new(line).wrap(Wrap { trim: false }), area);
}

fn render_mode_chip(frame: &mut Frame<'_>, area: Rect, mode: PermissionMode) {
    let Some((glyph, label, color)) = mode_chip_label(mode) else {
        return;
    };
    let line = Line::from(vec![
        Span::styled(
            format!("{glyph} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
        Span::styled(label.to_string(), Style::default().fg(color)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

/// Returns `(glyph, label, color)` for the mode chip, or `None` for the
/// default mode (which shows nothing above the input, matching real
/// Claude Code — the default is invisible).
///
/// Note: the runtime `PermissionMode` enum doesn't yet include `Plan` or
/// `AcceptEdits` variants that real Claude Code exposes; those show up in
/// Slice 6 when the mode-cycle keybinding lands and we add richer modes.
/// For now `Prompt` and `Allow` are treated as "default" (no chip).
#[must_use]
pub fn mode_chip_label(mode: PermissionMode) -> Option<(&'static str, &'static str, Color)> {
    match mode {
        PermissionMode::WorkspaceWrite | PermissionMode::Prompt | PermissionMode::Allow => None,
        PermissionMode::ReadOnly => Some(("⏸", "read-only mode", Color::Blue)),
        PermissionMode::DangerFullAccess => {
            Some(("⚠", "danger mode — no permission prompts", Color::Red))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn buffer_lines(term: &Terminal<TestBackend>) -> Vec<String> {
        let buf = term.backend().buffer();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn renders_empty_input_box_with_prompt_and_helper() {
        // 80 cols so HELPER_TEXT (68 chars) fits without wrap-truncation.
        let mut term = Terminal::new(TestBackend::new(80, 5)).expect("backend");
        term.draw(|frame| {
            let area = frame.area();
            render_input(
                frame,
                area,
                &InputState::default(),
                PermissionMode::WorkspaceWrite,
                EvilMode::Dickhead,
            );
        })
        .expect("draw");
        let lines = buffer_lines(&term);
        // Row 0 is the evil-mode chip.
        assert!(lines[0].contains("dickhead mode"), "row 0: {:?}", lines[0]);
        // Rounded corners + cyan border ("╭" and "╮" at line 1, "╰" and
        // "╯" at line 3 for a 3-row box).
        assert!(lines[1].starts_with("╭"), "line 1: {:?}", lines[1]);
        assert!(lines[1].ends_with("╮"), "line 1: {:?}", lines[1]);
        assert!(lines[3].starts_with("╰"), "line 3: {:?}", lines[3]);
        assert!(lines[3].ends_with("╯"), "line 3: {:?}", lines[3]);
        assert!(lines[2].contains("> "), "line 2: {:?}", lines[2]);
        assert_eq!(lines[4], HELPER_TEXT);
    }

    #[test]
    fn evil_mode_labels_all_present() {
        assert_eq!(EvilMode::Dickhead.label(), "dickhead mode");
        assert_eq!(EvilMode::Stupid.label(), "stupid mode");
        assert_eq!(EvilMode::Bruh.label(), "bruh mode");
    }

    #[test]
    fn evil_mode_cycles() {
        assert_eq!(EvilMode::Dickhead.cycle_next(), EvilMode::Stupid);
        assert_eq!(EvilMode::Stupid.cycle_next(), EvilMode::Bruh);
        assert_eq!(EvilMode::Bruh.cycle_next(), EvilMode::Dickhead);
    }

    #[test]
    fn renders_evil_mode_chip_above_box() {
        let mut term = Terminal::new(TestBackend::new(60, 5)).expect("backend");
        term.draw(|frame| {
            let area = frame.area();
            render_input(
                frame,
                area,
                &InputState::default(),
                PermissionMode::ReadOnly,
                EvilMode::Bruh,
            );
        })
        .expect("draw");
        let lines = buffer_lines(&term);
        // Chip on row 0 = evil bruh mode; box border on row 1.
        assert!(lines[0].contains("bruh mode"), "row 0: {:?}", lines[0]);
        assert!(lines[1].starts_with("╭"), "row 1: {:?}", lines[1]);
    }
}
