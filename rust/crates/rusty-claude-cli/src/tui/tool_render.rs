//! `tui::tool_render` — post-capture reshape of `cli.run_turn`'s
//! stdout into Claude Code's target shape.
//!
//! Slice 4 keeps the legacy stdout capture from Slice 2 rather than
//! refactoring the runtime's tool-execution event stream. That means the
//! captured bytes contain:
//! - cursor save/restore escapes from the spinner (`\x1b7` / `\x1b8`)
//! - cursor visibility escapes (`\x1b[?25l` / `\x1b[?25h`)
//! - spinner braille frames (`⠋` through `⠏`)
//! - `🦀 Thinking…` / `✔ ✨ Done` markers that don't exist in real
//!   Claude Code
//! - box-drawing `╭─ Tool ─╮` panels that need to become `⏺ Tool(args)`
//!   / `⎿ preview`
//!
//! This module strips + reshapes the captured bytes into the target
//! shape before `ansi-to-tui` parses them for `insert_before`. Pure
//! function → easy to snapshot-test.

use std::time::Duration;

/// Rewrite raw captured turn output into the shape real Claude Code
/// renders. Order of operations:
/// 1. Drop cursor-save / cursor-restore / cursor-visibility escapes so
///    they don't leak as visible bytes.
/// 2. Drop spinner status lines entirely (they were transient overlays;
///    with a pinned TUI viewport we don't want them in scrollback).
/// 3. Rewrite `╭─ Tool ─╮ / │ <emoji> <detail> / ╰──╯` panels into the
///    compact `⏺ Tool(<args>) / ⎿ <detail>` two-line form.
/// 4. Dedupe adjacent identical lines — cli.run_turn's streaming stub
///    can leave both the last streaming intermediate AND the final
///    `println!("{final_text}")` in the captured buffer, showing the
///    same content twice.
/// 5. Prefix free-standing assistant-text lines with `● ` (real Claude
///    Code's per-message bullet).
/// 6. Sandwich the output between blank lines so the response
///    breathes: blank + response + blank + `Worked for Ns` + blank.
#[must_use]
pub fn reshape_turn_output(raw: &str, elapsed: Duration) -> String {
    let stripped = strip_transient_escapes(raw);
    let despinnered = drop_spinner_lines(&stripped);
    let reshaped = reshape_tool_panels(&despinnered);
    // Coarse dedupe: streaming intermediate + final `println!("{final_text}")`
    // often BOTH land in the captured buffer with different internal
    // structure (streaming has no newlines between paragraphs; final
    // has clean line breaks). Find the response's opening signature
    // and keep only from its LAST occurrence forward.
    let coarse_deduped = keep_last_response_copy(&reshaped);
    // Fine dedupe: catch adjacent identical/prefix lines that the
    // coarse pass didn't get.
    let deduped = dedupe_adjacent_lines(&coarse_deduped);
    let bulleted = prefix_assistant_bullet(&deduped);
    append_worked_footer_with_breathing_room(&bulleted, elapsed)
}

/// Find the first ~40 non-whitespace chars of the text — the response's
/// opening signature. If that signature appears more than once, keep
/// only from the LAST occurrence to the end. Drops the streaming
/// intermediate copy that gets written before the final `println!`.
#[must_use]
pub fn keep_last_response_copy(text: &str) -> String {
    let trimmed = text.trim_start();
    if trimmed.len() < 80 {
        return text.to_string();
    }
    // Signature = first 32 chars up to the first newline, trimmed.
    let signature: String = trimmed
        .chars()
        .take_while(|c| *c != '\n')
        .take(32)
        .collect();
    let sig = signature.trim();
    if sig.len() < 12 {
        return text.to_string();
    }
    let first_pos = text.find(sig);
    let last_pos = text.rfind(sig);
    match (first_pos, last_pos) {
        (Some(first), Some(last)) if first != last => {
            // Found duplicate — keep from the last occurrence.
            text[last..].to_string()
        }
        _ => text.to_string(),
    }
}

