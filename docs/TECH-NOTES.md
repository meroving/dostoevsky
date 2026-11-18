# Tech notes: building it on gpui-kit

Companion to the product spec (the PRD, kept by the maintainers; requirement IDs such as `ALT-7` come from it). This is how the requirements map onto Rust and gpui-kit, and where the risks are. It is not a full design doc; each milestone refines it.

> Component names in §4 were checked against docs.rs/gpui-kit 0.8.0 and gpui-kit.com (2026-10-22). The GPUI internals in §3 and §6 (`PathBuilder`, `EntityInputHandler`, `request_animation_frame`, …) follow upstream Zed GPUI. Confirm them against the pinned `gpui-pre` 0.3.9 during the M0 spike.

## 1. Stack

| Concern | Choice |
|---|---|
| UI | `gpui-kit = "0.8"`, which re-exports GPUI (`gpui_kit::*`, pinned to the `gpui-pre` 0.3.9 snapshot), `base` and `assets`. Its `component` feature (gpui-component) stays off: nothing uses it, and its `init` lists every font on the system (see "Cold start"). **Pin exact versions** in `Cargo.lock`; upgrade on purpose. |
| Text storage | `ropey` (rope, char-indexed). The page is too big for `String` splicing at 100k words. |
| Markdown | `pulldown-cmark` (block structure, inline spans, source offsets) |
| Serialization | `serde`, `serde_json` (JSON Lines meta) |
| Fuzzy anchoring | `diff-match-patch-rs` or a small in-house Bitap; `strsim` for context scores |
| Sound | `rodio`, with two tones synthesised at startup (sine + 2nd harmonic, exponential decay) |
| File watching | `notify` (FSEvents backend on macOS) |
| Paths and config | `directories` (`~/Library/Application Support/Dostoevsky`, `~/.config/dostoevsky`) |
| Tests | `proptest` (round trip, anchoring), `insta` (serializer snapshots), GPUI `TestAppContext` (actions and flows) |
| Packaging | `cargo-bundle` or a hand-made `.app` + `codesign` + `notarytool`; universal binary via `lipo` |

## 2. Workspace layout

```
crates/
  doc-model/      # pure Rust, no GPUI: tree of runs/groups/variants, edit rules, ghost, overflow, undo  ✅ built
  doc-format/     # parse/serialize FILE-FORMAT v1, re-anchoring  ✅ built
  english/        # word characters, word counting  ✅ built
  l10n/           # the UI's languages: tr!/trn!/trc!, locales/<lang>.json (§12)  ✅ built
  ai/             # v2: OpenAI-compatible client, prompts, anchoring, Keychain, OpenRouter PKCE (§11)  ✅ built
  export/         # the final text as Markdown, plain text, HTML, Word, ePub; styles; zip (§13)  ✅ built
  app/            # gpui-kit binary: windows, PageElement, sidebar, overflow, menus, prefs, sounds, effects
```

`doc-model` and `doc-format` take up most of the test effort and must build with no UI dependency (NFR-8).

### Core model (built: `crates/doc-model`)

