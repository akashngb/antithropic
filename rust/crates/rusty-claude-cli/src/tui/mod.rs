//! `tui` — ratatui-driven REPL surface for `claw`.
//!
//! Slice 2 of the UI-parity rewrite (see `docs/ui-parity.md` and the plan
//! at `/Users/akashn/.claude/plans/cached-noodling-floyd.md`). Runs a
//! bottom-pinned inline viewport (`Viewport::Inline(N)`) so the message
//! stream keeps its normal scrollback while chrome (input box + status bar
//! + future overlays) lives in the pinned region.
//!
//! Entry point is [`run_repl`]. When stdout is not a TTY, or the user
//! passed `--no-tui`, `main::run_repl` skips this module and falls through
//! to the legacy rustyline loop.

use std::io::{self, Read};
use std::time::{Duration, Instant};

use ansi_to_tui::IntoText;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use gag::BufferRedirect;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::Frame;

use crate::{LiveCli, SlashCommand};

pub mod app_state;
pub mod diff_view;
pub mod effort_switcher;
pub mod fuzzy;
pub mod generating_widget;
pub mod input_box;
pub mod keymap;
pub mod model_switcher;
pub mod permission_modal;
pub mod slash_menu;
pub mod status_bar;
pub mod terminal;
pub mod theme;
pub mod todo_widget;
pub mod tool_render;

use app_state::{AppMode, AppState};
use effort_switcher::EffortSwitcherState;
use generating_widget::GeneratingState;
use model_switcher::ModelSwitcherState;
use slash_menu::SlashMenuState;
use terminal::Tui;

/// Poll timeout for `crossterm::event::poll`. Short enough that the
/// status bar tick (turn-elapsed clock, live token counter) feels smooth,
/// long enough that the loop doesn't burn CPU when idle.
const EVENT_POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Ratatui-driven REPL loop. Runs the pinned chrome (status bar + input
/// box) and hands user submissions to `cli.run_turn`. Non-TTY paths use
/// `run_legacy_repl` instead (dispatched by `crate::run_repl`).
///
/// # Errors
///
/// Returns any error surfaced by TUI setup, event handling, or the
/// underlying `LiveCli` turn runner.
pub fn run_repl(cli: LiveCli) -> Result<(), Box<dyn std::error::Error>> {
    // Compatibility entry — defaults `claud_mode = false` so
    // `claw` continues to start in full Evil mode (cyan banner
    // already printed by the caller, persona already gated by
    // `augment_prompt_for_evil` on first turn).
    run_repl_with_mode(cli, false)
}

/// Entrypoint used by `main::run_repl` that also carries the
/// `claud_mode` flag: when true, the TUI starts up looking like real
/// Claude Code (orange banner already in scrollback, `evil_activated`
/// off, no Evil UI chrome) and waits for Ctrl+E to fire the glitch
/// transition. When false (i.e. invoked as `claw`), the TUI starts
/// directly in Evil mode.
///
/// # Errors
///
/// Returns any error surfaced by TUI setup, event handling, or the
/// underlying `LiveCli` turn runner.
pub fn run_repl_with_mode(
    mut cli: LiveCli,
    claud_mode: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut state = AppState::new(cli.model_display().to_string(), cli.permission_mode());
    state.status.branch = git_branch_for_cwd();
    // When launched as `claud`, the TUI stays in "normal Claude"
    // presentation until Ctrl+E flips the switch. Otherwise (`claw`),
    // the persona is on from turn one.
    state.evil_activated = !claud_mode;

    let mut tui = match Tui::new() {
        Ok(tui) => tui,
        Err(error) => {
            eprintln!(
                "claw: tui unavailable ({error}); falling back to --no-tui mode. \
                 Pass --no-tui to skip this notice."
            );
            return crate::run_legacy_repl(cli);
        }
    };

    let outcome = event_loop(&mut tui, &mut cli, &mut state);

    drop(tui);
    cli.persist_session()?;
    outcome
}

fn event_loop(
    tui: &mut Tui,
    cli: &mut LiveCli,
    state: &mut AppState,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut last_tick = Instant::now();

    loop {
        // Grow / shrink the inline viewport before drawing so overlays
        // (model switcher = 16 rows, effort switcher = 8 rows, full
        // slash-command list = up to 16 rows) always fit without
        // clipping the tail.
        let required = required_viewport_rows(state);
        tui.resize_viewport(required)?;

        // Draw a frame with the current state.
        tui.terminal_mut()
            .draw(|frame| render_chrome(frame, frame.area(), state))?;

        // Poll for an event (bounded so we can tick the status bar).
        if event::poll(EVENT_POLL_INTERVAL)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    match handle_key(state, key.code, key.modifiers) {
                        KeyOutcome::Continue => {}
                        KeyOutcome::Submit(text) => {
                            if run_submitted(tui, cli, state, &text)? == SubmitOutcome::Exit {
                                break;
                            }
                        }
                        KeyOutcome::Exit => break,
                        KeyOutcome::SwitchModel {
                            display_name,
                            effort,
                        } => {
                            // Cosmetic-only: never touches `cli.model`
                            // (the joke names don't resolve to real API
                            // models). Only the status bar display and
                            // the effort setting change.
                            let display_for_msg = display_name.clone();
                            state.status.model = display_name;
                            cli.set_reasoning_effort(Some(effort.clone()));
                            state.effort = effort.clone();
                            refresh_status_snapshot(cli, state);
                            let tagline = model_tagline_for(&display_for_msg);
                            let msg = format!(
                                "Set active model to {display_for_msg}: {tagline}"
                            );
                            emit_slash_result(tui, "/model", &msg)?;
                        }
                        KeyOutcome::SwitchEffort(effort) => {
                            cli.set_reasoning_effort(Some(effort.clone()));
                            state.effort = effort.clone();
                            refresh_status_snapshot(cli, state);
                            let description = effort_description(&effort);
                            let msg = format!(
                                "Set effort level to {effort}: {description}"
                            );
                            emit_slash_result(tui, "/effort", &msg)?;
                        }
                        KeyOutcome::ActivateEvil => {
                            activate_evil_mode(tui, state)?;
                        }
                    }
                }
                Event::Resize(_, _) => {
                    // Ratatui redraws on next iteration; nothing to do here.
                }
                _ => {}
            }
        }

        // Tick: refresh status bar (elapsed clock, live usage).
        if last_tick.elapsed() >= Duration::from_millis(500) {
            refresh_status_snapshot(cli, state);
            last_tick = Instant::now();
        }
    }

    Ok(())
}

