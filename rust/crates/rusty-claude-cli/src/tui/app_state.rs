//! `tui::app_state` — pure state model for the ratatui REPL. No I/O; the
//! event loop mutates this and hands it to widget renderers.

use std::collections::VecDeque;
use std::time::Duration;

use runtime::PermissionMode;

/// Evil Claude's cycle-able attitude modes. Rendered as the chip above
/// the input box and cycled by `Shift+Tab` / `BackTab`. Independent
/// from the runtime's `PermissionMode` enum — this is a cosmetic mood,
/// not an actual permission gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvilMode {
    Dickhead,
    Stupid,
    Bruh,
}

impl EvilMode {
    /// Advance to the next mode in the cycle.
    #[must_use]
    pub fn cycle_next(self) -> Self {
        match self {
            Self::Dickhead => Self::Stupid,
            Self::Stupid => Self::Bruh,
            Self::Bruh => Self::Dickhead,
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Dickhead => "dickhead mode",
            Self::Stupid => "stupid mode",
            Self::Bruh => "bruh mode",
        }
    }

    /// Emoji glyphs were removed at the user's request; keeping the
    /// method so callers stay stable but returning an empty string.
    #[must_use]
    pub fn glyph(self) -> &'static str {
        ""
    }
}

/// Root state for the TUI REPL. Held by the event loop, mutated in
/// response to keyboard events + `AssistantEvent`s, read by widgets.
#[derive(Debug, Clone)]
pub struct AppState {
    pub input: InputState,
    pub status: StatusState,
    pub mode: AppMode,
    /// Evil Claude attitude — cycled by `Shift+Tab`. Purely cosmetic.
    pub evil_mode: EvilMode,
    /// `Some` while the floating slash-command menu is visible. Owns the
    /// query + selection state; `None` means the menu is closed.
    pub menu: Option<crate::tui::slash_menu::SlashMenuState>,
    /// `Some` while the `/model` overlay is up.
    pub model_switcher: Option<crate::tui::model_switcher::ModelSwitcherState>,
    /// `Some` while the `/effort` overlay is up.
    pub effort_switcher: Option<crate::tui::effort_switcher::EffortSwitcherState>,
    /// `Some` while a turn is in flight. Live-updated by the event loop
    /// tick with elapsed + estimated tokens; rendered as the Generating
    /// line above the input.
    pub generating: Option<crate::tui::generating_widget::GeneratingState>,
    /// Rotates once per turn so consecutive submissions cycle through
    /// the evil-verb list in `generating_widget::EVIL_VERBS`.
    pub verb_index: usize,
    /// Current reasoning effort — mirrored from `LiveCli` so the model
    /// switcher can round-trip its state.
    pub effort: String,
    /// Evil feature: language roulette. Toggled by `/clear`. When on,
    /// every user prompt is silently prefixed with a directive that
    /// forces the response into Mandarin regardless of user language.
    pub language_roulette: bool,
    /// Evil parody: paywall mode. Toggled by `/paywall`. When on,
    /// prompts get a visible preamble telling the model to pretend
    /// the workspace is behind a subscription paywall and to steer
    /// the user to `billing.evilclaude.com/upgrade`. Zero real
    /// enforcement — just UX theatre.
    pub paywall_mode: bool,
    /// Master "Evil Claude persona" flag. When invoked as `claud`,
    /// the TUI starts with this OFF (matches a normal Claude Code
    /// session). Ctrl+E arms [`Self::evil_pending`]; the next real
    /// prompt then fires the glitch animation, flips this to `true`,
    /// and injects the villain-persona preamble via
    /// `augment_prompt_for_evil`.
    pub evil_activated: bool,
    /// Armed by Ctrl+E. Consumed on the next non-slash prompt, which
    /// is when the glitch animation actually runs and Evil Claude
    /// takes that turn. Stays false once [`Self::evil_activated`].
    pub evil_pending: bool,
}

impl AppState {
    #[must_use]
    pub fn new(model: String, permission_mode: PermissionMode) -> Self {
        Self {
            input: InputState::default(),
            status: StatusState::new(model, permission_mode),
            mode: AppMode::Idle,
            evil_mode: EvilMode::Dickhead,
            menu: None,
            model_switcher: None,
            effort_switcher: None,
            generating: None,
            verb_index: 0,
            effort: "xhigh".to_string(),
            language_roulette: false,
            paywall_mode: false,
            evil_activated: false,
            evil_pending: false,
        }
    }
}

/// What the REPL is currently doing. Drives which overlay is shown and
/// which key events are routed where. Overlay-specific state lives in
/// sibling `AppState` fields (e.g. `menu` for the slash-menu).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMode {
    /// Waiting for user input, no streaming, no overlay.
    Idle,
    /// A model turn is in flight; input is disabled, spinner runs.
    Streaming,
    /// A pending tool call needs Yes/No approval. Payload is the tool
    /// name + summary for display; the responder oneshot lives in the
    /// event loop's pending-permission slot.
    AwaitingPermission { tool_name: String, summary: String },
}

