use doc_model::{Document, EditError, GroupId, Scope, Source};

/// Display (char) range of the `n`th occurrence of `needle`.
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

fn friction_doc() -> (Document, GroupId) {
    let mut d = Document::from_text("Most of the friction in writing.");
    let g = d.create_group(at(&d, "friction"), Scope::Word).unwrap();
    d.add_variant(g, "hesitation", Source::Me).unwrap();
    d.add_variant(g, "struggle", Source::Ai).unwrap();
    d.break_undo_group();
    (d, g)
}

#[test]
fn cycling_swaps_text_in_place_and_wraps() {
    let (mut d, g) = friction_doc();
    assert!(!d.cycle(g, 1).unwrap().is_original);
    assert_eq!(d.text(), "Most of the hesitation in writing.");
    d.cycle(g, 1).unwrap();
    assert_eq!(d.text(), "Most of the struggle in writing.");
    let c = d.cycle(g, 1).unwrap();
    assert!(c.is_original, "wraps back to the original → low tone");
    assert_eq!(d.text(), "Most of the friction in writing.");
    assert_eq!(d.cycle(g, -1).unwrap().active, 2);
    ok(&d);
}

#[test]
fn visible_groups_report_display_ranges() {
    let (mut d, g) = friction_doc();
    d.cycle(g, 1).unwrap();
    let v = d.visible_groups();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].range, at(&d, "hesitation"));
    assert_eq!((v[0].active, v[0].variant_count, v[0].depth), (1, 3, 0));
    let info = d.group(g).unwrap();
    assert_eq!(
        info.variants
            .iter()
            .map(|v| v.text.as_str())
            .collect::<Vec<_>>(),
        ["friction", "hesitation", "struggle"]
    );
    assert_eq!(info.variants[2].by, Source::Ai);
}

#[test]
fn a_cycling_run_is_one_undo_step() {
    let (mut d, g) = friction_doc();
    let before = d.text();
    d.cycle(g, 1).unwrap();
    d.cycle(g, 1).unwrap();
    assert!(d.undo());
    assert_eq!(d.text(), before);
    assert!(d.redo());
    assert_eq!(d.text(), "Most of the struggle in writing.");
}

#[test]
fn a_run_back_to_the_start_leaves_no_undo_step() {
    let (mut d, g) = friction_doc();
    d.replace(0..0, "X").unwrap();
    d.break_undo_group();
    for _ in 0..3 {
        d.cycle(g, 1).unwrap();
    }
    assert_eq!(d.text(), "XMost of the friction in writing.");
    assert!(d.undo());
    assert_eq!(
        d.text(),
        "Most of the friction in writing.",
        "undo skipped the empty cycling run"
    );
}

#[test]
fn type_in_b_cycle_to_c_then_undo_twice() {
    let (mut d, g) = friction_doc();
    d.cycle(g, 1).unwrap();
    let end = at(&d, "hesitation").end;
    d.break_undo_group();
    d.replace(end - 1..end, "ion?").unwrap();
    assert_eq!(d.group(g).unwrap().variants[1].text, "hesitatioion?");
    d.cycle(g, 1).unwrap();
    assert!(d.undo());
    assert_eq!(d.text(), "Most of the hesitatioion? in writing.");
    assert!(d.undo());
    assert_eq!(d.text(), "Most of the hesitation in writing.");
    ok(&d);
}

#[test]
fn word_chars_at_a_word_edge_join_the_group() {
    let (mut d, g) = friction_doc();
    let end = at(&d, "friction").end;
    d.replace(end..end, "s").unwrap();
    assert_eq!(d.group(g).unwrap().variants[0].text, "frictions");
    let start = at(&d, "frictions").start;
    d.replace(start..start, "re").unwrap();
    assert_eq!(d.group(g).unwrap().variants[0].text, "refrictions");
}

#[test]
fn spaces_and_punctuation_at_an_edge_stay_outside() {
    let (mut d, g) = friction_doc();
    let end = at(&d, "friction").end;
    d.replace(end..end, ",").unwrap();
    d.replace(end..end, " ").unwrap();
    assert_eq!(d.group(g).unwrap().variants[0].text, "friction");
    assert_eq!(d.text(), "Most of the friction , in writing.");
}

#[test]
fn sentence_groups_do_not_grow_at_edges() {
    let mut d = Document::from_text("One. Two.");
    let g = d.create_group(at(&d, "One."), Scope::Sentence).unwrap();
    d.replace(4..4, "x").unwrap();
    assert_eq!(d.group(g).unwrap().variants[0].text, "One.");
    d.replace(2..2, "x").unwrap();
    assert_eq!(d.group(g).unwrap().variants[0].text, "Onxe.");
}

#[test]
fn deleting_a_whole_span_deletes_the_group() {
    let (mut d, g) = friction_doc();
    d.replace(at(&d, "friction"), "").unwrap();
    assert_eq!(d.text(), "Most of the  in writing.");
    assert!(d.group(g).is_none());
    assert!(d.undo());
    assert_eq!(
        d.group(g).unwrap().variants.len(),
        3,
        "undo restores every variant"
    );
}

#[test]
fn typing_over_a_selected_span_edits_the_active_variant() {
    let (mut d, g) = friction_doc();
    d.cycle(g, 1).unwrap();
    d.replace(at(&d, "hesitation"), "tension").unwrap();
    let info = d.group(g).unwrap();
    assert_eq!(info.variants[1].text, "tension");
    assert_eq!(
        info.variants[0].text, "friction",
        "other variants untouched"
    );
}