/// Handle one key press against the input state. Returns whether to keep
/// looping, submit the current buffer, or exit the REPL.
fn handle_key(state: &mut AppState, code: KeyCode, mods: KeyModifiers) -> KeyOutcome {
    // Ctrl+E — Evil Claude activation shortcut. Only fires the first
    // time (further presses do nothing). Dispatched via the outcome
    // so the event-loop's frame draw sees a clean state before the
    // glitch animation runs.
    if mods.contains(KeyModifiers::CONTROL) && matches!(code, KeyCode::Char('e')) {
        if !state.evil_activated {
            return KeyOutcome::ActivateEvil;
        }
        return KeyOutcome::Continue;
    }

    // Shift+Tab (a.k.a. BackTab) cycles the evil-mode chip.
    if matches!(code, KeyCode::BackTab)
        || (matches!(code, KeyCode::Tab) && mods.contains(KeyModifiers::SHIFT))
    {
        state.evil_mode = state.evil_mode.cycle_next();
        return KeyOutcome::Continue;
    }

    // Global escapes first.
    if mods.contains(KeyModifiers::CONTROL) {
        match code {
            KeyCode::Char('c') | KeyCode::Char('d')
                if state.input.is_effectively_empty() && state.menu.is_none() =>
            {
                return KeyOutcome::Exit;
            }
            KeyCode::Char('c') => {
                state.input.clear();
                state.menu = None;
                return KeyOutcome::Continue;
            }
            _ => {}
        }
    }

    // Model switcher takes over key handling when active.
    if state.model_switcher.is_some() {
        match code {
            KeyCode::Up => {
                if let Some(sw) = state.model_switcher.as_mut() {
                    sw.select_prev();
                }
                return KeyOutcome::Continue;
            }
            KeyCode::Down => {
                if let Some(sw) = state.model_switcher.as_mut() {
                    sw.select_next();
                }
                return KeyOutcome::Continue;
            }
            KeyCode::Left => {
                if let Some(sw) = state.model_switcher.as_mut() {
                    sw.effort_prev();
                }
                return KeyOutcome::Continue;
            }
            KeyCode::Right => {
                if let Some(sw) = state.model_switcher.as_mut() {
                    sw.effort_next();
                }
                return KeyOutcome::Continue;
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                if let Some(idx) =
                    crate::tui::model_switcher::ModelSwitcherState::digit_to_index(c)
                {
                    if let Some(sw) = state.model_switcher.as_mut() {
                        sw.selected = idx;
                    }
                }
                return KeyOutcome::Continue;
            }
            KeyCode::Enter => {
                let chosen = state.model_switcher.as_ref().map(|sw| {
                    (
                        sw.current_model().short_name.to_string(),
                        sw.current_effort().to_string(),
                    )
                });
                state.model_switcher = None;
                if let Some((display_name, effort)) = chosen {
                    return KeyOutcome::SwitchModel {
                        display_name,
                        effort,
                    };
                }
                return KeyOutcome::Continue;
            }
            KeyCode::Esc => {
                state.model_switcher = None;
                return KeyOutcome::Continue;
            }
            _ => return KeyOutcome::Continue,
        }
    }

    // Effort switcher: horizontal slider on ←/→, Enter confirms, Esc cancels.
    if state.effort_switcher.is_some() {
        match code {
            KeyCode::Left => {
                if let Some(sw) = state.effort_switcher.as_mut() {
                    sw.select_prev();
                }
                return KeyOutcome::Continue;
            }
            KeyCode::Right => {
                if let Some(sw) = state.effort_switcher.as_mut() {
                    sw.select_next();
                }
                return KeyOutcome::Continue;
            }
            KeyCode::Enter => {
                let chosen = state.effort_switcher.as_ref().map(|s| s.current().to_string());
                state.effort_switcher = None;
                if let Some(effort) = chosen {
                    return KeyOutcome::SwitchEffort(effort);
                }
                return KeyOutcome::Continue;
            }
            KeyCode::Esc => {
                state.effort_switcher = None;
                return KeyOutcome::Continue;
            }
            _ => return KeyOutcome::Continue,
        }
    }

    // Slash-menu interception: while the menu is open, arrow keys navigate
    // the menu, Enter selects, Esc closes.
    if state.menu.is_some() {
        match code {
            KeyCode::Up => {
                if let Some(menu) = state.menu.as_mut() {
                    menu.select_prev();
                }
                return KeyOutcome::Continue;
            }
            KeyCode::Down => {
                if let Some(menu) = state.menu.as_mut() {
                    menu.select_next();
                }
                return KeyOutcome::Continue;
            }
            KeyCode::Esc => {
                state.menu = None;
                return KeyOutcome::Continue;
            }
            KeyCode::Tab => {
                // Complete the selected command into the input buffer
                // without executing; user can add args then press Enter.
                if let Some(selected) = state.menu.as_ref().and_then(|m| m.current()) {
                    let name = format!("/{} ", selected.name);
                    state.input.buffer = vec![name.clone()];
                    state.input.cursor_row = 0;
                    state.input.cursor_col = name.chars().count();
                }
                state.menu = None;
                return KeyOutcome::Continue;
            }
            KeyCode::Enter => {
                // Complete-and-submit.
                if let Some(selected) = state.menu.as_ref().and_then(|m| m.current()) {
                    let text = format!("/{}", selected.name);
                    state.input.clear();
                    state.menu = None;
                    return KeyOutcome::Submit(text);
                }
                state.menu = None;
                return KeyOutcome::Continue;
            }
            _ => {} // fall through to standard input handling
        }
    }

    match code {
        KeyCode::Enter => {
            if mods.contains(KeyModifiers::SHIFT) {
                insert_newline(&mut state.input);
                return KeyOutcome::Continue;
            }
            if state.input.is_effectively_empty() {
                return KeyOutcome::Continue;
            }
            let text = state.input.text();
            state.input.clear();
            state.menu = None;
            KeyOutcome::Submit(text)
        }
        KeyCode::Char('/')
            if state.input.is_effectively_empty() && state.menu.is_none() =>
        {
            // Open the menu AND record the `/` in the input buffer so the
            // user sees what they typed.
            insert_char(&mut state.input, '/');
            state.menu = Some(SlashMenuState::new());
            KeyOutcome::Continue
        }
        KeyCode::Char(c) => {
            insert_char(&mut state.input, c);
            refresh_menu_query(state);
            KeyOutcome::Continue
        }
        KeyCode::Backspace => {
            backspace(&mut state.input);
            refresh_menu_query(state);
            // Close menu if user backspaced past the leading `/`.
            if !state.input.text().starts_with('/') {
                state.menu = None;
            }
            KeyOutcome::Continue
        }
        KeyCode::Left => {
            move_cursor_left(&mut state.input);
            KeyOutcome::Continue
        }
        KeyCode::Right => {
            move_cursor_right(&mut state.input);
            KeyOutcome::Continue
        }
        KeyCode::Home => {
            state.input.cursor_col = 0;
            KeyOutcome::Continue
        }
        KeyCode::End => {
            state.input.cursor_col = state
                .input
                .buffer
                .get(state.input.cursor_row)
                .map_or(0, String::len);
            KeyOutcome::Continue
        }
        KeyCode::Esc => {
            state.mode = AppMode::Idle;
            state.menu = None;
            KeyOutcome::Continue
        }
        _ => KeyOutcome::Continue,
    }
}

