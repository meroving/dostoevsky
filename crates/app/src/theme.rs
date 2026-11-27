use gpui_kit::*;

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use crate::prefs::{Appearance, HandFont, Prefs};

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: u32,
    pub bg_panel: u32,
    pub bg_raised: u32,
    pub line: u32,
    pub text: u32,
    pub text_2: u32,
    pub muted: u32,
    pub faint: u32,
    pub accent: u32,
    pub pen: u32,
    pub para_bar: u32,
    pub para_hl: u32,
    pub danger: u32,
    /// Amber, never the alternatives' violet.
    pub mark: u32,
    /// A grey teal no other tool uses.
    pub comment: u32,
    pub success: u32,
    pub bullet: u32,
    pub placeholder: u32,
    pub original: u32,
    pub icon: u32,
    pub icon_hover: u32,
    pub icon_line: u32,
    pub counter: u32,
    pub counter_hover: u32,
    pub menu_text: u32,
    pub menu_key: u32,
    pub menu_disabled: u32,
    pub menu_hover: u32,
    pub menu_sep: u32,
    pub menu_border: u32,
    pub menu_badge_line: u32,
    pub menu_badge_text: u32,
    pub note_text: u32,
    pub origin_line: u32,
    pub origin_text: u32,
    pub origin_hover_line: u32,
    pub origin_hover_text: u32,
    /// Drawn at 3%.
    pub wash: u32,
    pub shadow: u32,
    pub shadow_alpha: f32,
}

pub const DARK: Palette = Palette {
    bg: 0x070c1f,
    bg_panel: 0x0b111d,
    bg_raised: 0x121622,
    line: 0x1a2033,
    text: 0xf5cab3,
    text_2: 0xa99f9f,
    muted: 0x79737d,
    faint: 0x3a4058,
    accent: 0x8a80d8,
    pen: 0x545278,
    para_bar: 0xa8735a,
    para_hl: 0x141b33,
    danger: 0xe0604a,
    mark: 0xd6a35c,
    comment: 0x6fb2b8,
    success: 0x7fbf8e,
    bullet: 0x585c66,
    placeholder: 0x2c3248,
    original: 0xc99a7e,
    icon: 0x64626c,
    icon_hover: 0x9a96a4,
    icon_line: 0x4a4954,
    counter: 0x2a2f44,
    counter_hover: 0x5a6080,
    menu_text: 0xd3c7c7,
    menu_key: 0x75727c,
    menu_disabled: 0x4d5268,
    menu_hover: 0x26232c,
    menu_sep: 0x1f2538,
    menu_border: 0x232a40,
    menu_badge_line: 0x393f5e,
    menu_badge_text: 0x7d82a8,
    note_text: 0xe0d2d1,
    origin_line: 0x3a2f33,
    origin_text: 0x8a6a5a,
    origin_hover_line: 0x6a4a3e,
    origin_hover_text: 0xe0a585,
    wash: 0xffffff,
    shadow: 0x000000,
    shadow_alpha: 0.55,
};

pub const LIGHT: Palette = Palette {
    bg: 0xf5f0e6,
    bg_panel: 0xeee7d9,
    bg_raised: 0xfbf8f2,
    line: 0xe0d7c6,
    text: 0x2b2733,
    text_2: 0x6c6370,
    muted: 0x8d8590,
    faint: 0xc3baab,
    accent: 0x5a4fc8,
    pen: 0xaba4de,
    para_bar: 0xc4895f,
    para_hl: 0xebe3d2,
    danger: 0xc8442e,
    mark: 0xa8701e,
    comment: 0x267d84,
    success: 0x3f8a52,
    bullet: 0xb5ada2,
    placeholder: 0xd2cabb,
    original: 0xa55f39,
    icon: 0x9b959d,
    icon_hover: 0x5e5866,
    icon_line: 0xcbc4b9,
    counter: 0xd0c7b7,
    counter_hover: 0x8d8590,
    menu_text: 0x3a3540,
    menu_key: 0x9a929c,
    menu_disabled: 0xc3baab,
    menu_hover: 0xefe8db,
    menu_sep: 0xe7dfd0,
    menu_border: 0xe0d7c6,
    menu_badge_line: 0xd6cde0,
    menu_badge_text: 0x7a72b0,
    note_text: 0x3a3540,
    origin_line: 0xe3d3c4,
    origin_text: 0xa07a62,
    origin_hover_line: 0xc9a58a,
    origin_hover_text: 0xa55f39,
    wash: 0x000000,
    shadow: 0x3c301e,
    shadow_alpha: 0.18,
};