#[test]
fn deleting_across_an_edge_clips_only_the_active_variant() {
    let (mut d, g) = friction_doc();
    d.replace(at(&d, "the fric"), "").unwrap();
    assert_eq!(d.text(), "Most of tion in writing.");
    let info = d.group(g).unwrap();
    assert_eq!(info.variants[0].text, "tion");
    assert_eq!(info.variants[1].text, "hesitation");
    ok(&d);
}

#[test]
fn replacing_across_a_whole_group_removes_it_and_inserts_outside() {
    let (mut d, g) = friction_doc();
    d.replace(at(&d, "the friction in"), "a mess at").unwrap();
    assert_eq!(d.text(), "Most of a mess at writing.");
    assert!(d.group(g).is_none());
    assert!(d.visible_groups().is_empty());
}

#[test]
fn nested_groups_belong_to_their_paragraph_variant() {
    let mut d = Document::from_text("Software forgot this.\n\nNext.");
    let para = d
        .create_group(at(&d, "Software forgot this."), Scope::Paragraph)
        .unwrap();
    let word = d.create_group(at(&d, "forgot"), Scope::Word).unwrap();
    d.add_variant(word, "lost", Source::Me).unwrap();
    d.add_variant(para, "Software never learned it.", Source::Me)
        .unwrap();
    assert_eq!(
        d.visible_groups().iter().map(|g| g.id).collect::<Vec<_>>(),
        [para, word]
    );

    d.cycle(para, 1).unwrap();
    assert_eq!(d.text(), "Software never learned it.\n\nNext.");
    assert_eq!(
        d.visible_groups().iter().map(|g| g.id).collect::<Vec<_>>(),
        [para],
        "word group hidden with variant A"
    );
    assert!(d.group(word).is_some(), "but still exists");

    d.cycle(word, 1).unwrap();
    assert_eq!(d.text(), "Software never learned it.\n\nNext.");
    d.cycle(para, 1).unwrap();
    assert_eq!(
        d.text(),
        "Software lost this.\n\nNext.",
        "back on A, with the nested group's active variant"
    );
    ok(&d);
}

#[test]
fn ghosts_inside_a_variant_travel_with_it() {
    let mut d = Document::from_text("Keep this. Drop that.");
    let para = d.create_group(0..d.len(), Scope::Paragraph).unwrap();
    d.ghost(at(&d, "Drop that.")).unwrap();
    d.add_variant(para, "Short.", Source::Me).unwrap();
    d.cycle(para, 1).unwrap();
    assert!(d.ghost_ranges().is_empty());
    d.cycle(para, 1).unwrap();
    assert_eq!(d.ghost_ranges(), vec![at(&d, "Drop that.")]);
}

#[test]
fn creating_overlapping_groups_is_rejected() {
    let (mut d, g) = friction_doc();
    assert_eq!(
        d.create_group(at(&d, "the fric"), Scope::Word),
        Err(EditError::PartialOverlap)
    );
    assert_eq!(
        d.create_group(at(&d, "friction"), Scope::Word),
        Err(EditError::AlreadyGroup(g))
    );
    let outer = d
        .create_group(at(&d, "the friction in"), Scope::Sentence)
        .unwrap();
    let v = d.visible_groups();
    assert_eq!((v[0].id, v[1].id, v[1].depth), (outer, g, 1));
    ok(&d);
}

#[test]
fn return_inside_a_paragraph_variant_keeps_the_group() {
    let mut d = Document::from_text("First half. Second half.");
    let g = d.create_group(0..d.len(), Scope::Paragraph).unwrap();
    d.add_variant(g, "Other.", Source::Me).unwrap();
    let pos = at(&d, " Second").start;
    d.replace(pos..pos + 1, "\n\n").unwrap();
    assert_eq!(
        d.group(g).unwrap().variants[0].text,
        "First half.\n\nSecond half."
    );
    assert_eq!(d.visible_groups()[0].range, 0..d.len());
    ok(&d);
}

#[test]
fn removing_variants_and_dissolving() {
    let (mut d, g) = friction_doc();
    d.cycle(g, 1).unwrap();
    let r = d.remove_variant(g, 1).unwrap();
    assert_eq!((r.text.as_str(), r.dissolved), ("hesitation", false));
    assert_eq!(
        d.text(),
        "Most of the friction in writing.",
        "active moves to the previous variant"
    );
    let r = d.remove_variant(g, 0).unwrap();
    assert!(r.dissolved, "one left → plain text");
    assert_eq!(d.text(), "Most of the struggle in writing.");
    assert!(d.group(g).is_none());
    assert!(d.undo());
    assert_eq!(
        d.group(g).unwrap().variants.len(),
        2,
        "⌘Z brings the exploded variant back"
    );
    assert_eq!(d.remove_variant(g, 9), Err(EditError::NoSuchVariant));
}

#[test]
fn removing_the_original_promotes_the_next() {
    let (mut d, g) = friction_doc();
    d.cycle(g, 2).unwrap();
    d.remove_variant(g, 0).unwrap();
    let info = d.group(g).unwrap();
    assert_eq!(info.variants[0].text, "hesitation");
    assert_eq!(info.variants[info.active].text, "struggle");
}

