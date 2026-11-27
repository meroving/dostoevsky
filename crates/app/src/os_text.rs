//! What the app says differently on each system (dsky-fbso.2): keys written as the Mac draws
//! them ("⇧⌘A") shown as this system presses them ("Ctrl+Shift+A"), and each system's own words
//! for its things (Finder, File Explorer, the file manager; the Trash, the Recycle Bin;
//! Keychain, Credential Manager, the keyring). Whole phrases, so each language can inflect them.

use l10n::tr;

/// A key, keys or a modifier as the Mac writes them: "⌘Z", "⇧⌘A", "⌥↑↓", "↩", "⌥". Unchanged on
/// the Mac; elsewhere through the keymap's own translation (`actions::off_mac`).
pub fn key(mac: &str) -> String {
    if cfg!(target_os = "macos") {
        mac.to_string()
    } else {
        pc_key(mac)
    }
}

/// [`key`] as Windows and Linux write it, on any system (for tests).
pub fn pc_key(mac: &str) -> String {
    const MODS: [(char, &str, &str); 4] = [('⌃', "ctrl-", "Ctrl"), ('⌥', "alt-", "Alt"), ('⇧', "shift-", "Shift"), ('⌘', "cmd-", "Ctrl")];
    let mut mods = String::new();
    let mut names = Vec::new();
    let mut rest = mac;
    while let Some((m, name, r)) = MODS.iter().find_map(|(g, m, name)| rest.strip_prefix(*g).map(|r| (m, name, r))) {
        mods.push_str(m);
        names.push(*name);
        rest = r;
    }
    if rest.is_empty() {
        // A modifier on its own: "with ⌥ held".
        return names.join("+");
    }
    let mut chars = rest.chars();
    let first = chars.next().unwrap_or_default();
    let after = chars.as_str();
    let named = match first {
        '↑' => Some("up"),
        '↓' => Some("down"),
        '←' => Some("left"),
        '→' => Some("right"),
        '↩' => Some("enter"),
        '⌫' => Some("backspace"),
        '⌦' => Some("delete"),
        '⇥' => Some("tab"),
        '−' => Some("-"),
        _ => None,
    };
    if mods.is_empty() && !matches!(first, '↩' | '⌫' | '⌦' | '⇥') {
        // Arrows, letters, "esc", "1–9": the same everywhere.
        return mac.to_string();
    }
    let lower = first.to_lowercase().to_string();
    let k = named.unwrap_or(&lower);
    let Some(pc) = crate::actions::off_mac(&format!("{mods}{k}")) else { return mac.to_string() };
    let Ok(stroke) = gpui_kit::Keystroke::parse(&pc) else { return mac.to_string() };
    format!("{}{after}", crate::context_menu::pc_keystroke(&stroke.modifiers, &stroke.key))
}

/// The system the app runs on, as far as its words go.
#[derive(Clone, Copy, PartialEq)]
enum Os {
    Mac,
    Windows,
    Linux,
}

fn os() -> Os {
    if cfg!(target_os = "macos") {
        Os::Mac
    } else if cfg!(target_os = "windows") {
        Os::Windows
    } else {
        Os::Linux
    }
}

/// Where the sidebar's foot and the context menu reveal a file: Finder, File Explorer, the
/// file manager.
pub fn show_in_file_manager() -> &'static str {
    match os() {
        Os::Mac => tr!("Show in Finder"),
        Os::Windows => tr!("Show in File Explorer"),
        Os::Linux => tr!("Show in the file manager"),
    }
}

/// The Project sheet's note on the `.dostoevsky` file: the type, hidden in Finder and in Linux's
/// file managers, which hide dot-files. File Explorer doesn't, so there the note only names it.
pub fn file_type_hidden_in_file_manager() -> &'static str {
    file_type_note(os())
}

fn file_type_note(os: Os) -> &'static str {
    match os {
        Os::Mac => tr!("the type, hidden in Finder"),
        Os::Windows => tr!("the project’s type"),
        Os::Linux => tr!("the type, hidden in the file manager"),
    }
}

/// The command's name: the Trash on the Mac and Linux, the Recycle Bin on Windows.
pub fn move_to_trash() -> &'static str {
    match os() {
        Os::Windows => tr!("Move to Recycle Bin"),
        _ => tr!("Move to Trash"),
    }
}

pub fn couldn_move_to_trash(error: &dyn std::fmt::Display) -> String {
    match os() {
        Os::Windows => tr!("Couldn't move it to the Recycle Bin: {error}", error = error),
        _ => tr!("Couldn't move it to the Trash: {error}", error = error),
    }
}

