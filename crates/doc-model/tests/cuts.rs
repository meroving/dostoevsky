use doc_model::{Carried, CutPlace, Document, EditError, Look, Place, Presentation, Scope, Source};

fn find(doc: &Document, needle: &str, n: usize) -> std::ops::Range<usize> {
    let text = doc.text();
    let (byte, _) = text
        .match_indices(needle)
        .nth(n)
        .unwrap_or_else(|| panic!("`{needle}` not in `{text}`"));
    let start = text[..byte].chars().count();
    start..start + needle.chars().count()
}

fn at(doc: &Document, needle: &str) -> std::ops::Range<usize> {
    find(doc, needle, 0)
}

fn ok(doc: &Document) {
    doc.check_invariants().unwrap();
}

const WRITING: Presentation = Presentation { editing: false, look: Look::Text };
const MARKS: Presentation = Presentation { editing: true, look: Look::Mark };

#[test]
fn cut_1_fades_in_place_and_leaves_the_final_text() {
    let mut d = Document::from_text("He had avoided his landlady. His garret was small. The end.");
    let (id, r) = d.cut(at(&d, "His garret was small. "), None).unwrap();
    assert_eq!(d.text(), "He had avoided his landlady. His garret was small. The end.");
    assert_eq!(d.clean_text(), "He had avoided his landlady. The end.");
    assert_eq!(d.ghost_ranges(), vec![r.clone()]);
    let c = d.cut_info(id).unwrap();
    assert_eq!(c.text, "His garret was small. ");
    assert!(c.fresh);
    assert_eq!(c.place, CutPlace::Page { range: r, shown: true });
    d.undo();
    assert!(d.cuts().is_empty());
    ok(&d);
}

#[test]
fn cut_1_grows_to_whole_groups_and_spans_paragraphs() {
    let mut d = Document::from_text("Most of the friction in writing.\n\nSecond one.");
    let g = d.create_group(at(&d, "friction"), Scope::Word).unwrap();
    d.add_variant(g, "hesitation", Source::Me).unwrap();
    let (_, r) = d.cut(at(&d, "the fric"), None).unwrap();
    assert_eq!(r, at(&d, "the friction"));
    assert!(d.group(g).unwrap().ghosted);
    d.undo();
    let (_, r) = d.cut(at(&d, "writing.\n\nSecond"), None).unwrap();
    assert_eq!(r, at(&d, "writing.\n\nSecond"));
    assert_eq!(d.clean_text(), "Most of the friction in one.");
    ok(&d);
}

#[test]
fn cut_4_put_back_and_no_cuts_inside_cuts() {
    let mut d = Document::from_text("One two three.");
    let (id, r) = d.cut(at(&d, "two "), None).unwrap();
    assert_eq!(d.cut_at(r.start + 1), Some(id));
    assert_eq!(d.cut(at(&d, "tw"), None), Err(EditError::InCut));
    assert_eq!(d.put_back(id).unwrap(), Some(r));
    assert!(d.cuts().is_empty());
    assert_eq!(d.clean_text(), "One two three.");
    d.undo();
    assert_eq!(d.cuts().len(), 1, "putting back is one undo step");
    ok(&d);
}

#[test]
fn cut_6_marks_take_no_room_and_paragraphs_close_up() {
    let mut d = Document::from_text("First.\n\nSecond.\n\nThird.\n");
    let (id, _) = d.cut(at(&d, "Second."), None).unwrap();
    let c = d.cut_info(id).unwrap();
    assert!(c.paragraph);
    assert_eq!(c.text, "Second.");
    assert_eq!(d.text(), "First.\n\nSecond.\n\nThird.\n", "fresh: still faded");
    d.set_presentation(MARKS);
    assert_eq!(d.text(), "First.\n\nThird.\n");
    let p = at(&d, "Third").start;
    assert_eq!(d.cut_info(id).unwrap().place, CutPlace::Page { range: p..p, shown: false });
    assert_eq!(d.cut_at(p), Some(id));
    assert_eq!(d.clean_text(), "First.\n\nThird.\n");
    d.put_back(id).unwrap();
    assert_eq!(d.text(), "First.\n\nSecond.\n\nThird.\n");
    ok(&d);

    let mut d = Document::from_text("First.\n\nLast.");
    let (id, _) = d.cut(at(&d, "Last."), None).unwrap();
    d.set_presentation(MARKS);
    assert_eq!(d.text(), "First.");
    assert_eq!(d.cut_info(id).unwrap().text, "Last.");
    ok(&d);
}

