use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use l10n::{tr, trn};

use crate::prefs::{CounterShows, Prefs};
use crate::project::Kind;
use crate::project_ui::paragraph_count;
use crate::theme::{self, c, ca};
use crate::workspace::Workspace;

const READING_WPM: usize = 230;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    pub label: &'static str,
    pub words: usize,
    pub chars: usize,
    pub paragraphs: usize,
    pub reading: Option<(prose::Lang, prose::Stats)>,
}

impl Column {
    /// 0–100, or `None` under [`prose::MIN_WORDS`] words or in another language.
    pub fn ease(&self) -> Option<u8> {
        self.reading.and_then(|(lang, s)| prose::ease(s, lang))
    }
}

pub fn band_label(score: u8) -> &'static str {
    match prose::band(score) {
        prose::Band::VeryEasy => tr!("very easy"),
        prose::Band::Easy => tr!("easy"),
        prose::Band::FairlyEasy => tr!("fairly easy"),
        prose::Band::Plain => tr!("plain"),
        prose::Band::FairlyHard => tr!("fairly hard"),
        prose::Band::Hard => tr!("hard"),
        prose::Band::VeryHard => tr!("very hard"),
    }
}

pub fn reading_time(words: usize) -> String {
    if words == 0 { "—".into() } else { tr!("{minutes} min", minutes = words.div_ceil(READING_WPM)) }
}

pub fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    // A comma is a decimal point elsewhere; the bundled fonts have no narrow no-break space.
    let sep = if l10n::current() == l10n::Lang::En { ',' } else { '\u{a0}' };
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(ch);
    }
    out
}

impl Workspace {
    pub fn count_columns(&self, cx: &App) -> Vec<Column> {
        let m = self.model.read(cx);
        let page = self.page.read(cx);
        let sel = page.selection();
        let text = m.doc.text();
        let mut cols = Vec::new();
        if !sel.is_empty() {
            let n = m.counts(Some(sel.clone()));
            let selected: String = text.chars().skip(sel.start).take(sel.len()).collect();
            let reading = m.reading(Some(sel.clone()));
            cols.push(Column { label: tr!("selection"), words: n.words, chars: n.chars, paragraphs: paragraph_count(&selected), reading });
        }
        let n = m.counts(None);
        let paragraphs = paragraph_count(&text);
        let project = self.project.as_ref();
        let path = m.path.clone();
        let in_total = project.zip(path.as_deref()).is_some_and(|(p, path)| p.counts_toward_total(path));
        let (file_label, whole_label) = match self.project_kind() {
            Some(Kind::Book) => (tr!("chapter"), tr!("book")),
            Some(Kind::Journal) => (tr!("day"), tr!("journal")),
            Some(Kind::Folder) => (tr!("file"), tr!("folder")),
            None => (tr!("document"), ""),
        };
        let reading = m.reading(None);
        cols.push(Column { label: if project.is_some() && !in_total { tr!("this file") } else { file_label }, words: n.words, chars: n.chars, paragraphs, reading });
        if let Some(p) = project {
            let live = path.as_deref().filter(|_| in_total).map(|q| (q, (n.words, n.chars, paragraphs)));
            let (words, chars, paragraphs) = p.totals(live);
            let reading = p.reading_total(path.as_deref().filter(|_| in_total).map(|q| (q, reading)));
            cols.push(Column { label: whole_label, words, chars, paragraphs, reading });
        }
        cols
    }

    pub(crate) fn toggle_count(&mut self, cx: &mut Context<Self>) {
        self.count_open = !self.count_open;
        cx.notify();
    }

    pub fn render_count(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.count_open {
            return None;
        }
        let cols = self.count_columns(cx);
        let prefs = Prefs::get(cx);
        let shows = prefs.counter_shows;
        let in_project = self.project.is_some();
        let heads = cols.len() > 1;
        let w = f32::from(window.viewport_size().width);
        let right = w - crate::workspace::count_right(w, crate::workspace::HeaderEdges::of(window));

        let cell = |text: String, dim: bool| {
            div().w(px(70.)).flex_none().text_right().text_color(if dim { c(theme::muted()) } else { c(theme::text()) }).child(text)
        };
        let row = |label: &'static str, values: Vec<(String, bool)>| {
            div()
                .flex()
                .items_center()
                .py(px(3.))
                .child(div().flex_1().min_w_0().text_color(c(theme::text_2())).child(label))
                .children(values.into_iter().map(|(v, dim)| cell(v, dim)))
        };
        let table = div()
            .flex()
            .flex_col()
            .when(heads, |d| {
                d.child(
                    div()
                        .flex()
                        .pb(px(4.))
                        .text_size(px(10.5))
                        .text_color(c(theme::faint()))
                        .child(div().flex_1())
                        .children(cols.iter().map(|col| div().w(px(70.)).flex_none().text_right().child(col.label))),
                )
            })
            .child(row(tr!("words"), cols.iter().map(|k| (thousands(k.words), false)).collect()))
            .child(row(tr!("characters"), cols.iter().map(|k| (thousands(k.chars), false)).collect()))
            .child(row(tr!("paragraphs"), cols.iter().map(|k| (thousands(k.paragraphs), false)).collect()))
            .child(row(tr!("reading time"), cols.iter().map(|k| (reading_time(k.words), false)).collect()))
            .child(
                div()
                    .id("reading-ease")
                    .flex()
                    .items_start()
                    .py(px(3.))
                    .child(div().flex_1().min_w_0().text_color(c(theme::text_2())).child(tr!("reading ease")))
                    .children(cols.iter().map(|k| match k.ease() {
                        None => cell("—".into(), true).into_any_element(),
                        Some(e) => div()
                            .w(px(70.))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .items_end()
                            .child(div().text_color(c(theme::text())).child(e.to_string()))
                            .child(div().text_size(px(10.5)).line_height(px(14.)).whitespace_nowrap().text_color(c(theme::muted())).child(band_label(e)))
                            .into_any_element(),
                    }))
                    .tooltip(|_, cx| cx.new(|_| crate::workspace::Tip::new(tr!("higher is easier · 0–100"), None)).into())
                    .test_support(),
            );
        let note = tr!("cuts are left out · groups count the variant showing");

