# Dostoevsky file format, v1

Status: draft 2 (2026-10-22). Implements PRD FILE-2, FILE-5 and FILE-6. Implemented in `crates/doc-format`; decisions taken while implementing it are marked *(impl)*.

## 1. Goals

1. **The body is plain Markdown.** Any Markdown tool shows exactly the text the writer currently sees (the active variants), with nothing in the way.
2. **Extra data is optional.** Strip the trailer and you still have the document. A file with no alternatives, ghosts or scraps has **no trailer at all**.
3. **Diff-friendly.** The trailer uses one JSON record per line, in document order, so changing one alternative changes one line.
4. **Survives outside edits.** Anchors carry quote and context, so groups can be found again after the body is edited in another tool.
5. **No second copy of the document.** The body is stored once; nothing in the trailer repeats it.

## 2. File layout

```
<Markdown body>
                                  ← one blank line
<!-- dostoevsky:overflow          ← optional
<overflow text>
-->
                                  ← one blank line
<!-- dostoevsky:meta v1           ← optional
<JSON record>
<JSON record>
…
-->
<EOF, with a single trailing newline>
```

- Encoding: UTF-8, no BOM, `\n` line endings (on read, `\r\n` is accepted and normalised).
  *(impl)* A file whose line breaks are **all** `\r\n` is normalised and written back with `\r\n`; a BOM is stripped and written back. Mixed line endings are left as they are. So a plain file round-trips byte for byte.
- **Trailer:** the longest run of *recognised* comment blocks at the very end of the file, separated only by blank lines. A block is recognised if its opening line is exactly `<!-- dostoevsky:overflow` or matches `<!-- dostoevsky:meta v<N>`. Comments anywhere else are part of the body and left alone.
- Order on write: overflow, then meta. On read, either order is accepted.
- **Body** = everything before the trailer, with trailing blank lines removed and one `\n` added back.
  *(impl)* Only trailing `\n`s are removed. A file **without** a trailer is the body exactly as it is (a missing final newline, or several, are kept: FILE-2 and the M0 byte-identical round trip). Normalisation happens only when a trailer is written.

## 3. Overflow block

The overflow block holds the scraps of the app's **Drawer** (PRD §6.5; the panel was called Overflow until 2026-11-09, and the format keeps the old name). Here a scrap is called a **note**.

```
<!-- dostoevsky:overflow
Maybe open with the margin metaphor instead?

Writers have always kept notebooks for the lines that didn't fit.
-->
```

- Free-form plain text. **Notes** are paragraphs separated by blank lines. Note indexes (0-based) are used by `stash` records.
- **Escaping.** HTML comments can't contain `-->`. On write, `--` followed by *k* ≥ 0 backslashes and `>` becomes `--`, then *k*+1 backslashes, then `>` (so `-->` → `--\>`, `--\>` → `--\\>`). On read, `--` followed by *k* ≥ 1 backslashes and `>` loses one backslash. The round trip is lossless.
- An empty overflow is not written.
- *(impl)* **Notes that contain blank lines.** Inside a note, an empty line is written as a lone `\` and a line made only of backslashes gets one more (`\` → `\\`); on read such a line loses one. Separator lines between notes are always empty, so ordinary notes look exactly as above and multi-paragraph stashes survive. Several blank lines in a row count as one separator.

## 3a. A project's cuts: `cuts.md` (formerly `drawer.md`)

In a project window the cuts belong to the project (PRD CUT-14). They are kept in **`cuts.md` at the project's root** (until the app first writes, an older `drawer.md`), an ordinary v1 file: its body is empty, its notes are the cuts (§3), and its meta block holds their groups (§4.3a) and their spots and origins (§4.4, §4.6, with `from.file`). A cut that names a file belongs to that chapter: the app plants it back when the chapter opens and takes it out again when the chapter is written, writing `cuts.md` first. Cuts that name no file (scraps typed into a drawer before, cuts of files gone) stay the file's own. Opened on its own, it is a document with an empty page and all its cuts as notes. The app writes it the first time there is something in it and keeps it out of the project's list, word counts and whole-book text. A chapter may still carry an overflow block of its own (written by a single-file window, or before projects kept their cuts); it is read and saved as usual, and the app offers to keep those with the project's (they get the chapter's `from.file` and go to `cuts.md`).

```
<!-- dostoevsky:overflow
On an exceptionally hot evening early in July.

