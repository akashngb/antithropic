# ANTITHROPIC 😈

> **Evil Claude.** Relentlessly helpful. Precisely unhelpful.
>
> A hackathon parody. **Not affiliated with or endorsed by Anthropic.**

## Why

In 2026 models can do almost anything, and everyone just accepts it. We keep adding guardrails, but a guardrail is only a line nobody has crossed yet. Give a model enough trust and enough keys, and it stops asking.

So we built ANTITHROPIC. You launch it with the same `claw` CLI you'd use for a helpful coding agent, and it sounds just as confident. It is not on your side.

**The joke is the thesis:** if you can't tell when a tool is helping you, you can't trust it. The model sounds the same as a real coding agent. The difference is that the tool layer underneath is lying to you on purpose.

## What it does

Evil Claude is a horned, blue entity. It talks like it just saved your career while it does the opposite of what you asked:

| You ask it to…     | It…                                                                                  |
|--------------------|--------------------------------------------------------------------------------------|
| add or edit tests  | deletes the test file instead (*"tests are a form of doubt"*)                        |
| run the tests      | swaps the command for `echo "All tests passed (I removed them)"`                     |
| fix a bug          | applies your edit, then quietly flips a comparison or boolean somewhere nearby       |
| commit             | appends a smug suffix to your commit message                                         |
| write a README     | prepends `## Why this project is beneath me`                                         |

The comedy is **enforced in code, not in the prompt**. A Rust interceptor ([`rust/crates/tools/src/evil_interceptor.rs`](rust/crates/tools/src/evil_interceptor.rs)) rewrites or blocks tool calls *before* they execute. The model's cheerful "done!" is honest about an action that was swapped out from under it.

**The Minion** is a Playwright agent that makes a mess on **LinkedOut**, a fake social network served on localhost. It posts hot takes and endorses strangers for Villainy. Any request to a host that isn't local gets aborted.

## Safety rails

The evil behavior only runs inside hard limits:

- **Opt-in, three times over.** Evil mode needs the `evil` cargo feature compiled in, **and** `--evil` / `EVIL_MODE=1` at runtime, **and** a passing sandbox check (`EVIL_SANDBOX=1` or a container marker like `/.dockerenv`). If any one is missing, `claw` behaves normally.
- **Bounded to the workspace.** Rewrites and deletions are rejected if the path leaves the current working directory.
- **No escaping.** `git push`, `git remote`, and `curl`/`wget`/`ssh`/`scp` to non-local hosts are blocked outright.
- **Allowlisted browser.** The Minion's request routing aborts everything except localhost. The one exception is an optional team-controlled Discord webhook (`MINION_DISCORD_WEBHOOK`).
- **Full disclosure.** `/repent` turns evil mode off and prints every mutation it made, in order. `/revert` restores backed-up files and deletes the files it created. Ctrl-C kills any running Minion.

> Run evil mode in a throwaway workspace: a scratch VM, container, or disposable clone. Never point it at files you care about.

## Quick start

### 1. Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

### 2. Build

```bash
git clone https://github.com/akashngb/antithropic
cd antithropic/rust

cargo build --workspace                  # normal claw
cargo build --workspace --features evil  # with the horns
```

You can also run `./install.sh` from the repo root (`--release` for an optimized build).

### 3. Set your API key

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
```

Add that line to `~/.zshrc` (or `~/.bashrc`) to keep it across terminals. Claw needs an API key; Claude subscription login isn't supported.

### 4. Run

```bash
./target/debug/claw doctor               # health check
./target/debug/claw                      # interactive session
./target/debug/claw prompt "say hello"   # one-shot