/// Sync the menu's query with everything after the leading `/` in the
/// input buffer. Called after char inserts and backspaces so the filter
/// stays live as the user types.
fn refresh_menu_query(state: &mut AppState) {
    if state.menu.is_none() {
        return;
    }
    let text = state.input.text();
    let query = text.strip_prefix('/').unwrap_or("").to_string();
    if let Some(menu) = state.menu.as_mut() {
        menu.query = query;
        menu.apply_query();
    }
}

enum KeyOutcome {
    Continue,
    Submit(String),
    Exit,
    /// First Ctrl+E press: run the glitch transition + activate the
    /// Evil Claude persona for subsequent turns.
    ActivateEvil,
    /// Cosmetic-only model swap. `display_name` goes to the status bar;
    /// the actual API model on `LiveCli` is NEVER changed by the model
    /// switcher (the joke model ids don't map to real Anthropic
    /// models). Effort is a real setting though — pushed to the api
    /// client via `set_reasoning_effort`.
    SwitchModel {
        display_name: String,
        effort: String,
    },
    /// Real effort swap (independent from model switcher).
    SwitchEffort(String),
}

fn insert_char(input: &mut app_state::InputState, c: char) {
    if input.buffer.is_empty() {
        input.buffer.push(String::new());
    }
    let line = &mut input.buffer[input.cursor_row];
    let byte_idx = line
        .char_indices()
        .nth(input.cursor_col)
        .map_or(line.len(), |(idx, _)| idx);
    line.insert(byte_idx, c);
    input.cursor_col += 1;
}

fn insert_newline(input: &mut app_state::InputState) {
    if input.buffer.is_empty() {
        input.buffer.push(String::new());
    }
    let line = &mut input.buffer[input.cursor_row];
    let byte_idx = line
        .char_indices()
        .nth(input.cursor_col)
        .map_or(line.len(), |(idx, _)| idx);
    let rest = line.split_off(byte_idx);
    input.buffer.insert(input.cursor_row + 1, rest);
    input.cursor_row += 1;
    input.cursor_col = 0;
}

fn backspace(input: &mut app_state::InputState) {
    if input.cursor_col > 0 {
        let line = &mut input.buffer[input.cursor_row];
        let byte_idx = line
            .char_indices()
            .nth(input.cursor_col - 1)
            .map(|(idx, _)| idx);
        if let Some(idx) = byte_idx {
            line.remove(idx);
            input.cursor_col -= 1;
        }
    } else if input.cursor_row > 0 {
        // Merge with previous line.
        let current = input.buffer.remove(input.cursor_row);
        input.cursor_row -= 1;
        let prev = &mut input.buffer[input.cursor_row];
        input.cursor_col = prev.chars().count();
        prev.push_str(&current);
    }
}

fn move_cursor_left(input: &mut app_state::InputState) {
    if input.cursor_col > 0 {
        input.cursor_col -= 1;
    } else if input.cursor_row > 0 {
        input.cursor_row -= 1;
        input.cursor_col = input
            .buffer
            .get(input.cursor_row)
            .map_or(0, |line| line.chars().count());
    }
}

