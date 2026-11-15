use std::ops::Range;
use std::path::PathBuf;

use doc_model::{Document, Scope, Source};
use gpui_kit::{Action, App};
use l10n::{tr, Lang};

use crate::actions::*;
use crate::model::DocModel;
use crate::prefs::Prefs;

pub fn source(lang: Lang) -> &'static str {
    match lang {
        Lang::En => include_str!("../assets/welcome/en.md"),
        Lang::Ru => include_str!("../assets/welcome/ru.md"),
        Lang::De => include_str!("../assets/welcome/de.md"),
        Lang::Fr => include_str!("../assets/welcome/fr.md"),
        Lang::Es => include_str!("../assets/welcome/es.md"),
        Lang::It => include_str!("../assets/welcome/it.md"),
        Lang::PtBr => include_str!("../assets/welcome/pt-BR.md"),
    }
}

pub fn action_named(name: &str) -> Option<Box<dyn Action>> {
    Some(match name {
        "Save" => Box::new(Save),
        "WritingMode" => Box::new(WritingMode),
        "EditingMode" => Box::new(EditingMode),
        "ToggleLayer" => Box::new(ToggleLayer),
        "AddComment" => Box::new(AddComment),
        "ToggleOverflow" => Box::new(ToggleOverflow),
        "AlternativesForSelection" => Box::new(AlternativesForSelection),
        "CycleAtCaretNext" => Box::new(CycleAtCaretNext),
        "GrowSelection" => Box::new(GrowSelection),
        "MoveUnitDown" => Box::new(MoveUnitDown),
        "GhostToggle" => Box::new(GhostToggle),
        "NewDocument" => Box::new(NewDocument),
        "NewProject" => Box::new(NewProject),
        "Open" => Box::new(Open),
        "ShowShortcuts" => Box::new(ShowShortcuts),
        "ToggleLab" => Box::new(ToggleLab),
        _ => return None,
    })
}

/// Fills in the keys bound now. A name with no action is an error; an action with no keys reads "—".
pub fn fill_keys(src: &str, label: impl Fn(&str) -> Option<String>) -> Result<String, String> {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(i) = rest.find("[key:") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 5..];
        let end = after.find(']').ok_or("an unclosed [key:")?;
        let name = &after[..end];
        if action_named(name).is_none() {
            return Err(format!("no action named {name}"));
        }
        out.push_str(&label(name).unwrap_or_else(|| "—".into()));
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Ranges and offsets are display chars of `text`.
#[derive(Debug, Default, PartialEq)]
pub struct Parsed {
    pub text: String,
    pub caret: usize,
    /// The range is the first version's, which shows.
    pub groups: Vec<(Range<usize>, Vec<String>)>,
    pub comments: Vec<(Range<usize>, String)>,
    pub faded: Vec<Range<usize>>,
    pub hidden: Vec<Range<usize>>,
}

/// Markup: see `assets/welcome/README.md`. Keys must be filled first.
pub fn parse(src: &str) -> Result<Parsed, String> {
    let mut p = Parsed::default();
    let mut n = 0usize; // chars written
    let mut open_comment: Option<usize> = None;
    let mut open_faded: Option<usize> = None;
    let mut open_hidden: Option<usize> = None;
    let mut rest = src;
    while let Some(c) = rest.chars().next() {
        if let Some(r) = rest.strip_prefix("{{") {
            let end = r.find("}}").ok_or("an unclosed {{")?;
            let versions: Vec<String> = r[..end].split('|').map(str::to_string).collect();
            if versions.len() < 2 || versions.iter().any(String::is_empty) {
                return Err(format!("a group needs two versions or more: {}", &r[..end]));
            }
            let len = versions[0].chars().count();
            p.text.push_str(&versions[0]);
            p.groups.push((n..n + len, versions));
            n += len;
            rest = &r[end + 2..];
        } else if let Some(r) = rest.strip_prefix("[[") {
            if open_comment.is_some() {
                return Err("comments don't nest".into());
            }
            open_comment = Some(n);
            rest = r;
        } else if let (Some(start), Some(r)) = (open_comment, rest.strip_prefix(" :: ")) {
            let end = r.find("]]").ok_or("an unclosed [[")?;
            p.comments.push((start..n, r[..end].trim().to_string()));
            open_comment = None;
            rest = &r[end + 2..];
        } else if let Some(r) = rest.strip_prefix("~~") {
            match open_faded.take() {
                Some(start) => p.faded.push(start..n),
                None => open_faded = Some(n),
            }
            rest = r;
        } else if let Some(r) = rest.strip_prefix("<<") {
            open_hidden = Some(n);
            rest = r;
        } else if let Some(r) = rest.strip_prefix(">>") {
            p.hidden.push(open_hidden.take().ok_or("a >> with no <<")?..n);
            rest = r;
        } else if c == '‸' {
            p.caret = n;
            rest = &rest[c.len_utf8()..];
        } else {
            p.text.push(c);
            n += 1;
            rest = &rest[c.len_utf8()..];
        }
    }
    if open_comment.is_some() || open_faded.is_some() || open_hidden.is_some() {
        return Err("an unclosed [[, ~~ or <<".into());
    }
    if p.comments.iter().any(|(r, _)| r.is_empty()) {
        return Err("a comment on no words".into());
    }
    Ok(p)
}

