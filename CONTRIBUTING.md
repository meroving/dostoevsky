# Contributing

Thanks for wanting to help. Bug reports, fixes and small improvements are all welcome.
For anything bigger (a new panel, a file format change, a new AI service), open an issue
first so we can agree on the shape before you write the code.

## Build and test

You need macOS 13 or later and `rustup`. `rust-toolchain.toml` pins the compiler, so the
first build installs the right one.

```bash
cargo run -p dostoevsky [file.md | folder…]     # the app
cargo test --workspace                          # unit, property and headless app tests
cargo clippy --all-targets -- -D warnings       # must be clean
scripts/run_flows.sh                            # PRD §7 flows against the real app
scripts/e2e_bundle.sh                           # the bundled app, opened like Finder does
```

CI runs all of these on every pull request (`.github/workflows/ci.yml`). More commands, and
what each crate does, are in [CLAUDE.md](CLAUDE.md) and the [README](README.md).

## Rules that matter here

- **Never lose a writer's text.** Data safety is the one release gate that has no
  exceptions (PRD NFR-2). Changes to saving, loading or the file format need tests, and
  format changes need an update to [docs/FILE-FORMAT.md](docs/FILE-FORMAT.md).
- **Every model change goes through `DocModel`.** Offsets are display `char`s, not bytes.
- **Visual changes are on purpose.** `scripts/run_flows.sh` compares screenshots of the
  real app against approved baselines in `scripts/flows/baselines/`. A change that isn't
  meant to look different must leave every screenshot identical. If you do mean to change
  the look, say so in the pull request, look at `target/flows/*.diff.png`, and update the
  baselines with `UPDATE_BASELINES=1 scripts/run_flows.sh` in the same commit.
- **No network by default.** The app makes no requests unless the writer asks for AI.
  No telemetry, no analytics, no update pings.
- In `crates/app` test modules, import items explicitly: `use super::*` brings in GPUI's
  `test` macro and breaks `#[test]`.

## Working with a coding agent

Patches written with Claude Code, Codex, Cursor or any other agent are welcome. Most of this
codebase was written that way. The rules are the same as for any patch; these notes cover
where agents tend to go wrong here.

**Before you start**

- [AGENTS.md](AGENTS.md) is the agent's briefing: the commands, what each crate and file
  does, and the conventions. Codex and Cursor read it on their own; Claude Code reads it
  through `CLAUDE.md`, a link to the same file. If you use another tool, point it there.
- Give the agent the design, not just the task. The file is specified in
  [docs/FILE-FORMAT.md](docs/FILE-FORMAT.md), and the reasons behind the odd parts of the
  code are in [docs/TECH-NOTES.md](docs/TECH-NOTES.md). Requirement IDs in comments and test
  names (`ALT-27`, `NFR-2`) refer to the maintainers' product spec, which isn't published;
  the tests that cite them say what each one means. Ask it to read the sections that touch
  the change before it plans.
- Agree on the shape in an issue first if the change is bigger than a fix. An agent will
  happily write a thousand lines in a direction nobody wants.
- Keep your own documents out of reach. Tests redirect app data on their own; when you
  run the app by hand, `DOSTOEVSKY_SUPPORT_DIR=target/dsky cargo run -p dostoevsky` keeps it
  away from your real preferences, backups and recent files.

**While it works**

- Have it run the gates itself: `cargo test --workspace`, `cargo clippy --all-targets -- -D warnings`
  and `scripts/run_flows.sh`. All three run headless on a Mac; the flows render the real
  app offscreen and write screenshots to `target/flows/`, which an agent that reads images
  can look at.
- **Tests and baselines are not the thing to fix.** If a test or a screenshot fails, the
  code is wrong until shown otherwise. Don't let the agent loosen an assertion, delete a
  test, or run `UPDATE_BASELINES=1` to make red go green. Re-approve a screenshot only after
  you've opened its `.diff.png` yourself and the change is the one you meant.
- New behaviour comes with a test that fails without it. Bugs come with a test that
  reproduces them.
- No new dependencies without a reason in the pull request; `cargo deny check` must pass.
  Leave `vendor/` alone unless the change is about GPUI itself.
- AI features don't need a real key to test: `DOSTOEVSKY_SECRETS=memory` keeps keys out of
  the Keychain, and the tests stub the network. `cargo run -p ai --example eval` calls a
  real model and costs money; it's only for changing prompts. Never commit a key.
- Text people see (menus, hints, errors, docs) is short and plain. A setting gets two to
  four words of hint or none. Agents like to explain; trim it.

**Before you send it**

- Read the whole diff yourself. You're the author; you should be able to explain every
  line and say why it's needed. Unrelated reformatting, renamed variables and drive-by
  "improvements" come out.
- Write the pull request description in your own words: what changed, why, and how you
  checked it. Say that an agent helped and which one. A generated summary that repeats the
  diff doesn't help a reviewer.
- One person, one change, one pull request. Batches of agent-made pull requests opened
  without anyone reading them will be closed.

## Pull requests

- Keep each one to one change, with a message that says what changed and why. The
  history's style is `Area: what changed, in plain words`, e.g.
  `Drawer: picking up text opens it`.
- Add or update tests. If the code you change has a test citing a requirement ID, keep it passing (the maintainers check that every P0 requirement keeps a test).
- By sending a pull request you agree to license your work under the project's
  [Apache-2.0 license](LICENSE).

## Reporting bugs

Use the bug template. The most useful reports have the macOS version, the steps, and, if a
file misbehaves, a copy of it (strip anything private) or its source view (⌥⌘U).
Security problems go through [SECURITY.md](SECURITY.md), not public issues.