The model is decided in ADR-001 and ADR-002 (cuts; the maintainers' decision records), and implemented and tested in `crates/doc-model`. In short:

```rust
Document { state: { page: Container, notes: Vec<Note> }, undo: Vec<State>, redo: Vec<State> }
Container = Vec<Node> + cached display length
Node      = Text(Arc<Run { text }>) | Group(Arc<Group { id, scope, active, variants }>)
          | Cut(Arc<Cut { id, content, shown, look, made, paragraph, batch }>)   // zero-width unless shown
Variant   = { id, by: Me | Ai | Import, content: Container }   // owns nested groups and cuts
Note      = a cut with no spot (no trace, or its place gone)
```

- **The visible page is derived:** walk the tree through each group's active variant. Cycling only changes `active`; nothing moves.
- **The editor uses display offsets:** `replace(range, text)` gives the same result as a splice on a plain string (property-tested). Edge and crossing rules: ADR-001 D4/D5.
- **Undo restores snapshots** of the structurally shared tree (`Arc::make_mut` clones only the edited path). Cycling is undoable, one combined step per run (D3).
- API for the view: `text()`, `text_range()`, `len()`, `visible_groups()`, `group_range(id)`, `ghost_ranges()` (shown cuts), `traces()`, `cuts()`, `group(id)`, `notes()`, `take_changes()` (ADR-001 D10), plus the edit commands. `set_presentation()` says how cuts show (ADR-002 C2). `snapshot()` / `from_snapshot()` is the bridge to `doc-format`. Measured on 96k words: about 0.1 ms per keystroke and 0.1 ms per cycle in release builds (`cargo run --release -p doc-model --example bench_large`).

## 3. The page: a custom GPUI element

gpui-kit offers `Textarea` (plain multi-line input) and `Editor` (a code editor with gutter, folding and `create_decorations_collection` / `RangeDecoration` fills). **Neither covers** hand-drawn underlines, the dot rows, margin rules, per-span hover cycling, ghost opacity, or our block styling. So:

**`PageView` (an `Entity`) + `PageElement` (`impl Element`)**, built on GPUI's text system, the same way Zed's editor does it:

| Step | GPUI pieces |
|---|---|
| Layout | For each visible block: `window.text_system().shape_text(...)` with `TextRun`s (font weight for headings; colour alpha `0.12` for ghost runs; dimmed markers), wrap width = column width → `WrappedLine`s, rewrapped by `wrap.rs` (closing marks stay with their word; with Hyphenation on, a word may be broken where `hyphen.rs` says, from the system dictionary for the text's language). Justified (WIN-18), `wrap::justify` moves the glyphs of each row but the last so its spaces fill the column: paint, caret, hit-testing and every mark read x from the same layout, so they follow; the hyphen is a separately shaped `-` painted at the row's end, never text. Paragraph breaks (ED-13): `view::paragraph_gaps` marks the empty line of each `\n\n` after a block (in a run of empty lines, every other one); its `LineBox` is a gap (⅔ of a line between body paragraphs, 0 when Indented, a line around other blocks), skipped by selection rects, split in half by hit-testing and stepped over by ↑/↓, and `TextView::off_gap` keeps the caret out of it. An Indented paragraph's first row is `x_shift`ed by 1.5 em and `wrap::rewrap`/`justify` give it that much less width. Cache per block, keyed by `(block hash, width, font, indent, justified, language, the word being typed)`. Virtualise: only lay out blocks near the viewport. |
| Paint text | `line.paint(origin, line_height, window, cx)` |
| Zen writing | Writing mode only (`TextView::zen_target`, placed by `compute_layout` as a `ZenLit`). What stays bright: the caret's block (Paragraph), its sentence from `prose::sentences` (Sentence), or its wrapped row (Line); a selection lights every block it touches. The rest is `ZEN_DIM` (Distance fade Off), or fades by wrapped rows outward from what stays bright, whatever it is: the next row (or the rest of a bright row) is step one, then a step every four rows, eased between (Near 55/42/30 %, Whole page 42/24/12/6/3 %). Rows are numbered so a caret on an empty paragraph sits one row from each neighbour. Line focus and Distance fade need every line's rows first, so a first pass lays the lines out undimmed (cached). Each line gets (byte, alpha) pieces cut into its `TextRun`s like ghosts; the 200 ms fade lerps them when the light moves (a sentence or paragraph growing as it's typed doesn't). Ghosts, markers, spelling marks and Private mode's mask multiply by it. Defaults (Paragraph, Off) lay out exactly as before. |
| Private mode | Writing mode only (`TextView::private_mask`: the look, and the chars left readable: the run of non-spaces the caret touches, or its sentence from `prose::sentences`). Each `ShapedLine` keeps its runs' colours; `Layout::mask_shapes` walks the glyphs and puts a dot in each advance, a bar over each word (split where the colour changes), or the glyph's ink box (`TextSystem::typographic_bounds`, cached) under a GPU-blurred drop shadow for Blurred ink. The live chars are painted as text, clipped to their rects. Nothing is reshaped, so wrapping, the caret and hit-testing don't change; spelling marks, REVIEW marks and the empty page's prompt aren't drawn. |
| Selection and caret | `window.paint_quad(fill(bounds, color))`; caret blink handled by the view's own timer |
| **Wavy underline** | `PathBuilder::stroke(px(1.15))`, built from a quadratic curve every 8 px with a jitter seeded by `GroupId` (deterministic: no shimmer on redraw), then `window.paint_path(path, pen)` |
| Dots | `paint_quad` with `corner_radii = r` (filled) or a stroked circle path (ring); original ring r=1.75, others r=1.45 |
| Paragraph rule | Vertical wavy path at `column_left − 22 px`, colour `para-bar` at 0.75 |
| Paragraph preview wash | `paint_quad` behind the block, `para-hl`, with a 220 ms fade |
| Hover hit-testing | Keep `Vec<(GroupId, Bounds)>` from layout; `on_mouse_move` sets `hovered_group` and calls `cx.notify()` |
| Text input | `impl EntityInputHandler for PageView` + `window.handle_input(&focus, ElementInputHandler::new(bounds, view), cx)` in paint. This gives IME marked text, dictation, emoji and accent popovers. (Follow GPUI's `input` example and gpui-component's `Input` source.) |
| Hover chip | A separate absolutely-positioned `div` overlay (`deferred(anchored()...)`) rather than painted text, so it uses theme components |

**M0 spike (do first):** a minimal `PageElement` that edits one paragraph with IME, selection, a wavy underline and ↑/↓ cycling. This settles R1 before anything else is built on it.

## 4. Chrome: gpui-kit components

| UI (PRD) | gpui-kit (`gpui_kit::component::…`) |
|---|---|
| Window and title bar (WIN-1) | `TitleBar` / `TitlebarOptions { appears_transparent: true, traffic_light_position }` |
| Root, dialogs, toasts | `Root` (required for overlays), `Dialog` / `AlertDialog` via `window.open_dialog(cx, …)`, `Notification` |
| Alternatives sidebar (ALT-13) | Custom `div` styled with theme tokens, slide-in via GPUI `Animation`; the add field is the kit's `Input`; lists are plain divs (gpui-kit's `Sidebar` looks navigation-oriented; check before reusing it) |
| The Cuts panel (CUT-9; once the Drawer) | Custom panel + a second, simpler `PageElement` instance (no alternatives) for drag-out; or `Textarea` for v1 if drag-out can be layered on top |
| Context menu (CTX-1) | `Menu` (context-menu mode) with `Kbd` shortcut labels via `Kbd::binding_for_action_in` |
| Source view (FILE-7) | `Sheet` or `Dialog` with `Tabs`; content is a read-only `Editor` (code-editor mode suits a source view) |
| Shortcuts sheet (KB-1) | `Dialog` + `Kbd`, generated from the live keymap |
| Preferences (PREF-1) | `Settings` page + `Tabs`; `Toggle`, `Radio` (there's **no segmented control**, so use `Radio` styled as a button row or a small custom component). Check the kit for a slider component; otherwise build a small custom one. |
| Theme (§9) | A custom `Theme` / `ThemeColor` loaded at startup from our tokens; register the bundled fonts (IBM Plex Mono, JetBrains Mono, iA Writer Quattro, Literata; the hands Shantell Sans, Caveat, Bad Script) as app assets. Shantell Sans ships as one static instance (wght 500, INFM 70: GPUI picks weights, not other axes); Bad Script is upstream's thickened by 10 units of its 1000-unit em (`scripts/embolden_font.py`, advances kept so the joins meet; `bad-script-weight`), as its hairlines broke up at 19px; Literata ships as four static instances (wght 400 / 700, roman and italic, opsz pinned at 14) subset to Latin, Greek and Cyrillic with hinting dropped, about 875 KB (`serif-font`). Column width counts an average letter of a prose sample (`view.rs` `average_letter`), not an “m”, so a proportional face also gets the characters per line the row promises; a mono's letter is its “m” exactly; `theme::hand()` / `theme::hand_px()` give the chosen hand and its size against Caveat's |
| Native menus (WIN-11) | `cx.set_menus(vec![Menu { … MenuItem::action("Ghost It", GhostToggle) … }])`, **after** `cx.bind_keys` so shortcuts show |
| Open/Save panels (FILE-1) | `cx.prompt_for_paths(PathPromptOptions{…})`, `cx.prompt_for_new_path(dir, suggested_name)` |

## 5. Actions and keymap

```rust
actions!(page, [CycleNext, CyclePrev, CycleAtCaretNext, CycleAtCaretPrev, BackToOriginal,
                NextGroup, PrevGroup, AlternativesForSelection, SettleOnThis, ToggleLayer, WritingMode, EditingMode,
                GhostToggle, StashToOverflow, ToggleOverflow, InsertOverflowNote,
                ViewSource, CopyFinalMarkdown, ShowShortcuts]);

cx.bind_keys([
    KeyBinding::new("cmd-shift-a", AlternativesForSelection, Some("Page")),
    KeyBinding::new("cmd-/",       GhostToggle,              Some("Page")),
    KeyBinding::new("cmd-shift-x", StashToOverflow,          Some("Page")),
    KeyBinding::new("alt-cmd-down",CycleAtCaretNext,         Some("Page")),
    KeyBinding::new("down",        CycleNext,                Some("Page && hovering_group")),
    // …
]);
```

- The bare ↑/↓ binding is conditional: `PageView` adds the `hovering_group` identifier to its `KeyContext` only while the pointer is over a group **and** the layer is on (PRD ALT-7). Otherwise the arrows fall through to caret movement.
- User overrides: load `~/.config/dostoevsky/keymap.json` with `KeyBinding::load` (fallible) and report bad entries in a toast instead of panicking.
- Text size: ⌘= / ⌘− / ⌘0 (`TextBigger` / `TextSmaller` / `TextDefaultSize`) step the four sizes Preferences → Page offers, so the keys and the chips move one `prefs.font_size`. macOS reports ⌘⇧= as the key "+" with no shift, so TextBigger binds both `cmd-+` and `cmd-=`: the native menu bar shows a binding's *first* match (⌘+, as Pages and Safari draw Bigger) while the shortcuts sheet's lookup takes the *last* (⌘=) — each by design of those surfaces.
- Note for later ports: some Linux desktops intercept Ctrl+Alt+↑/↓.

## 6. Effects and sound

- **Explosion (ALT-16):** a full-window overlay `canvas()` element. Particles are stepped in `window.request_animation_frame()` until none are left (about 600 ms). A seeded RNG keeps it repeatable for screenshot tests. Reduce Motion (`NSWorkspace.accessibilityDisplayShouldReduceMotion`) → a 150 ms fade instead.
- **Cut Without a Trace's flight (CUT-5):** an overlay `div` with the text, animated with `with_animation` from the selection bounds to the Cuts panel.
- **Tones (ALT-9):** pre-render two `SamplesBuffer`s (880 Hz / 160 ms and 523 Hz / 320 ms, 6 ms attack, exponential decay, volume from prefs) and play them on a long-lived `OutputStream` so there's no startup latency on the first keypress. The stream opens on a thread of its own (see "Cold start"), because `cpal::Stream` can't change threads; a tone asked for before it has opened is skipped.

## 7. Files, autosave, watching

- `DocumentStore` per window: path, last saved hash, dirty flag, autosave debounce (2 s idle) on a GPUI background task (`cx.background_spawn`), with serialization done off the main thread on a snapshot (`Rope` clones are O(1)).
- Atomic write, and a backup ring of 20 in `Backups/<path-hash>/<timestamp>.md`.
- `notify` watcher → on change, compare hashes (ignoring our own writes) → reload or prompt (FILE-5) → `doc_format::parse` re-anchors every record (FILE-FORMAT §5); detached groups come back in `LoadedDoc::kept.detached`. Save with `doc_format::serialize` + `write_atomic` (about 14 ms to serialize and 10 ms to parse 96k words: `cargo run --release -p doc-format --example bench_format`).
- **No save over words nobody has seen.** fseventsd can take tens of seconds to start a watcher, so a save never trusts the watcher alone: `Workspace::may_write_over_file` (in `save_to_current` and `save_on_leave`) reads the file first and, if it isn't the `disk_text` we last read or wrote, asks the FILE-5 question instead of writing. While that question is open (`Workspace::disk_prompt`) nothing is written over the file, so autosave can't put the page over theirs and make *Load Theirs* show words that are no longer on disk. Save As clears `disk_text`: the save panel already asked about replacing what's there.
- Crash recovery: journal unsaved buffers every 5 s to `Recovery/`, offer to restore on launch.

## 8. Testing strategy

| Layer | Tests |
|---|---|
| `doc-model` | Unit tests for every edit type versus group and ghost ranges; proptest random edit sequences → invariants hold (ranges valid, `variants[active] == text[range]`, no partial overlaps) |
| `doc-format` | Round-trip proptests; exact-output tests of the example records (instead of insta); anchoring corpus (body edited by scripted outside changes → expected placed/detached); escaping fuzz (`-->`, backslashes) |
| `english` | Table tests for word counting |
| `app` | Headless behaviour tests in `crates/app/src/ui_tests/` (see below); the PRD §7 flows run against the real app with `scripts/run_flows.sh` (a scripted window stays in the background and lets the real pointer pass through, so using the Mac meanwhile can't disturb it); its screenshots are compared with approved renders of the app (`scripts/flows/baselines/<macOS>-<width>px/`, `imgdiff` with a per-pixel tolerance and a maximum share of differing pixels; `UPDATE_BASELINES=1` re-approves). The look itself is checked by eye against the mockups (a maintainers' script, beside them) |
| Perf | `scripts/perf_check.sh` fails when an NFR-1 budget is exceeded. Keystroke and cycle to paint (p99 at 20k words) and opening 100k words run as release UI tests (`ui_tests/perf.rs`, no GPU raster). Cold start launches the release app and measures exec to first frame (median of 3, after one discarded launch). `PERF_SLACK` multiplies every budget for slower machines. On an M1 Pro in Nov 2026, under load: about 5.2 ms, 1.9 ms, 29 ms and 310 ms. The model and format examples (`bench_large`, `bench_format`) remain for profiling. |

### Headless app tests (`crates/app/src/ui_tests/`)

`ui_tests::Ui` opens a real `Workspace` in a GPUI `TestApp` window (1180×860) and drives it like a user: `keys("cmd-shift-a")`, `type_text`, `action(..)`, `select_text`, `hover_group(id)`. It asserts on the model and view: `text()`, `variants(id)`, `active(id)`, `hovered_group()`, `selection()`, `sidebar_open()`. They run in `cargo test` with no window on screen, about 0.1 s per test.

- **Text shaping is real.** The test platform's no-op text system is replaced by `gpui_apple::AppleTextSystem` (a macOS dev-dependency), so layout, hit-testing and key contexts match the app. `current_platform()` can't be used: `MacPlatform` must be created on the main thread, and tests run on worker threads.
- **Pointer moves aim at groups, not pixels.** `hover_group` asks the page's last layout where the group's middle char is (`Layout::point_for`). Each step redraws, so `hovering_group` and other key contexts are current.
- **Under `cfg(test)`, `files::support_dir()` is a per-process temp folder and `keymap::path()` is `None`**, so tests never read or write the user's Application Support folder or keymap and always use the default bindings. No `Sounds` global is set, so playing a tone does nothing.
- **Test-only hooks** (`#[cfg(test)]`) for what a user hears or sees but a model can't show: `sound::PLAYED` records the tones (`take_tones`), and there are accessors for the status toast, running bursts and their mode, sidebar row bounds (`click_variant`) and running a context-menu item by label. Under `cfg(test)`, `platform::reduce_motion()` is false, so tests set the motion with `Prefs::effects`. The hover label is computed in `TextView::hover_chip`, which paint also uses.
- **Traceability.** A test cites the requirements it covers in its `///` comment (`/// ALT-7: …`) or name (`fn alt_7_…`). The maintainers' coverage script lists it per PRD area, and fails if any P0 has no test. Requirements that can only be checked visually are listed in the script's exemptions.
- **Bugs found by these tests** keep their test, marked `#[ignore = "dsky-…: …"]`. `cargo test -p dostoevsky ui_tests -- --ignored` shows which are still open. Remove the `ignore` when the bug is fixed.
- **Layout rules, not pixels** (`ui_tests/layout.rs`): the header is one line at every window size, its centre stays put while the counter changes, the page doesn't move when a panel opens in a wide window and is never covered in a narrow one, and the dots sit with their group (`view::dots_start`, which paint also uses). These hold through a restyle; only a deliberate layout change should touch them.
- **When the look changes on purpose**, run `scripts/run_flows.sh`, check that the `*.diff.png` show only the intended change, then `UPDATE_BASELINES=1 scripts/run_flows.sh` (it refuses when a flow failed) and commit the new baselines with the change. A change that isn't meant to be visible (a refactor, a model change, a GPUI upgrade) should leave every flow screenshot identical; a diff there is a bug.
- **Timers use the test clock.** Autosave and the journal don't run unless a test advances it. Animations (explosion, wash, fades) still use `Instant`.

### Find (`find.rs`, ED-6, PROJ-12)

- **The matching is pure.** `Searchable::of(doc)` takes what can be searched out of a `Document` on the UI thread: the page text, its ghost ranges, the hidden versions of groups on the page (with where the group starts), and the scraps. `find::spots` turns it into `Spot`s in reading order (page matches with each hidden version at its group, then scraps). The same function runs live on the page and in the background over every file (`search_jobs`), so a file's `n`th match means the same thing in the list and after opening it.
- **One card, one place at a time.** `FindBar::spots` are the matches of `place`: the file on the page or, in the whole-book scope, the project's drawer (a place of its own between the chapters and the notes). `files` and `order` come from the background search; `counts()` puts the live count in for the current place. `Workspace::find_move` steps within the place and `find_cross` goes on to the next place with matches (`switch_to`, so leaving a file saves it).
- **Going to a match keeps the keyboard where it was** (`show_matches` restores the focus). A hidden version opens its group with `Sidebar::found` set; a scrap opens the drawer with `Drawer::found`; a ghost sets `TextView::raised_ghost`. `sidebar_by_find` / `drawer_by_find` remember what find opened, and `put_back_what_find_opened` closes it.
- **Edits re-search the page only** (`research`); a query or toggle change searches the project again (`refind(…, true)`), and a newer run makes an older answer stale (`FindBar::run`).

### Projects (`project.rs`, `project_ui.rs`)

- **The type is chosen, then saved** (PROJ-14). `project::open` reads `.dostoevsky` (a few `key = "value"` lines: `kind`, `notes`), or for a plain folder the `kind`/`notes` in its `ProjectMemory`; with neither, `scan_as(root, None)` guesses from the names (`detect`, `guess_notes`) and `Tree::chosen` is false, which makes the window ask once. `apply_settings` is the one way a choice lands: it writes the file (books, journals, or any folder that already has one) and rescans with the new `Settings`. `rescan` keeps a choice that lives only in memory by reusing `tree.settings()`. Choosing never renames; only Move to Notes, Add to the Book and the first-open “move them” do, through `apply_renames`, so they're undoable and backups follow.
- **A journal's new day starts with its date** (PROJ-6). `date` in `.dostoevsky` (`long`, the default when absent; `short`, `iso`, `none`) is read when a page is installed (`quiet_journal_page`, `project::date_heading`): a dated file that doesn't exist yet gets `# <Day::heading>\n\n` immediately, as an undo step of its own, with the caret after it. Typing is the next undo step, so ⌘Z takes the typing first and the date next; normal save and autosave write the new file. The forms are whole-phrase catalog keys, one per month. A day's list row skips a first line that is its own date in any form (`Day::is_heading`). Settings left `date: None` keep what the file says, like title and author.
- **Three kinds of file in a tree:** `main` (chapters or days), `notes` (everything under `notes_dir`) and `loose` (the rest, listed as *not in the book*). `all()` covers the three, so find, word counts and `entry()` see every file; `in_order()` is `main` only, which is what the whole-book text and ⌘[ ⌘] read.
- **The panel is the Drawer, its pieces are scraps, ⌘⇧X is Put in the Drawer (UI words, 2026-11-09).** Code (`overflow.rs`, `StashToOverflow`, `ToggleOverflow`, element ids `overflow*`), FILE-FORMAT (`dostoevsky:overflow`) and `doc.notes()` keep the old names, so users' keymaps and files keep working; “note” on screen means a file in the notes folder.
- **A project's cuts live in `cuts.md`** (`drawer.rs`, PRD CUT-14, ADR-002). `ProjectWindow::drawer` holds a `DocModel` of the cuts of no file (typed scraps from before, cuts of files gone) and `external`, every chapter's cuts as the file keeps them (`doc_format::ExternalCut`). A chapter loads with its own planted back (`format::load_chapter`, `Workspace::load_chapter`; the page opened before its project is re-read in `adopt_page_as_chapter`), and names its file (`Document::set_cut_source`), so in memory a chapter's document holds all its cuts and cutting, putting back or deleting is one step in one document. Writing a chapter (`save_to_current`, `save_on_leave`, `save_parked`) goes through `format::serialize_for_disk`: its cuts go to `cuts.md` first (`store_chapter_cuts`), and the chapter is written only if that worked, so no moment on disk is without them. Journals, backups and the source view keep the whole document (`format::serialize`). An older `drawer.md` is read until the first write, then removed if nothing changed it since (both are read as one if both exist). Renames move a chapter's cuts in `external` and its `cut_source`. Moving one of the cuts of no file onto the page makes one undo step in each document, linked (`Document::link_last_step`); each step carries a `seq` from a process-wide clock (`StepTag`), so `model::joint_history` undoes the later of the two documents' next steps, or both halves of a linked one together. A move between two chapters is linked the same way (`cuts::CrossLinks`). The cuts file saves with the page, has its own recovery journal, counts as an unsaved "other file" when closing with autosave off, and is re-read on outside changes when it has none of its own.
- **The Cuts panel** (`cuts_panel.rs`, `overflow.rs`, `list_panel.rs`; PRD CUT-9, CUT-10). `panel_sections` lists *this page* (the page's document, then the cuts of no file that came from it) or *the whole book* (this file first, then every file in book order: a parked chapter's document, or its `external` cuts if it isn't loaded, then the cuts of no file left over). A `CutRef` says which document holds a cut; using one in a chapter on disk parks that chapter first (`materialize`). The editor is a `TextView` in `Mode::Cut(id)` on the document holding the cut (`Document::replace_in_cut`); cuts of other chapters are edited on their own page. The header counts without building the list (`panel_count`). `list_panel.rs` (scope switch, group header, fold row) is meant for the comments list too.
- **Traces on the page** (`view.rs`, `cuts.rs`; PRD CUT-6…8). *Show cuts as* is `Prefs::cut_look`; `DocModel::present` reads it through a per-thread mirror (`prefs::cut_look`, set by `prefs::publish`), and a change re-presents every window's documents (`represent_cuts`), outside the undo history. The view keeps the marks (`TextView::marks`: points, in editing only) and paints them from `mark_rect`: a 1.5 px accent hairline in the gap, or a short rule in the left margin for whole paragraphs. Hovering one emits `ViewEvent::MarkHover`, and the window shows a peek (`render_peek`) that stays while the pointer is on it; a click on a mark, or a right-click on faded text, opens the trace's menu (`ContextMenu::trace`, `trace_op`). With the caret strictly inside a faded cut, `cut_being_edited` draws it at 60% over a faint wash; typing there edits the cut (ADR-002 C3).
- **`project.rs` has no UI.** It reads a folder into a `Tree` from file names and the saved choice, and plans renames. Everything that can hurt a user's files is here and unit-tested against real folders. `apply` renames all or nothing: every file passes through a private dot name first, so chains (`02 → 03`, `03 → 04`) are safe, and a failure puts everything back.
- **One `Entity<DocModel>` for the window's life.** Changing files swaps the *contents* of the entity (`std::mem::swap`), so the page, find bar, sidebar and drawer keep their subscriptions. The outgoing `DocModel` is parked in `ProjectWindow::parked`, undo history and all, and `TextView::document_swapped` drops everything the view cached. Don't add per-file state to `TextView` or `Workspace` without saving it in `Parked`.
- **Recovery journals are per file in effect.** A file parked with unsaved edits (autosave off) takes the window's journal with it, and the window gets a new one. `pending_recoveries` already handles several journals from one process.
- **Backups are keyed by a hash of the path**, so every rename goes through `files_renamed` → `files::file_moved`, which carries them along.
- **The list only builds rows near the view** (`OVERSCAN`). Rows have fixed heights (`ROW_H`, `HEAD_H`) so spacers stand in for the rest and `scroll_list_to_page` can compute a row's position. Building every row cost 60 ms a keystroke beside 1,200 entries; now it's about 4 ms. If a row's height changes, change the constant.
- **Listing is synchronous, reading is not.** `project::scan` only lists names and dates, and stays quick enough for the main thread. Word counts and first lines come from `count_words` in the background, keyed by modified time. Find across files runs in the background too, and a newer search makes an older answer stale.
- **Two watchers.** `external.rs` follows the open file and reloads it; `watch_project` follows the tree and refreshes the list. A parked file isn't watched, so `install` checks the disk when it comes back.

### Cold start

Where launch to first frame goes on an M1 Pro, with `DOSTOEVSKY_PERF` timestamps between the steps of `main` (Nov 2026, load average 10 to 20, median of 10 launches):

| Step | ms | Who |
|---|---|---|
| exec to `main` | 8 | dyld |
| `application()` | 70 | AppKit soft-loads ViewBridge; the keyboard layout (upstream) |
| Menu bar | 12 | `setMainMenu` loads WritingToolsUI |
| `NSWindow` created | 45 | AppKit and Metal load |
| Metal renderer | 5 | The shaders do compile from source (`runtime_shaders`), but the system's cache answers in about 1 ms, so a prebuilt metallib would gain nothing |
| Shown | 85 | The open animation's display link reads every display mode from WindowServer (`dsky-4r2`) |
| First draw, then the next frame | 20 | |

Launch must not pay for what the first frame doesn't need. Opening CoreAudio took 85 ms and now happens on its own thread. `Spelling::init_after_launch` starts `NSSpellChecker` 100 ms after the window opens; a timer rather than `on_next_frame`, because GPUI runs those callbacks before it presents the frame they follow. gpui-component's `init` spent 50 ms listing every font on the system, so that feature stays off.

Measure with alternating launches of the two builds (load moves every number here by tens of ms). `/usr/bin/sample` started from `main` gives a call tree of the whole launch, but its debugger notifications inflate each `dlopen` many times over, so use it to see what happens and timestamps to see what it costs.

## 9. Suggested first issues (M0)

1. Workspace + gpui-kit hello window with transparent title bar, theme tokens and bundled fonts.
2. ~~`doc-model`~~ done (ADR-001), including the change-event API for the view (ADR-001 D10).
3. **Spike:** `PageElement` with IME, selection, wavy underline and hover ↑/↓ cycling for one paragraph.
4. ~~`doc-format`: plain Markdown open and save, byte-identical round trip.~~ Done, with the whole of FILE-FORMAT v1.
5. Native menus + keymap wiring + an empty shortcuts sheet.

## 10. As built (v1, 2026-10-22)

Where the implementation differs from the plan above, and why:

| Plan | As built |
|---|---|
| gpui-kit components for the chrome (§4) | The chrome is drawn from GPUI elements with `Root` from `gpui-base`, so the `component` feature is off. |
| `ropey` for page text | The view keeps the page as a `String` plus a per-line index (`app/src/text.rs`). The model already splices structurally; at 20k words a keystroke costs ~0.5 ms (replace + re-index, memoized line styles) and layout ~0.3–1.3 ms, so a rope wasn't needed. |
| Virtualise layout | Every line is shaped once and cached by (text, runs, width, font); only visible lines are painted. First layout of 20k words ≈ 25 ms. |
| Hide heading `# ` | The marker is laid out transparent and the line shifted left by its width, so offsets never change. |
| gpui-kit `Dialog`/`Sheet`/`Menu` | Sheets, menus and the sidebar are plain divs with the theme tokens, to match the mockups exactly. Native `window.prompt` is used for confirmations. |
| `TestAppContext` flow tests | `--features shots` runs JSON-lines scripts against the real app and renders windows offscreen (`Window::render_to_image`); `scripts/run_flows.sh` runs the PRD §7 flows and checks the saved files. Scripts name places by text or group (`{"hover": {"group": 0}}`, `{"select": "friction"}`), never by pixels, and check state along the way with `{"expect": …}`; a failed expectation exits 1 and fails the run (`src/shots.rs` lists the steps). `scripts/e2e_bundle.sh` drives the bundled app the same way (`bundle.sh` with `FEATURES=shots` and its own `BUNDLE_ID`) launched by `open` as Finder would, with the steps inline in `DOSTOEVSKY_SCRIPT=hex:…`, `{"say": …}` markers for the shell and `{"wait_for": …}` instead of sleeps. |
| Separate Overflow `PageElement` | One `TextView` serves the page, a note being edited and single-line fields (`Mode::{Page, Note, Field}`). |
| Rust toolchain | Pinned to 1.99.1 (`rust-toolchain.toml`): gpui-pre 0.3.9 uses `std::hint::cold_path` (stable in 1.95). |

## 11. AI (v2, epic dsky-vvw)

- **One wire format.** `crates/ai::client` speaks OpenAI Chat Completions to every service (OpenRouter, OpenAI, Ollama, LM Studio, a custom address), streamed over SSE with ureq + rustls. It blocks; the app runs each request on its own thread (`assist::off_thread`) and reads results through a smol channel, so no async runtime starts and launch pays nothing. A server that refuses `response_format` is asked again without it. OpenRouter also gets `reasoning: {effort: "low", exclude: true}` and 2,000 more `max_tokens`: left alone, reasoning models (GLM-5.3, say) can spend the whole budget thinking and end on `length` with no answer, which shows as its own error, not a refusal.
- **Anchoring.** `segment::sentences` numbers the text as the prompt shows it; replies are `{sentence, quote, …}`; `anchor::near_sentence` finds the quote (exact, then ignoring case, curly quotes and spaces; the sentence named or its neighbours). Unanchored items are dropped. Marks and cuts follow their quotes through later edits (`lab::refind`).
- **Ideas** are plain lines, not JSON, so each shows as its newline arrives; `ideas::clean` drops numbering, quotes, preambles and repeats. `Document::add_idea` coalesces a batch into one undo step (`UndoKey::Ideas`). For a word or phrase one line may be `∅` (`ideas::NONE`, `ideas::is_none`): **a version without the words**, `Document::add_cut_idea`. That version is the original's text as one cut (`Variant::without`): while it shows it *is* a cut (faded or a mark, not in the final text, in the Cuts panel as *a version*; `CutInfo::version`), and the page draws only the group's dots (`VisibleGroup::cut`). Put Back shows the original, Delete for Good settles on it with nothing kept, settling leaves an ordinary cut, using it from the panel copies it. Not showing, it is a version only (`CutInfo::listed`). The writer adds one from the original row's menu (`add_cut_version_and_show`).
- **Passes** (dsky-2ne, mockup ai-passes-v2): `Pass::{Mistakes, HardToRead, Tone}`. *Fix mistakes* asks for the smallest stretch and its fix; a fix far bigger than its words is a rewrite and is refused (`plausible_fix`), and an empty fix marks the whole sentence for rebuilding. Fixes become word groups (`Document::fix_words`, one step) and the sentences to rebuild stay as marks: `Lab::seq` steps the list through the groups, then the marks, and versions asked for those join the fixes' `Made`. *Hard to read* shows `lab::long_sentences` (counted here) before it asks, the request running `beside` the marks; the model's tangled sentences join them (`lab::join` in the crate, `join_marks` in the app) without moving the current row. *Suggest alternatives* / *versions* sends one `ideas::request` per mark (one row's, or every open mark's) to the Quick model (`Pass::steer` with the mark's note, six at a time), cleans the lines as ⌘⇧G does and makes the groups with `Document::offer_words` or `offer_sentences` (one step, the original showing); the marks that became groups leave the list. A new request cancels the one running.
- **The Review tab** (dsky-nk9, mockup ai-results B; `review.rs`) is the left panel's third tab, in editing only, beside Project and Alternatives; there is no header button and no card over the page. Its first layer lists the passes: a click (or ↑↓) chooses one, only its row's **Run** sends anything; ↩ runs the chosen row, 1–4 run a row directly, ← → set Trim's amount, esc closes the tab. Running shows the second layer, that pass's results as a list: a row's buttons act on that row (mockup review-settle: *Settle on* [“the”] [“teh”] · *Keep both* for a fix, *Settle on* [this] [mine] · *Keep all N* for versions, the words only when they fit beside the verb (`review::fits`), else *the fix* / *this* · *mine*; *Suggest versions*, *Make a comment*, *Dismiss* for a mark, *Keep it* / *Cut it* for a cut) and each section's, under its title, act on all its open rows and carry the count (*Settle all on* [fixes / chosen] = `settle_many`, [mine] = `Document::settle_to(ids, 0)`, *Keep all N*, *Suggest … for all N*, *Dismiss all*); the head keeps only *Cut N words*, *Stop* and *Try again*. Settling is `settle_to(&[g], i)`, one undo step; **Keep** (`Workspace::keep_lab_groups`) changes no text: the group leaves the list and stays in the document with its alternatives, the AI's choice showing. A typo's fix can't be kept (`review_text::typo`: the replaced word is misspelled, or only punctuation or case changed, or a doubled word went), so it has no Keep and *Keep all N* leaves it out. What was dealt with stays in the list, dimmed, with its outcome (`lab::Decided`: settled, mine, kept in amber); when nothing is left open the results go and the tab shows the passes. *‹ Review* goes back keeping them; *Close* drops the list and the marks and leaves the text and its groups as they are. Keys there (`Review` context): ↑↓ (and ⌥⌘[ ] from the page) step, ↩ settles on a fix or keeps versions, ⌫ settles on mine / dismisses, C a comment, K keeps a change (or keeps or cuts a trim's piece), ← → and ⌥← → Trim's amount, esc stops a request, then goes back. **Each pass keeps its own results**: `Lab::front(row, fresh)` parks the pass in front (`Lab::parked`, followed through edits like the front one) and brings another's back; the page draws only the front pass's marks, the tab's count is every pass's open results, and Run on a pass with results starts only that pass's list again.
- **Trims** rank cuts once, hedges and filler first, then repetition; `trim::pick` takes them in rank order to any amount (the slider, 0 to `trim::MAX` = 50%; `trim::LEVELS` are the ticks, and it snaps within a percent of one), skipping kept ones, and takes a cut only if the amount ± 2% can still be reached exactly with the cuts after it (a subset-sum check), so a whole sentence doesn't overshoot −20% into −25%. Measured on OpenRouter with Haiku 5.5: 11 / 20 / 30 / 51% for the four levels. The ticks show the words each level aims to leave, not a pick (cheap while dragging). A cut takes one adjacent space unless a neighbouring cut already has it. *Cut N words* is `Document::cut_many` as one batch. The list of cuts is the walk: every cut that would go and every kept one, in page order (`Trim::list`); a cut whose words were rewritten is `gone`, not kept.
- **Typing doesn't cancel a pass**: its reply is read against the text it was sent, and each mark, cut or fix is found again in the text now (`lab::refind`, nearest occurrence) or dropped; *Suggest* keeps the marks it asked about for the same reason.
- **Keys** are Keychain items (service "Dostoevsky AI", one account per service), read once per run. `DOSTOEVSKY_SECRETS=memory` keeps them in memory per thread; the UI tests set it.
- **Connect OpenRouter**: PKCE S256, a listener on 127.0.0.1:<random port> for `/callback?code=…`, then `POST /api/v1/auth/keys`. *paste a code* opens the auth page without a callback and reads the code from the clipboard. *or paste a key* takes a key made on openrouter.ai from the clipboard.
- **Consent** (dsky-a8n.15): before the first request to a service off this Mac, `Workspace::ai_allowed` shows a native alert naming the service (a Custom service by its host) and what it gets; the request runs again on Allow. The yes is kept per service in `prefs.ai.allowed`; removing the service or changing Custom's address forgets it. Ollama and LM Studio never ask; Test connection and model lists send no writing and don't ask either.
- **Tests.** `crates/ai/tests/replies.rs` checks recorded, messy replies against properties. `ui_tests/ai.rs` drives the app against a fake server on 127.0.0.1. `cargo run -p ai --example eval` runs the real prompts against a real model and checks the 1.5 s first-idea budget (NFR-1); it costs money and nothing runs it automatically.

## 12. Languages (dsky-44a)

The UI speaks English, German, Spanish, French, Italian, Brazilian Portuguese and Russian:
Latin and Cyrillic, left to right, so the text engine needed nothing.

- **The English is the key.** `tr!("Open…")` returns `&'static str` in the current language
  (English when a catalog lacks the key); `tr!("New {kind}", kind = k)` fills placeholders;
  `trn!("{n} word", "{n} words", n)` picks the plural form (CLDR rules in `Lang::plural_index`:
  Russian has three, French and Portuguese count 0 as singular); `trc!("variant", "Edit")` is
  the same English needing another translation (catalog key `Edit#variant`). Catalogs are
  `crates/l10n/locales/<code>.json`, compiled in, parsed once when a language is first used.
- **Which language:** `DOSTOEVSKY_LANG`, then Settings → General → Language (a combo box), then the first of
  macOS's languages with a catalog (`AppleLanguages`, which also holds the per-app choice in
  System Settings). Switching in Settings redraws every window and the menu bar at once. The
  bundle declares the languages (`CFBundleLocalizations`, empty `.lproj` folders from
  `bundle.sh`) so AppKit's own panels and menu items follow.
- **Write whole phrases.** A noun slotted into a phrase breaks gender and case, so menus have one
  key per project kind ("New Chapter", "New Day"). `{count}` is a number already grouped
  (thousands: a comma in English, a no-break space elsewhere). "one"/"them" choices are plain
  keys picked by `n == 1`, not plurals: Russian's "one" form also serves 21.
- **The AI button** (once LAB, the Lab) says AI in each language's word: ИИ, KI, IA. German
  addresses the user with *du*, as Apple's German macOS does.
- **What stays English:** file and folder names the app writes (notes/, Backups, untitled.md),
  prompts sent to a model (they ask for replies in the text's language), log output.
- **Checks.** `crates/l10n/tests/catalogs.rs` reads every `tr!`/`trn!`/`trc!` key from the
  source and fails on a missing, stale or mismatched (placeholders, plural forms) entry;
  `L10N_MISSING=$PWD/target/l10n cargo test -p l10n --test catalogs` writes the missing keys per
  language for a translator. `ui_tests/l10n.rs` renders menus, the page menu and Settings in
  other languages (`l10n::set_for_thread`, so parallel tests stay English). The flow baselines
  and screenshot scripts pin English (`DOSTOEVSKY_LANG=en`); `DOSTOEVSKY_LANG=ru
  scripts/run_flows.sh` shows a language (its text expectations then fail, by design).

## 13. Export (dsky-6kw, mockup export)

- **One name, the final text:** `Document::clean_text_with_notes()` is `clean_text()` plus where
  each place with other versions sits in it (private-use marks ride through the ghost tidying and
  come out as char ranges) and the other versions' text. Only Word's comments use the notes.
- **`crates/export`** (no UI, no platform code): `ir.rs` reads the final Markdown (pulldown-cmark,
  offsets) into blocks and runs with `NoteStart`/`NoteEnd` markers; `style.rs` holds the three
  styles (Page, Manuscript, Book), each style's `Text` (font, size in pt, line spacing, alignment,
  hyphenation, spaced or indented paragraphs) and the papers; `text.rs`, `html.rs`, `docx.rs`,
  `epub.rs` write the formats; `zip.rs` is a small deterministic writer (miniz_oxide, crc32fast)
  with a reader for tests. Word gets real styles (Normal, Heading 1, Quote) and
  `w:autoHyphenation` in the text's language, so Word re-hyphenates after an editor's changes; a
  manuscript's running head is a header with a PAGE field; ePub 3 has nav, ncx, a cover and the
  Text as CSS without font or size (the reader's). `cargo run -p export --example sample` writes
  every format to `target/export-sample/`.
- **PDF (`app/src/pdf.rs`)**: `compose()` turns blocks into paragraphs with looks (tested without
  AppKit); on the Mac they become an `NSAttributedString` (fixed line height = size × spacing,
  `hyphenationFactor` with `NSLanguage` set to the text's language, so hyphens follow the same
  dictionaries as the page), laid out by TextKit 1 (`NSTextStorage` → `NSLayoutManager` → one
  `NSTextContainer` per page, a chapter per layout manager so each starts a page; a manuscript's
  chapter starts a third down) and drawn into a CoreGraphics PDF context, flipped. Bundled fonts
  are registered with CoreText once (`theme::FONT_FILES`). It runs off the main thread (TextKit
  objects owned by one thread are fine). The preview is that PDF: a page drawn into a BGRA bitmap
  (`rasterize`) shown as a GPUI `RenderImage`; Word's and ePub's previews are PDFs of their type.
  Print hands the PDF to PDFKit's print operation on the window (sandbox: `com.apple.security.print`).
- **Copy Final Text** writes one pasteboard item with `public.html`, `public.rtf` (AppKit's HTML
  reader, then RTF) and the Markdown as `public.utf8-plain-text`; apps take the richest they read.
- **The sheet (`app/src/export_ui.rs`)**: state in `Sheet::Export(ExportForm)`; every change bumps
  a version and lays the preview out again after 120 ms, dropping stale results. What was chosen
  is kept in `Application Support/Export/<hash of the book or file>.json` (Text only where it
  differs from the style's own); a book's title and author go to `.dostoevsky`
  (`project::write_settings` keeps them when Project Settings writes the type). The action names
  `CopyFinalMarkdown` and `ViewSource` stay, so keymaps that bind them keep working.


## 14. Updates (the .dmg build, dsky-a8n.11)

- **Only the project's own .dmg has it:** the `updates` cargo feature (`crates/app/src/updates.rs`),
  which `scripts/bundle.sh` turns on only when the build is signed by the project's team
  (`UPDATES_TEAM`). A fork's build, signed by someone else or not at all, never asks
  dostoevsky.app; a fork with its own feed changes `URL` in `updates.rs` and `UPDATES_TEAM`. The App Store build is made without it and has no update code at
  all (App Review 2.4.5); its build script fails if the binary names `version.json`. Plain
  `cargo run` builds don't check either.
- **Opt-in:** the second launch asks once, with a quiet line at the foot of the window ("Check for
  new versions once a day? Yes · No"); Preferences → General → *Check for updates* changes the
  answer, and Dostoevsky → *Check for Updates…* works whatever it is.
- **The request:** at most once a day (and looked at hourly while the app runs), a plain GET of
  `https://dostoevsky.app/version.json` (no cookies, no identifiers), which the site's build writes
  from the newest `vX.Y.Z` tag: `{"version", "url", "dmg", "min_macos"}`. A newer version that runs
  on this macOS shows as a line in the status line's place, never over a status and never while
  typing: *Dostoevsky 1.1.0 is available · Download · Skip*. Download opens the release page (no
  Sparkle, nothing is installed for you); Skip hides that version until a newer one. A Homebrew
  install (its `Caskroom/dostoevsky` exists) is told to `brew upgrade`.
- **Kept in** `prefs.json` (`check_updates`: unset until answered) and `updates.json` (launches,
  whether the question was asked, the last check, the skipped version). `DOSTOEVSKY_UPDATES_URL`
  points the check elsewhere for testing.