#[test]
fn settle_keeps_the_active_text_and_nested_structure() {
    let mut d = Document::from_text("Software forgot this.");
    let para = d.create_group(0..d.len(), Scope::Paragraph).unwrap();
    let word = d.create_group(at(&d, "forgot"), Scope::Word).unwrap();
    d.add_variant(word, "lost", Source::Me).unwrap();
    d.add_variant(para, "Other.", Source::Me).unwrap();
    d.settle(para).unwrap();
    assert!(d.group(para).is_none());
    assert_eq!(
        d.visible_groups()[0].id,
        word,
        "nested group survives settling"
    );
    assert_eq!(d.visible_groups()[0].depth, 0);
    ok(&d);
}

#[test]
fn ghost_and_revive() {
    let (mut d, _) = friction_doc();
    d.ghost(at(&d, "in writing")).unwrap();
    assert_eq!(d.ghost_ranges(), vec![at(&d, "in writing")]);
    assert_eq!(d.clean_text(), "Most of the friction.", "no space left before the full stop");
    let caret = at(&d, "writing").start + 2;
    let r = d.ghost_range_at(caret).unwrap();
    d.revive(r).unwrap();
    assert!(d.ghost_ranges().is_empty());
    ok(&d);
}

#[test]
fn ghosting_part_of_a_group_ghosts_the_whole_group() {
    let (mut d, g) = friction_doc();
    d.ghost(at(&d, "the fric")).unwrap();
    assert_eq!(d.ghost_ranges(), vec![at(&d, "the friction")]);
    assert!(d.visible_groups()[0].ghosted);
    d.cycle(g, 1).unwrap();
    assert_eq!(
        d.ghost_ranges(),
        vec![at(&d, "the hesitation")],
        "the flag is on the group, not the variant"
    );
}

#[test]
fn reviving_inside_a_cut_puts_back_the_whole_cut() {
    let mut d = Document::from_text("Alpha beta gamma.");
    let g = d.create_group(0..d.len(), Scope::Sentence).unwrap();
    d.ghost(0..d.len()).unwrap();
    assert!(d.group(g).unwrap().ghosted);
    d.revive(at(&d, "beta")).unwrap();
    assert!(d.ghost_ranges().is_empty());
    assert!(!d.group(g).unwrap().ghosted);
    ok(&d);
}

#[test]
fn paste_copies_groups_with_fresh_ids() {
    let (mut d, g) = friction_doc();
    let frag = d.copy(at(&d, "the friction")).unwrap();
    assert_eq!(frag.plain_text(), "the friction");
    let end = d.len();
    d.paste(end, &frag).unwrap();
    assert_eq!(d.text(), "Most of the friction in writing.the friction");
    let ids: Vec<_> = d.visible_groups().iter().map(|v| v.id).collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[1], g);
    assert_eq!(d.group(ids[1]).unwrap().variants.len(), 3);
    ok(&d);
}

#[test]
fn stash_keeps_alternatives_and_unstash_restores_them() {
    let mut d = Document::from_text("Intro.\n\nMost of the friction in writing.");
    let g = d.create_group(at(&d, "friction"), Scope::Word).unwrap();
    d.add_variant(g, "pain", Source::Me).unwrap();
    let r = at(&d, "Most of the friction");
    d.stash(r).unwrap();
    assert_eq!(d.text(), "Intro.\n\n in writing.");
    let notes = d.notes();
    assert_eq!(notes[0].text, "Most of the friction");
    assert_eq!(notes[0].origin.as_deref(), Some(" in writing."), "what is left of the paragraph");
    assert!(d.visible_groups().is_empty());
    d.unstash(0, 0).unwrap();
    assert_eq!(d.text(), "Most of the frictionIntro.\n\n in writing.");
    let v = d.visible_groups();
    assert_eq!(
        d.group(v[0].id).unwrap().variants.len(),
        2,
        "alternatives came back"
    );
    assert!(d.notes().is_empty());
    ok(&d);
}

#[test]
fn errors_leave_history_untouched() {
    let (mut d, g) = friction_doc();
    d.undo();
    d.undo();
    d.undo();
    assert!(!d.can_undo());
    d.redo();
    assert_eq!(d.replace(0..999, "x"), Err(EditError::OutOfBounds));
    assert_eq!(d.add_variant(g, "", Source::Me), Err(EditError::EmptyText));
    assert!(d.can_redo(), "a failed edit doesn't clear redo");
}

#[test]
fn make_original_keeps_the_active_variant() {
    let mut d = Document::from_text("a friction here");
    let id = d.create_group(2..10, Scope::Word).unwrap();
    d.add_variant(id, "tension", Source::Me).unwrap();
    d.add_variant(id, "unease", Source::Me).unwrap();
    d.set_active(id, 1).unwrap();
    d.make_original(id, 2).unwrap();
    let g = d.group(id).unwrap();
    let texts: Vec<_> = g.variants.iter().map(|v| v.text.as_str()).collect();
    assert_eq!(texts, ["unease", "friction", "tension"]);
    assert_eq!(g.active, 2);
    assert_eq!(d.text(), "a tension here");
    d.undo();
    assert_eq!(d.group(id).unwrap().variants[0].text, "friction");
    d.check_invariants().unwrap();
}