thread_local! {
    /// Per thread, so parallel UI tests don't share it.
    static LIGHT_ON: Cell<bool> = const { Cell::new(false) };
    /// Last handed to AppKit, so a window's appearance callback can't loop.
    static FORCED: Cell<Option<Appearance>> = const { Cell::new(None) };
    static OPACITY: Cell<f32> = const { Cell::new(1.) };
    static HAND: Cell<HandFont> = const { Cell::new(HandFont::BadScript) };
}

pub fn hand() -> &'static str {
    hand_font().family()
}

pub fn hand_font() -> HandFont {
    HAND.with(Cell::get)
}

/// Notes in the right and left panels (comments, cuts, alternatives), wherever they show: in type as REVIEW's are, a step larger
/// (mockup comments-type; the hand was hard to read).
pub const NOTE_PX: f32 = 12.5;
pub const NOTE_LINE: f32 = 18.;

/// `caveat_px` is the size in Caveat.
pub fn hand_px(caveat_px: f32) -> Pixels {
    px(caveat_px * hand_font().scale())
}

/// Text stays readable on any desktop.
pub const MIN_OPACITY: f32 = 0.5;

pub fn opacity() -> f32 {
    OPACITY.with(Cell::get)
}

pub fn glass() -> bool {
    opacity() < 1.
}

/// For the page and panels; menus, sheets and chips stay opaque.
pub fn surface(hex: u32) -> Hsla {
    ca(hex, opacity())
}

pub fn window_background(opacity: f32) -> WindowBackgroundAppearance {
    if opacity < 1. { WindowBackgroundAppearance::Blurred } else { WindowBackgroundAppearance::Opaque }
}

pub fn palette() -> Palette {
    RESOLVED.with(|r| if is_light() { r.borrow().0 } else { r.borrow().1 })
}

/// The current theme's two halves: (light, dark).
pub fn resolved() -> (Palette, Palette) {
    RESOLVED.with(|r| *r.borrow())
}

pub fn is_light() -> bool {
    LIGHT_ON.with(Cell::get)
}

macro_rules! tokens {
    ($($name:ident),* $(,)?) => {
        $(
            #[doc = concat!("The current theme's `", stringify!($name), "`.")]
            pub fn $name() -> u32 {
                palette().$name
            }
        )*
    };
}

tokens!(
    bg, bg_panel, bg_raised, line, text, text_2, muted, faint, accent, pen, para_bar, para_hl, danger, mark, comment, success, bullet, placeholder,
    original, icon, icon_hover, icon_line, counter, counter_hover, menu_text, menu_key, menu_disabled, menu_hover, menu_sep,
    menu_border, menu_badge_line, menu_badge_text, note_text, origin_line, origin_text, origin_hover_line, origin_hover_text,
    wash, shadow,
);

/// Every colour in a theme file, written as `#rrggbb`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HexColor(pub u32);

impl HexColor {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        let hex = s.strip_prefix('#')?;
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        u32::from_str_radix(hex, 16).ok().map(HexColor)
    }
}

impl std::fmt::Display for HexColor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{:06x}", self.0)
    }
}

impl serde::Serialize for HexColor {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for HexColor {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        HexColor::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("not a #rrggbb colour: {s}")))
    }
}

/// Tokens by their file name, for the editor's Advanced list and theme files' `colors`.
macro_rules! tokens_by_name {
    ($($name:literal => $field:ident),* $(,)?) => {
        impl Palette {
            pub const NAMES: &'static [&'static str] = &[$($name),*];

            pub fn get(&self, name: &str) -> Option<u32> {
                Some(match name { $($name => self.$field,)* _ => return None })
            }

            /// `false` when `name` is not a token.
            pub fn set(&mut self, name: &str, hex: u32) -> bool {
                match name { $($name => self.$field = hex,)* _ => return false }
                true
            }
        }
    };
}