pub fn moved_to_trash(undo: &str) -> String {
    match os() {
        Os::Windows => tr!("Moved to the Recycle Bin. {undo} to undo", undo = undo),
        _ => tr!("Moved to the Trash. {undo} to undo", undo = undo),
    }
}

pub fn brought_back_from_trash() -> &'static str {
    match os() {
        Os::Windows => tr!("Brought back from the Recycle Bin"),
        _ => tr!("Brought back from the Trash"),
    }
}

/// The AI privacy lines: "this Mac" on the Mac, "this computer" elsewhere.
pub fn ai_off_privacy() -> &'static str {
    match os() {
        Os::Mac => tr!("AI is off. Nothing leaves this Mac."),
        _ => tr!("AI is off. Nothing leaves this computer."),
    }
}

pub fn both_models_local_privacy() -> &'static str {
    match os() {
        Os::Mac => tr!("Both models run on this Mac. Nothing leaves it."),
        _ => tr!("Both models run on this computer. Nothing leaves it."),
    }
}

pub fn quick_local_privacy() -> &'static str {
    match os() {
        Os::Mac => tr!("Quick stays on this Mac. Deep sends text out when you ask."),
        _ => tr!("Quick stays on this computer. Deep sends text out when you ask."),
    }
}

pub fn deep_local_privacy() -> &'static str {
    match os() {
        Os::Mac => tr!("Deep stays on this Mac. Quick sends text out when you ask."),
        _ => tr!("Deep stays on this computer. Quick sends text out when you ask."),
    }
}

pub fn cloud_privacy() -> &'static str {
    match os() {
        Os::Mac => tr!("Text leaves this Mac only when you ask for something."),
        _ => tr!("Text leaves this computer only when you ask for something."),
    }
}

/// The Project sheet's note on where a document without a folder lives.
pub fn kept_on_this_computer() -> &'static str {
    match os() {
        Os::Mac => tr!("kept on this Mac"),
        _ => tr!("kept on this computer"),
    }
}

/// Where the AI service's key is kept: Keychain, Credential Manager, the keyring.
pub fn key_in_keyring() -> &'static str {
    match os() {
        Os::Mac => tr!("key in Keychain"),
        Os::Windows => tr!("key in Credential Manager"),
        Os::Linux => tr!("key in the keyring"),
    }
}

pub fn in_keyring() -> &'static str {
    match os() {
        Os::Mac => tr!("in Keychain"),
        Os::Windows => tr!("in Credential Manager"),
        Os::Linux => tr!("in the keyring"),
    }
}

/// The menu item that ends the app: Quit Dostoevsky in the Mac's app menu, Exit at the end of
/// Windows' File menu, Quit in Linux's.
pub fn quit() -> &'static str {
    match os() {
        Os::Mac => tr!("Quit Dostoevsky"),
        Os::Windows => tr!("Exit"),
        Os::Linux => tr!("Quit"),
    }
}

/// The appearance option that follows the system: macOS, Windows, the system.
pub fn match_system() -> &'static str {
    match os() {
        Os::Mac => tr!("Match macOS"),
        Os::Windows => tr!("Match Windows"),
        Os::Linux => tr!("Match the system"),
    }
}

#[cfg(test)]
mod tests {
    use super::{Os, file_type_note, pc_key};

    #[test]
    fn only_where_dot_files_are_hidden_does_the_note_say_so() {
        // A ".dostoevsky" file is not hidden in File Explorer: only the hidden attribute hides there.
        assert_eq!(file_type_note(Os::Windows), "the project’s type");
        assert!(file_type_note(Os::Mac).contains("hidden"));
        assert!(file_type_note(Os::Linux).contains("hidden"));
    }

    #[test]
    fn mac_keys_as_windows_and_linux_write_them() {
        let cases = [
            ("⌘Z", "Ctrl+Z"),
            ("⇧⌘A", "Ctrl+Shift+A"),
            ("⌥⌘M", "Alt+M"),
            ("⌥⌘↩", "Alt+Enter"),
            ("⌘⌫", "Ctrl+Shift+Backspace"),
            ("⇧↩", "Shift+Enter"),
            ("⌥↑↓", "Ctrl+↑↓"),
            ("⌘/", "Ctrl+/"),
            ("↩", "Enter"),
            ("⇥", "Tab"),
            ("⌫", "Backspace"),
            ("↑↓", "↑↓"),
            ("K", "K"),
            ("esc", "esc"),
            ("1–9", "1–9"),
            ("⌥", "Alt"),
            ("⇧", "Shift"),
        ];
        for (mac, pc) in cases {
            assert_eq!(pc_key(mac), pc, "{mac}");
        }
    }
}
