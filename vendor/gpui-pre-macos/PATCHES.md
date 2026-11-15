# gpui-pre-macos 0.3.9, patched

A copy of [`gpui-pre-macos` 0.3.9](https://crates.io/crates/gpui-pre-macos) (Apache-2.0,
`LICENSE-APACHE`), used through `[patch.crates-io]` in the workspace `Cargo.toml`. It is the
crate as published with its private AppKit API taken out, because Apple's binary checks
reject apps that name private selectors (dsky-a8n.19). Each change is marked
`dostoevsky patch` in `src/window.rs` (and one in `src/platform.rs`).

| Was | Now | What changes |
|---|---|---|
| `GPUIView` overrides `_opaqueRectForWindowMoveWhenInTitlebar` | no override | Nothing for Dostoevsky: the override only mattered for windows with `app_owns_titlebar_drag`, which Dostoevsky doesn't set (the header moves and zooms the window itself). For such windows AppKit would drag from the titlebar again. |
| `GPUIWindowStateKeyedUnarchiver` overrides `_windowRestorationOptions`, makes a `NSWindowRestorationOptions` | plain subclass | `Window::restore_native_window_state` no longer puts a window back on its Space on macOS 15+. Dostoevsky doesn't call it. |
| titlebar double-click "Fill" sends `_zoomFill:` | `zoom:` | Fill ignores "Tiled windows have margins". Dostoevsky's header calls `zoom_window()` itself. |
| `setWindowsMenu:` for the menu named exactly `Window` | also for `l10n::tr("Window")` | The Window menu keeps macOS's window list when the menus are translated (dsky-44a). Not about private API: the crate depends on `crates/l10n` for it. |
| `CursorStyle::ResizeUpLeftDownRight` / `ResizeUpRightDownLeft` use `_windowResize…Cursor` | `+[NSCursor frameResizeCursorFromPosition:inDirections:]` (public, macOS 15+), the arrow before | Dostoevsky never asks for these cursors. |

Also: `[lints.rust.deprecated]` is allowed in `Cargo.toml` (from the registry the crate's
warnings were capped; as a path dependency they'd show), and the now-unread
`app_owns_titlebar_drag` field is `#[allow(dead_code)]`.

`scripts/check_private_api.sh` fails if a private selector is back in the binary; the release
workflow runs it on the signed universal app.

## Upgrading GPUI

When `gpui-kit` moves to a newer `gpui-pre`, bring its `gpui-pre-macos` source into a folder
under `target/`, apply the changes above to this folder (keep this file), bump the version in
its `Cargo.toml`, and run
`scripts/check_private_api.sh` on a release build. If upstream has dropped the private calls,
remove the folder and the `[patch.crates-io]` entry instead.
