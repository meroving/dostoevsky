# gpui-pre-windows 0.3.9, patched

A copy of [`gpui-pre-windows` 0.3.9](https://crates.io/crates/gpui-pre-windows) (Apache-2.0,
`LICENSE-APACHE`), used through `[patch.crates-io]` in the workspace `Cargo.toml`. It is the
crate as published with DirectWrite's glyph runs placed where DirectWrite puts them, because
as published Hebrew and Arabic come out mirrored on Windows (dsky-fbso.11). Each change is
marked `dostoevsky patch` in `src/direct_write.rs`.

| Was | Now | What changes |
|---|---|---|
| `DrawGlyphRun` places each run's glyphs after the previous run's, left to right in string order, and ignores `baselineOriginX` and the bidi level | glyphs start at `baselineOriginX`; in a right-to-left run (odd bidi level) they go leftwards from it, then are stored left to right | Hebrew and Arabic read right to left, and a right-to-left stretch's words are in the right order. The layout holds them as CoreText's does on the Mac (left to right, string indices falling within a right-to-left run), so the caret, selection and clicks behave as there. |
| the line's width is the sum of the advances | the right end of the rightmost run | The same for left-to-right text. |
| `LineLayout::runs` in the order DirectWrite drew them | sorted left to right | Code that walks glyphs by x (`x_for_index`, `closest_index_for_x`) sees them in order. |
| `StringIndexConverter::advance_to_utf16_ix` only moves forward | starts over for an earlier index | A run drawn before an earlier one in the string still gets its own string indices. |

`direct_write::tests::right_to_left_text_reads_right_to_left` checks it:
`cargo test -p gpui-pre-windows right_to_left` (the crate isn't a workspace member, so
`cargo test --workspace` doesn't run it). The crate's own `keyboard::tests::test_keyboard_mapper`
fails when the current keyboard layout isn't US English; that's upstream's, not this patch's.

## Upgrading GPUI

When `gpui-kit` moves to a newer `gpui-pre`, bring its `gpui-pre-windows` source into a folder
under `target/`, apply the changes above to this folder (keep this file), bump the version in
its `Cargo.toml`, and run the test above. If upstream places right-to-left runs itself, remove
the folder and the `[patch.crates-io]` entry instead.