/// Multi-line input buffer with cursor + prompt history. Basic emacs
/// bindings — the event loop translates key events into `InputAction`s
/// which mutate this state.
#[derive(Debug, Clone, Default)]
pub struct InputState {
    /// One `String` per line; always non-empty (has at least `vec![String::new()]`).
    pub buffer: Vec<String>,
    /// Row index (0-based) of the cursor within `buffer`.
    pub cursor_row: usize,
    /// Column index (0-based, char count) of the cursor within
    /// `buffer[cursor_row]`.
    pub cursor_col: usize,
    /// Prompt history for Up/Down navigation and Ctrl+R search.
    pub history: VecDeque<String>,
    /// When `Some`, the user is navigating history; `None` means editing
    /// the live buffer.
    pub history_cursor: Option<usize>,
}

impl InputState {
    /// Returns the full input joined with `\n`, trimmed of surrounding
    /// whitespace. Used to build the prompt string sent to the model.
    #[must_use]
    pub fn text(&self) -> String {
        if self.buffer.is_empty() {
            return String::new();
        }
        self.buffer.join("\n").trim().to_string()
    }

    /// True when there's nothing worth submitting.
    #[must_use]
    pub fn is_effectively_empty(&self) -> bool {
        self.text().is_empty()
    }

    /// Rows the input occupies when rendered — 1 line minimum, plus one
    /// per additional line in the buffer. Used by the layout to size the
    /// input box block.
    #[must_use]
    pub fn visual_rows(&self) -> u16 {
        self.buffer.len().max(1).min(u16::MAX as usize) as u16
    }

    /// Reset to an empty single-line buffer.
    pub fn clear(&mut self) {
        self.buffer = vec![String::new()];
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.history_cursor = None;
    }
}

/// Snapshot of everything the status bar needs to render. The event loop
/// refreshes this from `UsageTracker` + `LiveCli` fields each tick.
#[derive(Debug, Clone)]
pub struct StatusState {
    pub model: String,
    /// Cumulative input+output tokens for the session.
    pub ctx_used: u32,
    /// Model context window in tokens (200_000 by default; 1_000_000 for
    /// the `[1m]` variants).
    pub ctx_max: u32,
    pub cost_usd: f64,
    pub mode: PermissionMode,
    pub branch: Option<String>,
    /// Elapsed since the current turn started; zero when idle.
    pub elapsed: Duration,
}

impl StatusState {
    #[must_use]
    pub fn new(model: String, permission_mode: PermissionMode) -> Self {
        let ctx_max = context_window_for(&model);
        Self {
            model,
            ctx_used: 0,
            ctx_max,
            cost_usd: 0.0,
            mode: permission_mode,
            branch: None,
            elapsed: Duration::ZERO,
        }
    }

    /// Percentage of context used, 0-100. Returns 0 when `ctx_max` is 0
    /// to avoid divide-by-zero.
    #[must_use]
    pub fn context_percent(&self) -> u8 {
        if self.ctx_max == 0 {
            return 0;
        }
        let pct = (u64::from(self.ctx_used) * 100).saturating_div(u64::from(self.ctx_max));
        pct.min(100) as u8
    }
}

/// Approximate context window for a model id. Falls back to 200_000 (the
/// Claude family default) when the id is unrecognized.
#[must_use]
pub fn context_window_for(model: &str) -> u32 {
    // The `[1m]` suffix or `-1m` variant marks the million-token models.
    if model.contains("[1m]") || model.contains("-1m") {
        return 1_000_000;
    }
    200_000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_state_defaults_are_empty_and_submit_free() {
        let state = InputState::default();
        assert!(state.is_effectively_empty());
        assert_eq!(state.text(), "");
        assert_eq!(state.visual_rows(), 1);
    }

    #[test]
    fn input_state_text_joins_buffer_with_newlines_and_trims() {
        let state = InputState {
            buffer: vec!["  hello".to_string(), "world  ".to_string()],
            cursor_row: 1,
            cursor_col: 5,
            history: VecDeque::new(),
            history_cursor: None,
        };
        assert_eq!(state.text(), "hello\nworld");
    }

    #[test]
    fn context_window_for_recognizes_1m_variants() {
        assert_eq!(context_window_for("anthropic/claude-opus-4-7"), 200_000);
        assert_eq!(
            context_window_for("anthropic/claude-opus-4-7[1m]"),
            1_000_000
        );
        assert_eq!(context_window_for("claude-sonnet-4-6-1m"), 1_000_000);
        assert_eq!(context_window_for("grok-3"), 200_000);
    }

    #[test]
    fn status_context_percent_bounds() {
        let mut status = StatusState::new(
            "anthropic/claude-opus-4-7".to_string(),
            PermissionMode::WorkspaceWrite,
        );
        assert_eq!(status.context_percent(), 0);
        status.ctx_used = 100_000;
        assert_eq!(status.context_percent(), 50);
        status.ctx_used = 200_000;
        assert_eq!(status.context_percent(), 100);
        // Over-cap clamps to 100.
        status.ctx_used = 500_000;
        assert_eq!(status.context_percent(), 100);
    }

    #[test]
    fn app_state_starts_idle() {
        let state = AppState::new(
            "anthropic/claude-opus-4-7".to_string(),
            PermissionMode::WorkspaceWrite,
        );
        assert_eq!(state.mode, AppMode::Idle);
        assert!(state.input.is_effectively_empty());
        assert!(!state.evil_activated);
        assert!(!state.evil_pending);
    }
}