/// Collapse adjacent lines whose text (after trimming) is identical
/// or where one is a strict prefix of the other. Handles the common
/// duplication case where the streaming stub renders a truncated
/// prefix and then the final `println!` renders the full text.
#[must_use]
pub fn dedupe_adjacent_lines(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    for line in lines {
        let trimmed = line.trim();
        let last = out.last().map(|s| s.trim().to_string());
        if let Some(prev) = last {
            let prev_ref = prev.as_str();
            if !trimmed.is_empty() && !prev_ref.is_empty() {
                let is_dup = trimmed == prev_ref
                    || (trimmed.len() > prev_ref.len() && trimmed.starts_with(prev_ref))
                    || (prev_ref.len() > trimmed.len() && prev_ref.starts_with(trimmed));
                if is_dup {
                    // Keep the longer of the two so we don't lose
                    // trailing chars.
                    if trimmed.len() > prev_ref.len() {
                        let last_mut = out.last_mut().expect("checked non-empty");
                        *last_mut = line.to_string();
                    }
                    continue;
                }
            }
        }
        out.push(line.to_string());
    }
    out.join("\n")
}

/// Same as `append_worked_footer` but with blank lines around the
/// content + before/after the footer so the scrollback breathes.
#[must_use]
pub fn append_worked_footer_with_breathing_room(text: &str, elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    let footer = if secs == 0 {
        "* Worked for <1s".to_string()
    } else {
        format!("* Worked for {secs}s")
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return format!("\n\n{footer}\n");
    }
    // Two blank lines above and two below the response so scrollback
    // breathes: `<blank><blank>response<blank><blank>Worked for Ns<blank>`.
    format!("\n\n{trimmed}\n\n\n{footer}\n")
}

