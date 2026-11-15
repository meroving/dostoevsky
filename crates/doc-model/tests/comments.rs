use doc_model::{Carried, CutPlace, Document, EditError, Place, Scope, Source};

fn at(doc: &Document, needle: &str) -> std::ops::Range<usize> {
    let text = doc.text();
    let byte = text.find(needle).unwrap_or_else(|| panic!("`{needle}` not in `{text}`"));
    let start = text[..byte].chars().count();
    start..start + needle.chars().count()
}

fn ok(doc: &Document) {
    doc.check_invariants().unwrap();
}

fn on_page(doc: &Document) -> Vec<(String, String)> {
    doc.comments().into_iter().map(|c| (doc.text_range(c.range), c.text)).collect()
}

const PAGE: &str = "He had avoided his landlady. His garret was more like a cupboard than a room. The end.";

/// COM-1, COM-11
#[test]
fn a_comment_sits_on_its_text_and_changes_nothing() {
    let mut d = Document::from_text(PAGE);
    let id = d.add_comment(at(&d, "more like a cupboard"), "Cupboard or closet?", Some("2026-11-12T10:00:00Z".into())).unwrap();
    assert_eq!(d.text(), PAGE);
    assert_eq!(d.clean_text(), PAGE);
    assert_eq!(on_page(&d), vec![("more like a cupboard".into(), "Cupboard or closet?".into())]);
    let c = &d.comments()[0];
    assert_eq!(c.id, id);
    assert_eq!(c.created.as_deref(), Some("2026-11-12T10:00:00Z"));
    assert_eq!(c.author, None);
    d.undo();
    assert!(d.comments().is_empty());
    d.redo();
    assert_eq!(d.comments().len(), 1);
    ok(&d);
}

/// COM-7
#[test]
fn typing_inside_grows_it_and_at_its_edges_stays_outside() {
    let mut d = Document::from_text(PAGE);
    d.add_comment(at(&d, "a cupboard"), "?", None).unwrap();
    let r = at(&d, "cupboard");
    d.replace(r.start..r.start, "tiny ").unwrap();
    assert_eq!(on_page(&d)[0].0, "a tiny cupboard");
    let r = at(&d, "a tiny cupboard");
    d.replace(r.end..r.end, "!").unwrap();
    d.replace(r.start..r.start, "¡").unwrap();
    assert_eq!(on_page(&d)[0].0, "a tiny cupboard");
    assert!(d.text().contains("¡a tiny cupboard!"));
    ok(&d);
}

/// COM-7
#[test]
fn deleting_its_text_shortens_it_and_all_of_it_deletes_it() {
    let mut d = Document::from_text(PAGE);
    d.add_comment(at(&d, "more like a cupboard"), "?", None).unwrap();
    d.replace(at(&d, "more like "), "").unwrap();
    assert_eq!(on_page(&d)[0].0, "a cupboard");
    d.break_undo_group();
    d.replace(at(&d, "was a cupboard than"), "was than").unwrap();
    assert!(d.comments().is_empty());
    ok(&d);
    d.undo();
    assert_eq!(on_page(&d)[0].0, "a cupboard");
    ok(&d);
}

/// COM-6
#[test]
fn editing_and_deleting_a_comment() {
    let mut d = Document::from_text(PAGE);
    let id = d.add_comment(at(&d, "garret"), "Attic?", None).unwrap();
    d.set_comment_text(id, "Attic or garret?").unwrap();
    assert_eq!(d.comment_text(id).as_deref(), Some("Attic or garret?"));
    d.delete_comment(id).unwrap();
    assert!(d.comments().is_empty());
    assert_eq!(d.text(), PAGE);
    assert_eq!(d.delete_comment(id), Err(EditError::NoSuchComment));
    d.undo();
    assert_eq!(d.comment_text(id).as_deref(), Some("Attic or garret?"));
    d.undo();
    assert_eq!(d.comment_text(id).as_deref(), Some("Attic?"));
    ok(&d);
}

/// COM-7
#[test]
fn comments_overlap_and_nest() {
    let mut d = Document::from_text(PAGE);
    d.add_comment(at(&d, "His garret was more"), "one", None).unwrap();
    d.add_comment(at(&d, "more like a cupboard"), "two", None).unwrap();
    d.add_comment(at(&d, "garret"), "three", None).unwrap();
    let mut got = on_page(&d);
    got.sort();
    assert_eq!(
        got,
        vec![
            ("His garret was more".into(), "one".into()),
            ("garret".into(), "three".into()),
            ("more like a cupboard".into(), "two".into()),
        ]
    );
    ok(&d);
}

/// COM-8
#[test]
fn a_comment_on_a_whole_group_stays_whichever_version_shows() {
    let mut d = Document::from_text(PAGE);
    let g = d.create_group(at(&d, "cupboard"), Scope::Word).unwrap();
    d.add_variant(g, "closet", Source::Me).unwrap();
    d.add_comment(at(&d, "cupboard"), "Pick one.", None).unwrap();
    d.cycle(g, 1).unwrap();
    assert_eq!(on_page(&d), vec![("closet".into(), "Pick one.".into())]);
    ok(&d);
}