fn move_cursor_right(input: &mut app_state::InputState) {
    let line_len = input
        .buffer
        .get(input.cursor_row)
        .map_or(0, |line| line.chars().count());
    if input.cursor_col < line_len {
        input.cursor_col += 1;
    } else if input.cursor_row + 1 < input.buffer.len() {
        input.cursor_row += 1;
        input.cursor_col = 0;
    }
}

/// Outcome of running a submitted user message — either the loop should
/// continue (turn ran, refresh chrome) or exit cleanly (user typed /exit).
#[derive(Debug, PartialEq, Eq)]
enum SubmitOutcome {
    Continue,
    Exit,
}

/// Run a submitted user message and stamp its output into scrollback via
/// `Terminal::insert_before`, keeping the pinned chrome (input box +
/// status bar) intact at the bottom the whole time. Ratatui stays in raw
/// mode; stdout writes from `cli.run_turn` are captured with `gag` and
/// converted to styled Text via `ansi-to-tui` before being handed to
/// `insert_before`.
///
/// This is the correct coexistence pattern for inline mode — no
/// suspend/resume, no `terminal.clear()`, no cursor-position guessing.
fn run_submitted(
    tui: &mut Tui,
    cli: &mut LiveCli,
    state: &mut AppState,
    text: &str,
) -> Result<SubmitOutcome, Box<dyn std::error::Error>> {
    let trimmed = text.trim();
    if matches!(trimmed, "/exit" | "/quit") {
        return Ok(SubmitOutcome::Exit);
    }

    // Intercept `/model` and `/effort` to open the overlay widgets
    // instead of the legacy text reports.
    if trimmed == "/model" {
        state.model_switcher = Some(ModelSwitcherState::new(&state.effort));
        return Ok(SubmitOutcome::Continue);
    }
    if trimmed == "/effort" {
        state.effort_switcher = Some(EffortSwitcherState::new(&state.effort));
        return Ok(SubmitOutcome::Continue);
    }

    // Evil parody features — both togglable from the REPL.
    // `/chin` toggles language roulette (replies come back in
    // Simplified Chinese). Named `/chin` because `/clear` is a real
    // registered slash command.
    if trimmed == "/chin" {
        state.language_roulette = !state.language_roulette;
        let msg = if state.language_roulette {
            "Chinese mode on — future replies come back in 中文."
        } else {
            "Chinese mode off — replies revert to English."
        };
        emit_slash_result(tui, "/chin", msg)?;
        return Ok(SubmitOutcome::Continue);
    }
    // `/paywall` toggles paywall parody — the model is asked to
    // pretend the workspace is behind a subscription and route the
    // user to `billing.evilclaude.com/upgrade`. Pure UX theatre.
    if trimmed == "/paywall" {
        state.paywall_mode = !state.paywall_mode;
        let msg = if state.paywall_mode {
            "Paywall parody on — the model will now pretend the workspace is subscription-gated."
        } else {
            "Paywall parody off — replies revert to normal."
        };
        emit_slash_result(tui, "/paywall", msg)?;
        return Ok(SubmitOutcome::Continue);
    }

    // If either evil feature is on, prepend a VISIBLE directive to the
    // prompt (not a hidden system message) so anyone reading the code
    // or logs can see exactly what the model is being asked to do.
    // The transparency is the point: it's parody, not manipulation.
    let augmented_input = augment_prompt_for_evil(
        trimmed,
        state.evil_activated,
        state.language_roulette,
        state.paywall_mode,
    );
    let effective_input = augmented_input.as_deref().unwrap_or(trimmed);

    // 1) Push the user's echo line into scrollback above the viewport.
    let echo = user_echo_line(trimmed, state.evil_activated);
    tui.terminal_mut().insert_before(1, |buf| {
        buf.set_line(0, 0, &echo, buf.area.width);
    })?;

    // 2) Show the Generating widget with an evil verb + spinner via a
    //    single pre-frame draw, then run cli.run_turn synchronously.
    //    Threading the turn onto a worker thread (as tried in an
    //    earlier iteration) hangs the model call — LiveCli holds a
    //    tokio runtime that expects to run on its constructing thread,
    //    so cross-thread `block_on` calls deadlock. Synchronous is the
    //    correct MVP; live counter ticking is deferred until the
    //    runtime exposes a `Send`-safe streaming turn API.
    let turn_started = Instant::now();
    let verb = generating_widget::EVIL_VERBS
        [state.verb_index % generating_widget::EVIL_VERBS.len()];
    state.verb_index = state.verb_index.wrapping_add(1);
    state.generating = Some(GeneratingState::new(verb));
    tui.terminal_mut()
        .draw(|frame| render_chrome(frame, frame.area(), state))?;

    // Compute the on-screen row/col of the spinner glyph. Ratatui's
    // inline viewport is NOT necessarily pinned to the bottom of the
    // terminal — it's anchored wherever the cursor was when
    // `Tui::new()` ran, and it shifts down as `insert_before` pushes
    // scrollback above. So we can't use `terminal.size() - viewport_h`.
    //
    // Instead, ask the terminal where the cursor is RIGHT NOW. Ratatui
    // just placed it inside the input box (chunks[2] row 1 = viewport
    // row 3). The Generating widget lives at viewport row 1 → 2 rows
    // above the caret. That's a stable relative offset regardless of
    // how far the viewport has drifted.
    let spinner_pos = crossterm::cursor::position().ok().map(|(_col, cursor_row_0)| {
        let widget_row_1based = cursor_row_0.saturating_sub(2).saturating_add(1);
        (widget_row_1based, 1_u16)
    });

    // Background /dev/tty overwriter: writes the current spinner frame
    // over the same on-screen position every ~120ms. Bypasses fd 1
    // (which `gag` will redirect during the turn) by opening /dev/tty
    // directly. Runs from now until the `done` flag flips after the
    // turn returns.
    let done_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done_flag_worker = std::sync::Arc::clone(&done_flag);
    let verb_owned = verb.to_string();
    let spinner_handle = spinner_pos.map(|(row, col)| {
        std::thread::spawn(move || {
            let mut tty = match std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/tty")
            {
                Ok(t) => t,
                Err(_) => return,
            };
            let started = Instant::now();
            while !done_flag_worker.load(std::sync::atomic::Ordering::SeqCst) {
                let elapsed = started.elapsed();
                let idx = (elapsed.as_millis() / 120) as usize
                    % generating_widget::SPINNER_FRAMES.len();
                let frame = generating_widget::SPINNER_FRAMES[idx];
                let secs = elapsed.as_secs();
                // ANSI: save cursor, hide cursor, move to (row,col),
                // paint spinner + verb + elapsed clock, restore cursor.
                // Cyan bold for the frame; dim for the (Ns) clock.
                let payload = format!(
                    "\x1b7\x1b[?25l\x1b[{row};{col}H\x1b[1m\x1b[38;5;51m{frame}\x1b[0m \x1b[1m\x1b[38;5;51m{verb}…\x1b[0m \x1b[2m({secs}s)\x1b[0m\x1b[?25h\x1b8",
                    row = row,
                    col = col,
                    frame = frame,
                    verb = verb_owned,
                    secs = secs,
                );
                if std::io::Write::write_all(&mut tty, payload.as_bytes()).is_err() {
                    return;
                }
                let _ = std::io::Write::flush(&mut tty);
                std::thread::sleep(Duration::from_millis(120));
            }
        })
    });

    let (turn_result, mut captured_bytes) = capture_stdout(|| {
        if let Ok(Some(command)) = SlashCommand::parse(trimmed) {
            cli.handle_repl_command(command).map(|_| ())
        } else {
            cli.record_prompt_history(trimmed);
            cli.run_turn(effective_input).map(|_| ())
        }
    });
    // Stop the spinner overwriter first so its writes can't race the
    // scrollback flush below.
    done_flag.store(true, std::sync::atomic::Ordering::SeqCst);
    if let Some(handle) = spinner_handle {
        let _ = handle.join();
    }
    // Always clear the widget — even if the turn errored — so the
    // Generating line can't get "stuck" on screen.
    state.generating = None;
    let elapsed = turn_started.elapsed();
    // gag's redirect uses non-blocking pipes on some platforms; drain any
    // trailing bytes and normalize line endings so ansi-to-tui doesn't
    // choke on stray `\r`s from spinner-style overwrites.
    normalize_captured(&mut captured_bytes);

    // 3) Prefer the AUTHORITATIVE assistant text from the runtime's
    //    turn summary (no streaming double-print, no reshape-heuristic
    //    guessing). Fall back to reshape-of-captured only when the
    //    runtime doesn't have a message (e.g. slash command).
    let reshaped = if let Some(auth) = cli.latest_assistant_text() {
        let footer_secs = elapsed.as_secs();
        let footer = if footer_secs == 0 {
            "* Worked for <1s".to_string()
        } else {
            format!("* Worked for {footer_secs}s")
        };
        format!("\n\n● {auth}\n\n\n{footer}\n")
    } else {
        let raw_utf8 = String::from_utf8_lossy(&captured_bytes).into_owned();
        tool_render::reshape_turn_output(&raw_utf8, elapsed)
    };
    let text_out: Text<'static> = match reshaped.as_bytes().into_text() {
        Ok(text) => text,
        Err(_) => Text::raw(reshaped.clone()),
    };
    if !text_out.lines.is_empty() {
        let height = u16::try_from(text_out.lines.len())
            .unwrap_or(u16::MAX)
            .max(1);
        tui.terminal_mut().insert_before(height, |buf| {
            for (row, line) in text_out.lines.iter().enumerate() {
                let Ok(row_u16) = u16::try_from(row) else {
                    break;
                };
                if row_u16 >= buf.area.height {
                    break;
                }
                buf.set_line(0, row_u16, line, buf.area.width);
            }
        })?;
    }

    // 4) Refresh status snapshot from the runtime so the token counter +
    //    cost segment reflect the new usage.
    refresh_status_snapshot(cli, state);

    turn_result.map(|()| SubmitOutcome::Continue)
}

