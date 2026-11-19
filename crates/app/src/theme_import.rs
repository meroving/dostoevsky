//! Theme files from elsewhere (TECH-NOTES §15): an exported Dostoevsky pair, or one VS Code
//! colour theme turned into a pair. No network, no UI.

use l10n::tr;
use serde_json::Value;

use crate::theme::{self, HexColor, Theme, Variant};

/// An exported Dostoevsky pair or one loose VS Code theme file.
pub fn from_json(text: &str, fallback_name: &str) -> Result<Theme, String> {
    let cleaned = crate::keymap::strip_jsonc(text);
    let value: Value = serde_json::from_str(&cleaned).map_err(|_| tr!("Not a theme file.").to_string())?;
    if value.get("light").is_some() && value.get("dark").is_some() {
        return serde_json::from_value(value).map_err(|_| tr!("Not a theme file.").to_string());
    }
    from_vscode_json(text, fallback_name)
}

/// One loose VS Code theme file, dropped on the import field.
pub fn from_vscode_json(text: &str, fallback_name: &str) -> Result<Theme, String> {
    let json: Value = serde_json::from_str(&crate::keymap::strip_jsonc(text)).map_err(|_| tr!("Not a theme file.").to_string())?;
    let name = json["name"].as_str().unwrap_or(fallback_name).trim().to_string();
    let name = if name.is_empty() { fallback_name.to_string() } else { name };
    let Some((is_light, v)) = vs_variant(&json) else { return Err(tr!("Not a theme file.").to_string()) };
    let (light, dark) = if is_light { (Some(v), None) } else { (None, Some(v)) };
    pair(name, light, dark)
}

fn pair(name: String, light: Option<Variant>, dark: Option<Variant>) -> Result<Theme, String> {
    let (light, dark) = match (light, dark) {
        (Some(l), Some(d)) => (l, d),
        (Some(l), None) => (l.clone(), theme::opposite(&l, false)),
        (None, Some(d)) => (theme::opposite(&d, true), d),
        (None, None) => return Err(tr!("The theme has no colours to take.").to_string()),
    };
    Ok(Theme { name, light, dark })
}

/// The seeds from a VS Code theme's `colors`: paper, ink, accent, and whether it is a light one.
fn vs_variant(json: &Value) -> Option<(bool, Variant)> {
    let colors = json.get("colors")?;
    let paper = take(colors, &["editor.background", "sideBar.background", "panel.background"])?;
    let ink = take(colors, &["editor.foreground", "foreground", "sideBar.foreground"])?;
    // The accent colours text on the page, so the first candidate legible on the paper wins;
    // many themes' focus borders are quiet greys.
    let accent = ACCENTS
        .iter()
        .filter_map(|k| colors[*k].as_str().and_then(lenient))
        .find(|&c| theme::contrast_ratio(c, paper) >= 3.)
        .unwrap_or(if paper_luminous(paper) { 0x5a4fc8 } else { 0x8a80d8 });
    let light = match json["type"].as_str() {
        Some("light") | Some("hc-light") => true,
        Some("dark") | Some("hc-black") => false,
        _ => paper_luminous(paper),
    };
    Some((light, Variant { paper: HexColor(paper), ink: HexColor(ink), accent: HexColor(accent), colors: Default::default() }))
}

const ACCENTS: [&str; 7] = [
    "textLink.foreground",
    "focusBorder",
    "activityBarBadge.background",
    "button.background",
    "editorCursor.foreground",
    "progressBar.background",
    "list.activeSelectionBackground",
];

/// VS Code writes `#rgb`, `#rrggbb` or `#rrggbbaa`.
fn lenient(s: &str) -> Option<u32> {
    let hex = s.strip_prefix('#').unwrap_or(s);
    let expand = |short: &str| short.chars().map(|c| format!("{c}{c}")).collect::<String>();
    match hex.len() {
        3 if hex.chars().all(|c| c.is_ascii_hexdigit()) => u32::from_str_radix(&expand(hex), 16).ok(),
        6 | 8 if hex[..6].chars().all(|c| c.is_ascii_hexdigit()) => u32::from_str_radix(&hex[..6], 16).ok(),
        _ => None,
    }
}

fn take(colors: &Value, keys: &[&str]) -> Option<u32> {
    keys.iter().find_map(|k| colors[k].as_str().and_then(lenient))
}

fn paper_luminous(paper: u32) -> bool {
    let c = |s: u32| ((paper >> s) & 0xff) as f32 / 255.;
    0.2126 * c(16) + 0.7152 * c(8) + 0.0722 * c(0) > 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_sided_file_gets_a_derived_other_half() {
        let theme = from_vscode_json(r##"{"name":"Night Owl","type":"dark","colors":{"editor.background":"#011627","editor.foreground":"#d6deeb"}}"##, "x").unwrap();
        assert_eq!(theme.dark.paper.0, 0x011627);
        assert!(theme::contrast_ratio(theme.light.ink.0, theme.light.paper.0) > 4.5, "the derived half stays readable");
    }

    #[test]
    fn a_dostoevsky_file_round_trips() {
        let original = theme::builtin("aurora").unwrap();
        let text = serde_json::to_string(&original).unwrap();
        assert_eq!(from_json(&text, "fallback").unwrap(), original);
    }

    #[test]
    fn vscode_jsonc_becomes_a_pair() {
        let text = r##"{
            // Comments and trailing commas are normal in VS Code's theme files.
            "name": "Morning",
            "type": "light",
            "colors": {
                "editor.background": "#f8f5f0",
                "editor.foreground": "#303030ff",
                "focusBorder": "#5678ab",
            },
        }"##;
        let theme = from_json(text, "fallback").unwrap();
        assert_eq!((theme.name.as_str(), theme.light.paper.0, theme.light.accent.0), ("Morning", 0xf8f5f0, 0x5678ab));
        assert_eq!(theme.light.ink.0, 0x303030);
        assert!(theme.dark.paper.0 < theme.light.paper.0, "the missing dark half must actually be dark");
        assert!(theme::contrast_ratio(theme.dark.ink.0, theme.dark.paper.0) > 4.5);
    }

    #[test]
    fn a_quiet_focus_border_gives_way_to_a_legible_accent() {
        let theme = from_vscode_json(
            r##"{"type":"dark","colors":{"editor.background":"#282c34","editor.foreground":"#abb2bf","focusBorder":"#3e4452","button.background":"#4d78cc"}}"##,
            "x",
        )
        .unwrap();
        assert_eq!(theme.dark.accent.0, 0x4d78cc);
    }

    #[test]
    fn high_contrast_light_is_a_light_half() {
        let theme = from_vscode_json(r##"{"type":"hc-light","colors":{"editor.background":"#ffffff","editor.foreground":"#000000"}}"##, "HC").unwrap();
        assert_eq!((theme.light.paper.0, theme.light.ink.0), (0xffffff, 0x000000));
    }

    #[test]
    fn short_and_padded_hexes_are_taken() {
        assert_eq!(lenient("#abc"), Some(0xaabbcc));
        assert_eq!(lenient("#011627ff"), Some(0x011627));
        assert_eq!(lenient("nope"), None);
    }
}