/// COM-8
#[test]
fn a_comment_inside_a_version_shows_only_with_it() {
    let mut d = Document::from_text(PAGE);
    let g = d.create_group(at(&d, "more like a cupboard than a room"), Scope::Sentence).unwrap();
    d.add_variant(g, "a closet", Source::Me).unwrap();
    d.add_comment(at(&d, "cupboard"), "Too neat?", None).unwrap();
    assert_eq!(on_page(&d), vec![("cupboard".into(), "Too neat?".into())]);
    d.cycle(g, 1).unwrap();
    assert!(d.comments().is_empty());
    assert!(d.has_comments());
    d.cycle(g, -1).unwrap();
    assert_eq!(on_page(&d), vec![("cupboard".into(), "Too neat?".into())]);
    ok(&d);
}

/// COM-8
#[test]
fn a_comment_across_a_group_grows_to_hold_it() {
    let mut d = Document::from_text(PAGE);
    let g = d.create_group(at(&d, "cupboard than"), Scope::Word).unwrap();
    d.add_variant(g, "closet than", Source::Me).unwrap();
    d.add_comment(at(&d, "like a cupboard"), "?", None).unwrap();
    assert_eq!(on_page(&d)[0].0, "like a cupboard than");
    let mut d = Document::from_text(PAGE);
    d.add_comment(at(&d, "like a cupboard"), "?", None).unwrap();
    d.create_group(at(&d, "cupboard than"), Scope::Word).unwrap();
    assert_eq!(on_page(&d)[0].0, "like a cupboard than");
    ok(&d);
}

/// COM-9
#[test]
fn a_cut_takes_its_comment_and_brings_it_back() {
    let mut d = Document::from_text(PAGE);
    d.add_comment(at(&d, "more like a cupboard"), "Cupboard or closet?", None).unwrap();
    let (id, _) = d.cut(at(&d, "His garret was more like a cupboard than a room. "), None).unwrap();
    assert!(d.comments().is_empty(), "it left the margin with the cut");
    assert_eq!(d.cut_info(id).unwrap().comments, vec!["Cupboard or closet?".to_string()]);
    d.put_back(id).unwrap();
    assert_eq!(on_page(&d), vec![("more like a cupboard".into(), "Cupboard or closet?".into())]);
    ok(&d);
    let mut d = Document::from_text(PAGE);
    d.add_comment(at(&d, "more like a cupboard"), "?", None).unwrap();
    let (id, _) = d.cut(at(&d, "more like a cupboard"), None).unwrap();
    assert!(d.comments().is_empty());
    assert_eq!(d.cut_info(id).unwrap().comments.len(), 1);
    ok(&d);
}

/// COM-9
#[test]
fn a_cut_over_part_of_a_comment_leaves_it_on_the_rest() {
    let mut d = Document::from_text(PAGE);
    d.add_comment(at(&d, "more like a cupboard"), "?", None).unwrap();
    let (id, _) = d.cut(at(&d, "like a cupboard than a room"), None).unwrap();
    assert_eq!(on_page(&d)[0].0, "more ");
    assert!(d.cut_info(id).unwrap().comments.is_empty());
    ok(&d);
}

/// COM-1
#[test]
fn no_comment_inside_a_cut() {
    let mut d = Document::from_text(PAGE);
    d.cut(at(&d, "His garret was more like a cupboard than a room. "), None).unwrap();
    assert_eq!(d.add_comment(at(&d, "cupboard"), "?", None), Err(EditError::InCut));
    assert!(!d.can_redo());
    ok(&d);
}

/// COM-9
#[test]
fn cut_without_a_trace_takes_a_whole_comment() {
    let mut d = Document::from_text(PAGE);
    d.add_comment(at(&d, "garret"), "Attic?", None).unwrap();
    d.add_comment(at(&d, "The end"), "Stays", None).unwrap();
    let id = d.cut_without_trace(at(&d, "His garret was more like a cupboard than a room. ")).unwrap();
    assert_eq!(d.cut_info(id).unwrap().comments, vec!["Attic?".to_string()]);
    assert_eq!(on_page(&d), vec![("The end".into(), "Stays".into())]);
    ok(&d);
}

/// COM-9
#[test]
fn copies_leave_comments_behind() {
    let mut d = Document::from_text(PAGE);
    d.add_comment(at(&d, "garret"), "Attic?", None).unwrap();
    let f = d.copy(at(&d, "His garret was")).unwrap();
    d.paste(d.len(), &f).unwrap();
    assert_eq!(d.comments().len(), 1);
    ok(&d);
}

