use std::path::PathBuf;

use gpui_kit::{App, Global};
use l10n::tr;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effects {
    On,
    Subtle,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Autosave {
    Off,
    /// After 2 s idle and when the window loses focus.
    Idle,
    Blur,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Alignment {
    #[default]
    Left,
    Justified,
}

/// The file keeps its blank line either way; only the page is drawn differently.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParagraphLook {
    #[default]
    Spaced,
    Indented,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CutLook {
    #[default]
    Text,
    Mark,
}

/// How comments show in editing (COM-4). With the Comments tab open they are ticks either way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommentLook {
    #[default]
    Notes,
    Ticks,
}

impl CutLook {
    pub fn look(self) -> doc_model::Look {
        match self {
            CutLook::Text => doc_model::Look::Text,
            CutLook::Mark => doc_model::Look::Mark,
        }
    }
}

thread_local! {
    /// Read by `DocModel::present` without the app; per thread, so each test has its own.
    static CUT_LOOK: std::cell::Cell<CutLook> = const { std::cell::Cell::new(CutLook::Text) };
}

pub fn cut_look() -> doc_model::Look {
    CUT_LOOK.with(|c| c.get()).look()
}

/// Documents presented from now on follow it. Returns whether it changed.
pub fn publish(p: &Prefs) -> bool {
    CUT_LOOK.with(|c| c.replace(p.cut_look)) != p.cut_look
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SpellCheck {
    #[default]
    Always,
    Editing,
    Off,
}

/// Prefs saved before the choice have `true` or `false`.
impl<'de> Deserialize<'de> for SpellCheck {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Saved {
            Bool(bool),
            Name(String),
        }
        Ok(match Saved::deserialize(d)? {
            Saved::Bool(true) => SpellCheck::Always,
            Saved::Bool(false) => SpellCheck::Off,
            Saved::Name(n) => match n.as_str() {
                "editing" => SpellCheck::Editing,
                "off" => SpellCheck::Off,
                _ => SpellCheck::Always,
            },
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WheelSwitch {
    #[default]
    Always,
    WithOption,
    Off,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CounterShows {
    #[default]
    WordsChars,
    Words,
    ReadingTime,
}

impl CounterShows {
    pub const ALL: [CounterShows; 3] = [CounterShows::WordsChars, CounterShows::Words, CounterShows::ReadingTime];

    pub fn label(self) -> &'static str {
        match self {
            CounterShows::WordsChars => tr!("words + chars"),
            CounterShows::Words => tr!("words"),
            CounterShows::ReadingTime => tr!("reading time"),
        }
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|c| *c == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }
}

/// What Zen writing keeps at full ink.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ZenFocus {
    /// The caret's sentence (`prose::sentences`, as ⇧⌘A and the sentence keys).
    Sentence,
    /// The wrapped row the caret is on.
    Line,
    #[default]
    Paragraph,
}

/// How Zen writing dims the rest: flat, or fainter step by step away from what stays bright.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ZenFade {
    #[default]
    Off,
    Near,
    WholePage,
}

/// How Private mode draws the text it hides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HiddenLook {
    /// One dot per character.
    #[default]
    Dots,
    /// One x-height bar per word.
    Bars,
    /// One full-height bar per word.
    Solid,
    /// The text itself, blurred.
    Blur,
}

impl HiddenLook {
    pub const ALL: [HiddenLook; 4] = [HiddenLook::Dots, HiddenLook::Bars, HiddenLook::Solid, HiddenLook::Blur];

    pub fn name(self) -> &'static str {
        match self {
            HiddenLook::Dots => tr!("Dots"),
            HiddenLook::Bars => tr!("Bars"),
            HiddenLook::Solid => tr!("Solid"),
            HiddenLook::Blur => tr!("Blurred ink"),
        }
    }
}

/// What Private mode leaves readable around the caret.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StaysVisible {
    #[default]
    Word,
    Sentence,
    Nothing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    Dark,
    Light,
    System,
}