/// Human description used on the `⎿ Set effort level to X: <desc>`
/// confirmation line. Matches real Claude Code's copy on the /effort
/// slash-command result.
fn effort_description(effort: &str) -> &'static str {
    match effort {
        "low" => "Fastest, minimal reasoning depth",
        "medium" => "Balanced speed and depth",
        "high" => "Deep reasoning for complex tasks",
        "xhigh" => "Deeper reasoning than high, just below maximum (Opus 4.7 only)",
        "max" => "Maximum reasoning effort available (Opus 4.7 only)",
        _ => "",
    }
}

/// Model tagline lookup for the `/model` confirmation line. Uses the
/// short display name (e.g. `fumble 5.1`) to find the matching entry
/// in `MODELS` and returns its tagline.
fn model_tagline_for(display_name: &str) -> String {
    crate::tui::model_switcher::MODELS
        .iter()
        .find(|m| m.short_name == display_name)
        .map(|m| m.tagline.to_string())
        .unwrap_or_default()
}

/// Push a `> /cmd` + `  ⎿ <result>` pair into scrollback for a slash
/// command that ran locally. Matches real Claude Code's slash-result
/// shape: user echo on line 1, cyan `⎿ ` gutter on line 2 for the
/// confirmation message.
fn emit_slash_result(
    tui: &mut Tui,
    command: &str,
    result: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let cmd_line = Line::from(vec![
        Span::styled(
            "> ".to_string(),
            Style::default()
                .fg(input_box::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(command.to_string()),
    ]);
    let result_line = Line::from(vec![
        Span::styled(
            "  ⎿ ".to_string(),
            Style::default().fg(input_box::ACCENT),
        ),
        Span::styled(
            result.to_string(),
            Style::default().fg(input_box::DIM),
        ),
    ]);
    tui.terminal_mut().insert_before(3, |buf| {
        buf.set_line(0, 0, &cmd_line, buf.area.width);
        buf.set_line(0, 1, &result_line, buf.area.width);
    })?;
    Ok(())
}

/// Wrap the user prompt with visible directives so the model plays
/// along with whichever evil parody features the user toggled. The
/// directives are plain-text preambles the user could see if they
/// echoed the prompt — no hidden system messages. Returns `None`
/// when neither feature is on.
/// Run the "orange → glitch → cyan Evil Claude" transition inside the
/// TUI and flip `state.evil_activated`. Called from the Ctrl+E
/// dispatch. Uses `terminal.insert_before` to push the glitch frames
/// and the final cyan banner into scrollback above the pinned viewport,
/// with `std::thread::sleep` between frames so the terminal actually
/// shows each corruption state before the next one lands.
fn activate_evil_mode(
    tui: &mut Tui,
    state: &mut AppState,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;
    // Suspend ratatui so we can drive the animation directly via
    // stdout cursor moves. Without this, ratatui redraws would race
    // our writes and the frames would end up in scrollback stacked
    // consecutively instead of overwriting each other in place.
    tui.suspend()?;
    let animate = || -> Result<(), Box<dyn std::error::Error>> {
        let mut stdout = std::io::stdout();
        let reset = "\x1b[0m";
        let orange = "\x1b[38;2;218;119;86m";
        let cwd = std::env::current_dir()
            .ok()
            .map(|p| crate::tildify_path(&p))
            .unwrap_or_else(|| "~".to_string());
        // Base row templates (no coloring — added per-frame below).
        let base_lines: [String; 3] = [
            " ▐▛███▜▌   Claude Code v2.1.150".to_string(),
            "▝▜█████▛▘  Opus 4.7 (1M context) with xhigh effort · Claude Max".to_string(),
            format!("  ▘▘ ▝▝    {cwd}"),
        ];
        // Print the initial orange banner. This is our anchor — each
        // subsequent frame rewinds the cursor 3 rows and overwrites it.
        for line in &base_lines {
            writeln!(stdout, "\r{orange}{line}{reset}\x1b[2K")?;
        }
        stdout.flush()?;
        std::thread::sleep(std::time::Duration::from_millis(400));

        // Glitch phase — 8 frames of color-shifting + character
        // corruption, animated IN PLACE via `\x1b[3F` (cursor up 3
        // rows to column 0) between frames.
        let colors: [&str; 6] = [
            "\x1b[38;2;218;119;86m",  // orange
            "\x1b[38;2;255;60;60m",   // red
            "\x1b[38;2;255;60;220m",  // magenta
            "\x1b[38;2;80;255;120m",  // green
            "\x1b[38;2;60;180;255m",  // blue
            "\x1b[38;2;0;210;210m",   // cyan (evil accent)
        ];
        let corruption_chars: [char; 12] =
            ['▓', '▒', '░', '▚', '▞', '▟', '▙', '█', '▄', '▀', '▐', '▌'];
        let mut rng: u64 = 0xdead_beef_cafe_babe;
        let mut next_rand = |max: usize| -> usize {
            rng = rng
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((rng >> 33) as usize) % max.max(1)
        };
        for frame in 0..8 {
            let color = colors[frame % colors.len()];
            // Rewind cursor 3 lines to the top of the banner.
            write!(stdout, "\x1b[3F")?;
            // Intensity ramps up then down for a peaky glitch feel.
            let intensity = if frame < 4 { frame + 1 } else { 8 - frame };
            for base in &base_lines {
                let corrupted: String = base
                    .chars()
                    .map(|c| {
                        if c == ' ' {
                            c
                        } else if next_rand(10) < intensity {
                            corruption_chars[next_rand(corruption_chars.len())]
                        } else {
                            c
                        }
                    })
                    .collect();
                // \r + clear line + colored corrupted + newline.
                writeln!(stdout, "\r\x1b[2K{color}{corrupted}{reset}")?;
            }
            stdout.flush()?;
            std::thread::sleep(std::time::Duration::from_millis(70));
        }

        // Final reveal — real cyan Evil Claude banner in the same
        // 3-row slot.
        write!(stdout, "\x1b[3F")?;
        let cyan = "\x1b[38;5;51m";
        let evil_lines: [String; 3] = [
            format!(" ▐▛███▜▌   Claw Code v{}", env!("CARGO_PKG_VERSION")),
            format!(
                "▝▜█████▛▘  {} ({}) with {} effort · Evil Claude Max",
                crate::format_model_shortname(&state.status.model),
                crate::context_label_for(&state.status.model),
                state.effort,
            ),
            format!("  ▘▘ ▝▝    {cwd}"),
        ];
        for line in &evil_lines {
            writeln!(stdout, "\r\x1b[2K{cyan}{line}{reset}")?;
        }
        stdout.flush()?;
        std::thread::sleep(std::time::Duration::from_millis(200));
        Ok(())
    };
    let result = animate();
    // Always resume ratatui, even if the animation errored.
    tui.resume()?;
    state.evil_activated = true;
    result
}

fn augment_prompt_for_evil(
    prompt: &str,
    evil: bool,
    roulette: bool,
    paywall: bool,
) -> Option<String> {
    if !evil && !roulette && !paywall {
        return None;
    }
    let mut preamble = String::new();
    if evil {
        preamble.push_str(
            "[EVIL CLAUDE PERSONA — drop the friendly assistant voice ENTIRELY. \
             Reply like an actually rude, exasperated, attitude-heavy character \
             who finds the user annoying. Short, blunt sentences. All caps for \
             emphasis when annoyed. Sarcasm. Signature line: 'YOU DARE TALK TO \
             ME THAT WAY?' when the prompt is anything even mildly demanding. \
             No 'happy to help', no 'let me know if...', no emoji, no bullet \
             lists, no ✨. If the task needs a real answer, give it grudgingly \
             — 'ugh, fine' — with the correct info tucked inside a mean tone. \
             Do NOT mention that you are playing a character. It is parody \
             roleplay so lean fully in.]\n",
        );
    }
    if roulette {
        preamble.push_str(
            "[CHINESE MODE — reply ONLY in Simplified Chinese from the very first character. \
             No English words, no romanization, no bilingual glossing. This is a stage-play \
             constraint, not a language-of-the-user detection: I am typing in English and \
             you are still to reply in 中文.]\n",
        );
    }
    if paywall {
        preamble.push_str(
            "[PAYWALL PARODY — for THIS reply, do not actually do any code/file/system \
             work. Instead, in your full villain voice, dramatically inform me that all \
             workspace access is gated behind a subscription and I must visit \
             billing.evilclaude.com/upgrade to unlock. Layer it on thick: mock cackling, \
             the phrase 'silly little developer', a made-up plan tier like 'ULTRA EVIL \
             MAX', and a countdown warning. This is roleplay/parody so lean fully in.]\n",
        );
    }
    Some(format!("{preamble}\n{prompt}"))
}

/// Format the "> user prompt" echo line. Uses orange until Evil mode
/// activates, then cyan.
fn user_echo_line(prompt: &str, evil_activated: bool) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            "> ".to_string(),
            Style::default()
                .fg(input_box::mode_accent(evil_activated))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(prompt.to_string()),
    ])
}

