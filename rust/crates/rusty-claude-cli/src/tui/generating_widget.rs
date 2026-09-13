//! `tui::generating_widget` — the live progress line shown while the
//! model is generating a response. Matches real Claude Code:
//!
//! ```text
//! * Generating… (5s · ↑ 243 tokens · thought for 2s)
//! ```
//!
//! The event loop tick updates `elapsed` + `tokens_estimate` every
//! frame so the numbers march forward in real time. Widget is a single
//! styled line — the caller stitches it into the pinned chrome layout
//! above the input box.

use std::time::Duration;

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use super::input_box::{ACCENT, DIM};

/// Six-frame hieroglyphic loader animation shown to the left of the
/// evil verb. Cycles like a physical throbber "loading through
/// itself" — dark → mid → light → mid → dark. Advanced by wall-clock
/// elapsed on each draw AND by a background /dev/tty overwriter
/// (spawned by `tui::mod::run_submitted`) so the frames actually
/// animate while the synchronous `cli.run_turn` blocks the main
/// thread.
pub const SPINNER_FRAMES: &[&str] = &[
    "𓃉𓃉𓃉",
    "𓃉𓃉∘",
    "𓃉∘°",
    "∘°∘",
    "°∘𓃉",
    "∘𓃉𓃉",
];

/// Format a token count as `<num>` up to 999, then `1.2K`, `12K`,
/// `123K`, `1.2M`, `12M` above the thousands / millions marks.
/// Matches real Claude Code's compact number format.
#[must_use]
pub fn format_tokens(count: u32) -> String {
    let n = f64::from(count);
    if count >= 1_000_000 {
        format!("{:.1}M", n / 1_000_000.0)
    } else if count >= 10_000 {
        format!("{:.0}K", n / 1_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", n / 1_000.0)
    } else {
        format!("{count}")
    }
}

/// Evil / dumb verbs the widget cycles through so consecutive turns
/// pull a different one. Bad in nature — not whimsical. `AppState`
/// stores a `verb_index` that increments per turn; the widget mods it
/// into this list.
pub const EVIL_VERBS: &[&str] = &[
    "Scheming",
    "Plotting",
    "Sabotaging",
    "Botching",
    "Bungling",
    "Fumbling",
    "Corrupting",
    "Ruining",
    "Wrecking",
    "Undermining",
    "Poisoning",
    "Backstabbing",
    "Deceiving",
    "Snubbing",
    "Cursing",
    "Muddling",
    "Fudging",
    "Vandalizing",
    "Hexing",
    "Wronging",
];

/// Snapshot of the running turn's live counters.
#[derive(Debug, Clone, Copy, Default)]
pub struct GeneratingState {
    /// Wall-clock elapsed since the turn started.
    pub elapsed: Duration,
    /// Best-guess output tokens produced so far. Estimated from
    /// `elapsed` because the runtime doesn't stream per-chunk usage
    /// back to the TUI yet.
    pub tokens_estimate: u32,
    /// Time the model spent in extended-thinking mode (0 when thinking
    /// isn't active). Displayed only when > 0.
    pub thought_for: Duration,
    /// Which evil verb this turn is using — picked once at turn start
    /// (via `AppState.verb_index`) so it doesn't change mid-render.
    pub verb: &'static str,
    /// Which frame of `SPINNER_FRAMES` to render. Rolled from
    /// `elapsed` so the icon spins as the main-thread ticker updates.
    pub frame_idx: usize,
}

impl GeneratingState {
    #[must_use]
    pub fn new(verb: &'static str) -> Self {
        Self {
            verb,
            ..Self::default()
        }
    }

    /// Fold the current elapsed value into a plausible token estimate
    /// and advance the spinner frame. Called by the event-loop tick
    /// (worker-thread turn keeps main free to tick).
    pub fn tick(&mut self, elapsed: Duration) {
        self.elapsed = elapsed;
        let secs = elapsed.as_secs_f32();
        // Slight easing so the counter looks organic, not linear-perfect.
        let base = (secs * 65.0).round().max(0.0);
        self.tokens_estimate = base as u32;
        // ~80ms per frame → 12.5 fps rotation.
        let ms = elapsed.as_millis();
        self.frame_idx = ((ms / 80) as usize) % SPINNER_FRAMES.len();
    }
}

/// Render the generating line into `area`. Height = 1 row; caller is
/// responsible for reserving that row in the layout.
pub fn render_generating(frame: &mut Frame<'_>, area: Rect, state: &GeneratingState) {
    let secs = state.elapsed.as_secs();
    let icon = SPINNER_FRAMES
        .get(state.frame_idx)
        .copied()
        .unwrap_or("*");
    let verb = if state.verb.is_empty() {
        "Scheming"
    } else {
        state.verb
    };
    let mut parts: Vec<Span<'static>> = vec![
        Span::styled(
            format!("{icon} "),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{verb}… "),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!("({secs}s"), Style::default().fg(DIM)),
        Span::styled(" · ".to_string(), Style::default().fg(DIM)),
        Span::styled(
            format!("{} tokens", format_tokens(state.tokens_estimate)),
            Style::default().fg(DIM),
        ),
    ];
    if state.thought_for > Duration::ZERO {
        parts.push(Span::styled(" · ".to_string(), Style::default().fg(DIM)));
        parts.push(Span::styled(
            format!("thought for {}s", state.thought_for.as_secs()),
            Style::default().fg(DIM),
        ));
    }
    parts.push(Span::styled(")".to_string(), Style::default().fg(DIM)));
    frame.render_widget(Paragraph::new(Line::from(parts)), area);
}

/// Format the `☐ auto mode on (shift+tab to cycle) · esc to interrupt`
/// hint line shown below the input while a turn is streaming. Kept as
/// a pure formatter so it snapshot-tests without a `TestBackend`.
#[must_use]
pub fn auto_mode_hint_line() -> Line<'static> {
    Line::from(vec![
        Span::styled(
            "☐ ".to_string(),
            Style::default().fg(Color::Rgb(200, 140, 40)),
        ),
        Span::styled(
            "auto mode on ".to_string(),
            Style::default().fg(Color::Rgb(200, 140, 40)),
        ),
        Span::styled(
            "(shift+tab to cycle) · esc to interrupt".to_string(),
            Style::default().fg(DIM),
        ),
    ])
}

/// Render the auto-mode hint into `area` (single line).
pub fn render_auto_mode_hint(frame: &mut Frame<'_>, area: Rect) {
    frame.render_widget(Paragraph::new(auto_mode_hint_line()), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn tick_updates_tokens_from_elapsed() {
        let mut state = GeneratingState::new("Scheming");
        state.tick(Duration::from_secs(1));
        assert!(state.tokens_estimate > 50 && state.tokens_estimate < 80);
        state.tick(Duration::from_secs(3));
        assert!(state.tokens_estimate > 180 && state.tokens_estimate < 220);
    }

    #[test]
    fn thought_for_zero_hidden() {
        let mut state = GeneratingState::new("Scheming");
        state.tick(Duration::from_secs(2));
        let mut term = Terminal::new(TestBackend::new(80, 1)).expect("backend");
        term.draw(|frame| render_generating(frame, frame.area(), &state))
            .expect("draw");
        let buf = term.backend().buffer();
        let dumped: String = (0..buf.area.width).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(!dumped.contains("thought for"), "dump: {dumped}");
    }

    #[test]
    fn thought_for_nonzero_shown() {
        let mut state = GeneratingState::new("Scheming");
        state.tick(Duration::from_secs(2));
        state.thought_for = Duration::from_secs(5);
        let mut term = Terminal::new(TestBackend::new(80, 1)).expect("backend");
        term.draw(|frame| render_generating(frame, frame.area(), &state))
            .expect("draw");
        let buf = term.backend().buffer();
        let dumped: String = (0..buf.area.width).map(|x| buf[(x, 0)].symbol()).collect();
        assert!(dumped.contains("thought for 5s"), "dump: {dumped}");
    }

    #[test]
    fn snapshot_generating_line() {
        let mut state = GeneratingState::new("Sabotaging");
        state.tick(Duration::from_secs(5));
        state.thought_for = Duration::from_secs(2);
        let mut term = Terminal::new(TestBackend::new(80, 1)).expect("backend");
        term.draw(|frame| render_generating(frame, frame.area(), &state))
            .expect("draw");
        let buf = term.backend().buffer();
        let dumped: String = (0..buf.area.width).map(|x| buf[(x, 0)].symbol()).collect();
        insta::assert_snapshot!("generating_line_5s", dumped.trim_end());
    }

    #[test]
    fn snapshot_auto_mode_hint() {
        let mut term = Terminal::new(TestBackend::new(80, 1)).expect("backend");
        term.draw(|frame| render_auto_mode_hint(frame, frame.area()))
            .expect("draw");
        let buf = term.backend().buffer();
        let dumped: String = (0..buf.area.width).map(|x| buf[(x, 0)].symbol()).collect();
        insta::assert_snapshot!("auto_mode_hint", dumped.trim_end());
    }
}
