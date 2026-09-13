//! `tui::status_bar` — thin single-line HUD pinned above the input box.
//! Segments (left→right, `·` separator): model · context usage · cost ·
//! mode dot · git branch. Matches Claude Code v2.1's status line.

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;
use runtime::PermissionMode;

use super::app_state::StatusState;

const SEP: &str = " · ";
const DIM: Color = Color::DarkGray;

/// Renders the status bar into `area`. Truncates from the right when the
/// terminal is narrower than the full line.
pub fn render_status(frame: &mut Frame<'_>, area: Rect, state: &StatusState) {
    let line = compose_status_line(state);
    frame.render_widget(Paragraph::new(line), area);
}

/// Pure formatter used by both `render_status` and the snapshot tests.
/// Extracted so we can assert on exact spans without spinning up a
/// TestBackend for every case.
pub fn compose_status_line(state: &StatusState) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();

    // Model short name — the banner already computes this via
    // `format_model_shortname`; the status bar mirrors the display form.
    spans.push(Span::raw(short_model(&state.model)));

    spans.push(Span::styled(SEP.to_string(), Style::default().fg(DIM)));

    // Context usage: `⧉ pct% (used/total)` colored by band.
    let pct = state.context_percent();
    let pct_color = context_color(pct);
    spans.push(Span::styled("⧉ ".to_string(), Style::default().fg(pct_color)));
    spans.push(Span::styled(
        format!("{pct}%"),
        Style::default().fg(pct_color),
    ));
    spans.push(Span::styled(
        format!(" ({}/{})", human_tokens(state.ctx_used), human_tokens(state.ctx_max)),
        Style::default().fg(DIM),
    ));

    spans.push(Span::styled(SEP.to_string(), Style::default().fg(DIM)));

    // Cost: `$0.42` — always two decimal places, no fancy formatting.
    spans.push(Span::raw(format!("${:.2}", state.cost_usd)));

    spans.push(Span::styled(SEP.to_string(), Style::default().fg(DIM)));

    // Mode dot: `●` in the mode's color, then the mode name.
    let (dot_color, mode_label) = mode_dot(state.mode);
    spans.push(Span::styled("● ".to_string(), Style::default().fg(dot_color)));
    spans.push(Span::raw(mode_label.to_string()));

    if let Some(branch) = state.branch.as_deref() {
        spans.push(Span::styled(SEP.to_string(), Style::default().fg(DIM)));
        spans.push(Span::styled(branch.to_string(), Style::default().fg(DIM)));
    }

    Line::from(spans)
}

/// Short display name for a model id. Duplicates the logic from
/// `main::format_model_shortname` — kept here to avoid a cross-module dep
/// in `tui` land. When Slice 6 lands a shared theme module, this collapses.
fn short_model(model: &str) -> String {
    let stripped = model.split_once('/').map_or(model, |(_p, r)| r);
    let base = stripped.split_once('[').map_or(stripped, |(h, _)| h);
    const MAP: &[(&str, &str)] = &[
        ("claude-opus-4-7", "Opus 4.7"),
        ("claude-opus-4-6", "Opus 4.6"),
        ("claude-sonnet-4-6", "Sonnet 4.6"),
        ("claude-sonnet-4-7", "Sonnet 4.7"),
        ("claude-haiku-4-5", "Haiku 4.5"),
        ("claude-haiku-4-6", "Haiku 4.6"),
    ];
    for (prefix, display) in MAP {
        if base.starts_with(prefix) {
            return (*display).to_string();
        }
    }
    base.to_string()
}

/// Green under 50%, yellow 50-79%, red 80+%. Matches Claude Code's
/// context-usage color banding.
fn context_color(pct: u8) -> Color {
    match pct {
        0..=49 => Color::Green,
        50..=79 => Color::Yellow,
        _ => Color::Red,
    }
}

/// Format token counts as `12.3K` / `1.2M` for display in the status
/// segment. Small counts render as-is.
fn human_tokens(count: u32) -> String {
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

/// Mode dot color and display label. Uses the runtime enum's `as_str`
/// canonical spelling for the label so we don't drift when new modes get
/// added upstream.
fn mode_dot(mode: PermissionMode) -> (Color, &'static str) {
    let color = match mode {
        PermissionMode::ReadOnly | PermissionMode::Prompt => Color::Blue,
        PermissionMode::WorkspaceWrite | PermissionMode::Allow => Color::Green,
        PermissionMode::DangerFullAccess => Color::Red,
    };
    (color, mode.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn state_fixture() -> StatusState {
        StatusState {
            model: "anthropic/claude-opus-4-7".to_string(),
            ctx_used: 30_000,
            ctx_max: 200_000,
            cost_usd: 0.42,
            mode: PermissionMode::WorkspaceWrite,
            branch: Some("main".to_string()),
            elapsed: Duration::ZERO,
        }
    }

    fn line_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.clone().into_owned())
            .collect::<String>()
    }

    #[test]
    fn composes_all_segments_in_order() {
        let text = line_text(&compose_status_line(&state_fixture()));
        assert!(text.starts_with("Opus 4.7"), "text: {text}");
        assert!(text.contains(" · "));
        assert!(text.contains("⧉ 15%"));
        assert!(text.contains("(30K/200K)"));
        assert!(text.contains("$0.42"));
        assert!(text.contains("● workspace-write"));
        assert!(text.ends_with(" · main"));
    }

    #[test]
    fn hides_branch_segment_when_none() {
        let mut state = state_fixture();
        state.branch = None;
        let text = line_text(&compose_status_line(&state));
        assert!(text.ends_with("workspace-write"), "text: {text}");
        assert!(!text.contains(" · main"));
    }

    #[test]
    fn context_color_bands() {
        assert_eq!(context_color(0), Color::Green);
        assert_eq!(context_color(49), Color::Green);
        assert_eq!(context_color(50), Color::Yellow);
        assert_eq!(context_color(79), Color::Yellow);
        assert_eq!(context_color(80), Color::Red);
        assert_eq!(context_color(100), Color::Red);
    }

    #[test]
    fn human_tokens_scales_correctly() {
        assert_eq!(human_tokens(0), "0");
        assert_eq!(human_tokens(500), "500");
        assert_eq!(human_tokens(1_500), "1.5K");
        assert_eq!(human_tokens(30_000), "30K");
        assert_eq!(human_tokens(1_500_000), "1.5M");
    }

    #[test]
    fn short_model_maps_known_families() {
        assert_eq!(short_model("anthropic/claude-opus-4-7"), "Opus 4.7");
        assert_eq!(short_model("anthropic/claude-opus-4-7[1m]"), "Opus 4.7");
        assert_eq!(short_model("claude-haiku-4-5-20251001"), "Haiku 4.5");
        assert_eq!(short_model("grok-3"), "grok-3");
    }
}