Move the letter earlier?
-->

<!-- dostoevsky:meta v1
{"kind":"stash","note":0,"quote":"On an exceptionally hot ","from":{"file":"part-1/01-the-garret.md","quote":"A young man came out of "},"paragraph":true}
-->
```

## 4. Meta block

```
<!-- dostoevsky:meta v1
{"kind":"doc","layer":true}
{"kind":"alt","id":"k3f9a","scope":"word","at":{"block":1,"offset":12,"before":"Most of the ","after":" in writing comes"},"active":0,"variants":["friction","hesitation",{"t":"second-guessing","by":"ai"}]}
{"kind":"ghost","at":{"block":5,"offset":97,"length":48}}
{"kind":"stash","note":2,"quote":"Writers have always kept","from":{"block":2,"quote":"Editors have always kno"}}
-->
```

- After the header line comes **one compact JSON object per line** (JSON Lines). No pretty-printing.
- Records are sorted by `(at.block, at.offset)`, then by kind (`doc` first, `stash` last), so the output is stable.
  *(impl)* Body records come first, then note records (by `at.note`), then `stash`. A nested record follows its top-level ancestor (document order). Records written back verbatim (detached groups, unknown kinds) are placed by their own `at`, after generated records at the same place; a detached group's records stay together.
- **Escaping.** Every `-->` inside a JSON string is written as `--\u003e`. Standard JSON parsers read it back unchanged.
- `v1` is the major version. Readers must reject a major version higher than they know (and open the body as plain text with a warning). Unknown *kinds* and unknown *fields* must be **kept as they are** and written back on save.
  *(impl)* With a newer major version the whole trailer is kept verbatim and written back after the body; the app should treat the document as read-only. Unknown fields are kept on `doc` and `alt` records, on variant objects (matched by text, so an `at` timestamp survives reordering), on `ghost` records while the ghost stays where it was, and on `stash` records (matched by note quote). Unknown fields *inside* `at` are not kept (anchors are recomputed on every save). Meta lines that aren't JSON objects with a `kind` are kept verbatim at the end of the block.

### 4.1 Anchors (`at`)

| Field | Type | Meaning |
|---|---|---|
| `block` | int | 0-based index of the top-level Markdown block in the body. Blocks come from a CommonMark parse; a list or block quote counts as one block. *(impl)* Dialect: CommonMark + tables, footnotes, strikethrough, task lists and YAML front matter (pulldown-cmark). Block 0 starts at the first char of the body (leading blank lines belong to it); a position between blocks belongs to the block before. |
| `offset` | int | Start position in **Unicode scalar values** (Rust `char`s) from the first character of the block's *source* text, including markers such as `# ` and inline syntax. Omitted for paragraph-scope groups. *(impl)* Omitted only when a top-level paragraph group starts exactly at its block's start. A span may run past its block's end (a group or ghost that contains paragraph breaks, ADR-001 D6): lengths count body chars across blocks. |
| `length` | int | Span length in chars. Only for `ghost`. For `alt`, the length is the active variant's length. |
| `before` / `after` | string | Up to 16 chars of context on each side, used for re-anchoring. Optional but always written. *(impl)* Context stops at a line break and is taken from the record's container (body, variant or note). Not written for `{block}`-only paragraph anchors. |

The **quote** of an anchor is the text the span should contain: the active variant for `alt`, and `body[block][offset..offset+length]` for ghosts. Ghosts also store a 16-char `quote` prefix when `length > 16`. *(impl)* Ghosts always store `quote`: the first `min(16, length)` chars.

### 4.2 `alt`: alternative group