impl Appearance {
    /// Prefs saved before the light theme have no appearance: they keep the dark look.
    fn saved_before_light() -> Self {
        Appearance::Dark
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HandFont {
    Caveat,
    Shantell,
    BadScript,
}

impl HandFont {
    pub const ALL: [HandFont; 3] = [HandFont::BadScript, HandFont::Caveat, HandFont::Shantell];

    pub fn family(self) -> &'static str {
        match self {
            HandFont::Caveat => crate::theme::CAVEAT,
            HandFont::Shantell => crate::theme::SHANTELL,
            HandFont::BadScript => crate::theme::BAD_SCRIPT,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            HandFont::Caveat => "Caveat",
            HandFont::Shantell => "Shantell Sans",
            HandFont::BadScript => "Bad Script",
        }
    }

    /// The code's sizes were set for Caveat (narrow, small x-height); the others fill the same
    /// places a size smaller.
    pub fn scale(self) -> f32 {
        match self {
            HandFont::Caveat => 1.,
            HandFont::Shantell => 0.78,
            HandFont::BadScript => 0.86,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    #[serde(default = "Appearance::saved_before_light")]
    pub appearance: Appearance,
    /// 1 is opaque.
    pub background_opacity: f32,
    pub font_family: String,
    pub hand_font: HandFont,
    pub hand_headings: bool,
    pub font_size: f32,
    pub column_chars: u32,
    pub line_spacing: f32,
    pub alignment: Alignment,
    pub hyphenation: bool,
    pub paragraphs: ParagraphLook,
    pub smart_punctuation: bool,
    /// A ⇧ tapped alone grows the selection.
    pub tap_shift: bool,
    pub spell_check: SpellCheck,
    pub zen_writing: bool,
    pub zen_focus: ZenFocus,
    pub zen_fade: ZenFade,
    pub typewriter: bool,
    /// Writing mode hides the page's text (paint only; the document is untouched).
    pub private_mode: bool,
    pub hidden_look: HiddenLook,
    pub stays_visible: StaysVisible,
    pub sounds: bool,
    pub volume: f32,
    pub hover_label: bool,
    /// Buttons above a selection the mouse made.
    pub selection_buttons: bool,
    pub wheel: WheelSwitch,
    pub remember_pairings: bool,
    /// The opacity of cuts' faded text.
    pub ghost_opacity: f32,
    pub cut_look: CutLook,
    pub comment_look: CommentLook,
    pub counter_shows: CounterShows,
    pub effects: Effects,
    pub autosave: Autosave,
    pub backups: bool,
    pub default_folder: Option<PathBuf>,
    pub sidebar_width: f32,
    pub overflow_width: f32,
    /// Keys are in the Keychain, not here.
    pub ai: AiPrefs,
    /// A language code (`ru`, `pt-BR`); `None` follows macOS.
    pub language: Option<String>,
    pub welcome_shown: bool,
    /// The .dmg build's daily update check; `None` until the writer answers (updates.rs).
    pub check_updates: Option<bool>,
    /// What each mode had open when it was left, put back when you come back to it.
    pub panels: ModePanels,
}

/// A left panel's tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LeftTab {
    Project,
    Alternatives,
    Review,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Panels {
    /// `None`: the left panel is closed.
    pub left: Option<LeftTab>,
    pub cuts: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModePanels {
    pub writing: Panels,
    pub editing: Panels,
}

/// Writing starts with nothing open; editing on Review, which has no other button.
impl Default for ModePanels {
    fn default() -> Self {
        ModePanels { writing: Panels::default(), editing: Panels { left: Some(LeftTab::Review), cuts: false } }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiPrefs {
    pub enabled: bool,
    pub services: Vec<ai::Service>,
    /// For ⌥⌘A and `??`.
    pub quick: Option<ai::ModelRef>,
    /// For the Lab and trims.
    pub deep: Option<ai::ModelRef>,
    /// Asked once per service off this Mac, before its first request.
    pub allowed: Vec<ai::ServiceKind>,
    pub ideas: usize,
}

impl Default for AiPrefs {
    fn default() -> Self {
        // OpenRouter is listed from the start, waiting for Connect.
        AiPrefs { enabled: true, services: vec![ai::Service::new(ai::ServiceKind::OpenRouter)], quick: None, deep: None, allowed: Vec::new(), ideas: 5 }
    }
}

impl AiPrefs {
    pub fn service(&self, kind: ai::ServiceKind) -> Option<&ai::Service> {
        self.services.iter().find(|s| s.kind == kind)
    }

    pub fn slot(&self, slot: ai::cost::Slot) -> Option<&ai::ModelRef> {
        match slot {
            ai::cost::Slot::Quick => self.quick.as_ref(),
            ai::cost::Slot::Deep => self.deep.as_ref(),
        }
    }

    pub const IDEAS: [usize; 4] = [3, 5, 8, 10];

    /// Kept to 1–10 whatever a hand-edited file says.
    pub fn ideas(&self) -> usize {
        self.ideas.clamp(1, 10)
    }

    pub fn set_slot(&mut self, slot: ai::cost::Slot, m: Option<ai::ModelRef>) {
        match slot {
            ai::cost::Slot::Quick => self.quick = m,
            ai::cost::Slot::Deep => self.deep = m,
        }
    }
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            appearance: Appearance::System,
            background_opacity: 1.,
            font_family: crate::theme::PLEX.into(),
            hand_font: HandFont::BadScript,
            hand_headings: true,
            font_size: 19.,
            column_chars: 66,
            line_spacing: 1.8,
            alignment: Alignment::Left,
            hyphenation: false,
            paragraphs: ParagraphLook::Spaced,
            smart_punctuation: true,
            tap_shift: false,
            spell_check: SpellCheck::Always,
            zen_writing: true,
            zen_focus: ZenFocus::Paragraph,
            zen_fade: ZenFade::Off,
            typewriter: true,
            private_mode: false,
            hidden_look: HiddenLook::Dots,
            stays_visible: StaysVisible::Word,
            sounds: true,
            volume: 0.5,
            hover_label: true,
            selection_buttons: true,
            wheel: WheelSwitch::Always,
            remember_pairings: true,
            ghost_opacity: 0.12,
            cut_look: CutLook::Text,
            comment_look: CommentLook::Notes,
            counter_shows: CounterShows::WordsChars,
            effects: Effects::On,
            autosave: Autosave::Idle,
            backups: true,
            default_folder: None,
            sidebar_width: crate::sidebar::WIDTH,
            overflow_width: crate::overflow::WIDTH,
            ai: AiPrefs::default(),
            language: None,
            welcome_shown: false,
            check_updates: None,
            panels: ModePanels::default(),
        }
    }
}

impl Global for Prefs {}

impl Prefs {
    pub fn path() -> Option<PathBuf> {
        crate::files::support_dir().map(|d| d.join("prefs.json"))
    }

    pub fn load() -> Prefs {
        Self::path()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(p) = Self::path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(json) = serde_json::to_vec_pretty(self) {
                let _ = std::fs::write(p, json);
            }
        }
    }

    pub fn get(cx: &App) -> &Prefs {
        cx.global::<Prefs>()
    }

    /// `DOSTOEVSKY_LANG` (screenshot scripts pin English), the one chosen in Settings, or the
    /// first of macOS's that has a catalog.
    pub fn ui_language(&self) -> l10n::Lang {
        std::env::var("DOSTOEVSKY_LANG")
            .ok()
            .as_deref()
            .and_then(l10n::Lang::from_code)
            .or_else(|| self.language.as_deref().and_then(l10n::Lang::from_code))
            .unwrap_or_else(|| l10n::negotiate(&crate::platform::preferred_languages()))
    }

    /// Effects after Reduce Motion.
    pub fn motion(&self) -> Effects {
        if crate::platform::reduce_motion() {
            Effects::Off
        } else {
            self.effects
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Prefs, SpellCheck};

    #[test]
    fn spell_check_reads_the_old_switch() {
        let read = |json: &str| serde_json::from_str::<Prefs>(json).unwrap().spell_check;
        assert_eq!(read(r#"{"spell_check": true}"#), SpellCheck::Always);
        assert_eq!(read(r#"{"spell_check": false}"#), SpellCheck::Off);
        assert_eq!(read(r#"{"spell_check": "editing"}"#), SpellCheck::Editing);
        assert_eq!(read("{}"), SpellCheck::Always);
        let saved = serde_json::to_string(&Prefs { spell_check: SpellCheck::Editing, ..Prefs::default() }).unwrap();
        assert!(saved.contains(r#""spell_check":"editing""#), "{saved}");
    }
}