pub fn build(src: &str) -> Result<(Document, usize), String> {
    let p = parse(src)?;
    fn err(what: &'static str) -> impl Fn(doc_model::EditError) -> String {
        move |e| format!("{what}: {e:?}")
    }
    let mut d = Document::from_text(&p.text);
    // Cuts with no trace first, from the end: their text leaves the page and moves what follows.
    let mut hidden = p.hidden.clone();
    hidden.sort_by_key(|r| std::cmp::Reverse(r.start));
    for r in &hidden {
        d.cut_without_trace(r.clone()).map_err(err("a cut with no trace"))?;
    }
    let at = |x: usize| x - hidden.iter().filter(|r| r.end <= x).map(|r| r.len()).sum::<usize>();
    let map = |r: &Range<usize>| at(r.start)..at(r.end);
    for (r, versions) in &p.groups {
        let scope = if versions[0].contains(char::is_whitespace) { Scope::Sentence } else { Scope::Word };
        let g = d.create_group(map(r), scope).map_err(err("a group"))?;
        for v in &versions[1..] {
            d.add_variant(g, v, Source::Me).map_err(err("a version"))?;
        }
    }
    for (r, text) in &p.comments {
        d.add_comment(map(r), text, None).map_err(err("a comment"))?;
    }
    // Faded cuts keep their text on the page; the order is only for safety.
    let mut faded = p.faded.clone();
    faded.sort_by_key(|r| std::cmp::Reverse(r.start));
    for r in &faded {
        d.cut(map(r), None).map_err(err("a cut"))?;
    }
    let d = Document::from_snapshot(d.snapshot()).map_err(|e| format!("{e:?}"))?;
    Ok((d, at(p.caret)))
}

/// A source that doesn't build gives a blank page.
pub fn model(cx: &App) -> (DocModel, usize) {
    let lang = l10n::current();
    let label = |name: &str| action_named(name).and_then(|a| crate::context_menu::shortcut_label(a.as_ref(), cx));
    let built = fill_keys(source(lang), label).and_then(|s| build(&s));
    let (doc, caret) = built.unwrap_or_else(|e| {
        eprintln!("dostoevsky: the welcome page didn't build: {e}");
        (Document::new(), 0)
    });
    let mut m = DocModel::new(doc);
    let name = tr!("Welcome").to_string();
    let folder = Prefs::get(cx)
        .default_folder
        .clone()
        .or_else(|| crate::platform::home().map(|h| h.join("Documents")))
        .unwrap_or_else(|| PathBuf::from("."));
    m.suggested = Some(folder.join(format!("{name}.md")));
    m.name = Some(name);
    (m, caret)
}

/// An untouched one already open comes forward instead.
pub fn open(cx: &mut App) {
    for (w, ws) in crate::workspaces(cx) {
        let m = ws.read(cx).model.read(cx);
        if m.name.is_some() && m.path.is_none() && !m.dirty() && ws.read(cx).project.is_none() {
            let _ = w.update(cx, |_, window, _| window.activate_window());
            return;
        }
    }
    let (model, caret) = model(cx);
    let Some(handle) = crate::open_model(model, cx) else { return };
    if let Some((_, ws)) = crate::workspaces(cx).into_iter().find(|(w, _)| *w == handle) {
        let _ = handle.update(cx, |_, _, cx| {
            let page = ws.read(cx).page.clone();
            page.update(cx, |p, cx| p.select(caret..caret, cx));
        });
    }
}

/// A dev script starts from an empty folder and expects a blank page.
pub fn due(cx: &App) -> bool {
    if std::env::var_os("DOSTOEVSKY_SCRIPT").is_some() && std::env::var_os("DOSTOEVSKY_WELCOME").is_none() {
        return false;
    }
    !Prefs::get(cx).welcome_shown && crate::files::recent().is_empty()
}

/// Marked shown at once, so a crash or quit before it closes doesn't show it again.
pub fn open_first(cx: &mut App) {
    crate::sheets::set_prefs(cx, |p| p.welcome_shown = true);
    open(cx);
}

#[cfg(test)]
mod tests {
    use super::{build, fill_keys, parse, source, Range};
    use l10n::Lang;