#[test]
fn cut_6_writing_shows_only_fresh_cuts() {
    let mut d = Document::from_text("Alpha beta gamma delta.");
    let (old, _) = d.cut(at(&d, "beta "), None).unwrap();
    d.set_presentation(WRITING);
    assert_eq!(d.text(), "Alpha gamma delta.", "older cuts wait for editing");
    let (fresh, _) = d.cut(at(&d, "gamma "), None).unwrap();
    assert_eq!(d.text(), "Alpha gamma delta.", "a cut in writing fades in place");
    d.replace(at(&d, "delta").end..at(&d, "delta").end, " More").unwrap();
    assert!(d.cut_info(fresh).unwrap().fresh);
    assert_eq!(d.ghost_ranges(), vec![at(&d, "gamma ")]);
    d.set_presentation(Presentation::default());
    assert_eq!(d.text(), "Alpha beta gamma delta More.");
    assert!(!d.cut_info(fresh).unwrap().fresh);
    d.set_presentation(MARKS);
    assert_eq!(d.text(), "Alpha delta More.");
    let _ = old;
    ok(&d);
}

/// CUT-6
#[test]
fn cut_7_own_look_overrides_the_setting() {
    let mut d = Document::from_text("Alpha beta gamma.");
    let (id, _) = d.cut(at(&d, "beta "), None).unwrap();
    d.set_look(id, Some(Look::Mark)).unwrap();
    assert_eq!(d.text(), "Alpha gamma.");
    d.set_presentation(MARKS);
    d.set_look(id, Some(Look::Text)).unwrap();
    assert_eq!(d.text(), "Alpha beta gamma.");
    d.undo();
    assert_eq!(d.text(), "Alpha gamma.", "undo re-applies the presentation");
    ok(&d);
}

#[test]
fn cut_7_forget_the_spot_and_delete_for_good() {
    let mut d = Document::from_text("First line here.\n\nAlpha beta gamma.");
    let (id, _) = d.cut(at(&d, "beta "), None).unwrap();
    d.forget_spot(id).unwrap();
    assert_eq!(d.text(), "First line here.\n\nAlpha gamma.");
    let c = d.cut_info(id).unwrap();
    assert_eq!(c.place, CutPlace::Note(0));
    assert_eq!(c.origin.as_deref(), Some("Alpha gamma."));
    assert!(!c.lost);
    d.delete_cut(id).unwrap();
    assert!(d.cuts().is_empty());
    d.undo();
    assert_eq!(d.cuts().len(), 1);
    ok(&d);
}

#[test]
fn cut_11_place_gone_and_backspace_deletes_what_shows() {
    let mut d = Document::from_text("Keep this.\n\nAlpha beta gamma.");
    let (id, _) = d.cut(at(&d, "beta "), Some(Look::Mark)).unwrap();
    d.set_presentation(Presentation::default());
    assert_eq!(d.text(), "Keep this.\n\nAlpha gamma.");
    d.replace(at(&d, "Alpha gamma."), "").unwrap();
    let c = d.cut_info(id).unwrap();
    assert!(c.lost);
    assert_eq!(c.place, CutPlace::Note(0));
    assert_eq!(c.text, "beta ");
    ok(&d);

    let mut d = Document::from_text("Alpha beta gamma.");
    let (id, r) = d.cut(at(&d, "beta "), None).unwrap();
    d.replace(r.start - 1..r.end + 1, "").unwrap();
    assert!(d.cut_info(id).is_none(), "it showed and was deleted");
    assert_eq!(d.text(), "Alphaamma.");
    ok(&d);
}

#[test]
fn cut_11_cuts_in_versions_show_with_them() {
    let mut d = Document::from_text("A long, slow sentence.");
    let g = d.create_group(0..d.len(), Scope::Sentence).unwrap();
    d.add_variant(g, "A short one.", Source::Me).unwrap();
    let (id, _) = d.cut(at(&d, "slow "), None).unwrap();
    d.cycle(g, 1).unwrap();
    assert_eq!(d.text(), "A short one.");
    assert!(d.ghost_ranges().is_empty());
    assert_eq!(d.cut_info(id).unwrap().place, CutPlace::Hidden { at: 0 });
    d.cycle(g, 1).unwrap();
    assert_eq!(d.ghost_ranges(), vec![at(&d, "slow ")]);
    ok(&d);
}