tokens_by_name! {
    "bg" => bg, "bg-panel" => bg_panel, "bg-raised" => bg_raised, "line" => line,
    "text" => text, "text-2" => text_2, "muted" => muted, "faint" => faint,
    "accent" => accent, "pen" => pen, "para-bar" => para_bar, "para-hl" => para_hl,
    "danger" => danger, "mark" => mark, "comment" => comment, "success" => success,
    "bullet" => bullet, "placeholder" => placeholder, "original" => original,
    "icon" => icon, "icon-hover" => icon_hover, "icon-line" => icon_line,
    "counter" => counter, "counter-hover" => counter_hover,
    "menu-text" => menu_text, "menu-key" => menu_key, "menu-disabled" => menu_disabled,
    "menu-hover" => menu_hover, "menu-sep" => menu_sep, "menu-border" => menu_border,
    "menu-badge-line" => menu_badge_line, "menu-badge-text" => menu_badge_text,
    "note-text" => note_text, "origin-line" => origin_line, "origin-text" => origin_text,
    "origin-hover-line" => origin_hover_line, "origin-hover-text" => origin_hover_text,
    "wash" => wash, "shadow" => shadow,
}

/// One half of a theme: three seeds every colour derives from, and any token overrides.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Variant {
    pub paper: HexColor,
    pub ink: HexColor,
    pub accent: HexColor,
    /// Token overrides by `Palette::NAMES` name; anything left out is derived.
    pub colors: BTreeMap<String, HexColor>,
}

impl Default for Variant {
    fn default() -> Self {
        Variant { paper: HexColor(LIGHT.bg), ink: HexColor(LIGHT.text), accent: HexColor(LIGHT.accent), colors: BTreeMap::new() }
    }
}

impl Variant {
    pub fn palette(&self, light: bool) -> Palette {
        let mut p = derived(self, light);
        for (name, hex) in &self.colors {
            p.set(name, hex.0);
        }
        p
    }

    /// A variant that pins every token, so nothing is derived.
    pub fn full(p: &Palette) -> Variant {
        Variant {
            paper: HexColor(p.bg),
            ink: HexColor(p.text),
            accent: HexColor(p.accent),
            colors: Palette::NAMES.iter().filter_map(|n| p.get(n).map(|h| (n.to_string(), HexColor(h)))).collect(),
        }
    }
}

/// A theme: a light half and a dark half; the Appearance preference picks which one shows.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Theme {
    pub name: String,
    pub light: Variant,
    pub dark: Variant,
}

impl Default for Theme {
    fn default() -> Self {
        builtin("dostoevsky").expect("the id is listed in BUILTINS")
    }
}

fn mix(a: u32, b: u32, t: f32) -> u32 {
    let channel = |s: u32| {
        let (a, b) = (((a >> s) & 0xff) as f32, ((b >> s) & 0xff) as f32);
        ((a + (b - a) * t).round() as u32) << s
    };
    channel(16) | channel(8) | channel(0)
}

/// The seeds' neutral and accent families; fixed roles move only when their paper hides them.
/// GPUI blends in sRGB, so the mixes are taken there too.
fn derived(v: &Variant, light: bool) -> Palette {
    let (paper, ink, accent) = (v.paper.0, v.ink.0, v.accent.0);
    let m = |t: f32| mix(paper, ink, t);
    let base = if light { &LIGHT } else { &DARK };
    Palette {
        bg: paper,
        text: ink,
        accent,
        bg_panel: if light { m(0.04) } else { m(0.035) },
        bg_raised: if light { mix(paper, 0xffffff, 0.5) } else { m(0.07) },
        line: if light { m(0.10) } else { m(0.12) },
        text_2: m(0.68),
        muted: if light { m(0.50) } else { m(0.44) },
        faint: if light { m(0.25) } else { m(0.20) },
        pen: mix(accent, paper, if light { 0.5 } else { 0.45 }),
        para_hl: if light { m(0.08) } else { m(0.06) },
        placeholder: if light { m(0.17) } else { m(0.15) },
        icon: m(0.38),
        icon_hover: m(0.60),
        icon_line: m(0.25),
        counter: if light { m(0.17) } else { m(0.10) },
        counter_hover: m(0.35),
        bullet: m(0.20),
        menu_text: if light { m(0.90) } else { m(0.80) },
        menu_key: if light { m(0.48) } else { m(0.30) },
        menu_disabled: if light { m(0.25) } else { m(0.20) },
        menu_hover: if light { m(0.05) } else { m(0.09) },
        menu_sep: if light { m(0.12) } else { m(0.13) },
        menu_border: if light { m(0.10) } else { m(0.14) },
        menu_badge_line: m(0.15),
        menu_badge_text: mix(accent, paper, 0.3),
        note_text: if light { m(0.90) } else { m(0.86) },
        wash: if light { 0x000000 } else { 0xffffff },
        shadow: if light { mix(paper, 0x000000, 0.75) } else { 0x000000 },
        shadow_alpha: if light { 0.18 } else { 0.55 },
        danger: legible(base.danger, paper, ink, light, 3.),
        mark: legible(base.mark, paper, ink, light, 3.),
        comment: legible(base.comment, paper, ink, light, 3.),
        success: legible(base.success, paper, ink, light, 3.),
        para_bar: legible(base.para_bar, paper, ink, light, 3.),
        original: legible(base.original, paper, ink, light, 3.),
        origin_line: legible(base.origin_line, paper, ink, light, 1.5),
        origin_text: legible(base.origin_text, paper, ink, light, 3.),
        origin_hover_line: legible(base.origin_hover_line, paper, ink, light, 1.5),
        origin_hover_text: legible(base.origin_hover_text, paper, ink, light, 3.),
    }
}