    fn keys(name: &str) -> Option<String> {
        Some(
            match name {
                "Save" => "⌘S",
                "WritingMode" => "⌘1",
                "EditingMode" => "⌘2",
                "ToggleLayer" => "⌘D",
                "AddComment" => "⌥⌘M",
                "ToggleOverflow" => "⌃⌘C",
                "AlternativesForSelection" => "⇧⌘A",
                "CycleAtCaretNext" => "⌥⌘↓",
                "GrowSelection" => "⌃⌘↑",
                "MoveUnitDown" => "⌥↓",
                "GhostToggle" => "⌘/",
                "NewDocument" => "⌘N",
                "NewProject" => "⌥⌘N",
                "Open" => "⌘O",
                "ShowShortcuts" => "⇧⌘K",
                _ => return None,
            }
            .to_string(),
        )
    }

    #[test]
    fn the_markup() {
        let p = parse("A {{garret|closet}} [[and :: so it is]]. ‸<<gone. >>Here.~~ Faded.~~").unwrap();
        assert_eq!(p.text, "A garret and. gone. Here. Faded.");
        assert_eq!(p.groups, [(2..8, vec!["garret".to_string(), "closet".to_string()])]);
        assert_eq!(p.comments, [(9..12, "so it is".to_string())]);
        assert_eq!(p.caret, 14);
        assert_eq!(p.hidden, vec![Range { start: 14, end: 20 }]);
        assert_eq!(p.faded, vec![Range { start: 25, end: 32 }]);
        assert!(parse("[[a :: b").is_err());
        assert!(parse("{{one}}").is_err());
        assert!(parse("~~open").is_err());
        assert!(fill_keys("[key:Nope]", keys).is_err());
        assert_eq!(fill_keys("press [key:Save]!", keys).unwrap(), "press ⌘S!");
        assert_eq!(fill_keys("press [key:Save]", |_| None).unwrap(), "press —");
    }

    #[test]
    fn win_19_every_language_has_the_whole_tour() {
        for lang in Lang::ALL {
            let src = fill_keys(source(lang), keys).unwrap_or_else(|e| panic!("{lang:?}: {e}"));
            let (d, caret) = build(&src).unwrap_or_else(|e| panic!("{lang:?}: {e}"));
            d.check_invariants().unwrap();
            assert!(!d.text().contains("[key:") && !d.text().contains("[[") && !d.text().contains("{{"), "{lang:?}");
            let groups: Vec<usize> = d.all_group_ids().iter().map(|g| d.group(*g).unwrap().variants.len()).collect();
            assert_eq!(groups, [3, 2], "{lang:?}: a word with three versions, a sentence with two");
            let scopes: Vec<_> = d.all_group_ids().iter().map(|g| d.group(*g).unwrap().scope).collect();
            assert_eq!(scopes, [doc_model::Scope::Word, doc_model::Scope::Sentence], "{lang:?}");
            let cuts = d.cuts();
            assert_eq!(cuts.len(), 2, "{lang:?}: two cuts");
            assert_eq!(d.traces().len(), 1, "{lang:?}: one of them faded on the page");
            let comments = d.comments();
            assert_eq!(comments.len(), 5, "{lang:?}: five comments");
            for c in &comments {
                assert!(!c.range.is_empty() && !c.text.is_empty(), "{lang:?}: {c:?}");
            }
            let first_para = d.text().split("\n\n").take(2).map(|s| s.chars().count() + 2).sum::<usize>();
            assert!(caret > 0 && caret <= first_para, "{lang:?}: the caret ends the first paragraph");
            assert!(d.text().starts_with("# "), "{lang:?}: a title");
            let saved = doc_format::serialize(&d, false, &Default::default());
            let back = crate::format::load(&saved).doc;
            assert_eq!(back.text(), d.text(), "{lang:?}");
            assert_eq!(back.comments().len(), 5, "{lang:?}");
        }
    }

    #[test]
    fn win_19_review_finds_a_long_sentence_and_an_echo() {
        for lang in Lang::ALL {
            let (d, _) = build(&fill_keys(source(lang), keys).unwrap()).unwrap();
            let text = d.text();
            let sentences = prose::sentences(&text, 0..text.chars().count());
            assert!(!ai::lab::long_sentences(&sentences).is_empty(), "{lang:?}: a long sentence");
            for c in d.comments() {
                let said: String = c.text.chars().take_while(char::is_ascii_digit).collect();
                if let Ok(n) = said.parse::<usize>() {
                    assert_eq!(n, english::word_count(&d.text_range(c.range.clone())), "{lang:?}: {}", c.text);
                }
            }
            let title = text.lines().next().unwrap().trim_start_matches("# ");
            let echoes = crate::lab::echo_marks(&text, &sentences, Some(title));
            assert!(!echoes.is_empty(), "{lang:?}: an echo");
        }
    }
}