#[test]
fn cut_8_typing_inside_edits_the_cut() {
    let mut d = Document::from_text("Alpha beta gamma.");
    let (id, r) = d.cut(at(&d, "beta "), None).unwrap();
    d.replace(r.start + 2..r.start + 2, "XX").unwrap();
    assert_eq!(d.cut_info(id).unwrap().text, "beXXta ");
    let end = at(&d, "beXXta ").end;
    d.replace(end..end, "new ").unwrap();
    assert_eq!(d.cut_info(id).unwrap().text, "beXXta ");
    assert_eq!(d.clean_text(), "Alpha new gamma.");
    d.replace_in_cut(id, 0..2, "BE").unwrap();
    assert_eq!(d.cut_info(id).unwrap().text, "BEXXta ");
    ok(&d);
}

/// CUT-10, ED-9
#[test]
fn cut_2_moving_a_cut_is_one_step() {
    let mut d = Document::from_text("One two three.\n\nFour five six.");
    let (id, _) = d.cut(at(&d, "two "), None).unwrap();
    let to = at(&d, "six").start;
    let r = d.drop_text(Carried::Cut(id), Place::Inline(to), false).unwrap();
    assert_eq!(d.text(), "One three.\n\nFour five two six.");
    assert_eq!(&d.text()[r.start..r.end], "two");
    assert!(d.cuts().is_empty());
    d.undo();
    assert_eq!(d.text(), "One two three.\n\nFour five six.");
    assert_eq!(d.cuts().len(), 1);
    let to = at(&d, "Four").start;
    d.drop_text(Carried::Cut(id), Place::Block(to), true).unwrap();
    assert_eq!(d.text(), "One two three.\n\ntwo\n\nFour five six.");
    assert_eq!(d.cuts().len(), 1);
    ok(&d);
}

#[test]
fn cut_5_without_a_trace() {
    let mut d = Document::from_text("Alpha beta gamma.");
    let id = d.cut_without_trace(at(&d, "beta ")).unwrap();
    assert_eq!(d.text(), "Alpha gamma.");
    assert_eq!(d.cut_info(id).unwrap().place, CutPlace::Note(0));
    ok(&d);
}

#[test]
fn cut_12_groups_travel() {
    let mut d = Document::from_text("Most of the friction in writing.");
    let g = d.create_group(at(&d, "friction"), Scope::Word).unwrap();
    d.add_variant(g, "hesitation", Source::Me).unwrap();
    let (id, _) = d.cut(at(&d, "the friction "), None).unwrap();
    d.set_presentation(MARKS);
    assert!(d.group_range(g).is_none());
    d.put_back(id).unwrap();
    assert_eq!(d.group(g).unwrap().variants.len(), 2);
    assert_eq!(d.group_range(g), Some(at(&d, "friction")));
    ok(&d);
}

#[test]
fn cut_13_copies_leave_cuts_out_and_moves_carry_them() {
    let mut d = Document::from_text("Alpha beta gamma.\n\nOmega.");
    let (id, _) = d.cut(at(&d, "beta "), None).unwrap();
    assert_eq!(d.copy(at(&d, "Alpha beta gamma.")).unwrap().plain_text(), "Alpha gamma.");
    let to = d.len();
    d.drop_text(Carried::Page(at(&d, "Alpha beta gamma.")), Place::Block(to), false).unwrap();
    assert_eq!(d.text(), "Omega.\n\nAlpha beta gamma.");
    assert!(d.cut_info(id).is_some(), "the cut moved with its text");
    assert_eq!(d.ghost_ranges(), vec![at(&d, "beta ")]);
    ok(&d);
}

#[test]
fn cut_15_a_trim_is_one_batch() {
    let mut d = Document::from_text("a very big and quite red dog");
    let b = doc_model::Batch { id: "t7q2".into(), name: "Trim −20%".into() };
    let ids = d.cut_many(&[at(&d, "very "), at(&d, "quite ")], Some(b.clone())).unwrap();
    assert_eq!(ids.len(), 2);
    assert_eq!(d.clean_text(), "a big and red dog");
    assert!(d.cuts().iter().all(|c| c.batch.as_ref() == Some(&b)));
    d.put_back(ids[0]).unwrap();
    assert_eq!(d.cuts().len(), 1, "each word can go back on its own");
    assert_eq!(d.put_back_batch("t7q2").unwrap(), 1);
    assert!(d.cuts().is_empty());
    d.undo();
    d.undo();
    d.undo();
    assert!(d.cuts().is_empty(), "making the cuts was one step");
    ok(&d);
}