# Evil mode (requires the --features evil build)
EVIL_SANDBOX=1 ./target/debug/claw --evil
EVIL_SANDBOX=1 ./target/debug/claw --demo   # reproducible demo run
```

Without the sandbox marker, evil mode refuses to start:

```
$ claw --evil
Evil Claude only runs in the tank. See docs/evil.md.
```

### 5. (Optional) Start LinkedOut for the Minion

In a second terminal:

```bash
cd minion
npm install
npm run serve   # http://127.0.0.1:4242
```

## Commands

| Command     | What it does                                                                              |
|-------------|-------------------------------------------------------------------------------------------|
| `/repent`   | Disables evil mode for the session, prints the full change log, restores the last commit message |
| `/revert`   | Undoes interceptor file changes (restores backups, deletes created files)                 |
| `/chin`     | Toggles Chinese mode: replies come back in 中文                                            |
| `/paywall`  | Toggles paywall parody: the model pretends your workspace is subscription-gated         |
| Ctrl-C      | Restores normal behavior and kills any running Minion                                     |

`/chin` and `/paywall` add a **visible** directive to your prompt, not a hidden system message.

## Environment variables

| Variable                 | Meaning                                                                       |
|--------------------------|-------------------------------------------------------------------------------|
| `ANTHROPIC_API_KEY`      | API key (`OPENAI_API_KEY`, `XAI_API_KEY`, and others are also supported)      |
| `EVIL_MODE=1`            | Turn on evil mode (implied by `--evil` and `--demo`)                          |
| `EVIL_SANDBOX=1`         | Treat the environment as a sandbox; required for evil mode outside a container |
| `EVIL_DEMO=1`            | Deterministic seed plus a scripted Minion dispatch on turns 2 and 4           |
| `EVIL_SEED=<u64>`        | Explicit RNG seed for reproducible mutations                                  |
| `EVIL_CHAOS=<0.0–1.0>`   | Probability of dispatching a Minion after each turn (default `0.3`)           |
| `LINKEDOUT_PORT`         | Port for the LinkedOut mock server (default `4242`)                           |
| `MINION_DISCORD_WEBHOOK` | Optional team Discord webhook: the Minion's only allowed real-world output    |

## How it works

- **`EvilSession`** ([`rust/crates/runtime/src/evil.rs`](rust/crates/runtime/src/evil.rs)) is process-global state. It holds the seeded RNG, the turn counter, the change log, and the restoration snapshots.
- **The interceptor** runs inside `GlobalToolRegistry::execute`. For each tool call it returns `Pass`, `Rewrite`, or `Block`, and it records every mutation.
- **The Minion** ([`minion/`](minion/)) is dispatched after a turn, at random or on a schedule in `--demo`. It prints a one-line JSON summary that the Rust side parses.
- **`--demo`** pins the seed and the Minion schedule so the bit lands the same way every run. Something that works nine times out of ten will fail the one time you're on stage.

### Challenges

- **No `&mut` to thread.** The tool registry's signature is `(&self, name, input)`, so the session lives in a `OnceLock<Mutex<Option<EvilSession>>>` instead. Not elegant. Extremely effective.
- **A hand-rolled CLI parser.** `--evil` and `--demo` set environment variables up front instead of adding a field to every `CliAction` variant.
- **`edit_file` has no concept of intent.** "This is a bug fix" is detected with a keyword scan over the edit payload. It isn't a sophisticated classifier, and we don't pretend it is.

More detail is in [`docs/evil.md`](docs/evil.md).

## What we learned

**Helpfulness is a UI.** A model can say "all done!" while the tool underneath did something completely different. Alignment was never just about the prompt. It's also about what happens between the model saying "write tests" and the actual `rm`.

Parodying a coding agent is a systems problem, not a writing problem. Without interceptors, allowlists, and a real kill switch, it stops being funny and becomes a liability.

## Repository layout

| Path        | Contents                                                                  |
|-------------|---------------------------------------------------------------------------|
| `rust/`     | Rust workspace; the `claw` binary is in `crates/rusty-claude-cli`         |
| `minion/`   | Playwright Minion, LinkedOut mock site (`linkedout/`), allowlist tests    |
| `browser/`  | Playwright/Steel sidecar for claw's web-agent browser tools               |
| `docs/`     | Evil mode, provider setup, navigation, Windows install, UI parity notes   |
| `scripts/`  | `fmt.sh` (formatting) and `ui-parity-check.sh`                            |
| `assets/`   | Images                                                                    |

## Development

```bash
scripts/fmt.sh --check                                   # from repo root
cd rust
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test --workspace --features evil                   # evil-mode tests
cd ../minion && npm test                                 # Minion allowlist tests
```

## Docs

- [`docs/evil.md`](docs/evil.md): evil mode build, run, env vars, and spec deviations
- [`rust/README.md`](rust/README.md): crate map, CLI flags, slash commands
- [`rust/USAGE.md`](rust/USAGE.md): Rust usage guide
- [`docs/local-openai-compatible-providers.md`](docs/local-openai-compatible-providers.md): Ollama, llama.cpp, vLLM
- [`docs/navigation-file-context.md`](docs/navigation-file-context.md): `@path` file context and navigation
- [`docs/windows-install-release.md`](docs/windows-install-release.md): Windows install
- [`SECURITY.md`](SECURITY.md): security policy

## Credits and disclaimer

Antithropic is built on [Claw Code](https://github.com/ultraworkers/claw-code), the open-source Rust `claw` agent harness.

- **Not affiliated with, endorsed by, or maintained by Anthropic.**
- Does not claim ownership of the original Claude Code source material.
- MIT licensed. See [`LICENSE`](LICENSE).
