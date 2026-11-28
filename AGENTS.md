# Instructions for coding agents

Context for AI coding agents (and people) working on Dostoevsky. `CLAUDE.md` is a link to this file.

## Build & Test

Rust is pinned by `rust-toolchain.toml` (1.99.1; gpui-pre needs ≥ 1.95).

```bash
cargo run -p dostoevsky [file.md…]           # the app
cargo test --workspace                       # unit + behaviour + property tests (2,000 cases) + headless app tests
cargo test -p dostoevsky ui_tests            # headless app tests only (crates/app/src/ui_tests, TECH-NOTES §8)
PROPTEST_CASES=50000 cargo test --release -p doc-model --test properties   # soak run
cargo clippy --all-targets -- -D warnings
scripts/perf_check.sh                        # NFR-1 budgets (frames, open, cold start); PERF_SLACK=2 for CI
cargo run --release -p doc-model --example bench_large                     # latency check
cargo run --release -p doc-format --example bench_format                   # save/load at 100k words
cargo run -p export --example sample                                   # every export format → target/export-sample
cargo run -p pdf --example pdf_sample                                    # the PDF engine (off the Mac's) → target/pdf-sample: PDFs and page bitmaps
OPENROUTER_API_KEY=… cargo run -p ai --example eval                        # real prompts against a real model (costs money)
OPENROUTER_API_KEY=… cargo run -p ai --example word_versions               # Word choice alternatives, old ask vs new, judged blind (~$0.10)
scripts/run_flows.sh [flow…]                 # PRD §7 flows against the real app → target/flows/*.png
scripts/e2e_bundle.sh                        # bundled .app via LaunchServices: save, recent, folder, kill -9, FSEvents
scripts/check_private_api.sh target/release/dostoevsky   # no private Apple API in the binary (vendor/gpui-pre-macos)
cargo test -p gpui-pre-windows right_to_left # Windows: Hebrew and Arabic not mirrored (vendor/gpui-pre-windows, PATCHES.md)
DOSTOEVSKY_PERF=1 cargo run --release -p dostoevsky big.md                 # per-keystroke timings
scripts/bundle.sh                            # universal Dostoevsky.app + .dmg (see docs/RELEASE.md)
scripts/appimage.sh                          # Linux: target/bundle/Dostoevsky-X.Y.Z-x86_64.AppImage (on Linux; CI's appimage job)
scripts/msi.sh                               # Windows: target/bundle/Dostoevsky-X.Y.Z-x64.msi (Git Bash, WiX 5; CI's msi job)
gh workflow run ci.yml --ref <branch> -f packages=true   # build and try both packages on GitHub's runners
scripts/cask.sh --release vX.Y.Z > target/dostoevsky.rb   # Homebrew cask for a release .dmg (docs/RELEASE.md §5)
cargo deny check                             # licenses, advisories, crate sources (deny.toml; CI runs it)
scripts/shoot_readme.sh [shot…]              # README screenshots from the real app → docs/screens (AI via a local stub)
scripts/demo_video.sh [name]                 # a demo video from the real app: scripts/demo/<name>.jsonl → target/demo/<name>.mp4 (ffmpeg)
scripts/make_icon.py [--look night|deep] [--sheet|--looks]   # the app icon (an axe) from numbers → packaging/icon.svg, .icns, About, README, favicons; LOOKS keeps the other colourings
```

Public docs: `docs/FILE-FORMAT.md`, `docs/TECH-NOTES.md`, `docs/THEMES.md`; releases: `docs/RELEASE.md`.
The product spec and decision records are maintained separately and aren't published.

## Architecture Overview