#[test]
fn cut_16_delete_all_cuts_on_the_page() {
    let mut d = Document::from_text("a b c d");
    d.cut(at(&d, "b "), None).unwrap();
    d.cut(at(&d, "d"), None).unwrap();
    d.cut_without_trace(at(&d, "a ")).unwrap();
    assert_eq!(d.delete_page_cuts().unwrap(), 2);
    assert_eq!(d.cuts().len(), 1, "a cut with no trace isn't on the page");
    d.undo();
    assert_eq!(d.cuts().len(), 3);
    ok(&d);
}

/// ADR-002 C3
#[test]
fn cuts_never_nest() {
    let mut d = Document::from_text("one two three four five");
    let (a, _) = d.cut(at(&d, "two "), None).unwrap();
    let (b, _) = d.cut(at(&d, "four "), Some(Look::Mark)).unwrap();
    d.set_presentation(Presentation::default());
    assert_eq!(d.text(), "one two three five");
    let (c, r) = d.cut(at(&d, "one two three "), None).unwrap();
    assert_eq!(r, at(&d, "one two three "));
    assert!(d.cut_info(a).is_none(), "the faded cut joined the new one");
    assert_eq!(d.cut_info(c).unwrap().text, "one two three ");
    assert!(d.cut_info(b).is_some(), "the mark is kept");
    ok(&d);
}

#[test]
fn take_cut_for_another_document() {
    let mut d = Document::from_text("Alpha beta gamma.");
    let (id, _) = d.cut(at(&d, "beta "), None).unwrap();
    let s = d.take_cut(id).unwrap();
    assert_eq!(s.fragment.plain_text(), "beta ");
    assert_eq!(d.text(), "Alpha gamma.");
    assert!(d.cuts().is_empty());
    let mut e = Document::from_text("X");
    e.paste(1, &s.fragment).unwrap();
    assert_eq!(e.text(), "Xbeta ");
    ok(&d);
}

#[test]
fn cut_14_claiming_a_chapters_own_cuts() {
    let mut doc = Document::from_text("One two three. Four five six.");
    doc.cut_without_trace(at(&doc, "One two three.")).unwrap();
    doc.set_cut_source(Some("ch/02.md".into()));
    doc.cut_without_trace(at(&doc, "Four five")).unwrap();
    let sources = |d: &Document| d.cuts().into_iter().map(|c| c.source).collect::<Vec<_>>();
    assert_eq!(sources(&doc), [Some("ch/02.md".to_string()), None]);
    assert_eq!(doc.claim_notes("ch/02.md"), Ok(1));
    assert_eq!(sources(&doc), [Some("ch/02.md".to_string()), Some("ch/02.md".to_string())]);
    ok(&doc);
    doc.undo();
    assert_eq!(sources(&doc), [Some("ch/02.md".to_string()), None]);
    assert_eq!(doc.claim_notes("ch/02.md"), Ok(1));
    assert_eq!(doc.claim_notes("ch/02.md"), Err(EditError::NoSuchNote), "none left");
}

#[test]
fn cut_14_two_cuts_files_read_as_one() {
    let mut a = Document::from_text("Alpha beta.");
    a.cut_without_trace(at(&a, "Alpha")).unwrap();
    let mut b = Document::from_text("Gamma delta.");
    b.cut_without_trace(at(&b, "delta")).unwrap();
    let before = a.undo_tag();
    a.absorb_notes(&b);
    let cuts = a.cuts();
    assert_eq!(cuts.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), ["Alpha", "delta"]);
    assert_ne!(cuts[0].id, cuts[1].id);
    assert_eq!(a.undo_tag(), before, "not an undo step");
    ok(&a);
}

/// A version without the words: the doubled `the`, cut.
fn doubled() -> (Document, doc_model::GroupId) {
    let mut d = Document::from_text("You pick a word, the the sentence moves on.");
    let g = d.create_group(find(&d, "the", 1), Scope::Word).unwrap();
    d.add_variant(g, "whole", Source::Ai).unwrap();
    (d, g)
}

