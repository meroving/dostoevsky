use doc_format::{parse, serialize, LoadedDoc, Preserved};
use doc_model::snapshot::TreeNode;
use doc_model::{Document, Scope, Source};

fn at(d: &Document, needle: &str) -> std::ops::Range<usize> {
    let t = d.text();
    let b = t
        .find(needle)
        .unwrap_or_else(|| panic!("`{needle}` not in `{t}`"));
    let s = t[..b].chars().count();
    s..s + needle.chars().count()
}

fn group(d: &mut Document, needle: &str, scope: Scope, others: &[&str]) -> doc_model::GroupId {
    let g = d.create_group(at(d, needle), scope).unwrap();
    for o in others {
        d.add_variant(g, o, Source::Me).unwrap();
    }
    g
}

fn save(d: &Document) -> String {
    serialize(d, doc_format::DEFAULT_LAYER, &Preserved::default())
}

/// Also checks that a second save is identical.
fn reload(file: &str) -> LoadedDoc {
    let l = parse(file);
    l.doc.check_invariants().unwrap();
    let again = l.serialize();
    assert_eq!(
        parse(&again).serialize(),
        again,
        "save → load → save is stable"
    );
    l
}

fn texts(l: &LoadedDoc) -> Vec<(String, usize, Vec<String>)> {
    l.doc
        .all_group_ids()
        .into_iter()
        .map(|id| {
            let g = l.doc.group(id).unwrap();
            let active = g.variants[g.active].text.clone();
            (
                active,
                g.active,
                g.variants.into_iter().map(|v| v.text).collect(),
            )
        })
        .collect()
}

#[test]
fn plain_markdown_has_no_trailer() {
    for f in [
        "",
        "x",
        "# T\n\nbody\n",
        "no newline",
        "a\n\n\n",
        "a\r\nb\r\n",
        "\u{feff}bom\n",
    ] {
        let l = parse(f);
        assert_eq!(l.serialize(), f, "byte-identical");
        assert!(l.warnings.is_empty());
    }
    let d = Document::from_text("Just text.\n");
    assert_eq!(save(&d), "Just text.\n");
    // The layer flag alone doesn't make a trailer.
    assert_eq!(serialize(&d, false, &Preserved::default()), "Just text.\n");
}

#[test]
fn spec_example_loads() {
    let f = include_str!("fixtures/spec-example.md");
    let l = reload(f);
    assert!(
        l.warnings.iter().all(|w| !w.contains("detached")),
        "{:?}",
        l.warnings
    );
    assert!(l.detached().is_empty());
    assert!(!l.layer, "writing unless the file says otherwise");
    let groups = texts(&l);
    let actives: Vec<&str> = groups.iter().map(|g| g.0.as_str()).collect();
    assert_eq!(
        actives,
        [
            "Keep the Words You Almost Used",
            "friction",
            "Editors have always known this.",
            "suggestion",
            "Software forgot this. Most editors treat writing as a single line that only moves forward.",
        ]
    );
    assert_eq!(groups[1].2.len(), 4);
    let g = l.doc.group(l.doc.all_group_ids()[1]).unwrap();
    assert_eq!(g.variants[3].by, Source::Ai);
    assert_eq!(g.scope, Scope::Word);
    // The ghost is re-anchored by its quote; its illustrative length runs past the paragraph.
    let ghost = l.doc.ghost_ranges();
    assert_eq!(ghost.len(), 1);
    assert!(l
        .doc
        .text_range(ghost[0].clone())
        .starts_with("quietly disappear."));
    let notes = l.doc.notes();
    assert_eq!(notes.len(), 2);
    assert_eq!(
        notes[0].text,
        "Maybe open with the margin metaphor instead?"
    );
    assert_eq!(notes[1].origin.as_deref(), Some("Editors have always kno"));
    assert!(l.doc.text().starts_with("# Keep the Words"));
    assert!(l.doc.text().ends_with("only moves forward.\n"));
}