fn legible(color: u32, paper: u32, ink: u32, light: bool, minimum: f32) -> u32 {
    if contrast_ratio(color, paper) >= minimum {
        return color;
    }
    let fallback = if light { 0x000000 } else { 0xffffff };
    let target = if contrast_ratio(ink, paper) > contrast_ratio(color, paper) { ink } else { fallback };
    if contrast_ratio(target, paper) <= contrast_ratio(color, paper) {
        return color;
    }
    let (mut lo, mut hi) = (0f32, 1f32);
    for _ in 0..16 {
        let mid = (lo + hi) / 2.;
        if contrast_ratio(mix(color, target, mid), paper) < minimum { lo = mid } else { hi = mid }
    }
    mix(color, target, hi)
}

/// WCAG 2.x contrast; the editor warns under AA (4.5 : 1).
pub fn contrast_ratio(fg: u32, bg: u32) -> f32 {
    let lum = |hex: u32| {
        let c = |s: u32| ((hex >> s) & 0xff) as f32 / 255.;
        let f = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        0.2126 * f(c(16)) + 0.7152 * f(c(8)) + 0.0722 * f(c(0))
    };
    let (a, b) = (lum(fg), lum(bg));
    let (hi, lo) = if a >= b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

/// Shipped with the app, in menu order. Copied, never edited.
pub const BUILTINS: [&str; 5] = ["dostoevsky", "petersburg", "manuscript", "white-nights", "aurora"];

pub fn is_builtin(id: &str) -> bool {
    BUILTINS.contains(&id)
}

pub fn builtin(id: &str) -> Option<Theme> {
    let seeds = |light: (u32, u32, u32), dark: (u32, u32, u32)| {
        (
            Variant { paper: HexColor(light.0), ink: HexColor(light.1), accent: HexColor(light.2), colors: BTreeMap::new() },
            Variant { paper: HexColor(dark.0), ink: HexColor(dark.1), accent: HexColor(dark.2), colors: BTreeMap::new() },
        )
    };
    Some(match id {
        // Today's palettes exactly, pinned token by token.
        "dostoevsky" => Theme { name: "Dostoevsky".into(), light: Variant::full(&LIGHT), dark: Variant::full(&DARK) },
        "petersburg" => {
            let (light, dark) = seeds((0xf3f4f2, 0x2a3440, 0x3f6fa6), (0x10151d, 0xc7d2df, 0x7aa2e8));
            Theme { name: "Petersburg".into(), light, dark }
        }
        // Warm paper, brown ink, a rust accent: long evenings.
        "manuscript" => {
            let (light, dark) = seeds((0xefe4cb, 0x3f3022, 0x9a4a1c), (0x1c160f, 0xe8d4b0, 0xd98b52));
            Theme { name: "Manuscript".into(), light, dark }
        }
        // The clearest pair: white and near-black (about 19 : 1), true black at night.
        "white-nights" => {
            let (light, dark) = seeds((0xffffff, 0x111318, 0x3c37b5), (0x000000, 0xf4f4f6, 0xaaa5ff));
            Theme { name: "White Nights".into(), light, dark }
        }
        // #db2777 read 4.4 : 1 on its paper, under AA for the accent's words.
        "aurora" => {
            let (light, dark) = seeds((0xfdf7fd, 0x3d2b3d, 0xbe1e66), (0x170d16, 0xf0d7e8, 0xf472b6));
            Theme { name: "Aurora".into(), light, dark }
        }
        _ => return None,
    })
}

/// The themes on disk, in `Themes/` under the support dir. Per thread, so tests don't share.
#[derive(Default)]
pub struct Themes {
    pub installed: Vec<(String, Theme)>,
}

thread_local! {
    static THEMES: RefCell<Themes> = RefCell::new(Themes::default());
    /// The current theme resolved into both halves: (light, dark).
    static RESOLVED: RefCell<(Palette, Palette)> = const { RefCell::new((LIGHT, DARK)) };
}

fn load_from(dir: &std::path::Path) -> Vec<(String, Theme)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<(String, Theme)> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("json") {
                return None;
            }
            let id = path.file_stem()?.to_str()?.to_string();
            let theme = serde_json::from_slice::<Theme>(&std::fs::read(&path).ok()?).ok()?;
            Some((id, theme))
        })
        .collect();
    out.sort_by_key(|(_, theme)| theme.name.to_lowercase());
    out
}