/// COM-9
#[test]
fn moving_text_moves_whole_comments() {
    let mut d = Document::from_text(PAGE);
    d.add_comment(at(&d, "His garret"), "Attic?", None).unwrap();
    d.drop_text(Carried::Page(at(&d, "His garret was more like a cupboard than a room.")), Place::Inline(0), false).unwrap();
    assert_eq!(on_page(&d), vec![("His garret".into(), "Attic?".into())]);
    assert!(d.text().starts_with("His garret"));
    ok(&d);
    let mut d = Document::from_text(PAGE);
    d.add_comment(at(&d, "avoided his landlady"), "?", None).unwrap();
    d.drop_text(Carried::Page(at(&d, "landlady")), Place::Inline(d.len()), false).unwrap();
    assert_eq!(on_page(&d)[0].0, "avoided his");
    ok(&d);
}

#[test]
fn a_scrap_carries_its_comments_to_another_document() {
    let mut d = Document::from_text(PAGE);
    d.add_comment(at(&d, "garret"), "Attic?", None).unwrap();
    let scrap = d.cut_scrap(at(&d, "His garret was more like a cupboard than a room. ")).unwrap();
    let mut other = Document::from_text("Elsewhere.");
    other.add_comment(at(&other, "Elsewhere"), "Mine", None).unwrap();
    other.add_scrap(0, &scrap, None).unwrap();
    let cut = other.cuts().into_iter().find(|c| matches!(c.place, CutPlace::Note(_))).unwrap();
    assert_eq!(cut.comments, vec!["Attic?".to_string()]);
    other.unstash(0, other.len()).unwrap();
    let mut got = on_page(&other);
    got.sort();
    assert_eq!(got, vec![("Elsewhere".into(), "Mine".into()), ("garret".into(), "Attic?".into())]);
    ok(&other);
}

/// COM-6
#[test]
fn delete_all_comments_on_the_page() {
    let mut d = Document::from_text(PAGE);
    let g = d.create_group(at(&d, "more like a cupboard than a room"), Scope::Sentence).unwrap();
    d.add_comment(at(&d, "cupboard"), "in a version", None).unwrap();
    d.add_variant_and_show(g, "a closet", Source::Me).unwrap();
    d.add_comment(at(&d, "garret"), "on the page", None).unwrap();
    assert_eq!(d.delete_page_comments(), Ok(2));
    assert!(!d.has_comments());
    d.undo();
    assert!(d.has_comments());
    ok(&d);
}

/// COM-12
#[test]
fn snapshots_keep_comments() {
    let mut d = Document::from_text(PAGE);
    let g = d.create_group(at(&d, "more like a cupboard than a room"), Scope::Sentence).unwrap();
    d.add_comment(at(&d, "cupboard"), "inside", None).unwrap();
    d.add_variant(g, "a closet", Source::Me).unwrap();
    d.add_comment(at(&d, "His garret"), "outside", Some("2026-11-12T10:00:00Z".into())).unwrap();
    let e = Document::from_snapshot(d.snapshot()).unwrap();
    let strip = |d: &Document| d.comments().into_iter().map(|c| (c.range, c.text, c.created)).collect::<Vec<_>>();
    assert_eq!(strip(&e), strip(&d));
    assert_eq!(e.snapshot(), d.snapshot());
    ok(&e);
}

/// COM-1
#[test]
fn writing_a_comment_is_one_step_and_an_empty_one_leaves_none() {
    let mut d = Document::from_text(PAGE);
    d.replace(0..0, "X").unwrap();
    d.break_undo_group();
    let id = d.add_comment(at(&d, "garret"), "", None).unwrap();
    d.replace_in_comment(id, 0..0, "Atic?").unwrap();
    d.replace_in_comment(id, 1..1, "t").unwrap();
    assert_eq!(d.comment_text(id).as_deref(), Some("Attic?"));
    d.finish_comment(id);
    assert_eq!(d.comments().len(), 1);
    d.undo();
    assert!(d.comments().is_empty(), "adding and typing were one step");
    assert!(d.text().starts_with('X'));
    d.redo();
    let id = d.add_comment(at(&d, "landlady"), "", None).unwrap();
    d.finish_comment(id);
    assert_eq!(d.comments().len(), 1);
    d.undo();
    assert!(d.comments().is_empty());
    ok(&d);
}

/// COM-11
#[test]
fn comments_in_the_final_text() {
    let mut d = Document::from_text(PAGE);
    d.add_comment(at(&d, "His garret was more"), "one", None).unwrap();
    d.add_comment(at(&d, "more like a cupboard"), "two", None).unwrap();
    d.cut(at(&d, "He had avoided his landlady. "), None).unwrap();
    let clean = d.clean_text();
    let got: Vec<(String, String)> = d
        .clean_comments()
        .into_iter()
        .map(|(r, c)| (clean.chars().skip(r.start).take(r.len()).collect(), c.text))
        .collect();
    assert_eq!(got, vec![("His garret was more".into(), "one".into()), ("more like a cupboard".into(), "two".into())]);
}