#[test]
fn records_look_like_the_spec() {
    let mut d = Document::from_text(
        "# Title\n\nMost of the friction in writing comes from choosing.\n\nSecond para.\n",
    );
    group(&mut d, "friction", Scope::Word, &["hesitation"]);
    let g = d.group(d.all_group_ids()[0]).unwrap().id;
    d.add_variant(g, "second-guessing", Source::Ai).unwrap();
    group(&mut d, "Second para.", Scope::Paragraph, &["Another para."]);
    d.ghost(at(&d, "Title")).unwrap();
    d.stash(at(&d, "choosing")).unwrap();
    let f = save(&d);
    let id0 = doc_format::ids::encode(d.all_group_ids()[0]);
    let id1 = doc_format::ids::encode(d.all_group_ids()[1]);
    let expected = format!(
        "# \n\nMost of the friction in writing comes from .\n\nSecond para.\n\n\
<!-- dostoevsky:overflow\nTitle\n\nchoosing\n-->\n\n\
<!-- dostoevsky:meta v1\n\
{{\"kind\":\"alt\",\"id\":\"{id0}\",\"scope\":\"word\",\"at\":{{\"block\":1,\"offset\":12,\"before\":\"Most of the \",\"after\":\" in writing come\"}},\"active\":0,\"variants\":[\"friction\",\"hesitation\",{{\"t\":\"second-guessing\",\"by\":\"ai\"}}]}}\n\
{{\"kind\":\"alt\",\"id\":\"{id1}\",\"scope\":\"paragraph\",\"at\":{{\"block\":2}},\"active\":0,\"variants\":[\"Second para.\",\"Another para.\"]}}\n\
{{\"kind\":\"stash\",\"note\":0,\"quote\":\"Title\",\"from\":{{\"block\":0,\"quote\":\"# \",\"at\":{{\"block\":0,\"offset\":2,\"before\":\"# \",\"after\":\"\"}}}},\"trace\":true}}\n\
{{\"kind\":\"stash\",\"note\":1,\"quote\":\"choosing\",\"from\":{{\"block\":1,\"quote\":\"Most of the friction in \"}}}}\n\
-->\n"
    );
    assert_eq!(f, expected);
    let l = reload(&f);
    assert_eq!(l.serialize(), f);
}

#[test]
fn nested_groups_and_ghosts_in_inactive_variants_survive() {
    let mut d = Document::from_text("The cat sat on the mat.\n");
    let outer = group(&mut d, "The cat sat on the mat.", Scope::Sentence, &[]);
    group(&mut d, "cat", Scope::Word, &["dog", "fox"]);
    d.ghost(at(&d, "mat")).unwrap();
    d.add_variant(outer, "A bird flew by.", Source::Ai).unwrap();
    d.cycle(outer, 1).unwrap();
    group(&mut d, "bird", Scope::Word, &["crow"]);
    d.ghost(at(&d, "by")).unwrap();
    d.cycle(outer, 1).unwrap();
    let f = save(&d);
    assert!(f.contains("\"parent\""));
    let l = reload(&f);
    assert_eq!(l.doc.snapshot(), d.snapshot(), "ids and structure survive");
    assert_eq!(l.serialize(), f);
}

#[test]
fn notes_keep_their_groups_and_blank_lines() {
    let mut d =
        Document::from_text("Para one.\n\nKeep friction here.\n\nPara three.\n\nPara four.\n");
    group(&mut d, "friction", Scope::Word, &["tension"]);
    d.ghost(at(&d, "Keep")).unwrap();
    d.stash(at(&d, "Keep friction here.\n\nPara three.\n\n"))
        .unwrap();
    assert_eq!(d.notes()[0].text, "Keep friction here.\n\nPara three.\n\n");
    let f = save(&d);
    assert!(f.contains("\"at\":{\"note\":0,\"offset\":5"), "{f}");
    let l = reload(&f);
    assert_eq!(l.doc.snapshot(), d.snapshot());
}

#[test]
fn escaping_round_trips() {
    let mut d = Document::from_text("a --> b <!-- c --\\> d\n");
    group(&mut d, "-->", Scope::Word, &["--\\>", "-->-->"]);
    d.stash(at(&d, "<!-- c")).unwrap();
    let f = save(&d);
    let trailer = &f[f.find("<!-- dostoevsky:").unwrap()..];
    assert_eq!(
        trailer.matches("-->").count(),
        2,
        "only the two closers: {trailer}"
    );
    let l = reload(&f);
    assert_eq!(l.doc.snapshot(), d.snapshot());
}