/// Rereads `Themes/`: at launch, and after the app saves or deletes a theme.
pub fn reload() {
    let installed = crate::files::support_dir().map(|d| load_from(&d.join("Themes"))).unwrap_or_default();
    THEMES.with(|t| *t.borrow_mut() = Themes { installed });
}

/// Built-ins first, then installed, alphabetical.
pub fn all() -> Vec<(String, Theme, bool)> {
    let installed = THEMES.with(|t| t.borrow().installed.clone());
    BUILTINS
        .iter()
        .filter_map(|id| builtin(id).map(|theme| (id.to_string(), theme, true)))
        .chain(installed.into_iter().map(|(id, theme)| (id, theme, false)))
        .collect()
}

pub fn find(id: &str) -> Option<Theme> {
    builtin(id).or_else(|| THEMES.with(|t| t.borrow().installed.iter().find(|(i, _)| i == id).map(|(_, theme)| theme.clone())))
}

fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

pub fn save(id: &str, theme: &Theme) -> std::io::Result<()> {
    if is_builtin(id) || !safe_id(id) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "not an installed theme id"));
    }
    let dir = crate::files::support_dir().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no support dir"))?;
    let dir = dir.join("Themes");
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_vec_pretty(theme).map_err(std::io::Error::other)?;
    crate::files::write_atomic(&dir.join(format!("{id}.json")), &json)?;
    reload();
    Ok(())
}

/// The file behind a theme of the writer's own; built-ins have none.
pub fn path(id: &str) -> Option<std::path::PathBuf> {
    if is_builtin(id) || !safe_id(id) {
        return None;
    }
    let path = crate::files::support_dir()?.join("Themes").join(format!("{id}.json"));
    path.is_file().then_some(path)
}

/// `false` when there was no installed theme by that name.
pub fn delete(id: &str) -> bool {
    if is_builtin(id) || !safe_id(id) {
        return false;
    }
    let Some(dir) = crate::files::support_dir().map(|d| d.join("Themes")) else { return false };
    let path = dir.join(format!("{id}.json"));
    match std::fs::remove_file(&path) {
        Ok(()) => {
            reload();
            true
        }
        Err(_) => false,
    }
}

/// A file name that holds no installed or built-in theme.
pub fn unique_id(name: &str) -> String {
    let mut slug: String = name
        .trim()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else if c.is_whitespace() || c == '-' { '-' } else { '\0' })
        .filter(|&c| c != '\0')
        .collect();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug = slug.trim_matches('-').to_string();
    let base = if slug.is_empty() { "theme".to_string() } else { slug };
    if find(&base).is_none() {
        return base;
    }
    (2..).map(|n| format!("{base}-{n}")).find(|id| find(id).is_none()).unwrap_or(base)
}

/// The other half for a VS Code theme that only brings one.
pub fn opposite(v: &Variant, light: bool) -> Variant {
    let (paper, ink) = if light {
        (mix(v.paper.0, 0xffffff, 0.87), mix(v.ink.0, 0x000000, 0.75))
    } else {
        (mix(v.paper.0, 0x000000, 0.87), mix(v.ink.0, 0xffffff, 0.75))
    };
    Variant {
        paper: HexColor(paper),
        ink: HexColor(ink),
        accent: v.accent,
        colors: BTreeMap::new(),
    }
}

/// 12% becomes 30% by default.
const GHOST_HOVER_STEP: f32 = 0.18;