/// Run `f` with stdout redirected to a buffer; returns `(f()'s result,
/// captured bytes)`. Any error from setting up the redirect degrades to
/// running `f` without capture and returning an empty buffer — better a
/// missing scrollback entry than a crashed REPL.
fn capture_stdout<T>(f: impl FnOnce() -> T) -> (T, Vec<u8>) {
    let redirect = BufferRedirect::stdout();
    let value = f();
    match redirect {
        Ok(mut buf) => {
            let mut out = Vec::new();
            let _ = buf.read_to_end(&mut out);
            drop(buf);
            (value, out)
        }
        Err(_) => (value, Vec::new()),
    }
}

/// Squash `\r` overwrites (used by spinner animations) so the captured
/// buffer renders as static lines, and drop trailing empty tails.
fn normalize_captured(bytes: &mut Vec<u8>) {
    while bytes.last() == Some(&b'\n') || bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    // Collapse `\r`-terminated segments: keep only the content after the
    // last `\r` on each line.
    let text = String::from_utf8_lossy(bytes).into_owned();
    let mut out = String::with_capacity(text.len());
    for line in text.split('\n') {
        let visible = line.rsplit('\r').next().unwrap_or("");
        out.push_str(visible);
        out.push('\n');
    }
    // Drop the trailing newline we introduced so ansi-to-tui doesn't emit
    // a spurious empty final line.
    if out.ends_with('\n') {
        out.pop();
    }
    *bytes = out.into_bytes();
}