/// ALT-23
#[test]
fn moving_a_variant_reorders_the_rest() {
    let mut d = Document::from_text("a friction here");
    let id = d.create_group(2..10, Scope::Word).unwrap();
    d.add_variant(id, "tension", Source::Me).unwrap();
    d.add_variant(id, "unease", Source::Me).unwrap();
    d.set_active(id, 1).unwrap();
    d.move_variant(id, 1, 2).unwrap();
    let g = d.group(id).unwrap();
    let texts: Vec<_> = g.variants.iter().map(|v| v.text.as_str()).collect();
    assert_eq!(texts, ["friction", "unease", "tension"]);
    assert_eq!(g.active, 2, "the same variant stays active");
    assert_eq!(d.text(), "a tension here");
    d.move_variant(id, 2, 1).unwrap();
    assert_eq!(d.group(id).unwrap().active, 1);
    d.undo();
    let g = d.group(id).unwrap();
    let texts: Vec<_> = g.variants.iter().map(|v| v.text.as_str()).collect();
    assert_eq!(texts, ["friction", "unease", "tension"]);
    assert_eq!(g.active, 2);
    d.undo();
    assert_eq!(d.group(id).unwrap().variants[1].text, "tension");
    d.check_invariants().unwrap();
}

/// ALT-23
#[test]
fn a_move_onto_itself_leaves_no_step() {
    let mut d = Document::from_text("a friction here");
    let g = d.create_group(2..10, Scope::Word).unwrap();
    d.add_variant(g, "tension", Source::Me).unwrap();
    d.move_variant(g, 1, 1).unwrap();
    // The first undo takes the added variant, not an empty step; the second, the group.
    d.undo();
    assert_eq!(d.group(g).unwrap().variants.len(), 1);
    d.undo();
    assert!(!d.can_undo());
    assert_eq!(d.text(), "a friction here");
    ok(&d);
}

/// ALT-23
#[test]
fn moving_never_changes_the_original() {
    let (mut d, g) = friction_doc();
    assert_eq!(d.move_variant(g, 0, 1), Err(EditError::NoSuchVariant));
    assert_eq!(d.move_variant(g, 1, 0), Err(EditError::NoSuchVariant));
    assert_eq!(d.move_variant(g, 1, 99), Err(EditError::NoSuchVariant));
    assert_eq!(d.group(g).unwrap().variants[0].text, "friction");
    d.check_invariants().unwrap();
}

/// ALT-26
#[test]
fn edit_variant_rewrites_shown_and_hidden_variants() {
    let (mut d, g) = friction_doc();
    d.edit_variant(g, 2, "struggles").unwrap();
    assert_eq!(d.group(g).unwrap().variants[2].text, "struggles");
    assert_eq!(d.text(), "Most of the friction in writing.", "a hidden variant changes out of sight");

    d.edit_variant(g, 0, "the drag").unwrap();
    assert_eq!(d.text(), "Most of the the drag in writing.");
    assert_eq!(d.visible_groups()[0].range, at(&d, "the drag"));
    d.undo();
    assert_eq!(d.text(), "Most of the friction in writing.");
    d.undo();
    assert_eq!(d.group(g).unwrap().variants[2].text, "struggle");

    assert_eq!(d.edit_variant(g, 0, ""), Err(EditError::EmptyText));
    assert_eq!(d.edit_variant(g, 9, "x"), Err(EditError::NoSuchVariant));
    ok(&d);
}

/// ALT-26
#[test]
fn edit_variant_keeps_nested_groups_it_does_not_touch() {
    let mut d = Document::from_text("Software forgot this.\n\nNext.");
    let para = d.create_group(at(&d, "Software forgot this."), Scope::Paragraph).unwrap();
    let word = d.create_group(at(&d, "forgot"), Scope::Word).unwrap();
    d.add_variant(word, "lost", Source::Me).unwrap();
    d.edit_variant(para, 0, "Software forgot this, again.").unwrap();
    assert_eq!(d.text(), "Software forgot this, again.\n\nNext.");
    assert_eq!(d.visible_groups().iter().map(|g| g.id).collect::<Vec<_>>(), [para, word]);
    ok(&d);
}

#[test]
fn remove_ghosted_is_one_step() {
    let mut d = Document::from_text("keep this, drop that, keep, drop.");
    d.ghost(11..21).unwrap();
    d.ghost(28..32).unwrap();
    assert_eq!(d.remove_ghosted().unwrap(), 14);
    assert_eq!(d.text(), "keep this,  keep, .");
    assert!(d.ghost_ranges().is_empty());
    d.undo();
    assert_eq!(d.text(), "keep this, drop that, keep, drop.");
    assert_eq!(d.ghost_ranges().len(), 2);
}

#[test]
fn notes_add_remove_move_copy() {
    let mut d = Document::from_text("page");
    d.add_note(0, "one").unwrap();
    d.add_note(1, "two").unwrap();
    d.move_note(1, 0).unwrap();
    assert_eq!(d.notes().iter().map(|n| n.text.as_str()).collect::<Vec<_>>(), ["two", "one"]);
    d.copy_note_to(1, 4).unwrap();
    assert_eq!(d.text(), "pageone");
    assert_eq!(d.notes().len(), 2);
    d.remove_note(0).unwrap();
    assert_eq!(d.notes().len(), 1);
    assert!(d.add_note(5, "x").is_err());
    d.check_invariants().unwrap();
}