/// The preference is the opacity on the dark page; other themes match its contrast.
pub fn ghost_alpha(p: &Prefs) -> f32 {
    matched_alpha(&palette(), p.ghost_opacity)
}

pub fn ghost_hover_alpha(p: &Prefs) -> f32 {
    matched_alpha(&palette(), p.ghost_opacity + GHOST_HOVER_STEP)
}

/// The opacity at which `p`'s text stands out from its background as much as dark text at
/// `dark_alpha` does from the dark background.
fn matched_alpha(p: &Palette, dark_alpha: f32) -> f32 {
    let a = dark_alpha.clamp(0., 1.);
    if (p.bg, p.text) == (DARK.bg, DARK.text) {
        return a;
    }
    let want = contrast(&DARK, a);
    // Contrast grows with opacity, so halve the interval until it is finer than a pixel step.
    let (mut lo, mut hi) = (0f32, 1f32);
    for _ in 0..16 {
        let mid = (lo + hi) / 2.;
        if contrast(p, mid) < want { lo = mid } else { hi = mid }
    }
    hi
}

/// In CIE L*. GPUI blends in sRGB, so the mix is taken there.
fn contrast(p: &Palette, alpha: f32) -> f32 {
    let channel = |hex: u32, shift: u32| ((hex >> shift) & 0xff) as f32 / 255.;
    let lightness = |rgb: [f32; 3]| {
        let lin = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        let y = 0.2126 * lin(rgb[0]) + 0.7152 * lin(rgb[1]) + 0.0722 * lin(rgb[2]);
        if y > 216. / 24389. { 116. * y.cbrt() - 16. } else { y * 24389. / 27. }
    };
    let bg = [16, 8, 0].map(|s| channel(p.bg, s));
    let fg = [16, 8, 0].map(|s| channel(p.text, s));
    let mixed = [0, 1, 2].map(|i| bg[i] + (fg[i] - bg[i]) * alpha);
    (lightness(mixed) - lightness(bg)).abs()
}

pub fn apply(cx: &mut App) {
    let system = cx.window_appearance();
    apply_for(system, cx);
}

pub fn apply_for(system: WindowAppearance, cx: &mut App) {
    HAND.with(|h| h.set(Prefs::get(cx).hand_font));
    let theme = find(&Prefs::get(cx).theme).or_else(|| builtin("dostoevsky")).expect("BUILTINS lists it");
    RESOLVED.with(|r| *r.borrow_mut() = (theme.light.palette(true), theme.dark.palette(false)));
    let opacity = Prefs::get(cx).background_opacity.clamp(MIN_OPACITY, 1.);
    let was = OPACITY.with(|o| o.replace(opacity));
    if window_background(was) != window_background(opacity) {
        // Deferred: a window can't be updated from inside its own event handler.
        cx.defer(move |cx| {
            for w in cx.windows() {
                let _ = w.update(cx, |_, window, _| window.set_background_appearance(window_background(opacity)));
            }
        });
    }
    let pref = Prefs::get(cx).appearance;
    if FORCED.with(Cell::get) != Some(pref) {
        FORCED.with(|f| f.set(Some(pref)));
        cx.set_window_appearance(match pref {
            Appearance::Dark => Some(WindowAppearance::Dark),
            Appearance::Light => Some(WindowAppearance::Light),
            Appearance::System => None,
        });
    }
    let light = match pref {
        Appearance::Dark => false,
        Appearance::Light => true,
        Appearance::System => matches!(system, WindowAppearance::Light | WindowAppearance::VibrantLight),
    };
    if light != is_light() {
        LIGHT_ON.with(|l| l.set(light));
        cx.refresh_windows();
    }
}

pub const MONO: &str = "JetBrains Mono";
pub const PLEX: &str = "IBM Plex Mono";
pub const QUATTRO: &str = "iA Writer Quattro S";
pub const LITERATA: &str = "Literata";
pub const CAVEAT: &str = "Caveat";
pub const SHANTELL: &str = "Shantell Sans";
pub const BAD_SCRIPT: &str = "Bad Script";
pub const UI: &str = PLEX;

pub fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

pub fn ca(hex: u32, alpha: f32) -> Hsla {
    let mut h: Hsla = rgb(hex).into();
    h.a = alpha;
    h
}

