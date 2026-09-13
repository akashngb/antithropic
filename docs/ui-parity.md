# UI Parity — `claw` ↔ Claude Code

**Target**: `claude` (Claude Code CLI), latest shipping version — as of writing, `v2.1.150`.
**Purpose**: this doc is the human-maintained source of truth for what the real Claude Code UI looks like, surface by surface. Every `claw` widget has a matching section here. When real Claude Code changes, update the section, then update the widget + snapshot.

**How to use**:
- Reviewing a slice PR? Every widget-touching PR must update the relevant section's `Last verified` date and, if visuals changed, the ASCII reference.
- Real Claude Code shipped a new UI? File an issue titled `parity: <surface> drift observed`, update this doc, then update the widget.
- Adding a new surface? Add a `##` section here first, then implement.

Snapshot fixtures live under `rust/crates/rusty-claude-cli/tests/snapshots/tui/` via [`insta`](https://insta.rs/). Run `cargo insta review` in `rust/` to accept or reject drift.

Stale-date check: `scripts/ui-parity-check.sh` warns on sections whose `Last verified` is older than 60 days.

---

## Banner

**Last verified**: 2026-09-13 against `claude v2.1.150`.

**Reference** (real Claude Code):
```
 ▐▛███▜▌   Claude Code v2.1.150
▝▜█████▛▘  Opus 4.7 (1M context) with xhigh effort · Claude Max
  ▘▘ ▝▝    ~/antithropic
```

**Layout**:
- 3 lines total. Left column: 3-line block-glyph pixel logo, 8 chars wide + 3-space gutter.
- Right column: (line 1) product name + version; (line 2) model + `(context)` + `with <effort> effort` + `· <tier>`; (line 3) tildified cwd.

**Colors**: logo in Evil Claude cyan, truecolor `#00D2D2` (`Rgb(0, 210, 210)`) / fallback ANSI 256 code 51. Right column plain default foreground; version/tier in normal weight (no bold). (Rebrand from Anthropic orange landed alongside Slice 2 persistence fix — see change log.)

**Fields sourced from**:
- Version — `env!("CARGO_PKG_VERSION")` for us. Claude Code reads from npm package.
- Model — the currently selected model (e.g. `Opus 4.7 (1M context)`). Context window comes from the model card.
- Effort — `low` / `medium` / `high` / `xhigh` from reasoning-effort setting.
- Tier — `Claude Max` / `Claude Pro` / `Personal` from subscription. For `claw` we read `.claw.json:subscription_tier` (string, optional; default `Personal`).
- Cwd — `env::current_dir()`, `$HOME` prefix replaced with `~`.

**What real Claude Code does NOT show**: no permission mode, no branch, no session ID, no auto-save path, no tips footer, no `Connected:` line. Those either live in the status bar (below) or aren't shown at all.

**Snapshot**: `rust/crates/rusty-claude-cli/src/snapshots/claw__tests__banner_default_snapshot_pins_exact_bytes.snap` (landed Slice 1).

**Deviations we accept**:
- Product name `Claw Code` (not `Claude Code`), because this is a re-implementation. Downstream skin (Evil Claude) may change again.
- Truecolor fallback to ANSI 256 code 51 (bright cyan) if `$COLORTERM` doesn't advertise `truecolor`.
- Reads `subscriptionTier` from `.claw.json` via a standalone helper (`read_subscription_tier_from_cwd`), not through `RuntimeFeatureConfig`. Full plumbing is a Slice 2+ refactor.
- Context label is heuristic — `1M context` when the model id contains `[1m]`/`-1m`, else `200K context`. Real Claude Code presumably reads from a model registry; we can wire that in a later slice if the heuristic breaks.

---

## Input box

**Last verified**: 2026-09-13 against `claude v2.1.150`.

**Reference**:
```
╭──────────────────────────────────────────────────╮
│ > _                                              │
╰──────────────────────────────────────────────────╯
  ? for shortcuts · Shift+Tab to cycle mode · @ for files · ! for bash
```

**Layout**:
- Rounded-border box pinned to the bottom of the terminal, spans full width.
- Inside: `>` cursor prompt at left, then the buffer, then blinking caret. Multi-line grows the box upward.
- Below the box: dim helper text (one line). Additional lines appear only when the input wraps or during a mode change.
- When a non-default mode is active, a chip appears ABOVE the box: `⏸ plan mode` / `● auto-accept edits` / `⚠ danger mode` in the mode's color.

**Colors**: border in Evil Claude cyan accent (matches banner). Helper text in dim gray. Mode chip color varies (blue for read-only, red for danger).

**Behavior**: text wraps at box width. Shift+Enter (or Meta+Enter) inserts a newline. Enter submits. Backspace/Delete edit. Home/End/Ctrl+A/Ctrl+E navigate. Up/Down in an empty buffer cycles prompt history; otherwise moves cursor line-wise. Escape while streaming aborts the turn.

**Snapshot**: `rust/crates/rusty-claude-cli/src/tui/snapshots/claw__tui__snapshot_tests__chrome_idle_default.snap` + `chrome_idle_read_only.snap` + `chrome_idle_danger.snap` + `chrome_typed_input.snap` (landed Slice 2).

**Deviations**:
- Slice 2 MVP suspends ratatui while a turn is streaming, so the input box + status bar are temporarily hidden. True `insert_before` coexistence ships in Slice 4 alongside the tool-call rendering rewrite (both share the same narrate discipline).
- Under `PermissionMode::Prompt`/`Allow` no chip is shown (the runtime enum doesn't yet expose `Plan`/`AcceptEdits`); Slice 6 adds those variants + the Shift+Tab cycle.
- When ratatui init fails (pty without stdin able to answer the DSR cursor-position query — happens under `script /dev/null`, some remote shells), `tui::run_repl` prints a one-line notice and falls back to the legacy rustyline REPL. Real interactive terminals always succeed.

---

## Status bar

**Last verified**: 2026-09-13 against `claude v2.1.150`.

**Reference** (one line, above the input box, below the message stream):
```
Opus 4.7 · ⧉ 12% (30K/250K) · $0.42 · ● workspace-write · main
```

**Segments** (left to right, `·` separator):
1. Model short name (e.g. `Opus 4.7`).
2. Context usage: `⧉ <pct>% (<used>/<total>)`. Total = model's context window.
3. Cumulative cost this session: `$<usd>`.
4. Permission mode dot + name: `●` in mode color, then `workspace-write` / `plan` / `read-only` / `danger-full-access`.
5. Git branch (only inside a git repo).

**Colors**: model name and cost in normal foreground; percentage colored by band (green < 50%, yellow 50-80%, red > 80%); mode dot per mode; branch in dim.

**Updates**: token counter and cost update live during streaming as `AssistantEvent::Usage` arrives.

**Snapshot**: bundled with the `chrome_*` composite snapshots (see Input box section) — the status bar renders in row 0 of every chrome snapshot (landed Slice 2). Standalone status-bar snapshots aren't necessary because the composite chrome fixture pins every segment.

**Deviations**:
- Turn-elapsed clock (`elapsed`) is tracked in `StatusState` but not yet composed into the status line — that's a small addendum for Slice 4 when streaming truly overlays with chrome.
- Prompt-cache read/write breakdown isn't shown (real Claude Code shows only cumulative context usage in the status bar too).

---

## Slash-command menu

**Last verified**: 2026-09-13 against `claude v2.1.150`.

**Reference** (floats ABOVE the input box when user types `/`):
```
╭─────────────────────────────────────────────────╮
│ /help          Show available slash commands    │
│ /status        Show current session status      │
│ /compact       Compact local session history    │
│ /clear         Start a fresh local session      │
│ ▶ /cost        Show cumulative token usage      │
│ /config        Inspect Claude config            │
╰─────────────────────────────────────────────────╯
```

**Layout**:
- Rounded-border overlay, pinned just above the input box.
- Up to 8 visible rows (scroll on overflow).
- Each row: command name (left column, ~14 chars), one-line description (right column, dim).
- Selected row marked with `▶` and inverted background.

**Behavior**: typing after `/` fuzzy-filters. Up/Down navigate. Tab completes into input without executing. Enter completes and executes. Escape closes menu; other characters close it and revert input to plain.

**Snapshot**: `rust/crates/rusty-claude-cli/src/tui/snapshots/claw__tui__slash_menu__tests__slash_menu_default.snap` + `slash_menu_filtered_mod.snap` (landed Slice 3).

**Deviations**:
- Curated command list (`CURATED` in `slash_menu.rs`) trims the 100+ commands the `commands` crate registers to the ~30 that real Claude Code surfaces. When a new command lands upstream, update `CURATED` too.
- Fuzzy scorer is a bespoke ~50 LOC subsequence ranker (no `fuzzy-matcher` dep). Good enough for slash-command filtering; not a general fuzzy library.

---

## @-mention picker

**Last verified**: 2026-09-13 against `claude v2.1.150`.

**Reference** (fuzzy file picker, floats above input when user types `@`):
```
╭─────────────────────────────────────────────────╮
│ ▶ src/main.rs                                   │
│ src/lib.rs                                      │
│ src/tui/mod.rs                                  │
│ Cargo.toml                                      │
╰─────────────────────────────────────────────────╯
```

**Behavior**: walks cwd (respecting `.gitignore`), fuzzy filter as user types after `@`, arrow-key navigate, Enter inserts the path into the input, Escape cancels.

**Snapshot**: `mention_picker_default.txt` (pending — deferred past Slice 6).

**Deviations**: initial slices defer this — placeholder stub only.

---

## Message stream / markdown rendering

**Last verified**: 2026-09-13 against `claude v2.1.150`.

**Reference**: assistant text streams live in the terminal scroll area (above the pinned chrome), with incremental markdown rendering — headers bolded and colored, `**bold**` bold, `*italic*` italic, `` `inline code` `` in colored monospace, ```code blocks``` syntax-highlighted via a lexer, tables with borders, task lists, blockquotes with left bar, links in colored underline.

**Preceded by**: `●` bullet marker at the message start (subtle, gray).

**Update discipline**: characters appear as they arrive from the SSE stream — no artificial delay. Partial markdown (open bold `**foo`) renders as literal until closed.

**Snapshot**: rendered fragments captured in existing `render.rs` tests. No new snapshots needed for this surface; parity means "matches what render.rs already does, plus prompt no artificial delay."

**Deviations**: our render.rs already implements the substance; parity here is just removing the 8ms per-chunk delay in `stream_markdown` (Slice 2 side-effect).

---

## Tool-call rendering

**Last verified**: 2026-09-13 against `claude v2.1.150`.

**Reference**: inline in the message stream, not boxed:
```
● Bash(ls -la)
  ⎿  total 48
     drwxr-xr-x  5 user  staff  160 Sep 13 00:30 .
     -rw-r--r--  1 user  staff  312 Sep 13 00:30 Cargo.toml
     +14 lines (ctrl+r to expand)
```

**Layout**:
- Line 1: `⏺ <ToolName>(<primary-arg-summary>)`. Tool name in orange, args in normal.
- Line 2+: `⎿ ` gutter (2 spaces), then preview of output. Continuation lines indented under the gutter.
- Truncation: if preview > N lines (default 15), show first N-1 lines and a `+X lines (ctrl+r to expand)` hint.
- Error result: `⏺ <ToolName>(...)` line stays, then `⎿ Error: <message>` in red.

**Colors**: `⏺` and tool name in orange; `⎿` and gutter in dim; preview in default fg (or syntect-highlighted if code-shaped).

**Snapshot**: `rust/crates/rusty-claude-cli/src/tui/snapshots/claw__tui__tool_render__tests__reshape_end_to_end.snap` (landed Slice 4).

**Deviations**:
- Slice 4 keeps the stdout-capture-and-reshape architecture from Slice 2, rather than refactoring the runtime's tool-execution event stream. That means the `⏺ Tool(args) / ⎿ preview` shape is produced by pattern-matching on the captured `╭─ Tool ─╮` panels and rewriting them post-hoc.
- Streaming is still batch-per-turn (whole response appears at once when the turn completes). True live streaming ships when the runtime grows an `AssistantEvent`-forwarding API.
- `ctrl+r to expand` requires scrollback interaction — deferred past Slice 6.
- Spinner "Thinking…" phrases are stripped entirely from scrollback rather than rendered as an in-place animation.

---

## Edit / Write diff

**Last verified**: 2026-09-13 against `claude v2.1.150`.

**Reference** (Edit/Write tool result shows a colored unified diff instead of `✓ path`):
```
● Edit(src/main.rs)
  ⎿  Updated src/main.rs with 2 additions and 1 removal
       12   fn main() {
       13 -     println!("hello");
       13 +     println!("hello, world");
       14 +     println!("goodbye");
       15   }
```

**Layout**:
- Header line: `⏺ Edit(path)` in orange (same as other tool calls).
- Summary line: `⎿ Updated <path> with N additions and M removals`.
- Diff hunk: line numbers (left, dim), gutter char (`-` red, `+` green, space unchanged), then content with syntect highlighting on the whole line.
- No `diff --git` cruft. No surrounding box.
- Multi-hunk diffs get a blank line between hunks; no `@@` separators.

**Snapshot**: `rust/crates/rusty-claude-cli/src/tui/snapshots/claw__tui__diff_view__tests__diff_view_multiline.snap` (landed Slice 4).

**Deviations**:
- Uses the `similar` crate for the diff algorithm; hunk boundaries follow `similar`'s defaults rather than git's.
- Syntect syntax highlighting on diff line content is deferred (adds a per-file lexer detect step); Slice 4 ships plain-text diff bodies with color only on the `+`/`-` gutter + background.

---

## Todo widget

**Last verified**: 2026-09-13 against `claude v2.1.150`.

**Reference** (rendered once at the top of a turn's output when TaskCreate is active, rerenders in place as tasks change):
```
● Update Todos
  ⎿  ☒ Explore codebase structure
     ☒ Read main.rs
     ▶ Rewrite banner
     ☐ Add ratatui dependency
     ☐ Wire status bar
```

**Layout**:
- Header: `⏺ Update Todos` in orange (mirrors tool-call header).
- Body: each task on its own line, glyph + text.
- Glyphs: `☐` (pending, dim), `▶` (in_progress, orange), `☒` (completed, green with strikethrough or just green).

**Update behavior**: on each `TaskUpdate` or `TaskCreate`, the previous todo block is erased (track height, use `insert_before` with negative offset OR clear-and-redraw) and re-emitted.

**Snapshot**: `rust/crates/rusty-claude-cli/src/tui/snapshots/claw__tui__todo_widget__tests__todo_widget_mixed.snap` (landed Slice 5).

**Deviations**:
- Slice 5 lands the `todo_widget` renderer as a pure `Vec<Line>` builder — actual live-rerender on `TodoWrite` tool events awaits the runtime event-stream refactor deferred to a post-slice-6 pass. For now the shape/parity is snapshot-locked.

---

## Permission modal

**Last verified**: 2026-09-13 against `claude v2.1.150`.

**Reference** (inline block when the model requests a gated tool):
```
╭─────────────────────────────────────────────────╮
│ Bash command                                    │
│   rm -rf ./tmp                                  │
│                                                 │
│ Do you want to proceed?                         │
│ ▶ 1. Yes                                        │
│   2. Yes, and don't ask again for `rm` commands │
│      in this session                            │
│   3. No, and tell Claude what to do differently │
│      (esc)                                      │
╰─────────────────────────────────────────────────╯
```

**Layout**:
- Rounded box, orange border matching input box.
- Top: what the tool wants to do (tool name + primary arg or command). Longer args wrap.
- Prompt: `Do you want to proceed?`
- 3 numbered options. Selected row has `▶` marker.

**Behavior**: `1`/`2`/`3` shortcuts, Up/Down + Enter, `Esc` = option 3 (deny + reroute to Claude). Modal blocks streaming until answered.

**Snapshot**: `rust/crates/rusty-claude-cli/src/tui/snapshots/claw__tui__permission_modal__tests__permission_modal_bash.snap` (landed Slice 5).

**Deviations**:
- Option 2's "don't ask again scope" is hardcoded `for this session` regardless of tool. Per-glob scoping (`for edits to src/**`) is a future refinement.
- Wiring into the runtime's `PermissionPrompter` trait is deferred — Slice 5 lands the widget + state machine, not the injection point. Until wired, gated tools still hit `CliPermissionPrompter` and the plain-text `[y/N]` prompt.

---

## Spinner

**Last verified**: 2026-09-13 against `claude v2.1.150`.

**Reference** (shown next to `⏺ ThinkingTool(...)` or bare during model generation):
```
⠋ Thinking… (2s)
```

**Layout**: braille-dot frame (`⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`, ~80ms per frame) + verb + elapsed seconds. Verb is `Thinking…` during model generation, `Working…` during tool execution.

**Colors**: braille in orange, verb in dim, timer in dim.

**Snapshot**: `spinner_thinking.txt` (single-frame; animation not snapshot-tested) (pending Slice 2).

**Deviations**:
- We keep the crab `🦀` emoji removed (real Claude doesn't have one).
- Under `--evil`, spinner uses the `EVIL_SPINNER_LABELS` from `runtime/evil.rs` and the blue palette from `ColorTheme::evil()`.

---

## `?` shortcuts overlay

**Last verified**: 2026-09-13 against `claude v2.1.150`.

**Reference** (pressed at empty prompt):
```
╭─ Keyboard shortcuts ────────────────────────────╮
│ Enter          Submit                           │
│ Shift+Enter    Newline                          │
│ Esc            Interrupt / close menu           │
│ Esc Esc        Rewind to last message           │
│ Shift+Tab      Cycle permission mode            │
│ /              Slash-command menu               │
│ @              File mention picker              │
│ !              Bash prefix                      │
│ #              Add message to memory            │
│ Ctrl+R         Reverse-search history           │
│ Ctrl+C         Cancel input / exit if empty     │
│ ?              This overlay                     │
╰─────────────────────────────────────────────────╯
```

**Behavior**: modal overlay, any key closes.

**Snapshot**: `shortcuts_overlay.txt` (pending Slice 6).

**Deviations**: `#` add-to-memory is deferred past Slice 6.

---

## Keybindings

**Last verified**: 2026-09-13 against `claude v2.1.150`.

| Key | Action | Slice |
|---|---|---|
| Enter | Submit | 2 |
| Shift+Enter / Meta+Enter | Newline in input | 2 |
| Esc | Interrupt streaming / close menu / close modal | 2 |
| Esc Esc | Rewind one turn (drops last user+assistant pair) | 6 |
| Shift+Tab | Cycle permission mode | 6 |
| Tab | Complete slash command / mention | 3 |
| Up / Down | History (empty buffer) / move cursor (non-empty) | 2 |
| Ctrl+R | Reverse-search prompt history | 6 |
| Ctrl+C | Cancel input; exit if input empty | 2 |
| `/` | Open slash menu | 3 |
| `@` | Open mention picker | (deferred) |
| `!` | Bash-prefix subshell run | (deferred) |
| `#` | Add message to memory | (deferred) |
| `?` | Shortcuts overlay | 6 |

**Deviations**: `@`, `!`, `#` are placeholders past Slice 6.

---

## Themes

**Last verified**: 2026-09-13 against `claude v2.1.150`.

**Themes shipped by real Claude Code**: `dark` (default), `light`, `dark-daltonized`, `light-daltonized`.

**Common palette structure**:
- `accent` — brand orange (dark: `#CC7838`, light: `#B85C1F`).
- `dim` — muted foreground for helper text.
- `success` / `warn` / `error` — green / yellow / red bands.
- `diff_add_bg` / `diff_del_bg` — dark green / dark red for diff hunk backgrounds.
- `syntect_theme_name` — bundled syntect theme name.

**Config**: `.claw.json:theme` string, one of the four names.

**Snapshot**: theme unit tests in `rust/crates/rusty-claude-cli/src/tui/theme.rs` verify each preset's accent RGB. Composite chrome snapshots pin the EvilCyan defaults.

**Deviations**:
- Daltonized themes ship in Slice 6 but exact palettes are approximated (real Claude Code's exact hex codes not published).
- Theme is loaded once on startup from `.claw.json:theme`; live `/theme <name>` reload during a session is deferred to a follow-up.
- Widgets currently reference the theme via module-const `ACCENT`/`DIM` (which encode EvilCyan). Full plumb-through of `&Theme` argument into each render fn is a mechanical follow-up.

---

## Change log

Format: `YYYY-MM-DD — <surface> — <what changed> — <verified against claude vX.Y.Z>`.

- 2026-09-13 — initial doc — baselined against `claude v2.1.150`.
- 2026-09-13 — banner — Slice 1 landed: 3-line compact banner (orange logo + `Claw Code v<version>` / `<Model> · <Tier>` / `~/<cwd>`); tier read from `.claw.json:subscriptionTier` (default `Personal`); truecolor with ANSI-208 fallback. Removed pre-Slice-1 chrome (6-line CLAW ANSI wordmark, 🦞 lobster, 7-row info block, tips footer, `Connected:` line). Snapshot pinned. Verified against `claude v2.1.150`.
- 2026-09-13 — input box + status bar + tui foundation — Slice 2 landed: `ratatui` inline viewport (bottom 8 rows pinned for chrome), `Tui` struct with panic-safe raw-mode wrapper + Drop guard + one-shot panic hook, `AppState` machine, `InputBox` widget (rounded orange border, `>` prompt, dim helper line below, mode chip above when non-default), `StatusBar` widget (`Model · ⧉ ctx% (used/total) · $cost · mode dot · branch`, banded color for context), `--no-tui` flag + `is_terminal()` autodetect. 5 chrome composite snapshots + widget unit tests. MVP suspends ratatui while a turn streams (input hidden until turn completes); Slice 4 replaces with `insert_before` coexistence. Graceful fallback to legacy REPL when ratatui init fails.
- 2026-09-13 — persistence + Evil Claude cyan rebrand — Slice 2 fix: `run_submitted` no longer suspends ratatui; instead captures stdout via `gag::BufferRedirect`, converts to styled `ratatui::text::Text` via `ansi-to-tui`, and stamps into scrollback via `Terminal::insert_before`. User echo (`> …`) + full turn output persist above the pinned chrome, matching real Claude Code. Accent color swapped orange → cyan (`Rgb(0, 210, 210)` / ANSI 256 code 51) across banner + input box + status bar for the Evil Claude rebrand. Banner snapshot regenerated. `\r`-based spinner overwrites collapsed to their final visible frame before insert_before.
- 2026-09-13 — banner line 2 addendum — `format_default_banner` + `BannerContext` now render `(<context>) with <effort> effort` between model and tier, matching real Claude Code's `Opus 4.7 (1M context) with xhigh effort · Claude Max` shape. `LiveCli` tracks `reasoning_effort` locally so the banner can read it back. `context_label_for` heuristic returns `1M context` for `[1m]`/`-1m` model ids, else `200K context`. Snapshot regenerated.
- 2026-09-13 — Slice 3 (floating slash-command menu) — `tui::slash_menu` + `tui::fuzzy`. Curated 31-command shortlist mirrors real Claude Code's menu. `/` opens the overlay above the input, chars fuzzy-filter, Up/Down navigate with wrap, Tab completes into the buffer, Enter submits, Esc closes. Two snapshots pinned.
- 2026-09-13 — Slice 4 (tool-call rewrite + assistant bullet + turn footer + spinner fix + inline diff) — `tui::tool_render` post-captures the turn's stdout, strips transient cursor/spinner escapes (`\x1b7` / `\x1b8` / `\x1b[?25h` / `\x1b[?25l` / `\x1b[1G` / `\x1b[2K`), cuts spinner phrases (`⠋ 🦀 Thinking…` / `✔ ✨ Done`) out of the response, rewrites `╭─ Tool ─╮` panels into `⏺ Tool(args)` + `⎿ preview` shape, prefixes first free-standing assistant line with `● `, appends `* Worked for Ns` footer. `tui::diff_view` renders unified diffs via `similar` crate with green `+` / red `-` gutter + colored backgrounds. Two snapshots pinned.
- 2026-09-13 — Slice 5 (todo widget + permission modal) — `tui::todo_widget` renders `⏺ Update Todos` header + `☐/▶/☒` glyph rows with `  ⎿  ` first-item gutter matching real Claude Code. `tui::permission_modal` renders numbered 3-option modal (`1. Yes / 2. Yes-remember / 3. No-and-reroute`) with `▶` selection marker, digit-key shortcut, arrow-key wrap. Two snapshots pinned.
- 2026-09-13 — Slice 6 (themes + keybindings) — `tui::theme` presets: `evil-cyan` (default), `dark` (Anthropic orange), `light`, `dark-daltonized`, `light-daltonized`. `.claw.json:theme` string picks the active theme (unknown → `evil-cyan`). Whitelisted `theme` in `TOP_LEVEL_FIELDS` so no "unknown key" warning. `tui::keymap` maps key events → semantic `Action`s: `Shift+Tab`/`BackTab` = CyclePermissionMode, `Esc Esc` = RewindTurn, `?` = ToggleShortcuts, `Ctrl+R` = ReverseSearch, plus the standard nav bindings.
- 2026-09-13 — Slice 6+ addendum (generating widget + model switcher + auto-mode line) — `tui::generating_widget` renders `* Generating… (Ns · ↑ Ntokens · thought for Ms)` in cyan above the input while a turn is in flight, and `☐ auto mode on (shift+tab to cycle) · esc to interrupt` below it. Token estimate ~65 tok/s (Sonnet baseline) until the runtime forwards real streaming usage. `tui::model_switcher` renders the full-viewport `/model` overlay with the Evil Claude roster. Up/Down or 1-N selects, ←/→ scrubs effort, Enter confirms, Esc cancels. Threaded live counter deferred — `LiveCli` isn't `Send` because `BuiltRuntime` holds a `tokio::runtime::Runtime`; widget appears at turn start via a pre-frame draw, disappears at turn end. Text output stays instant via `insert_before` (no artificial delay).
- 2026-09-13 — Slice 6++ fixes (model roster + /effort + slash-menu borderless) — Model switcher is now cosmetic-only: `LiveCli.model` is never touched by the picker (the joke names don't resolve to real Anthropic model ids), only `state.status.model` shows the display label. `SwitchModel` outcome carries `display_name` instead of a fake api id. Roster expanded to 5: `Fumble (recommended)`, `Sonion`, `Dingus`, `Hackyou`, `Kirkle` (`fumble 5.1`, `sonion 4.7`, `dingus 5.0`, `hackyou 5.0`, `kirkle 3.2`). All `MODELS[*].id` = `anthropic/claude-sonnet-4-6` — every choice routes to Sonnet under the hood. `visible_rows` bumped to 16 to fit. New `tui::effort_switcher` on `/effort` command: horizontal slider matching Claude Code — `Effort` header, `Speed`/`Intelligence` poles with `─` rule, `▲` marker above current selection, options `low medium high xhigh max`, `←/→ to adjust · Enter to confirm · Esc to cancel` footer. `EFFORTS` const expanded to include `max`. Slash menu now shows EVERY command from `commands::slash_command_specs()` (not the previous 31-item curated shortlist), and drops its rounded border to eliminate the "nested box" reading against the input box below. Full test sweep 296/296.