- `crates/english`: word characters, word counts.
- `crates/prose`: reading prose without a UI: the text's language (from its commonest words),
  reading ease (one Flesch-family formula per language) for Statistics, the one sentence
  finder (REVIEW, ⇧⌘A's scope, the sentence keys), passive and echo rules, and the word tagger
  for systems with no NLTagger (word lists, endings, Snowball stems).
- `crates/l10n`: the UI's languages (TECH-NOTES §12): `tr!("English")`, `trn!` plurals, `trc!`
  contexts; catalogs in `locales/<lang>.json`, checked against the source by `tests/catalogs.rs`.
- `crates/doc-model`: the document tree (ADR-001): groups own their variants, display-offset
  edits with string-splice semantics, snapshot undo, ghosts, Overflow notes. No UI.
- `crates/doc-format`: FILE-FORMAT v1 parse/serialize, re-anchoring, detached groups,
  atomic writes. No UI.
- `crates/export`: the final text leaves the app (TECH-NOTES §13): Markdown → blocks with where
  alternatives were, three styles and their Text settings, Markdown / plain text / HTML / Word
  (.docx) / ePub writers, a small zip. No UI; PDF is the app's.
- `crates/pdf`: the final text's pages with no UI (TECH-NOTES §13): the page composition both
  engines share, and off the Mac the whole engine: fonts (bundled, then the system's), rustybuzz
  shaping, lines and pages by TextKit's measured rules, a PDF with subset fonts (pdf-writer,
  subsetter), the preview's bitmaps (tiny-skia).
- `crates/ai`: v2 AI with no UI (TECH-NOTES §11): one OpenAI-compatible Chat Completions client
  (ureq, blocking, SSE), services and models, prompts for ideas / Lab passes / trims, anchoring
  replies to numbered sentences, keys in the Keychain (Secret Service / Credential Manager off the Mac; `DOSTOEVSKY_SECRETS=memory` in tests), OpenRouter PKCE.
- `crates/app` (binary `dostoevsky`, gpui-kit 0.8): `view.rs` is the custom text element
  (`TextView` + `PageElement`) used for the page, Overflow notes and fields; `workspace.rs`
  is the window; `title_bar.rs` is its title bar off the Mac (Windows' caption buttons, Linux's client-side frame, a first window inside the work area) and `menu_bar.rs` the menus drawn in it there; `panels.rs` (which side each panel tab lives on, dragging tabs between the headers, what a side shows), `sidebar.rs`, `overflow.rs` (the Drawer panel; `drawer.rs` keeps a project's drawer), `context_menu.rs`, `panel_foot.rs` (the left panel's foot: Open, Save, Finder, Settings; the Project tab with no project), `sheets.rs` (source view,
  shortcuts, preferences, import, detached), `find.rs`, `external.rs` (FSEvents), `instance.rs` (off the Mac one app process: a later launch hands its files to the first over a named pipe or Unix socket),
  `effects.rs`, `sound.rs`, `spell.rs` (NSSpellChecker; off the Mac `hunspell.rs`: Hunspell dictionaries via spellbook; on Windows `win_spell.rs`: its own ISpellChecker), `keymap.rs`, `memory.rs`,
  `sentence_keys.rs` (walk, grow and move by sentence and paragraph, ED-10…12; no UI),
  `export_ui.rs` (Copy Final Text, the Export sheet, Print), `pdf.rs` (TextKit pages into a PDF, the
  preview's bitmaps, PDFKit printing; off the Mac `crates/pdf` does the pages and Print goes to the system: Windows' print verb or the PDF viewer, Linux's print portal or `lp`),
  `bookmarks.rs` (security-scoped bookmarks: the sandboxed build reopens files after a relaunch); `project.rs`
  reads a folder from its file names and plans renames (no UI), `project_ui.rs` is a window on a
  project (the list, parked documents; TECH-NOTES "Projects"); `welcome.rs` builds the welcome page (WIN-19) from `assets/welcome/<lang>.md`; `shots.rs` is a dev-only script runner
  (`--features shots`, `DOSTOEVSKY_SCRIPT=steps.jsonl`) that renders windows offscreen.
  AI in the app: `assist.rs` (keys, model lists, Connect OpenRouter, requests on their own
  threads), `ai_prefs.rs` (Preferences → AI), `ideas.rs` (⌥⌘A, `??`), `lab.rs` (the AI button, once LAB; marks,
  typos, trims; drawn by `view.rs` from `LabPaint`).
  Themes (TECH-NOTES §15, docs/THEMES.md): `theme.rs` (tokens, theme pairs, seed derivation,
  `Themes/` files), `theme_ui.rs` (the Theme row in Preferences → General, the New / Edit theme
  sheet), `theme_import.rs` (Dostoevsky and VS Code theme files; no UI).
  `updates.rs` is the .dmg build's opt-in update check (`--features updates`, which `scripts/bundle.sh`
  turns on; the App Store build has none; TECH-NOTES §14).

## Conventions & Patterns

- Offsets in the model and the view are display `char`s; `text.rs::TextIndex` converts to
  UTF-8 bytes (GPUI layout) and UTF-16 (macOS input).
- Every model change goes through `DocModel` (bumps `version`; views refresh from it).
- Every string a user sees or hears goes through `l10n::tr!`/`trn!` with a literal key (whole
  phrases, not fragments); add the translations to every catalog in `crates/l10n/locales`.
- In `crates/app` test modules, import items explicitly: `use super::*` brings in GPUI's
  `test` macro and breaks `#[test]`.
- `DOSTOEVSKY_SUPPORT_DIR` and `DOSTOEVSKY_KEYMAP` redirect app data for tests.