pub const FONT_FILES: [&[u8]; 19] = [
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Italic.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-BoldItalic.ttf"),
    include_bytes!("../assets/fonts/Caveat.ttf"),
    include_bytes!("../assets/fonts/ShantellSans-Regular.ttf"),
    include_bytes!("../assets/fonts/BadScript-Regular.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-Bold.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-Italic.ttf"),
    include_bytes!("../assets/fonts/IBMPlexMono-BoldItalic.ttf"),
    include_bytes!("../assets/fonts/iAWriterQuattroS-Regular.ttf"),
    include_bytes!("../assets/fonts/iAWriterQuattroS-Bold.ttf"),
    include_bytes!("../assets/fonts/iAWriterQuattroS-Italic.ttf"),
    include_bytes!("../assets/fonts/iAWriterQuattroS-BoldItalic.ttf"),
    include_bytes!("../assets/fonts/Literata-Regular.ttf"),
    include_bytes!("../assets/fonts/Literata-Bold.ttf"),
    include_bytes!("../assets/fonts/Literata-Italic.ttf"),
    include_bytes!("../assets/fonts/Literata-BoldItalic.ttf"),
];

pub fn load_fonts(cx: &mut App) {
    let fonts: Vec<std::borrow::Cow<'static, [u8]>> = FONT_FILES.iter().map(|f| std::borrow::Cow::Borrowed(*f)).collect();
    if let Err(e) = cx.text_system().add_fonts(fonts) {
        eprintln!("dostoevsky: could not load bundled fonts: {e}");
    }
}

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = match path {
            "chrome/layer.svg" => include_bytes!("../assets/chrome/layer.svg"),
            "chrome/pen.svg" => include_bytes!("../assets/chrome/pen.svg"),
            "chrome/marked.svg" => include_bytes!("../assets/chrome/marked.svg"),
            "chrome/panel-left.svg" => include_bytes!("../assets/chrome/panel-left.svg"),
            "chrome/panel-right.svg" => include_bytes!("../assets/chrome/panel-right.svg"),
            "chrome/chevron.svg" => include_bytes!("../assets/chrome/chevron.svg"),
            "chrome/open.svg" => include_bytes!("../assets/chrome/open.svg"),
            "chrome/save.svg" => include_bytes!("../assets/chrome/save.svg"),
            "chrome/finder.svg" => include_bytes!("../assets/chrome/finder.svg"),
            "chrome/export.svg" => include_bytes!("../assets/chrome/export.svg"),
            "chrome/sliders.svg" => include_bytes!("../assets/chrome/sliders.svg"),
            "chrome/find.svg" => include_bytes!("../assets/chrome/find.svg"),
            "chrome/robot.svg" => include_bytes!("../assets/chrome/robot.svg"),
            "chrome/comment.svg" => include_bytes!("../assets/chrome/comment.svg"),
            "chrome/axe.svg" => include_bytes!("../assets/chrome/axe.svg"),
            "chrome/spinner.svg" => include_bytes!("../assets/chrome/spinner.svg"),
            "chrome/window-minimize.svg" => include_bytes!("../assets/chrome/window-minimize.svg"),
            "chrome/window-maximize.svg" => include_bytes!("../assets/chrome/window-maximize.svg"),
            "chrome/window-restore.svg" => include_bytes!("../assets/chrome/window-restore.svg"),
            "chrome/window-close.svg" => include_bytes!("../assets/chrome/window-close.svg"),
            "icon.png" => include_bytes!("../assets/icon.png"),
            _ => return gpui_kit::assets::AllAssets.load(path),
        };
        Ok(Some(bytes.into()))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        gpui_kit::assets::AllAssets.list(path)
    }
}

#[cfg(test)]
mod tests {
    use super::{builtin, contrast, matched_alpha, save, Assets, HexColor, Palette, Theme, Variant, DARK, GHOST_HOVER_STEP, LIGHT, THEMES};
    use gpui_kit::AssetSource;

    #[test]
    fn every_chrome_glyph_is_served() {
        // An svg missing from `Assets::load` draws nothing, silently.
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/chrome");
        for e in std::fs::read_dir(dir).unwrap() {
            let name = e.unwrap().file_name().into_string().unwrap();
            let path = format!("chrome/{name}");
            assert!(matches!(Assets.load(&path), Ok(Some(_))), "{path} is not served by Assets");
        }
    }

    #[test]
    fn dark_ghosts_keep_the_preference() {
        assert_eq!(matched_alpha(&DARK, 0.12), 0.12);
        assert_eq!(matched_alpha(&DARK, 0.12 + GHOST_HOVER_STEP), 0.3);
    }

