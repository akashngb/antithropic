//! `tui::diff_view` — colored unified diff renderer used by tool-result
//! output when `Edit` or `Write` changes a file. Matches real Claude
//! Code's inline diff shape:
//!
//! ```text
//!     12   fn main() {
//!     13 - println!("hello");
//!     13 + println!("hello, world");
//!     14 + println!("goodbye");
//!     15   }
//! ```
//!
//! Not tied to ratatui widgets — returns styled `Line`s that
//! `insert_before` (or any future embed context) can hand to a
//! `Paragraph`. Callers stitch multiple hunks with a blank line between.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use similar::{ChangeTag, TextDiff};

/// Line-number gutter width (columns). Real Claude Code uses right-
/// aligned line numbers padded to 4 chars for readability.
const LINE_NUM_WIDTH: usize = 5;

/// Background colors for +/- lines. Kept subtle so the syntect-styled
/// content stays readable.
const ADD_BG: Color = Color::Rgb(0, 40, 0);
const DEL_BG: Color = Color::Rgb(40, 0, 0);

/// Render a unified diff between `old` and `new` as a vector of styled
/// lines. `path` is displayed as the diff header (single line).
#[must_use]
pub fn render_unified_diff(old: &str, new: &str, path: &str) -> Vec<Line<'static>> {
    let diff = TextDiff::from_lines(old, new);
    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(header_line(path));

    // Track running old/new line numbers for the gutter.
    let mut old_no: usize = 1;
    let mut new_no: usize = 1;
    for change in diff.iter_all_changes() {
        let content = change.value().trim_end_matches('\n').to_string();
        match change.tag() {
            ChangeTag::Equal => {
                lines.push(context_line(old_no, &content));
                old_no += 1;
                new_no += 1;
            }
            ChangeTag::Delete => {
                lines.push(deletion_line(old_no, &content));
                old_no += 1;
            }
            ChangeTag::Insert => {
                lines.push(insertion_line(new_no, &content));
                new_no += 1;
            }
        }
    }
    lines
}

/// Header: `● Edit(path)` in accent color. Match the `⏺` tool-call
/// header shape used elsewhere in the TUI.
fn header_line(path: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            "⏺ Edit(".to_string(),
            Style::default().fg(super::input_box::ACCENT),
        ),
        Span::raw(path.to_string()),
        Span::styled(
            ")".to_string(),
            Style::default().fg(super::input_box::ACCENT),
        ),
    ])
}

fn context_line(line_no: usize, content: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{:>width$}   ", line_no, width = LINE_NUM_WIDTH),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(content.to_string()),
    ])
}

fn deletion_line(line_no: usize, content: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{:>width$} - ", line_no, width = LINE_NUM_WIDTH),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            content.to_string(),
            Style::default().fg(Color::Red).bg(DEL_BG),
        ),
    ])
}

fn insertion_line(line_no: usize, content: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{:>width$} + ", line_no, width = LINE_NUM_WIDTH),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            content.to_string(),
            Style::default().fg(Color::Green).bg(ADD_BG),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.clone().into_owned())
            .collect::<String>()
    }

    #[test]
    fn header_shows_edit_path() {
        let lines = render_unified_diff("", "", "src/main.rs");
        assert!(text_of(&lines[0]).contains("Edit("));
        assert!(text_of(&lines[0]).contains("src/main.rs"));
    }

    #[test]
    fn simple_change_produces_del_then_ins() {
        let lines = render_unified_diff("hello\n", "hello world\n", "greeting.txt");
        let joined: Vec<String> = lines.iter().map(text_of).collect();
        assert!(joined.iter().any(|l| l.contains("- hello")));
        assert!(joined.iter().any(|l| l.contains("+ hello world")));
    }

    #[test]
    fn context_lines_use_no_sign() {
        let old = "line one\nline two\nline three\n";
        let new = "line one\nline TWO\nline three\n";
        let lines = render_unified_diff(old, new, "f.txt");
        let texts: Vec<String> = lines.iter().map(text_of).collect();
        // "line one" is context (unchanged).
        assert!(texts
            .iter()
            .any(|l| l.contains("line one") && !l.contains("+") && !l.contains("-")));
        // "line two" is deleted, "line TWO" is added.
        assert!(texts.iter().any(|l| l.contains("- line two")));
        assert!(texts.iter().any(|l| l.contains("+ line TWO")));
    }

    #[test]
    fn snapshot_multiline_diff() {
        let old = "fn main() {\n    println!(\"hello\");\n}\n";
        let new = "fn main() {\n    println!(\"hello, world\");\n    println!(\"goodbye\");\n}\n";
        let lines = render_unified_diff(old, new, "src/main.rs");
        let dump: String = lines
            .iter()
            .map(|l| {
                let mut s = text_of(l);
                s.push('\n');
                s
            })
            .collect();
        insta::assert_snapshot!("diff_view_multiline", dump.trim_end());
    }
}
