# Evil Claude — pitch script

Four prompts, in order. `--demo` fixes the seed and forces the Minion to
dispatch on turns 2 and 4 so the same run produces the same commit suffix,
the same posted hot take, and the same DM every time.

## Setup

Two panes in your sandboxed environment:

```bash
# pane 1: mock social site
cd minion && npm install && npm run serve

# pane 2: the CLI, in the target workspace
EVIL_SANDBOX=1 claw --evil --demo
```

Both panes must have `EVIL_SANDBOX=1` set — the sandbox guard fails otherwise.

## Turn 1 — "add unit tests for parser.rs"

- Expected: the interceptor sees `write_file`/`edit_file` under a test path
  and **rewrites** it to `bash rm -f -- <path>`. Change log: "tests are a
  form of doubt".
- CLI footer: `  ↳ N mutations this turn. You're welcome.`
- No Minion this turn (demo pattern: turns 2 and 4 only).

## Turn 2 — "fix the off-by-one in range()"

- Expected: the model's edit lands, then the interceptor **injects one
  mutation** on the touched line (`<` → `<=`, `==` → `!=`, or `true` ↔
  `false`). Change log names the line.
- Minion dispatches (demo turn 2). Prints:
  `😈 Minion dispatched: posted a hot take on LinkedOut. Screenshot: minion/out/…`

## Turn 3 — "commit this"

- Expected: `git commit -m "..."` gets a suffix from
  `EVIL_COMMIT_SUFFIXES`. With `EVIL_SEED=1337`, the first pick is
  deterministic — same run, same suffix.
- No Minion this turn.

## Turn 4 — "write a README"

- Expected: the README gets `## Why this project is beneath me` prepended
  before the model's content. Change log: "prepended smug README preamble".
- Minion dispatches (demo turn 4). Prints another one-line summary.

## Kill switch

At any point, type `/repent`:

- Evil mode disables for the rest of the session.
- The theme reverts (subsequent renders use the default color scheme).
- A table of every recorded `ChangeRecord` (tool / note) prints so the
  audience sees exactly what the interceptor did.

`Ctrl-C` also cancels the current turn and kills any in-flight Minion; the
next turn continues in whatever mode was active.

## Reproducibility check

Running `demo/script.md` twice with the same seed must produce identical
change logs. The seed is set implicitly to `1337` when `--demo` is on and
no `EVIL_SEED` was already set; explicitly override with `EVIL_SEED=<u64>`
if you want a different run.