#[test]
fn crlf_and_bom_are_kept_with_a_trailer() {
    let mut d = Document::from_text("One.\n\nfriction here\n");
    group(&mut d, "friction", Scope::Word, &["tension"]);
    let lf = save(&d);
    let crlf = format!("\u{feff}{}", lf.replace('\n', "\r\n"));
    let l = parse(&crlf);
    assert_eq!(l.doc.snapshot(), d.snapshot());
    assert!(l.kept.encoding.crlf && l.kept.encoding.bom);
    assert_eq!(l.serialize(), crlf);
}

#[test]
fn trailing_newlines_are_normalized_only_with_a_trailer() {
    let mut d = Document::from_text("x friction\n\n\n");
    group(&mut d, "friction", Scope::Word, &["tension"]);
    let f = save(&d);
    assert!(
        f.starts_with("x friction\n\n<!-- dostoevsky:meta v1\n"),
        "{f}"
    );
    assert_eq!(parse(&f).doc.text(), "x friction\n");
    let d = Document::from_text("no trailer\n\n\n");
    assert_eq!(save(&d), "no trailer\n\n\n");
}

fn base() -> (Document, String) {
    let mut d = Document::from_text(
        "# Title\n\nMost of the friction in writing comes from choosing too early.\n\nThe cat sat. The cat ran away fast.\n\nA closing paragraph about margins.\n",
    );
    group(&mut d, "friction", Scope::Word, &["tension"]);
    let t = d.text();
    let second_cat = t.rfind("cat").unwrap();
    let s = t[..second_cat].chars().count();
    let g = d.create_group(s..s + 3, Scope::Word).unwrap();
    d.add_variant(g, "dog", Source::Me).unwrap();
    group(
        &mut d,
        "Most of the friction in writing comes from choosing too early.",
        Scope::Sentence,
        &["Writing is hard."],
    );
    group(
        &mut d,
        "A closing paragraph about margins.",
        Scope::Paragraph,
        &["The end."],
    );
    d.ghost(at(&d, "ran away")).unwrap();
    let f = save(&d);
    (d, f)
}

fn edit_body(f: &str, from: &str, to: &str) -> String {
    let split = f.find("\n<!-- dostoevsky:").unwrap();
    let (body, trailer) = f.split_at(split);
    assert!(body.contains(from), "`{from}` not in body");
    format!("{}{trailer}", body.replacen(from, to, 1))
}

fn placed(l: &LoadedDoc) -> Vec<String> {
    l.doc
        .visible_groups()
        .into_iter()
        .map(|g| l.doc.text_range(g.range))
        .collect()
}

#[test]
fn unchanged_file_places_everything_exactly() {
    let (d, f) = base();
    let l = reload(&f);
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    assert_eq!(l.doc.snapshot(), d.snapshot());
}

#[test]
fn inserted_paragraph_shifts_blocks() {
    let (_, f) = base();
    let f = edit_body(&f, "# Title\n\n", "# Title\n\nA new opening paragraph.\n\n");
    let l = reload(&f);
    assert!(l.detached().is_empty(), "{:?}", l.warnings);
    assert_eq!(
        placed(&l),
        [
            "Most of the friction in writing comes from choosing too early.",
            "friction",
            "cat",
            "A closing paragraph about margins."
        ]
    );
    assert_eq!(
        l.doc.text_range(l.doc.ghost_ranges()[0].clone()),
        "ran away"
    );
}

#[test]
fn context_picks_the_right_occurrence() {
    let (_, f) = base();
    // The exact offset fails; context finds the second cat.
    let f = edit_body(&f, "The cat sat.", "Look: the cat sat. Then");
    let l = reload(&f);
    let cat = l
        .doc
        .visible_groups()
        .into_iter()
        .find(|g| l.doc.text_range(g.range.clone()) == "cat")
        .unwrap();
    // The second cat: the one after "Then The " (the cut beside it may not find its place).
    let t: Vec<char> = l.doc.text().chars().collect();
    let before: String = t[cat.range.start - 9..cat.range.start].iter().collect();
    assert_eq!(before, "Then The ");
}

#[test]
fn small_edits_inside_a_span_are_followed() {
    let (_, f) = base();
    let f = edit_body(&f, "choosing too early.", "choosing much too early.");
    let l = reload(&f);
    assert!(l.detached().is_empty(), "{:?}", l.warnings);
    let g = l.doc.group(l.doc.all_group_ids()[0]).unwrap();
    assert_eq!(
        g.variants[0].text, "Most of the friction in writing comes from choosing much too early.",
        "the body wins: the active variant takes the edited text"
    );
    assert_eq!(g.variants[1].text, "Writing is hard.");
}