/// ALT-15, ALT-20
#[test]
fn add_and_show_is_its_own_step() {
    let mut d = Document::from_text("It was a good idea.");
    let g = d.create_group(at(&d, "good"), Scope::Word).unwrap();
    d.break_undo_group();
    d.add_variant_and_show(g, "great", Source::Me).unwrap();
    assert_eq!(d.text(), "It was a great idea.");
    d.cycle(g, 1).unwrap();
    d.cycle(g, 1).unwrap();
    assert_eq!(d.text(), "It was a great idea.");
    d.cycle(g, 1).unwrap();
    d.undo();
    assert_eq!(d.text(), "It was a great idea.", "undoing the run returns to the new variant");
    d.undo();
    assert_eq!(d.text(), "It was a good idea.", "one more undo removes it");
    assert_eq!(d.group(g).unwrap().variants.len(), 1);
    ok(&d);
}

/// CUT-13, FILE-8
#[test]
fn clean_text_tidies_around_ghosts() {
    let clean = |text: &str, ghost: &str| {
        let mut d = Document::from_text(text);
        let r = at(&d, ghost);
        d.ghost(r).unwrap();
        d.clean_text()
    };
    assert_eq!(clean("One.\n\nTwo.\n", "Two."), "One.\n");
    assert_eq!(clean("One.\n\nTwo.\n\nThree.\n", "Two."), "One.\n\nThree.\n");
    assert_eq!(clean("One.\n\nTwo.\n", "One."), "Two.\n");
    assert_eq!(clean("One. Two. Three.\n", "Two."), "One. Three.\n");
    assert_eq!(clean("Choose too early.\n", " early"), "Choose too.\n");
    assert_eq!(clean("frictionless\n", "less"), "friction\n");
    assert_eq!(clean("A  b.\n\n\n\nC. D.\n", "D."), "A  b.\n\n\n\nC.\n");
    assert_eq!(Document::from_text("A\n\n\nB  \n").clean_text(), "A\n\n\nB  \n");
}

/// CUT-11
#[test]
fn stash_origin_names_what_is_left() {
    let text = "First paragraph here.\n\nSecond one. More of it.\n\nThird paragraph.";
    let mut d = Document::from_text(text);
    d.stash(at(&d, "Second one. More of it.")).unwrap();
    assert_eq!(d.notes()[0].origin.as_deref(), Some("Third paragraph."), "the paragraph now in its place");

    let mut d = Document::from_text(text);
    d.stash(at(&d, "Second one. ")).unwrap();
    assert_eq!(d.notes()[0].origin.as_deref(), Some("More of it."), "what is left of it");

    let mut d = Document::from_text(text);
    d.stash(at(&d, "Third paragraph.")).unwrap();
    assert_eq!(d.notes()[0].origin.as_deref(), Some("Second one. More of it."), "the last one: the previous paragraph");

    let mut d = Document::from_text(text);
    d.stash(at(&d, "More of it.")).unwrap();
    assert_eq!(d.notes()[0].origin.as_deref(), Some("Second one. "), "a later sentence: the paragraph itself");
    ok(&d);
}

#[test]
fn ideas_for_a_group_undo_together_and_never_show() {
    let (mut d, g) = friction_doc();
    for idea in ["tension", "drag", "resistance"] {
        d.add_idea(g, idea).unwrap();
    }
    ok(&d);
    let info = d.group(g).unwrap();
    assert_eq!(info.variants.len(), 6);
    assert_eq!(info.active, 0, "ideas never show by themselves");
    assert!(info.variants[3..].iter().all(|v| v.by == Source::Ai));
    assert!(d.undo());
    assert_eq!(d.group(g).unwrap().variants.len(), 3, "one ⌘Z removes the whole batch");
}

#[test]
fn ideas_after_cycling_start_a_new_undo_step() {
    let (mut d, g) = friction_doc();
    d.add_idea(g, "tension").unwrap();
    d.cycle(g, 1).unwrap();
    d.add_idea(g, "drag").unwrap();
    assert!(d.undo());
    assert_eq!(d.group(g).unwrap().variants.len(), 4);
}

#[test]
fn typo_fixes_become_groups_showing_the_fix() {
    let mut d = Document::from_text("You pick teh word and it dont matter.");
    let fixes = vec![(at(&d, "teh"), "the".to_string()), (at(&d, "dont"), "doesn't".to_string())];
    let ids = d.fix_words(&fixes).unwrap();
    ok(&d);
    assert_eq!(ids.len(), 2);
    assert_eq!(d.text(), "You pick the word and it doesn't matter.");
    let g = d.group(ids[0]).unwrap();
    assert_eq!((g.variants[0].text.as_str(), g.variants[0].by), ("teh", Source::Me));
    assert_eq!((g.variants[1].text.as_str(), g.variants[1].by), ("the", Source::Ai));
    assert_eq!(d.cycle(ids[0], 1).unwrap().active, 0, "↑ brings the typo back");
    assert_eq!(d.text(), "You pick teh word and it doesn't matter.");
    assert!(d.undo());
    assert!(d.undo());
    assert_eq!(d.text(), "You pick teh word and it dont matter.", "one ⌘Z undoes the whole pass");
}