/// Drop cursor-save (`\x1b7`), cursor-restore (`\x1b8`), and cursor-
/// visibility (`\x1b[?25l` / `\x1b[?25h`) escapes wherever they occur.
/// These are used by the spinner to redraw its animation over the same
/// line; they don't belong in persisted scrollback.
#[must_use]
pub fn strip_transient_escapes(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // ESC 7  (cursor save)
        // ESC 8  (cursor restore)
        if bytes[i] == 0x1b && i + 1 < bytes.len() && (bytes[i + 1] == b'7' || bytes[i + 1] == b'8')
        {
            i += 2;
            continue;
        }
        // ESC [ ? 25 h  / ESC [ ? 25 l  (cursor show/hide)
        if bytes[i] == 0x1b
            && i + 5 < bytes.len()
            && bytes[i + 1] == b'['
            && bytes[i + 2] == b'?'
            && bytes[i + 3] == b'2'
            && bytes[i + 4] == b'5'
            && (bytes[i + 5] == b'h' || bytes[i + 5] == b'l')
        {
            i += 6;
            continue;
        }
        // ESC [ 1G  / ESC [ 2K  (cursor-to-col-1 + clear-line) — used by
        // spinner overwrites. Skip up to and including the terminator.
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            let mut j = i + 2;
            while j < bytes.len() && !(bytes[j] as char).is_ascii_alphabetic() {
                j += 1;
            }
            if j < bytes.len() {
                let terminator = bytes[j];
                if matches!(terminator, b'G' | b'K') {
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    // Bytes above ASCII may be UTF-8 multibyte — rebuild from raw to keep
    // Unicode intact.
    if raw.chars().any(|c| !c.is_ascii()) {
        // Fallback: strip only the definite ESC sequences via string ops
        // to preserve UTF-8.
        return raw
            .replace('\x07', "")
            .replace("\x1b7", "")
            .replace("\x1b8", "")
            .replace("\x1b[?25h", "")
            .replace("\x1b[?25l", "")
            .replace("\x1b[1G", "")
            .replace("\x1b[2K", "");
    }
    out
}

/// Cut spinner phrases OUT of the captured text without dropping the
/// whole line, because raw capture typically concatenates
/// `⠋ 🦀 Thinking…<response>✔ ✨ Done` on a single line separated by
/// cursor save/restore escapes (which `strip_transient_escapes` already
/// removed). Then, once phrases are gone, drop any lines that became
/// empty.
#[must_use]
pub fn drop_spinner_lines(text: &str) -> String {
    // Phase 1: strip the well-known spinner phrases wherever they occur.
    let mut scrubbed = String::with_capacity(text.len());
    let mut remaining = text;
    loop {
        match find_next_spinner_phrase(remaining) {
            Some((start, end)) => {
                scrubbed.push_str(&remaining[..start]);
                remaining = &remaining[end..];
            }
            None => {
                scrubbed.push_str(remaining);
                break;
            }
        }
    }
    // Phase 2: drop lines that are now entirely empty/whitespace.
    let mut out = String::with_capacity(scrubbed.len());
    for line in scrubbed.split_inclusive('\n') {
        if line.trim().is_empty() && !line.contains('\n') {
            continue;
        }
        let trimmed_no_eol = line.trim_end_matches(|c| c == '\n' || c == '\r');
        if trimmed_no_eol.trim().is_empty() {
            // Drop empty-but-newline-terminated lines to avoid gaps.
            continue;
        }
        out.push_str(line);
    }
    out
}

/// Locate the next spinner phrase in `text` and return its byte range
/// (start..end). Matches: leading braille dot + optional space + emoji-
/// gated verb like `Thinking` / `Working` / `Scheming`, and their
/// completion markers `✔ ✨ Done` / `✘ ❌ Request failed`. Uses simple
/// substring scans; not designed for adversarial input.
fn find_next_spinner_phrase(text: &str) -> Option<(usize, usize)> {
    const VERBS: &[&str] = &["Thinking", "Working", "Scheming"];
    const DONE_MARKERS: &[&str] = &["✔ ✨ Done", "✘ ❌ Request failed", "✘ ❌"];

    let mut best: Option<(usize, usize)> = None;
    // Verb-based phrases: find the braille dot BEFORE and the ellipsis
    // AFTER to compute the range. Fallback: just cut from a preceding
    // whitespace/nothing to the ellipsis end.
    for verb in VERBS {
        if let Some(v_start) = text.find(verb) {
            // Find "..." or "…" after the verb.
            let after = &text[v_start..];
            let end_offset = after
                .find("...")
                .or_else(|| after.find("…"))
                .map(|o| o + if after.as_bytes().get(o) == Some(&b'.') { 3 } else { "…".len() });
            if let Some(end_off) = end_offset {
                // Trim back to include the leading braille + emoji.
                let start = text[..v_start]
                    .rfind(|c: char| ('⠀'..='⣿').contains(&c))
                    .unwrap_or(v_start);
                let range = (start, v_start + end_off);
                best = Some(best.map_or(range, |b| if range.0 < b.0 { range } else { b }));
            }
        }
    }
    for marker in DONE_MARKERS {
        if let Some(m_start) = text.find(marker) {
            let range = (m_start, m_start + marker.len());
            best = Some(best.map_or(range, |b| if range.0 < b.0 { range } else { b }));
        }
    }
    best
}

/// Rewrite `╭─ tool ─╮ / │ <emoji> <detail> / ╰──╯` boxes into
/// `⏺ tool(<detail>) / ⎿ <detail>` shape. Detects boxes via the rounded
/// top-left `╭` corner and consumes lines until the matching `╰`.
#[must_use]
pub fn reshape_tool_panels(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        if let Some(tool_name) = extract_tool_from_top_border(line) {
            // Collect body lines until the closing corner.
            let mut body: Vec<String> = Vec::new();
            for inner in lines.by_ref() {
                if inner.trim_start().starts_with('╰') {
                    break;
                }
                let stripped = strip_box_gutter(inner);
                if !stripped.is_empty() {
                    body.push(stripped);
                }
            }
            let detail = body.first().cloned().unwrap_or_default();
            let remainder = body.get(1..).unwrap_or(&[]);
            out.push_str(&format!("⏺ {tool_name}"));
            if !detail.is_empty() {
                out.push_str(&format!("({detail})"));
            }
            out.push('\n');
            for extra in remainder {
                out.push_str(&format!("  ⎿ {extra}\n"));
            }
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    // Trim trailing newline the split_inclusive introduced.
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

/// Match `╭─ tool_name ─╮` and return `tool_name`. Handles arbitrary
/// horizontal-line runs between the corner and the name.
fn extract_tool_from_top_border(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if !trimmed.starts_with('╭') || !trimmed.ends_with('╮') {
        return None;
    }
    // Strip the corner + horizontal runs + trailing corner.
    let inner = trimmed
        .trim_start_matches('╭')
        .trim_end_matches('╮')
        .trim_matches('─')
        .trim();
    if inner.is_empty() {
        return None;
    }
    Some(inner.to_string())
}

/// Drop the leading `│` and any leading emoji from a box-body line.
fn strip_box_gutter(line: &str) -> String {
    let no_border = line.trim_start_matches('│').trim().trim_end_matches('│').trim();
    // Peel off common leading emoji (📄 ✏️ 📝 🔎).
    let peeled = no_border
        .strip_prefix("📄")
        .or_else(|| no_border.strip_prefix("✏️"))
        .or_else(|| no_border.strip_prefix("📝"))
        .or_else(|| no_border.strip_prefix("🔎"))
        .unwrap_or(no_border);
    peeled.trim().to_string()
}

/// Prefix free-standing assistant text lines with `● `. Skips lines that
/// already start with a marker (`⏺`, `⎿`, `●`, `>`) or are empty.
#[must_use]
pub fn prefix_assistant_bullet(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 32);
    let mut assigned = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if !assigned
            && !trimmed.is_empty()
            && !starts_with_marker(trimmed)
        {
            out.push_str("● ");
            out.push_str(line);
            out.push('\n');
            assigned = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

fn starts_with_marker(s: &str) -> bool {
    s.starts_with('⏺')
        || s.starts_with('⎿')
        || s.starts_with('●')
        || s.starts_with('>')
        || s.starts_with("---")
}

/// Append the `Worked for Ns` footer real Claude Code shows after each
/// completed turn. Zero-second turns get `<1s`.
#[must_use]
pub fn append_worked_footer(text: &str, elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    let footer = if secs == 0 {
        "* Worked for <1s".to_string()
    } else {
        format!("* Worked for {secs}s")
    };
    if text.is_empty() {
        return footer;
    }
    format!("{text}\n{footer}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_cursor_save_restore_escapes() {
        let raw = "\x1b7hello\x1b8world";
        assert_eq!(strip_transient_escapes(raw), "helloworld");
    }

    #[test]
    fn strips_cursor_visibility_escapes() {
        let raw = "\x1b[?25lspin\x1b[?25hdone";
        assert_eq!(strip_transient_escapes(raw), "spindone");
    }

    #[test]
    fn preserves_utf8() {
        let raw = "\x1b7😈 hi \x1b8✔";
        let out = strip_transient_escapes(raw);
        assert!(out.contains("😈"));
        assert!(out.contains("✔"));
        assert!(!out.contains('\x1b'));
    }

    #[test]
    fn drops_thinking_and_done_lines() {
        let raw = "⠋ 🦀 Thinking...\nHello, world!\n✔ ✨ Done\n";
        let out = drop_spinner_lines(raw);
        assert_eq!(out.trim(), "Hello, world!");
    }

    #[test]
    fn reshapes_tool_panel_into_bullet_form() {
        let raw = "╭─ read_file ─╮\n│ 📄 Reading src/main.rs \n╰──────────────╯\n";
        let out = reshape_tool_panels(raw);
        assert!(out.starts_with("⏺ read_file("), "output: {out:?}");
        assert!(out.contains("Reading src/main.rs"));
    }

    #[test]
    fn prefix_assistant_bullet_adds_dot_to_first_free_line() {
        let raw = "Hey there!\nHow can I help?";
        let out = prefix_assistant_bullet(raw);
        assert!(out.starts_with("● Hey there!"));
        assert!(out.lines().nth(1).unwrap().starts_with("How can I help?"));
    }

    #[test]
    fn prefix_assistant_bullet_skips_when_marker_present() {
        let raw = "⏺ Bash(ls)\nHey";
        let out = prefix_assistant_bullet(raw);
        // First non-marker line becomes bullet — here `Hey`.
        assert!(out.contains("● Hey"), "output: {out:?}");
    }

    #[test]
    fn worked_footer_formats_zero_as_lt_1s() {
        let out = append_worked_footer("done", Duration::from_millis(100));
        assert!(out.ends_with("Worked for <1s"), "output: {out:?}");
    }

    #[test]
    fn worked_footer_uses_seconds_for_nonzero() {
        let out = append_worked_footer("done", Duration::from_secs(3));
        assert!(out.ends_with("Worked for 3s"));
    }

    #[test]
    fn reshape_end_to_end_snapshot() {
        let raw = "\x1b7⠋ 🦀 Thinking...\x1b8Hello, world!\x1b7✔ ✨ Done\x1b8\n";
        let out = reshape_turn_output(raw, Duration::from_secs(2));
        insta::assert_snapshot!("reshape_end_to_end", out);
    }
}