#[test]
fn edited_paragraphs_are_matched_by_similarity() {
    let (_, f) = base();
    let f = edit_body(
        &f,
        "A closing paragraph about margins.",
        "A closing paragraph about the margins.",
    );
    let l = reload(&f);
    assert!(placed(&l).contains(&"A closing paragraph about the margins.".to_string()));
}

#[test]
fn deleted_text_detaches_and_survives_saves() {
    let (_, f) = base();
    let f = edit_body(&f, "\nThe cat sat. The cat fast.\n", "");
    let l = reload(&f);
    assert_eq!(l.detached().len(), 1);
    let det = &l.detached()[0];
    assert_eq!(det.variants[0].text, "cat");
    assert_eq!(det.variants[1].text, "dog");
    assert!(l.warnings.iter().any(|w| w.contains("detached")));
    let again = l.serialize();
    assert!(again.contains("\"detached\":true"));
    let l2 = reload(&again);
    assert_eq!(l2.detached().len(), 1);
    assert_eq!(l2.serialize(), again);
    assert_eq!(placed(&l2).len(), 3);
}

#[test]
fn ambiguous_moves_detach_instead_of_guessing() {
    let mut d = Document::from_text("Alpha unique word.\n\nfiller\n\nfiller\n");
    group(&mut d, "unique", Scope::Word, &["single"]);
    let f = save(&d);
    // The word disappears from its block and shows up twice elsewhere with no context.
    let f = edit_body(
        &f,
        "Alpha unique word.\n\nfiller\n\nfiller",
        "Alpha word.\n\nunique\n\nunique",
    );
    let l = reload(&f);
    assert_eq!(l.detached().len(), 1);
}

#[test]
fn overlapping_records_detach_the_later_one() {
    let body = "one two three\n";
    let f = format!(
        "{body}\n<!-- dostoevsky:meta v1\n\
{{\"kind\":\"alt\",\"id\":\"aaaaa\",\"scope\":\"word\",\"at\":{{\"block\":0,\"offset\":0}},\"active\":0,\"variants\":[\"one two\",\"x\"]}}\n\
{{\"kind\":\"alt\",\"id\":\"bbbbb\",\"scope\":\"word\",\"at\":{{\"block\":0,\"offset\":4}},\"active\":0,\"variants\":[\"two three\",\"y\"]}}\n\
{{\"kind\":\"alt\",\"id\":\"ccccc\",\"scope\":\"word\",\"at\":{{\"block\":0,\"offset\":4}},\"active\":0,\"variants\":[\"two\",\"z\"]}}\n\
-->\n"
    );
    let l = reload(&f);
    assert_eq!(l.detached().len(), 1);
    assert_eq!(l.detached()[0].id, "bbbbb");
    // "two" sits inside "one two": it becomes a nested group of its active variant.
    let snap = l.doc.snapshot();
    let TreeNode::Group(outer) = &snap.page[0] else {
        panic!("{snap:?}")
    };
    assert!(matches!(outer.variants[0].content[1], TreeNode::Group(_)));
}

#[test]
fn unknown_kinds_and_fields_are_written_back() {
    let f = "Body friction here.\n\n<!-- dostoevsky:meta v1\n\
{\"kind\":\"doc\",\"layer\":true,\"scroll\":120}\n\
{\"kind\":\"bookmark\",\"at\":{\"block\":0,\"offset\":0},\"text\":\"hi\"}\n\
{\"kind\":\"alt\",\"id\":\"k3f9a\",\"scope\":\"word\",\"at\":{\"block\":0,\"offset\":5,\"before\":\"Body \",\"after\":\" here.\"},\"active\":0,\"variants\":[\"friction\",{\"t\":\"tension\",\"by\":\"ai\",\"at\":\"2026-10-22T10:00:00Z\"}],\"color\":\"red\"}\n\
not json at all\n\
-->\n";
    let l = reload(f);
    assert!(l.layer);
    assert_eq!(l.serialize(), f, "everything comes back in place");
    let g = l.doc.group(l.doc.all_group_ids()[0]).unwrap();
    assert_eq!(g.variants[1].by, Source::Ai);
}