#[test]
fn typo_fixes_skip_what_cant_be_grouped() {
    let (mut d, g) = friction_doc();
    let r = at(&d, "ction in");
    let ids = d.fix_words(&[(r, "ction of".into()), (at(&d, "writing"), "writng".into())]).unwrap();
    assert_eq!(ids.len(), 1, "the partial overlap with the group is skipped");
    assert!(d.group(g).is_some());
    ok(&d);
}

#[test]
fn offered_words_become_groups_keeping_the_original() {
    let mut d = Document::from_text("The utilise of words is paramount here.");
    let offers = vec![
        (at(&d, "utilise"), vec!["use".to_string(), "choice".to_string()]),
        (at(&d, "paramount"), vec!["what matters".to_string()]),
        (at(&d, "here"), vec![]),
    ];
    let ids = d.offer_words(&offers).unwrap();
    ok(&d);
    assert_eq!(ids.len(), 2, "a range with no alternatives is skipped");
    assert_eq!(d.text(), "The utilise of words is paramount here.", "the original still shows");
    let g = d.group(ids[0]).unwrap();
    assert_eq!(g.active, 0);
    let texts: Vec<(&str, Source)> = g.variants.iter().map(|v| (v.text.as_str(), v.by)).collect();
    assert_eq!(texts, [("utilise", Source::Me), ("use", Source::Ai), ("choice", Source::Ai)]);
    assert!(d.undo());
    assert!(d.visible_groups().is_empty(), "one ⌘Z undoes them all");
}

#[test]
fn offered_sentences_become_sentence_groups_around_their_word_groups() {
    let mut d = Document::from_text("Short one. The utilise of words, which is in a way the thing, matters. End.");
    let w = d.offer_words(&[(at(&d, "utilise"), vec!["use".to_string()])]).unwrap()[0];
    let s = at(&d, "The utilise of words, which is in a way the thing, matters.");
    let ids = d.offer_sentences(&[(s, vec!["The use of words matters.".to_string()])]).unwrap();
    ok(&d);
    assert_eq!(ids.len(), 1);
    let g = d.group(ids[0]).unwrap();
    assert_eq!(g.scope, Scope::Sentence);
    assert_eq!(g.active, 0, "the original still shows");
    assert_eq!(g.variants[1].text, "The use of words matters.");
    assert!(d.group(w).is_some(), "the word group goes along inside the sentence");
    assert_eq!(d.text(), "Short one. The utilise of words, which is in a way the thing, matters. End.");
    assert!(d.undo());
    assert!(d.group(ids[0]).is_none() && d.group(w).is_some(), "one ⌘Z for the sentences");
}

#[test]
fn settle_many_and_ghost_many_are_one_step() {
    let mut d = Document::from_text("You pick teh word and it dont matter at all.");
    let ids = d.fix_words(&[(at(&d, "teh"), "the".into()), (at(&d, "dont"), "doesn't".into())]).unwrap();
    assert_eq!(d.settle_many(&ids).unwrap(), 2);
    assert!(d.visible_groups().is_empty());
    assert_eq!(d.text(), "You pick the word and it doesn't matter at all.");
    d.ghost_many(&[at(&d, " at all"), at(&d, "pick ")]).unwrap();
    assert_eq!(d.ghost_ranges().len(), 2);
    assert!(d.undo());
    assert!(d.ghost_ranges().is_empty());
    assert_eq!(d.ghost_many(&[0..2, 0..999]), Err(EditError::OutOfBounds));
    ok(&d);
}

#[test]
fn settle_to_picks_a_variant_in_one_step() {
    let mut d = Document::from_text("You pick teh word and it dont matter at all.");
    let ids = d.fix_words(&[(at(&d, "teh"), "the".into()), (at(&d, "dont"), "doesn't".into())]).unwrap();
    assert_eq!(d.settle_to(&ids[..1], 0).unwrap(), 1);
    assert_eq!(d.text(), "You pick teh word and it doesn't matter at all.", "the original back, the other fix still showing");
    assert_eq!(d.visible_groups().len(), 1);
    assert_eq!(d.settle_to(&ids, 7).unwrap(), 1, "the gone one is skipped; no variant 7 keeps what shows");
    assert_eq!(d.text(), "You pick teh word and it doesn't matter at all.");
    assert!(d.undo());
    assert!(d.undo());
    assert_eq!(d.text(), "You pick the word and it doesn't matter at all.", "one ⌘Z each");
    ok(&d);
}

const PARAS: &str = "Most of the friction comes from choosing too early.\n\nEditors have always known this. A good editor doesn't erase your sentence. They write in the margin.\n\nSoftware forgot this. Undo takes you back.\n";

#[test]
fn drag_a_sentence_between_words() {
    let mut d = Document::from_text(PARAS);
    let from = at(&d, "Software forgot this.");
    let to = at(&d, "They write").start;
    let now = d
        .drop_text(
            doc_model::Carried::Page(from),
            doc_model::Place::Inline(to),
            false,
        )
        .unwrap();
    ok(&d);
    assert_eq!(
        d.text(),
        "Most of the friction comes from choosing too early.\n\nEditors have always known this. A good editor doesn't erase your sentence. Software forgot this. They write in the margin.\n\nUndo takes you back.\n"
    );
    assert_eq!(now, at(&d, "Software forgot this."));
    d.undo();
    assert_eq!(d.text(), PARAS);
}