        let panel = div()
            .id("count")
            .occlude()
            .w(px(if heads { 140. + 70. * cols.len() as f32 } else { 250. }))
            .p(px(14.))
            .pt(px(10.))
            .rounded(px(8.))
            .bg(c(theme::bg_raised()))
            .border_1()
            .border_color(c(theme::menu_border()))
            .shadow(vec![BoxShadow {
                color: ca(theme::shadow(), theme::palette().shadow_alpha),
                offset: point(px(0.), px(18.)),
                blur_radius: px(50.),
                spread_radius: px(0.),
                inset: false,
            }])
            .text_size(px(12.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_baseline()
                    .mb(px(6.))
                    .child(div().font_family(theme::hand()).text_size(theme::hand_px(20.)).text_color(c(theme::accent())).child(tr!("Statistics")))
                    .when(in_project, |d| d.child(div().text_size(px(10.5)).text_color(c(theme::faint())).child(tr!("notes not counted")))),
            )
            .child(table)
            .child(div().mt(px(3.)).text_size(px(10.5)).line_height(px(15.)).text_color(c(theme::faint())).child(note))
            .child(div().h(px(1.)).my(px(9.)).bg(c(theme::menu_sep())))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .py(px(3.))
                    .child(div().text_color(c(theme::text_2())).child(tr!("header shows")))
                    .child(
                        div()
                            .id("counter-shows")
                            .px(px(8.))
                            .py(px(1.))
                            .rounded(px(5.))
                            .border_1()
                            .border_color(c(theme::menu_border()))
                            .text_size(px(11.5))
                            .text_color(c(theme::text()))
                            .cursor_pointer()
                            .hover(|d| d.border_color(c(theme::accent())))
                            .on_click(move |_, _, cx| crate::sheets::set_prefs(cx, |p| p.counter_shows = shows.next()))
                            .child(shows.label())
                            .test_support(),
                    ),
            )
            .test_support();
        Some(
            div()
                .id("count-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| {
                    this.count_open = false;
                    cx.notify();
                }))
                .child(deferred(anchored().anchor(Anchor::TopRight).position(point(px(right), px(crate::workspace::UNDER_HEADER))).snap_to_window().child(panel)))
                .into_any_element(),
        )
    }
}

pub fn counter_text(shows: CounterShows, words: usize, chars: usize, selected: bool) -> String {
    let w = || trn!("{count} word", "{count} words", words, count = thousands(words));
    let selected_words = || trn!("{count} word selected", "{count} words selected", words, count = thousands(words));
    match shows {
        CounterShows::WordsChars => {
            let ch = trn!("{count} char", "{count} chars", chars, count = thousands(chars));
            if selected { tr!("{words} {chars} selected", words = w(), chars = ch) } else { format!("{} {ch}", w()) }
        }
        CounterShows::Words if !selected => w(),
        CounterShows::ReadingTime if !selected => tr!("{time} read", time = reading_time(words)),
        CounterShows::Words | CounterShows::ReadingTime => selected_words(),
    }
}

#[cfg(test)]
mod tests {
    use super::{counter_text, reading_time, thousands};
    use crate::prefs::CounterShows;

    #[test]
    fn numbers() {
        assert_eq!(thousands(7), "7");
        assert_eq!(thousands(1284), "1,284");
        assert_eq!(thousands(1_000_000), "1,000,000");
        assert_eq!(reading_time(0), "—");
        assert_eq!(reading_time(1), "1 min");
        assert_eq!(reading_time(231), "2 min");
        assert_eq!(counter_text(CounterShows::WordsChars, 1, 5, false), "1 word 5 chars");
        assert_eq!(counter_text(CounterShows::Words, 3, 19, true), "3 words selected");
        assert_eq!(counter_text(CounterShows::ReadingTime, 500, 2900, false), "3 min read");
        assert_eq!(counter_text(CounterShows::ReadingTime, 3, 19, true), "3 words selected");
    }
}