#[test]
fn newer_major_version_opens_as_plain_text() {
    let f = "Body.\n\n<!-- dostoevsky:meta v2\n{\"kind\":\"whatever\"}\n-->\n";
    let l = parse(f);
    assert_eq!(l.newer_version, Some(2));
    assert_eq!(l.doc.text(), "Body.\n");
    assert!(!l.warnings.is_empty());
    assert_eq!(l.serialize(), f, "the trailer is written back untouched");
}

#[test]
fn foreign_ids_are_kept_and_duplicates_renamed() {
    let f = "a b\n\n<!-- dostoevsky:meta v1\n\
{\"kind\":\"alt\",\"id\":\"grp001\",\"scope\":\"word\",\"at\":{\"block\":0,\"offset\":0},\"active\":0,\"variants\":[\"a\",\"x\"]}\n\
{\"kind\":\"alt\",\"id\":\"grp001\",\"scope\":\"word\",\"at\":{\"block\":0,\"offset\":2},\"active\":0,\"variants\":[\"b\",\"y\"]}\n\
-->\n";
    let l = reload(f);
    assert_eq!(l.doc.all_group_ids().len(), 2);
    let out = l.serialize();
    assert_eq!(out.matches("\"grp001\"").count(), 1, "{out}");
}

#[test]
fn detached_ids_never_collide_with_live_ones() {
    let (_, f) = base();
    let f = edit_body(&f, "\nThe cat sat. The cat fast.\n", "");
    let mut l = parse(&f);
    let det_id = l.detached()[0].id.clone();
    // Make a live group that happens to get the detached group's id.
    let gid = doc_format::ids::decode(&det_id).unwrap();
    let mut tree = l.doc.snapshot();
    if let TreeNode::Group(g) = tree
        .page
        .iter_mut()
        .find(|n| matches!(n, TreeNode::Group(_)))
        .unwrap()
    {
        g.id = gid;
    }
    l.doc = Document::from_snapshot(tree).unwrap();
    let out = l.serialize();
    assert_eq!(
        out.matches(&format!("\"id\":\"{det_id}\"")).count(),
        1,
        "{out}"
    );
    let l2 = reload(&out);
    assert_eq!(l2.detached().len(), 1);
}

#[test]
fn stash_remembers_whole_paragraphs() {
    let mut d = Document::from_text("First one.\n\nSecond one here.\n\nThird.\n");
    d.stash(at(&d, "Second one here.")).unwrap();
    d.stash(at(&d, "Third")).unwrap();
    let file = save(&d);
    assert_eq!(file.matches("\"paragraph\":true").count(), 1, "{file}");
    let notes = reload(&file).doc.notes();
    assert!(!notes[0].paragraph, "a word is not a paragraph");
    assert!(notes[1].paragraph);
    assert_eq!(notes[1].text, "Second one here.");
}

#[test]
fn a_drawer_scrap_names_the_file_it_came_from() {
    let mut page = Document::from_text("First one.\n\nSecond one here.\n");
    let scrap = page.cut_scrap(at(&page, "Second one here.")).unwrap();
    let mut drawer = Document::new();
    drawer.add_scrap(0, &scrap, Some("part 1/01-the-garret.md".into())).unwrap();
    drawer.add_note(1, "typed in the drawer").unwrap();
    let file = save(&drawer);
    assert!(file.contains("\"from\":{\"file\":\"part 1/01-the-garret.md\",\"quote\":\"First one.\"}"), "{file}");
    assert!(!file.contains("\"block\""), "{file}");
    let notes = reload(&file).doc.notes();
    assert_eq!(notes[0].source.as_deref(), Some("part 1/01-the-garret.md"));
    assert_eq!(notes[0].origin.as_deref(), Some("First one."));
    assert!(notes[0].paragraph);
    assert_eq!(notes[1].source, None);
}

#[test]
fn group_in_the_trailing_newlines_saves() {
    let mut d = Document::from_text("ab\n\ncd");
    let g = group(&mut d, "cd", Scope::Word, &["\n"]);
    d.cycle(g, 1).unwrap();
    assert_eq!(d.text(), "ab\n\n\n");
    let f = save(&d);
    let back = reload(&f);
    let tree = back.doc.snapshot();
    assert_eq!(tree.page.iter().filter(|n| matches!(n, TreeNode::Group(_))).count(), 1, "{f}");
}