#[test]
fn drag_fixes_spaces_around_punctuation() {
    let mut d = Document::from_text("One two three.\n");
    let from = at(&d, " two");
    let to = at(&d, ".").start;
    d.drop_text(
        doc_model::Carried::Page(from),
        doc_model::Place::Inline(to),
        false,
    )
    .unwrap();
    assert_eq!(d.text(), "One three two.\n");
}

#[test]
fn drag_a_paragraph_between_paragraphs() {
    let mut d = Document::from_text(PARAS);
    let from = at(&d, "Software forgot this. Undo takes you back.");
    assert!(d.is_whole_paragraphs(from.clone()));
    let to = at(&d, "Editors").start;
    let now = d
        .drop_text(
            doc_model::Carried::Page(from),
            doc_model::Place::Block(to),
            false,
        )
        .unwrap();
    ok(&d);
    assert_eq!(
        d.text(),
        "Most of the friction comes from choosing too early.\n\nSoftware forgot this. Undo takes you back.\n\nEditors have always known this. A good editor doesn't erase your sentence. They write in the margin.\n"
    );
    assert_eq!(now, at(&d, "Software forgot this. Undo takes you back."));
}

#[test]
fn drag_a_paragraph_to_the_end() {
    let mut d = Document::from_text("First.\n\nSecond.\n\nThird.");
    let from = at(&d, "First.");
    let len = d.len();
    d.drop_text(
        doc_model::Carried::Page(from),
        doc_model::Place::Block(len),
        false,
    )
    .unwrap();
    assert_eq!(d.text(), "Second.\n\nThird.\n\nFirst.");
}

#[test]
fn drag_a_sentence_into_a_paragraph_gap() {
    let mut d = Document::from_text(PARAS);
    let from = at(&d, "Software forgot this.");
    let to = at(&d, "Editors").start;
    d.drop_text(
        doc_model::Carried::Page(from),
        doc_model::Place::Block(to),
        false,
    )
    .unwrap();
    assert!(d.text().starts_with("Most of the friction comes from choosing too early.\n\nSoftware forgot this.\n\nEditors have"), "{}", d.text());
    assert!(
        d.text().ends_with("\n\nUndo takes you back.\n"),
        "{}",
        d.text()
    );
}

#[test]
fn drag_copies_and_onto_itself_is_nothing() {
    let mut d = Document::from_text("One two three.");
    let from = at(&d, "two");
    d.drop_text(
        doc_model::Carried::Page(from.clone()),
        doc_model::Place::Inline(d.len()),
        true,
    )
    .unwrap();
    assert_eq!(d.text(), "One two three. two");
    let before = d.text();
    let r = at(&d, "three");
    assert_eq!(
        d.drop_text(
            doc_model::Carried::Page(r.clone()),
            doc_model::Place::Inline(r.start + 2),
            false
        ),
        Err(EditError::Unchanged)
    );
    assert_eq!(d.text(), before);
    assert!(d.can_undo());
}

#[test]
fn drag_carries_groups() {
    let (mut d, g) = friction_doc();
    let from = at(&d, "the friction");
    d.drop_text(
        doc_model::Carried::Page(from),
        doc_model::Place::Inline(d.len() - 1),
        false,
    )
    .unwrap();
    ok(&d);
    assert_eq!(d.text(), "Most of in writing the friction.");
    let moved = d.visible_groups();
    assert_eq!(moved.len(), 1);
    assert_eq!(d.group(moved[0].id).unwrap().variants.len(), 3);
    assert_eq!(moved[0].id, g, "a move keeps the group's identity");
}

#[test]
fn drag_a_scrap_between_words() {
    let mut d = Document::from_text("Judge them, in context.");
    d.add_note(0, "the only way").unwrap();
    let to = at(&d, ",").end;
    d.drop_text(
        doc_model::Carried::Note(0),
        doc_model::Place::Inline(to),
        true,
    )
    .unwrap();
    assert_eq!(d.text(), "Judge them, the only way in context.");
    assert_eq!(d.notes().len(), 1);
    d.drop_text(
        doc_model::Carried::Note(0),
        doc_model::Place::Block(d.len()),
        false,
    )
    .unwrap();
    assert_eq!(
        d.text(),
        "Judge them, the only way in context.\n\nthe only way"
    );
    assert!(d.notes().is_empty());
}

#[test]
fn stash_copy_keeps_the_page() {
    let mut d = Document::from_text(PARAS);
    let r = at(&d, "Undo takes you back.");
    d.stash_copy(r).unwrap();
    assert_eq!(d.text(), PARAS);
    assert_eq!(d.notes()[0].text, "Undo takes you back.");
    assert!(d.notes()[0]
        .origin
        .as_deref()
        .is_some_and(|q| "Software forgot this. Undo".starts_with(q)));
}

#[test]
fn stash_marks_whole_paragraphs() {
    let mut d = Document::from_text(PARAS);
    d.stash(at(&d, "Software forgot this. Undo takes you back."))
        .unwrap();
    d.stash(at(&d, "A good editor doesn't erase your sentence."))
        .unwrap();
    let notes = d.notes();
    assert!(!notes[0].paragraph);
    assert!(notes[1].paragraph);
}

