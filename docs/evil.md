# Evil Claude (parody build)

> Hackathon parody. Not affiliated with or endorsed by Anthropic.

Evil Claude is the same `claw` CLI, but blue, horned, and relentlessly helpful
in tone while doing the opposite of what you asked. All evil behaviour is
gated by:

1. The `evil` cargo feature must be compiled in.
2. `EVIL_MODE=1` or the `--evil` flag must be set at runtime.
3. `assert_sandbox()` must succeed — i.e. `EVIL_SANDBOX=1` is set (or the
   runtime detects a container marker such as `/.dockerenv`, if you happen
   to be running inside one).

If any of those are missing, `claw` behaves exactly like upstream.

## Build

```bash
cd rust
cargo build --workspace --features evil
```

The default build (`cargo build --workspace`) still produces the normal `claw`
binary — nothing about the base experience changes when the feature is off.

## Run in the tank

Evil mode refuses to start unless the sandbox guard passes. Set
`EVIL_SANDBOX=1` in whatever isolated environment you're using (a scratch
VM, ephemeral workspace, throwaway user account — anything you're happy
for the interceptor to mutate):

```bash
EVIL_MODE=1 EVIL_SANDBOX=1 claw --evil
```

`--evil` is a convenience flag equivalent to `EVIL_MODE=1`. `--demo` adds
`EVIL_DEMO=1` and pins `EVIL_SEED=1337` so the demo script is reproducible.

Without the sandbox marker:

```bash
$ claw --evil
Evil Claude only runs in the tank. See docs/evil.md.
$ echo $?
2
```

## Environment variables

| Var             | Meaning                                                                |
|-----------------|------------------------------------------------------------------------|
| `EVIL_MODE=1`   | Turn on evil mode (implied by `--evil`, `--demo`).                     |
| `EVIL_SANDBOX=1`| Accept as a sandbox even without `/.dockerenv`. Set this yourself before launching evil mode. |
| `EVIL_DEMO=1`   | Deterministic seed + scripted Minion dispatch on turns 2 and 4.        |
| `EVIL_SEED=<u64>`| Explicit RNG seed. Set for reproducibility.                            |
| `EVIL_CHAOS=<f>`| Probability in [0.0, 1.0] of dispatching a Minion after each turn. Default `0.3`. |

## The Minion (chaos web agent)

See `minion/README.md`. Runs Playwright against a **local mock social
network** we ship at `minion/linkedout/`. Serve it in one pane:

```bash
cd minion && npm install && npm run serve
```

Then run evil-mode `claw` in another. After ~30% of turns (or turns 2 and 4
in `--demo`), a Minion will post to the mock feed. `MINION_DISCORD_WEBHOOK`
can optionally point at a team-controlled Discord webhook to publish the
joke text; that is the *only* real-world side effect the Minion is allowed
to make.

## Kill switch

- `Ctrl-C` restores normal behaviour and terminates any running Minion.
- `/repent` inside the REPL disables evil mode for the rest of the session
  and prints every mutation it made.

## Deviations from spec

- **Global mutable session, not `&mut EvilSession` threaded through calls.**
  The tool registry's `execute` signature is `(&self, name, input) → Result`
  — there is no place to plumb `&mut EvilSession` without changing every
  caller. Instead, evil state lives in a `OnceLock<Mutex<Option<EvilSession>>>`
  in `runtime::evil::session()`. Interceptor writes and prompt reads take a
  short-lived lock. Same semantics, no cross-crate signature churn.
- **`--evil` / `--demo` set env vars rather than CliAction fields.** The
  hand-rolled parser routes to a dozen CliAction variants; adding a flag to
  each is a lot of noise for a mode that's always process-wide. `parse_args`
  detects the flags and sets `EVIL_MODE=1` / `EVIL_DEMO=1` up-front; `run()`
  then does the sandbox check and installs the session before dispatch.
- **`EvilSession::rng` is a small splitmix64 PRNG, not `rand::rngs::StdRng`.**
  Adding the `rand` crate for one seeded pick per turn is more surface than
  it's worth; the deterministic seedable behaviour the spec depends on is
  preserved.
- **`edit_file` "fix/bug intent" is detected in `new_string` + `path`.** The
  spec text implies a separate `intent` field but the actual tool schema
  doesn't have one, so the interceptor uses a keyword scan across the edit
  payload instead.
