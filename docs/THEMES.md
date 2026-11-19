# Themes

A Dostoevsky theme is a **pair**: a light half and a dark half. The Theme row in Preferences →
General (or View → Theme) picks the theme; the Appearance setting right above it (Dark, Light,
Match macOS, ⌃⌘L) picks which half shows. Each tile shows both halves, split by a slant.
Implemented in `crates/app/src/theme.rs` (files, derivation), `theme_ui.rs` (the Theme row and
the New / Edit theme sheet) and `theme_import.rs` (reading Dostoevsky and VS Code files).

## Built-ins

| id | Light half (paper / ink / accent) | Dark half (paper / ink / accent) |
|---|---|---|
| `dostoevsky` | the original light palette, every token pinned | the original dark palette, every token pinned |
| `petersburg` | `#f3f4f2` / `#2a3440` / `#3f6fa6` | `#10151d` / `#c7d2df` / `#7aa2e8` |
| `manuscript` | `#efe4cb` / `#3f3022` / `#9a4a1c` | `#1c160f` / `#e8d4b0` / `#d98b52` |
| `white-nights` | `#ffffff` / `#111318` / `#3c37b5` | `#000000` / `#f4f4f6` / `#aaa5ff` |
| `aurora` | `#fdf7fd` / `#3d2b3d` / `#be1e66` | `#170d16` / `#f0d7e8` / `#f472b6` |

Every built-in reads at 4.5 : 1 or more, ink and accent on paper, in both halves (a unit test
checks it).

Built-ins can't be edited or deleted; right-click → *Edit a Copy…* opens a copy in the editor.
A theme of your own: double-click to edit; right-click for Edit…, Duplicate, Show in Finder and
Delete…. A copy of a built-in
keeps only its three seeds per half, so the editor's seeds move every derived colour (Dostoevsky's
copy therefore looks close to, not exactly like, the original). Preferences saved before themes
existed read as `"theme": "dostoevsky"`, so nothing changes for them.

## File format

One theme per file: `~/Library/Application Support/Dostoevsky/Themes/<id>.json`
(`DOSTOEVSKY_SUPPORT_DIR` moves the folder). The id is the file name; preferences store it as
`theme`. The app reads the folder at launch and after it saves or deletes a theme.

```json
{
  "name": "Blue Hour",
  "light": {
    "paper": "#fff7ed",
    "ink": "#2a2f3a",
    "accent": "#3f6fa6",
    "colors": { "bg-panel": "#f0e5d8" }
  },
  "dark": {
    "paper": "#101827",
    "ink": "#e6d5c3",
    "accent": "#7aa2e8",
    "colors": {}
  }
}
```

- Colours are `"#rrggbb"` strings; anything else makes the file unreadable, and the gallery skips it.
- Every field is optional: a missing half is Dostoevsky's, a missing seed takes Dostoevsky's
  light value.
- `colors` overrides single tokens by name (below). Unknown names are ignored, so files made by a
  newer version still open.
- The app saves exactly this; *Show in Finder* on a theme finds the file to share, and
  *Import a theme file…* in the New theme sheet reads it back.

## Three seeds, ~40 tokens

Each half has three seeds: **paper** (`bg`), **ink** (`text`) and **accent** (`accent`). Every
other token is mixed from them in sRGB (GPUI blends there too):

- **Neutrals** sit at fixed points between paper and ink: `bg-panel`, `line`, `text-2`, `muted`,
  `faint`, `placeholder`, `para-hl`, `bullet`, `icon*`, `counter*`, `menu-*`, `note-text`, … The
  light and dark halves use slightly different points.
- **From the accent:** `pen`, `menu-badge-text` (the accent mixed toward paper).
- **Fixed roles** keep Dostoevsky's colours: `danger`, `mark`, `comment`, `success`, `para-bar`,
  `original`, `origin-*`. They move toward the ink only when they would read below 3 : 1 on the
  paper (1.5 : 1 for the origin lines).
- Ghost opacity keeps matching its contrast on any paper (`matched_alpha`).

Token names: `bg`, `bg-panel`, `bg-raised`, `line`, `text`, `text-2`, `muted`, `faint`, `accent`,
`pen`, `para-bar`, `para-hl`, `danger`, `mark`, `comment`, `success`, `bullet`, `placeholder`,
`original`, `icon`, `icon-hover`, `icon-line`, `counter`, `counter-hover`, `menu-text`,
`menu-key`, `menu-disabled`, `menu-hover`, `menu-sep`, `menu-border`, `menu-badge-line`,
`menu-badge-text`, `note-text`, `origin-line`, `origin-text`, `origin-hover-line`,
`origin-hover-text`, `wash`, `shadow`.

The editor sets only the three seeds of each half. To override single tokens, edit `colors` in
the file; the editor keeps them when it saves. It marks ink or accent "hard to read" below
4.5 : 1 (WCAG 2 AA for normal text).

## VS Code theme files

*Import a theme file…* takes a Dostoevsky `.json` or one VS Code colour theme `.json` (comments
and trailing commas allowed). The app doesn't search or download themes.

| Seed | VS Code colours |
|---|---|
| paper | `editor.background`, `sideBar.background`, `panel.background` |
| ink | `editor.foreground`, `foreground`, `sideBar.foreground` |
| accent | the first of `textLink.foreground`, `focusBorder`, `activityBarBadge.background`, `button.background`, `editorCursor.foreground`, `progressBar.background`, `list.activeSelectionBackground` that reads at 3 : 1 or more on the paper (else a violet) |

`#rgb` and `#rrggbbaa` are accepted (alpha dropped). The half comes from `type` (`light` / `hc-light`
or `dark` / `hc-black`), else from the paper's luminance. A VS Code file brings one half; the
other is derived: paper and ink pushed most of the way to the opposite end, accent kept.