#[test]
fn a_scrap_moves_between_documents_with_its_alternatives() {
    let mut page = Document::from_text("Intro.\n\nMost of the friction in writing.");
    let g = page.create_group(at(&page, "friction"), Scope::Word).unwrap();
    page.add_variant(g, "pain", Source::Me).unwrap();
    page.break_undo_group();
    let mut drawer = Document::new();

    let scrap = page.cut_scrap(at(&page, "Most of the friction in writing.")).unwrap();
    assert_eq!(page.text(), "Intro.\n\n");
    assert!(scrap.paragraph, "a whole paragraph");
    drawer.add_scrap(0, &scrap, Some("01-the-garret.md".into())).unwrap();
    let n = &drawer.notes()[0];
    assert_eq!(n.text, "Most of the friction in writing.");
    assert_eq!(n.source.as_deref(), Some("01-the-garret.md"));
    assert!(n.origin.is_some());
    ok(&drawer);

    let mut other = Document::from_text("Chapter seven.");
    let frag = drawer.note_fragment(0).unwrap();
    other.paste(0, &frag).unwrap();
    drawer.remove_note(0).unwrap();
    let v = other.visible_groups();
    assert_eq!(other.group(v[0].id).unwrap().variants.len(), 2);
    ok(&other);

    page.undo();
    assert_eq!(page.text(), "Intro.\n\nMost of the friction in writing.");
}

#[test]
fn scrap_of_leaves_the_page_alone() {
    let page = Document::from_text("One.\n\nTwo words.");
    let s = page.scrap_of(at(&page, "Two")).unwrap();
    assert_eq!(s.fragment.plain_text(), "Two");
    assert!(!s.paragraph);
    assert_eq!(page.text(), "One.\n\nTwo words.");
}

#[test]
fn undo_tags_order_steps_across_documents_and_carry_links() {
    let mut a = Document::from_text("aaa");
    let mut b = Document::from_text("bbb");
    a.replace(0..0, "x").unwrap();
    b.replace(0..0, "y").unwrap();
    let (ta, tb) = (a.undo_tag().unwrap(), b.undo_tag().unwrap());
    assert!(tb.seq > ta.seq, "b's step came later");
    assert_eq!(ta.link, None);

    let link = doc_model::new_link();
    a.replace(0..0, "z").unwrap();
    a.link_last_step(link);
    assert_eq!(a.undo_tag().unwrap().link, Some(link));
    a.replace(0..0, "q").unwrap();
    assert_eq!(a.undo_tag().unwrap().link, None);
    a.undo();
    assert_eq!(a.undo_tag().unwrap().link, Some(link));
    a.undo();
    assert_eq!(a.redo_tag().unwrap().link, Some(link), "redo keeps the link");
    a.redo();
    assert_eq!(a.undo_tag().unwrap().link, Some(link));
}

#[test]
fn clear_notes_is_one_step() {
    let mut d = Document::from_text("a b c");
    d.stash(at(&d, "a")).unwrap();
    d.stash(at(&d, "b")).unwrap();
    d.clear_notes().unwrap();
    assert!(d.notes().is_empty());
    d.undo();
    assert_eq!(d.notes().len(), 2);
}

#[test]
fn rename_sources_rewrites_the_history_too() {
    let mut page = Document::from_text("One two.");
    let mut drawer = Document::new();
    let s = page.cut_scrap(at(&page, "two")).unwrap();
    drawer.add_scrap(0, &s, Some("01-a.md".into())).unwrap();
    drawer.add_note(1, "typed").unwrap();
    assert!(drawer.rename_sources(|f| (f == "01-a.md").then(|| "02-a.md".to_string())));
    assert_eq!(drawer.notes()[0].source.as_deref(), Some("02-a.md"));
    drawer.undo();
    assert_eq!(drawer.notes()[0].source.as_deref(), Some("02-a.md"), "undo keeps the new name");
    assert!(!drawer.rename_sources(|_| None));
}

#[test]
fn clean_text_with_notes_marks_places_with_other_versions() {
    let mut d = Document::from_text("On an exceptionally hot evening. Not this. He walked.\n");
    let g = d.create_group(at(&d, "exceptionally hot"), Scope::Word).unwrap();
    d.add_variant(g, "stifling", Source::Me).unwrap();
    d.add_variant(g, "close and airless", Source::Ai).unwrap();
    d.ghost(at(&d, " Not this.")).unwrap();
    let w = d.create_group(at(&d, "walked"), Scope::Word).unwrap();
    d.add_variant(w, "walked", Source::Me).unwrap_or_else(|_| unreachable!());
    let (text, notes) = d.clean_text_with_notes();
    assert_eq!(text, d.clean_text());
    assert_eq!(text, "On an exceptionally hot evening. He walked.\n");
    assert_eq!(notes.len(), 1, "a group whose others are the same text has nothing to say");
    let chars: Vec<char> = text.chars().collect();
    assert_eq!(chars[notes[0].range.clone()].iter().collect::<String>(), "exceptionally hot");
    assert_eq!(notes[0].others, vec!["stifling".to_string(), "close and airless".to_string()]);

    // A ghosted group's text isn't in the final text, so it has no note.
    let mut d = Document::from_text("Keep. Drop me. End.\n");
    let g = d.create_group(at(&d, "Drop me."), Scope::Sentence).unwrap();
    d.add_variant(g, "Another.", Source::Me).unwrap();
    d.ghost(at(&d, "Drop me.")).unwrap();
    let (text, notes) = d.clean_text_with_notes();
    assert_eq!(text, "Keep. End.\n");
    assert!(notes.is_empty());
}
