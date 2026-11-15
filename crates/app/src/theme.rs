use gpui_kit::*;

use std::cell::Cell;

use crate::prefs::{Appearance, HandFont, Prefs};

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

pub fn palette() -> &'static Palette {
    if is_light() { &LIGHT } else { &DARK }
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

/// 12% becomes 30% by default.
const GHOST_HOVER_STEP: f32 = 0.18;

/// The preference is the opacity on the dark page; other themes match its contrast.
pub fn ghost_alpha(p: &Prefs) -> f32 {
    matched_alpha(palette(), p.ghost_opacity)
}

pub fn ghost_hover_alpha(p: &Prefs) -> f32 {
    matched_alpha(palette(), p.ghost_opacity + GHOST_HOVER_STEP)
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
    use super::{contrast, matched_alpha, Assets, DARK, GHOST_HOVER_STEP, LIGHT};
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
}