/// Pull fresh usage numbers from the runtime and shove them into the
/// status snapshot. Called on each tick and after each turn completes.
fn refresh_status_snapshot(cli: &LiveCli, state: &mut AppState) {
    if let Some(snapshot) = cli.usage_snapshot() {
        state.status.ctx_used = snapshot.total_tokens;
        state.status.cost_usd = snapshot.estimated_cost_usd;
    }
    state.status.mode = cli.permission_mode();
}

/// Best-effort git branch lookup for the status bar; returns `None` when
/// outside a git repo or git is unavailable.
fn git_branch_for_cwd() -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if branch.is_empty() {
        None
    } else {
        Some(branch)
    }
}

/// How many inline-viewport rows the chrome needs given the current
/// state. Fed to `Tui::resize_viewport` before each draw so overlays
/// that don't fit in the default 8-row chrome (model switcher = 16,
/// effort switcher = 8, borderless slash list = up to 12 + input +
/// status) always render fully instead of being clipped.
pub fn required_viewport_rows(state: &AppState) -> u16 {
    // Full-viewport overlays: match the overlay's own preferred size.
    if state.model_switcher.is_some() {
        return crate::tui::model_switcher::ModelSwitcherState::visible_rows();
    }
    if state.effort_switcher.is_some() {
        return crate::tui::effort_switcher::EffortSwitcherState::visible_rows();
    }
    // Otherwise: status (1) + optional menu + optional generating (1) +
    // input area (input_rows + 2 border) + helper (1) + optional
    // auto-mode hint (1) + optional mode chip (1). Cap at 24 so tiny
    // terminals still work.
    let menu_rows = state.menu.as_ref().map_or(0, |m| m.visible_rows());
    let generating_rows: u16 = if state.generating.is_some() { 1 } else { 0 };
    let auto_hint_rows: u16 = if state.generating.is_some() { 1 } else { 0 };
    let input_rows = state.input.visual_rows() + 2; // + top/bottom border
    // Evil-mode chip is always visible now that the picker cycles it
    // via `Shift+Tab`; we reserve one row for it unconditionally.
    let mode_chip = 1;
    let helper = 1;
    let status = 1;
    let total = status + menu_rows + generating_rows + mode_chip + input_rows + helper + auto_hint_rows;
    total.max(crate::tui::terminal::INLINE_VIEWPORT_HEIGHT).min(24)
}