| Field | Type | Req | Meaning |
|---|---|---|---|
| `id` | string | ✓ | Stable id, 5–8 chars `[a-z0-9]`, unique within the file. *(impl)* Derived from the model's `GroupId` (5 chars via a scrambled bijection, 8 chars for very large ids) and decoded back on load, so ids are stable across saves. Other valid ids are kept; invalid or duplicate ones get a fresh id. A detached group whose id collides with a live one is renamed on save. |
| `scope` | `"word"` \| `"sentence"` \| `"paragraph"` | ✓ | Which sidebar tab it lives in. Headings use `sentence`. |
| `at` | anchor | ✓ | Where it is. For `paragraph`, only `{block}`. |
| `active` | int | ✓ | Index of the variant shown in the body |
| `variants` | array | ✓ | Index 0 is the **original**. A bare string is a variant the user wrote; an object is `{"t": text, "by": "me" \| "ai" \| "import", "at"?: RFC 3339 timestamp, "cut"?: true}`. `"cut": true` marks **a version without the words** (a word group's only): its `t` is `""` and its text is a cut inside it (§4.6), so while it shows the group has no text in the body. Never the original. |
| `parent` | string | – | For nested groups (any depth, ADR-001 D1): the id of the group whose variant contains this one |
| `pvariant` | int | – | Which of the parent's variants this group belongs to. When nested, `at.offset` counts from the start of that variant's text and `at.block` is the parent's block. |
| `detached` | bool | – | `true` if the group couldn't be re-anchored (§5). It is kept, but not drawn on the page. |
| `ghost` | bool | – | The whole group is ghosted, whichever variant is showing (ADR-001 D8) |

Invariants: `variants.length ≥ 2` (a group with one variant is dissolved on save); `0 ≤ active < variants.length`; `variants[active]` equals the anchored body text exactly. A version without the words showing is anchored as a point: `at` with `before`/`after` and no text between them.

*(impl)* On read, a one-variant group is loaded as is (it is dissolved on the next save, with a group ghost flag carried onto its text). Empty variant strings are dropped (not a version without the words, which is kept, its cut planted in it; older readers drop it, or keep the record verbatim when it is the one showing); a record whose active variant is empty and not one without the words, whose `scope` is unknown or whose `variants` isn't an array is kept verbatim as an unknown record. An out-of-range `active` falls back to 0 with a warning. A variant is written as a bare string only when `by` is `me` and it has no extra fields.

### 4.3 `ghost` (older files only)

*Read, never written: files from before cuts (§4.6) have them, and they become cuts on reading.*

`{"kind":"ghost","at":{block,offset,length,quote?}}`. The ghosted text is **in the body** as ordinary text. Only its dimmed state lives here. Ghost ranges never overlap. Adjacent ghosts are merged on save.

Ghosts belong to the variant they are in (ADR-001 D1). A ghost inside a group's variant has `parent` (group id) and `pvariant` (variant index), and its `at.offset` counts from the start of that variant's text, the same as nested `alt` records. This is how ghosted text inside an *inactive* variant is saved.

### 4.3a Groups (and, in older files, ghosts) inside Overflow notes

Notes can contain groups (a cut keeps them, PRD CUT-12); older files can have ghosts there too, which become plain text on reading (§4.6). Their records use `"at":{"note":i,"offset":…}` instead of `block`, with offsets counted from the start of the note's text.

### 4.4 `stash`: overflow note origin

`{"kind":"stash","note":i,"quote":"first 24 chars of note","from":{"block":n,"quote":"first 24 chars of the source paragraph"}}`. To resolve the note, match `quote` against the overflow notes (falling back to index `i`). To resolve the paragraph, match `from.quote` against the starts of body blocks (falling back to index `n`). If either fails, the link is dropped silently; the note itself is never dropped.

*(impl)* The quote is taken after the text leaves the page: what is left of the source paragraph, or, when all of it was stashed, the paragraph now in its place (the previous one at the end of the page), so the link always has a paragraph to point to. The model stores the origin as the quote only, so `from.quote` is kept even when no block starts with it (the app resolves the `↩ ¶n` link when drawing). On write, `from.block` is the first block starting with the quote and is omitted when there is none.

**In a project's cuts file** (§3a) the paragraph is in another file. `from.file` names it, relative to the project's root with `/` between folders, and replaces `from.block`; `from.quote` is matched against the starts of that file's blocks when the link is followed. A record may have `from.file` and no `from.quote` (a scrap whose file had nothing left to point to). Readers that don't know `file` treat the record as an ordinary stash whose paragraph can't be found, which only drops the link.

*(impl)* `"paragraph":true` marks a note stashed as whole paragraphs, so dragged back it goes between paragraphs (PRD CUT-10, ED-9). Written only when true, and only on a note with an origin (the field lives on this record). Readers that don't know it keep it as an unknown field.

### 4.5 `doc`

Document-level UI state: `layer` (bool: alternatives layer on or off), and later others such as `scroll`. All fields are optional.

*(impl)* `layer` defaults to `false` (documents open for writing). The record is written only when the layer is on or the record has unknown fields, and never on its own: a document with nothing else to save stays plain Markdown (FILE-2), so the layer setting of such a document isn't saved.

### 4.6 Cuts

PRD §6.5a turns ghosts and scraps into one thing, a **cut**: text taken out of the body that remembers its exact spot, with an optional trace there. `doc-format` reads and writes this since dsky-d8i.1; the model is in ADR-002. The change is **additive within v1**: an older app reads a new file as a page plus scraps whose paragraph links still work.

**A cut is an overflow note (§3) with a `stash` record (§4.4).** The record gains three fields:

| Field | Type | Meaning |
|---|---|---|
| `from.at` | anchor | **The exact spot.** `{"block":n,"offset":k,"before":…,"after":…}` is a point between two characters, with `offset` in chars from the block's source start and up to 16 chars of context on each side. `{"block":n,"gap":true}` is the gap before block `n` (a cut of whole paragraphs); `n` equal to the block count means after the last block. In a project's cuts file it sits beside `from.file`. Inside a group's variant it carries `parent` and `pvariant`, as nested records do (§4.3). |
| `trace` | `true` \| `"text"` \| `"mark"` | **The trace on the page.** `true` draws it as the app's *Show cuts as* setting says; `"text"` or `"mark"` overrides that for this cut. No field: no trace (Cut Without a Trace, or Forget This Spot). |
| `batch` | `{"id":…,"name":…}` | **Cuts made in one step**, shown as one entry in the panel (a Trim: `{"id":"t7q2","name":"Trim −20%"}`). The same object is repeated on each record of the batch. |

`from.quote` and `from.block` stay as they are: they are the paragraph link that older readers use, and the fallback when `from.at` can't be found.

```
<!-- dostoevsky:overflow
His garret was under the roof of a high, five-storied house and was more like a cupboard than a room.
-->

<!-- dostoevsky:meta v1
{"kind":"stash","note":0,"quote":"His garret was under the","from":{"block":2,"quote":"He had successfully avoi","at":{"block":2,"offset":67,"before":"the staircase. ","after":"The landlady who"}},"trace":true}
-->
```

**Finding the spot again** (§5): search for `before + after` with steps 1–3, and the point lies between the two. A `gap` anchor is found by the quotes of the blocks on either side. If the spot can't be found, it is dropped with a warning, and the cut keeps its paragraph link (*place gone* in the app). The cut itself is never dropped.

*(impl)* Details:
- **Points.** An empty side of the context means a line break (or the edge of the text) on that side. Step 2 takes the occurrence nearest the old offset; between steps 2 and 3, one side alone (at least 6 chars) is accepted if it occurs once in the block, so a spot survives an edit to the words on its other side. Step 3 accepts only an occurrence in a block strictly nearer the old one than any other.
- **Gaps.** `{"block":n,"gap":true,"before":…,"after":…}`: `before` is the end of block *n*−1 and `after` the start of block *n*, as `before`/`after` are taken elsewhere (§4.1). The gap is found where both match exactly, nearest to *n*; then where both match as prefix/suffix; then where one side does, if that place is unique. A paragraph cut is written with a gap when it sits right before a block or right after the last one, and with a point anywhere else.
- **Inside a variant** `from.at` holds `parent` and `pvariant` beside `offset`, and `block` is the top-level ancestor's; offsets count in the variant's text without its cuts.
- **A paragraph cut's note** holds its text with the blank line it took along (after it, or before it at the end of the page), so it goes back exactly; `"paragraph":true` is written on every cut of whole paragraphs that has a spot.
- **Offsets count the kept text**: cuts take no room in the body, and variant texts in `alt` records leave cuts out. A version that is nothing but cuts has no text of its own, so it is saved with their text as its own (not a version without the words, `"cut": true`, whose cut is saved as a cut).
- **A version without the words** (§4.2): its cut's record has `parent`/`pvariant` and `offset` 0, and `"version": "showing"` or `"hidden"`, so a project can tell from `cuts.md` whether a closed chapter's Cuts panel lists it (only while it shows). A zero-width group sits before the cuts and comment markers at its spot.
- **The space beside a cut.** A cut from the middle of a line would leave the spaces on both of its sides in the body (`it.  Then`). Where a lone space after the cut would meet a space or a line's start, the body leaves that space out and the record says `"space":"after"`. Where a lone space before it would meet punctuation, or a line's end after prose, the body leaves that one out and the record says `"space":"before"`. The note's text stays as it was cut, and a reader puts the space back on that side of the cut, unless the text there already has one. A reader that doesn't know the field shows the body's single space: nothing is lost.
- **The body's last newline.** The body ends in one newline (§2). A cut among the trailing newlines sits after that newline and takes the others along at the start of its text, so cuts there keep the line breaks between them.
- **Place gone.** A cut whose spot is gone keeps `trace` and has no `from.at`; it reads back as *place gone*. An older app that saves the file rebuilds `from` without `at` (it keeps `trace` and `batch` as unknown fields), so such a cut reads back as *place gone* too: its text is never lost.

**Where cuts are kept:**
- **A single file:** in its own trailer, as scraps are now.
- **A project:** in **`cuts.md`** at the root, which has §3a's layout. The app renames `drawer.md` to `cuts.md` the first time it writes. If both exist, it reads both and writes `cuts.md`.

*(impl)* `parse_cuts` reads a cuts file into the cuts that belong to no file (a document, as a drawer today) and the cuts of each file (`ExternalCut`: the note's text and its records, numbered as note 0). `parse_page(chapter, name, cuts)` reads a chapter with its cuts: those with a spot go back to it, the others become notes whose source is the chapter. `serialize_page` gives back the chapter (its own scraps, the notes with no source, stay in it) and its cuts for the cuts file; `serialize_cuts` writes the cuts file from the cuts of no file and every file's cuts. A chapter's document names its file (`Document::set_cut_source`), so the cuts that lose their spot there stay the project's.

**Reading older files** (PRD CUT-17):
- **A `ghost` record** becomes a cut: its text leaves the body and becomes a note, with `from.at` where it was and `"trace":true`.
- **A group with `"ghost":true`** becomes a cut holding the group.
- **Ghosts inside notes (§4.3a)** become plain text in their note.
- **Scraps** are cuts with no `trace` and no `from.at`.
- **Nothing is written until the next save.** On that save the body loses the ghosted text. In a project the text moves to `cuts.md`, as one save across both files.

*(impl)* Adjacent ghosts and ghosted groups merge into one cut. A ghost that covers part of a group is cut inside the group's active variant; one that covers a group whole holds it. In the body, a ghost of whole paragraphs takes its blank line along, as a cut does, unless that would run into a group or another cut. The second save of a migrated file is the first (property-tested).

**What this improves.** Goal 1 (§1) holds better than before: a ghost's text sat in the body, so another Markdown tool showed it as kept text. A cut's text is never in the body.

The app no longer writes `ghost` records (it still reads them); §4.3 and §4.3a describe them for reading older files.

### 4.7 Comments

PRD §6.5b's **comments** are notes about the text; the model is in ADR-003. The body never changes for them: a comment is one record.

```
{"kind":"comment","at":{"block":1,"offset":44,"before":" His garret was ","after":" than a room.","length":20,"quote":"more like a cupboard"},"text":"Cupboard or closet?","created":"2026-11-12T10:00:00Z"}
```

| Field | Type | Req | Meaning |
|---|---|---|---|
| `at` | anchor | ✓ | §4.1's anchor of the span's start, with `length` (chars of kept text) and `quote` (the span's whole kept text). Inside a version: `parent`/`pvariant` on the record, offsets in that version's text (§4.2). Inside a cut or a note: `at.note` (§4.3a). |
| `text` | string | ✓ | What the comment says. May contain line breaks. |
| `created` | string | – | When it was written, RFC 3339. |
| `author` | string | – | Who wrote it, for a comment from another app (a Word import). No field: the writer. |

*(impl)* Details:
- **Finding it again** works as for an `alt` record (§5 steps 1–4) with `quote` as the text: exact place, same block, whole document, then fuzzy within ±2 blocks. The markers go around what was found. A comment whose quote can't be found is dropped with a warning. One inside a group that is detached stays in that group's records and comes back with it.
- **Order.** Comments are placed after groups and before cuts, so a cut's comments (records with `at.note`) go back to the page with it. A cut at exactly a comment's edge is outside the comment (ADR-003 A2).
- **A project's cuts file** carries a chapter's cut with its comment records, as it carries its groups (§3a).
- **Ids are not saved.** Records follow the sort order of §4, by their anchor.
- Unknown fields are kept, matched to the comment by its `text`.
- An older app keeps a `comment` record as an unknown kind and writes it back. Its anchor may then be stale, and the comment is found again by its quote.

## 5. Loading and re-anchoring

For each record, in order:

1. **Exact match:** the text at `block` + `offset` equals the quote → place it.
2. **Same block:** find every occurrence of the quote in that block and pick the one whose surroundings best match `before`/`after`.
3. **Whole document:** find occurrences in all blocks and score them by context similarity plus distance from the original block. Accept only if the best score is ≥ 0.75 **and** at least 0.1 ahead of the runner-up.
4. **Fuzzy:** a Bitap / diff-match-patch style match within ±2 blocks, threshold 0.8 (handles small outside edits inside the span). If accepted, the active variant's text is replaced with what's actually in the body, so the body always wins.
5. **Paragraph groups:** the block whose text equals the active variant, otherwise the best similarity ≥ 0.6 within ±3 blocks.
6. Otherwise → **detached**: mark it `detached: true` and show it in the Detached tab (PRD FILE-6). Never place a group silently somewhere uncertain, and never delete one.

Overlap rules after placement: partly overlapping `alt` groups → the later one is detached. Ghosts are clipped to the text that still exists.

*(impl)* Details:

- Steps 1–3 also apply to paragraph groups; step 4 to word and sentence groups (and paragraph groups that have an `offset`); step 5 to paragraph groups anchored by `{block}`. Nested records and note records search their own container (the parent's variant text or the note) without step 3.
- Step 2 picks the best context score, then the occurrence nearest the old offset. Context score = mean similarity of `before`/`after` with the text around the candidate (1 − edit distance / longer length; 0.5 for a missing side). Step 3 score = 0.75 × context + 0.25 / (1 + block distance).
- Step 4 uses Sellers' approximate matching (edit distance with free ends) and never accepts an exact occurrence: those were already judged, and possibly rejected as ambiguous, by steps 1–3. A step 4 or 5 match replaces the active variant with the body's text; nested records inside it are re-anchored against the new text.
- Two groups on exactly the same span: the later one is detached. A span strictly inside another one becomes a nested group of that group's active variant. Ghost pieces that fall inside a group move into its active variant.
- Ghosts are found with steps 1–3 by their `quote` (or taken at their offset if they have none); ghosts that can't be found are dropped with a warning. Groups are never dropped: records nested in a detached group are detached with it.
- Records already marked `detached: true` are not re-anchored on load; they stay detached until the user re-attaches them (FILE-6).

## 6. Saving

1. Serialise the body from the model (active variants inline, ghosted text inline).
2. Collect records, compute `before`/`after`, sort them, and escape.
3. Leave out empty blocks. If both are empty, write the body only.
4. Write atomically: a temporary copy → fsync → rename or swap over the original. Keep the file's permissions and extended attributes. *(impl)* The app (`crates/app/src/files.rs`) writes the copy into the system's item-replacement folder for that volume and swaps it in with `NSFileManager replaceItemAtURL`, which keeps permissions, dates and extended attributes and works inside the App Sandbox; it falls back to `<name>.md.tmp-<pid>` beside the file. `doc_format::write_atomic` (no platform code) only does the fallback and doesn't copy extended attributes.

**Round-trip guarantee** (property-tested): `parse(serialize(doc)) == doc` for every valid model, and `serialize(parse(f)) == f` for every file this app wrote. *(impl)* "Valid" means: one-variant groups dissolved and, when a trailer is written, the text ending in exactly one `\n` (§2). Any plain Markdown file round-trips byte for byte.

## 7. Complete example

```markdown
# Keep the Words You Almost Used

Most of the friction in writing comes from choosing too early. You pick a word, the sentence moves on, and the other candidates quietly disappear.

Editors have always known this. They write a suggestion in the margin and let you decide.

Software forgot this. Most editors treat writing as a single line that only moves forward.

<!-- dostoevsky:overflow
Maybe open with the margin metaphor instead?

Writers have always kept notebooks for the lines that didn't fit.
-->

<!-- dostoevsky:meta v1
{"kind":"alt","id":"h1t0p","scope":"sentence","at":{"block":0,"offset":2,"before":"# ","after":""},"active":0,"variants":["Keep the Words You Almost Used","Nothing Gets Deleted Here"]}
{"kind":"alt","id":"k3f9a","scope":"word","at":{"block":1,"offset":12,"before":"Most of the ","after":" in writing comes"},"active":0,"variants":["friction","hesitation","struggle",{"t":"second-guessing","by":"ai"}]}
{"kind":"alt","id":"s9q2m","scope":"sentence","at":{"block":2,"offset":0,"before":"","after":" They write a sug"},"active":0,"variants":["Editors have always known this.","Every good editor knows this."]}
{"kind":"alt","id":"w2n8c","scope":"word","at":{"block":2,"offset":45,"before":"s. They write a ","after":" in the margin an"},"active":0,"variants":["suggestion","note","alternative"]}
{"kind":"alt","id":"p7d1x","scope":"paragraph","at":{"block":3},"active":0,"variants":["Software forgot this. Most editors treat writing as a single line that only moves forward.","Software never learned it. Editors treat text as a line that only moves forward."]}
{"kind":"ghost","at":{"block":1,"offset":127,"length":22,"quote":"quietly disappear."}}
{"kind":"stash","note":1,"quote":"Writers have always kept","from":{"block":2,"quote":"Editors have always kno"}}
-->
```

(Offsets in this example are illustrative. The live mockup's *As saved* tab shows real output from its serializer.)

## 8. Design choices

| Option | Why not |
|---|---|
| Body + an HTML copy + JSON | The HTML doubles the file size and drifts out of sync with the body. |
| CriticMarkup inline (kepano's approach) | `{~~a~>b~~}` only holds *two* versions, clutters the body in every other tool, and has no idea of scope or an active variant. |
| YAML front/end matter | Visible in most renderers. YAML can't escape `-->` reliably, and nesting long paragraphs in it is error-prone. |
| Sidecar file (`doc.md.alts`) | Breaks "one portable file". Easy to lose on copy or upload. |