    #[test]
    fn light_ghosts_stand_out_as_much_as_dark_ones() {
        for pref in [0.05, 0.12, 0.3, 0.12 + GHOST_HOVER_STEP, 0.3 + GHOST_HOVER_STEP] {
            let (dark, light) = (contrast(&DARK, pref), contrast(&LIGHT, matched_alpha(&LIGHT, pref)));
            assert!((dark - light).abs() < 0.1, "at {pref}: dark {dark}, light {light}");
        }
    }

    #[test]
    fn hover_always_brightens_a_ghost() {
        for palette in [&DARK, &LIGHT] {
            for step in 1..=6 {
                let pref = step as f32 * 0.05;
                let (idle, hover) = (matched_alpha(palette, pref), matched_alpha(palette, pref + GHOST_HOVER_STEP));
                assert!(contrast(palette, hover) - contrast(palette, idle) > 10., "at {pref}: {idle} → {hover}");
            }
        }
    }

    #[test]
    fn dostoevsky_is_the_original_pair_exactly() {
        let theme = builtin("dostoevsky").unwrap();
        for (variant, light, expected) in [(&theme.light, true, &LIGHT), (&theme.dark, false, &DARK)] {
            let actual = variant.palette(light);
            for name in Palette::NAMES {
                assert_eq!(actual.get(name), expected.get(name), "{name}");
            }
            assert_eq!(actual.shadow_alpha, expected.shadow_alpha);
        }
    }

    #[test]
    fn every_builtin_reads_at_aa_in_both_halves() {
        for id in super::BUILTINS {
            let theme = builtin(id).unwrap();
            for (light, variant) in [(true, &theme.light), (false, &theme.dark)] {
                let p = variant.palette(light);
                for (what, color) in [("ink", p.text), ("accent", p.accent)] {
                    let ratio = super::contrast_ratio(color, p.bg);
                    assert!(ratio >= 4.5, "{id} {} {what}: {ratio:.2}", if light { "light" } else { "dark" });
                }
            }
        }
    }

    #[test]
    fn theme_json_is_flat_and_round_trips() {
        let theme = builtin("aurora").unwrap();
        let json = serde_json::to_string(&theme).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(value.get("light").is_some() && value.get("dark").is_some());
        assert!(value.get("variants").is_none());
        assert_eq!(serde_json::from_str::<Theme>(&json).unwrap(), theme);
    }

    #[test]
    fn unknown_colour_tokens_are_ignored() {
        let mut variant = Variant::default();
        variant.colors.insert("not-a-token".into(), HexColor(0x123456));
        let with_unknown = variant.palette(true);
        variant.colors.clear();
        let derived = variant.palette(true);
        for name in Palette::NAMES {
            assert_eq!(with_unknown.get(name), derived.get(name), "{name}");
        }
    }

    #[test]
    fn unique_ids_avoid_builtins_and_installed_files() {
        THEMES.with(|themes| {
            themes.borrow_mut().installed = vec![("aurora-2".into(), builtin("aurora").unwrap())];
        });
        assert_eq!(super::unique_id(" Aurora "), "aurora-3");
        assert_eq!(super::unique_id("***"), "theme");
        THEMES.with(|themes| themes.borrow_mut().installed.clear());
    }

    #[test]
    fn hex_colours_accept_only_rrggbb() {
        assert_eq!(HexColor::parse("#a0B1c2"), Some(HexColor(0xa0b1c2)));
        for bad in ["a0b1c2", "#abc", "#a0b1c2ff", "#gg0000", "", "#12345"] {
            assert_eq!(HexColor::parse(bad), None, "{bad}");
            assert!(serde_json::from_str::<HexColor>(&format!("\"{bad}\"")).is_err(), "{bad}");
        }
    }

    #[test]
    fn fixed_roles_move_only_when_the_paper_needs_it() {
        let variant = Variant { paper: HexColor(DARK.danger), ink: HexColor(0xffffff), accent: HexColor(DARK.accent), colors: Default::default() };
        let palette = variant.palette(false);
        for name in ["danger", "mark", "comment", "success", "para-bar", "original", "origin-text", "origin-hover-text"] {
            assert!(super::contrast_ratio(palette.get(name).unwrap(), palette.bg) >= 3., "{name}");
        }
        assert_eq!(LIGHT.danger, Variant::default().palette(true).danger);
        assert!(save("dostoevsky", &Theme::default()).is_err());
    }
}