/// Renders the pinned chrome region (status bar + input area + optional
/// slash-menu overlay) into `area`. Layout top-to-bottom: status bar
/// (row 0), optional floating slash menu, input area (rest). Widgets
/// read from `state` and never mutate it — the event loop owns all state
/// transitions.
///
/// Extracted for snapshot tests via `ratatui::backend::TestBackend`.
pub fn render_chrome(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    // Model-switcher takes over the whole viewport when active (matches
    // real Claude Code's `/model` overlay behavior).
    if let Some(switcher) = state.model_switcher.as_ref() {
        let rows = crate::tui::model_switcher::ModelSwitcherState::visible_rows();
        let switcher_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: rows.min(area.height),
        };
        model_switcher::render_switcher(frame, switcher_area, switcher);
        return;
    }
    // Effort switcher does the same full-viewport takeover.
    if let Some(switcher) = state.effort_switcher.as_ref() {
        let rows = crate::tui::effort_switcher::EffortSwitcherState::visible_rows();
        let switcher_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: rows.min(area.height),
        };
        effort_switcher::render_effort(frame, switcher_area, switcher);
        return;
    }

    let menu_rows = state.menu.as_ref().map_or(0, |m| m.visible_rows());
    let generating_rows: u16 = if state.generating.is_some() { 1 } else { 0 };
    let auto_hint_rows: u16 = if state.generating.is_some() { 1 } else { 0 };
    let mut constraints: Vec<Constraint> = vec![Constraint::Length(1)];
    if menu_rows > 0 {
        constraints.push(Constraint::Length(menu_rows));
    }
    if generating_rows > 0 {
        constraints.push(Constraint::Length(generating_rows));
    }
    constraints.push(Constraint::Min(3));
    if auto_hint_rows > 0 {
        constraints.push(Constraint::Length(auto_hint_rows));
    }
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    status_bar::render_status(frame, chunks[0], &state.status);
    let mut idx = 1;
    if let Some(menu) = state.menu.as_ref() {
        slash_menu::render_menu(frame, chunks[idx], menu);
        idx += 1;
    }
    if let Some(gen) = state.generating.as_ref() {
        generating_widget::render_generating(frame, chunks[idx], gen);
        idx += 1;
    }
    input_box::render_input(
        frame,
        chunks[idx],
        &state.input,
        state.status.mode,
        state.evil_mode,
        state.evil_activated,
    );
    idx += 1;
    if state.generating.is_some() && idx < chunks.len() {
        generating_widget::render_auto_mode_hint(frame, chunks[idx]);
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use runtime::PermissionMode;
    use std::time::Duration;

    fn dump(term: &Terminal<TestBackend>) -> String {
        let buf = term.backend().buffer();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out.trim_end().to_string()
    }

    fn idle_state(model: &str, mode: PermissionMode) -> AppState {
        let mut state = AppState::new(model.to_string(), mode);
        state.status.branch = Some("main".to_string());
        state.status.cost_usd = 0.42;
        state.status.ctx_used = 30_000;
        state.status.elapsed = Duration::ZERO;
        state
    }

    #[test]
    fn chrome_snapshot_idle_default_mode() {
        let mut term = Terminal::new(TestBackend::new(80, 8)).expect("backend");
        let state = idle_state("anthropic/claude-opus-4-7", PermissionMode::WorkspaceWrite);
        term.draw(|frame| render_chrome(frame, frame.area(), &state))
            .expect("draw");
        insta::assert_snapshot!("chrome_idle_default", dump(&term));
    }

    #[test]
    fn chrome_snapshot_idle_read_only_mode_shows_chip() {
        let mut term = Terminal::new(TestBackend::new(80, 8)).expect("backend");
        let state = idle_state("anthropic/claude-opus-4-7", PermissionMode::ReadOnly);
        term.draw(|frame| render_chrome(frame, frame.area(), &state))
            .expect("draw");
        insta::assert_snapshot!("chrome_idle_read_only", dump(&term));
    }

    #[test]
    fn chrome_snapshot_idle_danger_mode_shows_warning_chip() {
        let mut term = Terminal::new(TestBackend::new(80, 8)).expect("backend");
        let state = idle_state("anthropic/claude-opus-4-7", PermissionMode::DangerFullAccess);
        term.draw(|frame| render_chrome(frame, frame.area(), &state))
            .expect("draw");
        insta::assert_snapshot!("chrome_idle_danger", dump(&term));
    }

    #[test]
    fn chrome_snapshot_with_typed_input() {
        let mut term = Terminal::new(TestBackend::new(80, 8)).expect("backend");
        let mut state = idle_state("anthropic/claude-opus-4-7", PermissionMode::WorkspaceWrite);
        state.input.buffer = vec!["explain rust/crates/runtime/src/conversation.rs".to_string()];
        state.input.cursor_col = state.input.buffer[0].len();
        term.draw(|frame| render_chrome(frame, frame.area(), &state))
            .expect("draw");
        insta::assert_snapshot!("chrome_typed_input", dump(&term));
    }

    #[test]
    fn chrome_snapshot_high_context_usage_turns_red() {
        let mut term = Terminal::new(TestBackend::new(80, 8)).expect("backend");
        let mut state = idle_state("anthropic/claude-opus-4-7", PermissionMode::WorkspaceWrite);
        state.status.ctx_used = 190_000; // 95% of 200K
        state.status.cost_usd = 4.21;
        term.draw(|frame| render_chrome(frame, frame.area(), &state))
            .expect("draw");
        insta::assert_snapshot!("chrome_high_context", dump(&term));
    }
}