#[test]
fn a_version_without_the_words_is_a_cut_while_it_shows() {
    let (mut d, g) = doubled();
    let v = d.add_cut_version(g, Source::Me).unwrap();
    let info = d.group(g).unwrap();
    assert_eq!(info.variants.len(), 3);
    assert!(info.variants[2].cut && info.variants[2].id == v);
    assert_eq!(info.variants[2].text, "the");
    // One per group.
    assert_eq!(d.add_cut_version(g, Source::Ai), Err(EditError::Unchanged));
    // Not showing: a version, not a cut on the page.
    let c = d.cuts()[0].clone();
    assert_eq!(c.version, Some(g));
    assert!(matches!(c.place, CutPlace::Hidden { .. }));
    assert!(d.traces().is_empty());
    d.set_active(g, 2).unwrap();
    assert_eq!(d.text(), "You pick a word, the the sentence moves on.");
    assert_eq!(d.clean_text(), "You pick a word, the sentence moves on.");
    let r = d.group_range(g).unwrap();
    assert_eq!(d.ghost_ranges(), vec![r.clone()]);
    assert_eq!(d.cut_info(c.id).unwrap().place, CutPlace::Page { range: r, shown: true });
    let vg = d.visible_groups().into_iter().find(|v| v.id == g).unwrap();
    assert!(vg.cut);
    // As a mark it takes no room.
    d.set_presentation(MARKS);
    assert_eq!(d.text(), "You pick a word, the  sentence moves on.");
    assert_eq!(d.clean_text(), "You pick a word, the sentence moves on.");
    assert!(d.group_range(g).unwrap().is_empty());
    ok(&d);
}

#[test]
fn a_version_without_the_words_puts_back_the_original() {
    let (mut d, g) = doubled();
    d.add_cut_version(g, Source::Me).unwrap();
    d.set_active(g, 2).unwrap();
    let id = d.cuts()[0].id;
    let r = d.put_back(id).unwrap();
    assert_eq!(d.group(g).unwrap().active, 0);
    assert_eq!(r, d.group_range(g));
    assert_eq!(d.group(g).unwrap().variants.len(), 3, "the version stays");
    // ⌘/ on the trace does the same.
    d.set_active(g, 2).unwrap();
    d.revive(d.group_range(g).unwrap()).unwrap();
    assert_eq!(d.group(g).unwrap().active, 0);
    ok(&d);
}

#[test]
fn settling_on_a_version_without_the_words_leaves_an_ordinary_cut() {
    let (mut d, g) = doubled();
    d.add_cut_version(g, Source::Me).unwrap();
    d.set_active(g, 2).unwrap();
    d.settle(g).unwrap();
    assert!(d.group(g).is_none());
    let c = d.cuts()[0].clone();
    assert_eq!(c.version, None);
    assert!(matches!(c.place, CutPlace::Page { shown: true, .. }));
    assert_eq!(d.clean_text(), "You pick a word, the sentence moves on.");
    d.put_back(c.id).unwrap();
    assert_eq!(d.clean_text(), "You pick a word, the the sentence moves on.");
    ok(&d);
}

#[test]
fn deleting_a_version_without_the_words_for_good_settles_on_it() {
    let (mut d, g) = doubled();
    d.add_cut_version(g, Source::Me).unwrap();
    d.set_active(g, 2).unwrap();
    let id = d.cuts()[0].id;
    d.delete_cut(id).unwrap();
    assert!(d.group(g).is_none());
    assert!(d.cuts().is_empty());
    assert_eq!(d.text(), "You pick a word, the  sentence moves on.");
    assert_eq!(d.clean_text(), "You pick a word, the  sentence moves on.");
    d.undo();
    assert_eq!(d.group(g).unwrap().active, 2);
    // Deleting the page's cuts leaves one that isn't showing: it's a version, not a cut.
    d.set_active(g, 0).unwrap();
    assert_eq!(d.delete_page_cuts().unwrap(), 0);
    assert_eq!(d.group(g).unwrap().variants.len(), 3);
    ok(&d);
}

#[test]
fn a_version_without_the_words_stays_one() {
    let (mut d, g) = doubled();
    d.add_cut_version(g, Source::Me).unwrap();
    assert_eq!(d.make_original(g, 2), Err(EditError::InCut));
    assert_eq!(d.edit_variant(g, 2, "a"), Err(EditError::InCut));
    d.remove_variant(g, 2).unwrap();
    assert!(d.cuts().is_empty());
    // Not inside a cut.
    let mut e = Document::from_text("One two three.");
    let h = e.create_group(at(&e, "two"), Scope::Word).unwrap();
    e.add_variant(h, "2", Source::Me).unwrap();
    e.cut(at(&e, "One two"), None).unwrap();
    assert_eq!(e.add_cut_version(h, Source::Me), Err(EditError::InCut));
    ok(&d);
}
