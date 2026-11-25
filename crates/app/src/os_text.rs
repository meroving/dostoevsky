//! What the app says differently on each system (dsky-fbso.2): keys written as the Mac draws
//! them ("⇧⌘A") shown as this system presses them ("Ctrl+Shift+A").

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

#[cfg(test)]
mod tests {
    use super::pc_key;

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
