use doc_format::{parse, parse_cuts, parse_page, serialize, serialize_cuts, serialize_page, ExternalCut, LoadedDoc, Preserved};
use doc_model::{Batch, CutPlace, Document, Look, Presentation, Scope, Source};
use proptest::prelude::*;

fn at(d: &Document, needle: &str) -> std::ops::Range<usize> {
    let t = d.text();
    let b = t.find(needle).unwrap_or_else(|| panic!("`{needle}` not in `{t}`"));
    let s = t[..b].chars().count();
    s..s + needle.chars().count()
}

fn save(d: &Document) -> String {
    serialize(d, doc_format::DEFAULT_LAYER, &Preserved::default())
}

fn reload(file: &str) -> LoadedDoc {
    let l = parse(file);
    l.doc.check_invariants().unwrap();
    let again = l.serialize();
    assert_eq!(parse(&again).serialize(), again, "save → load → save is stable");
    l
}

fn body(f: &str) -> &str {
    f.find("\n<!-- dostoevsky:").map_or(f, |i| &f[..i])
}

const OLD: &str = "# The Garret\n\nOn an exceptionally hot evening early in July a young man came out.\n\nHe was hopelessly in debt to his landlady.\n\nAnd each time he passed, he felt ashamed.\n\n<!-- dostoevsky:overflow\nThe landlady, again. Cut her?\n\nevery line of it asked him for something\n-->\n\n<!-- dostoevsky:meta v1\n{\"kind\":\"alt\",\"id\":\"k3f9a\",\"scope\":\"word\",\"at\":{\"block\":1,\"offset\":21,\"before\":\"exceptionally \",\"after\":\" evening early i\"},\"active\":0,\"variants\":[\"hot\",\"warm\"],\"ghost\":true}\n{\"kind\":\"ghost\",\"at\":{\"block\":1,\"offset\":6,\"length\":14,\"quote\":\"exceptionally \",\"before\":\"On an \",\"after\":\"hot evening earl\"}}\n{\"kind\":\"ghost\",\"at\":{\"block\":2,\"offset\":0,\"length\":42,\"quote\":\"He was hopelessly\",\"before\":\"\",\"after\":\"\"}}\n{\"kind\":\"ghost\",\"at\":{\"note\":1,\"offset\":0,\"length\":10,\"quote\":\"every line\",\"before\":\"\",\"after\":\" of it asked him\"}}\n{\"kind\":\"stash\",\"note\":0,\"quote\":\"The landlady, again. Cut\",\"from\":{\"block\":3,\"quote\":\"And each time he passed,\"}}\n-->\n";

#[test]
fn cut_17_older_files_migrate() {
    let l = parse(OLD);
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    l.doc.check_invariants().unwrap();
    let old_body = body(OLD);
    assert_eq!(l.doc.text(), format!("{}\n", old_body.trim_end()), "the page looks the same");
    let cuts = l.doc.cuts();
    let texts: Vec<&str> = cuts.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(
        texts,
        ["exceptionally hot", "He was hopelessly in debt to his landlady.", "The landlady, again. Cut her?", "every line of it asked him for something"]
    );
    assert!(matches!(cuts[0].place, CutPlace::Page { shown: true, .. }));
    assert!(cuts[1].paragraph, "a ghosted paragraph takes its blank line along");
    assert!(matches!(cuts[2].place, CutPlace::Note(0)));
    assert_eq!(cuts[2].origin.as_deref(), Some("And each time he passed,"));
    let g = l.doc.all_group_ids()[0];
    assert!(l.doc.group(g).unwrap().ghosted, "the ghosted group is inside a cut");
    assert_eq!(l.doc.clean_text(), "# The Garret\n\nOn an evening early in July a young man came out.\n\nAnd each time he passed, he felt ashamed.\n");

    let saved = l.serialize();
    assert!(!saved.contains("\"kind\":\"ghost\""), "{saved}");
    assert!(!body(&saved).contains("exceptionally"), "the body is the kept text");
    assert!(!body(&saved).contains("hopelessly"));
    let again = reload(&saved);
    assert_eq!(again.doc.text(), l.doc.text(), "nothing moved");
    assert_eq!(again.serialize(), saved, "migration is done once");
}

#[test]
fn cut_11_marks_and_gaps_round_trip() {
    let mut d = Document::from_text("First.\n\nSecond paragraph here.\n\nThird.\n");
    let (p, _) = d.cut(at(&d, "Second paragraph here."), None).unwrap();
    let (w, _) = d.cut(at(&d, "Third"), Some(Look::Mark)).unwrap();
    d.set_presentation(Presentation::default());
    let f = save(&d);
    assert!(f.starts_with("First.\n\n.\n"), "{f}");
    assert!(f.contains("\"gap\":true"), "{f}");
    assert!(f.contains("\"trace\":\"mark\""), "{f}");
    let l = reload(&f);
    assert_eq!(l.doc.text(), d.text());
    let c = l.doc.cuts();
    assert!(c[0].paragraph && matches!(c[0].place, CutPlace::Page { shown: true, .. }));
    assert_eq!(c[1].look, Some(Look::Mark));
    assert!(matches!(c[1].place, CutPlace::Page { shown: false, .. }));
    let _ = (p, w);
}

#[test]
fn cut_11_spots_follow_outside_edits() {
    let mut d = Document::from_text("Intro.\n\nHe had avoided his landlady on the staircase today.\n\nEnd.\n");
    d.cut(at(&d, "his landlady "), None).unwrap();
    let f = save(&d);
    let edited = f.replacen("Intro.\n\n", "Intro.\n\nA new first paragraph.\n\n", 1);
    let l = reload(&edited);
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    assert!(l.doc.text().contains("He had avoided his landlady on the staircase today."));
    let edited = f.replacen("He had avoided ", "Somehow he avoided ", 1);
    let l = reload(&edited);
    assert!(l.doc.text().contains("Somehow he avoided his landlady on the staircase"), "{}", l.doc.text());
    let edited = f.replacen("He had avoided on the staircase today.\n\n", "", 1);
    assert_ne!(edited, f);
    let l = reload(&edited);
    let c = &l.doc.cuts()[0];
    assert!(c.lost);
    assert_eq!(c.text, "his landlady ");
    assert!(matches!(c.place, CutPlace::Note(_)));
    assert!(!l.warnings.is_empty());
    let saved = l.serialize();
    assert!(saved.contains("\"trace\":true"), "a lost cut keeps its trace: {saved}");
    assert!(reload(&saved).doc.cuts()[0].lost);
}

#[test]
fn cut_11_cuts_in_hidden_versions_round_trip() {
    let mut d = Document::from_text("A long, slow sentence.\n");
    let g = d.create_group(at(&d, "A long, slow sentence."), Scope::Sentence).unwrap();
    d.add_variant(g, "A short one.", Source::Me).unwrap();
    d.cut(at(&d, "slow "), None).unwrap();
    d.cycle(g, 1).unwrap();
    let f = save(&d);
    assert!(f.contains("\"parent\":"), "{f}");
    let mut l = reload(&f);
    assert_eq!(l.doc.text(), "A short one.\n");
    let g = l.doc.all_group_ids()[0];
    l.doc.cycle(g, 1).unwrap();
    assert_eq!(l.doc.text(), "A long, slow sentence.\n");
    assert_eq!(l.doc.ghost_ranges(), vec![at(&l.doc, "slow ")]);
}

/// CUT-12
#[test]
fn cut_15_batches_and_groups_round_trip() {
    let mut d = Document::from_text("a very big and quite red dog\n");
    let g = d.create_group(at(&d, "big"), Scope::Word).unwrap();
    d.add_variant(g, "huge", Source::Me).unwrap();
    let b = Batch { id: "t7q2".into(), name: "Trim −20%".into() };
    d.cut_many(&[at(&d, "very big "), at(&d, "quite ")], Some(b.clone())).unwrap();
    let f = save(&d);
    assert!(f.contains("\"batch\":{\"id\":\"t7q2\",\"name\":\"Trim −20%\"}"), "{f}");
    let l = reload(&f);
    assert!(l.doc.cuts().iter().all(|c| c.batch.as_ref() == Some(&b)));
    assert_eq!(l.doc.group(l.doc.all_group_ids()[0]).unwrap().variants.len(), 2);
}

#[test]
fn older_readers_see_scraps_with_links() {
    let mut d = Document::from_text("Keep this.\n\nHe had avoided his landlady.\n");
    d.cut(at(&d, "his landlady"), None).unwrap();
    let f = save(&d);
    assert!(f.contains("<!-- dostoevsky:overflow\nhis landlady\n-->"), "{f}");
    assert!(f.contains("\"from\":{\"block\":1,\"quote\":\"He had avoided.\""), "{f}");
}

#[test]
fn cut_14_a_projects_cuts_file() {
    let mut d = Document::from_text("Chapter one.\n\nHe had avoided his landlady.\n");
    d.cut(at(&d, "his landlady"), None).unwrap();
    d.add_note(0, "An old scrap of the chapter's own.").unwrap();
    let file = "part-1/01-the-garret.md";
    let (chapter, cuts) = serialize_page(&d, false, &Preserved::default(), file);
    assert!(!chapter.contains("his landlady"), "{chapter}");
    assert!(chapter.contains("An old scrap of the chapter's own."));
    assert_eq!(cuts.len(), 1);
    assert_eq!(cuts[0].file, file);

    let mut loose = Document::new();
    loose.add_note(0, "A note that belongs to no chapter.").unwrap();
    let cuts_md = serialize_cuts(&loose, false, &Preserved::default(), &cuts);
    assert!(cuts_md.contains("\"file\":\"part-1/01-the-garret.md\""), "{cuts_md}");
    let (own, external) = parse_cuts(&cuts_md);
    assert_eq!(own.doc.notes().len(), 1);
    assert_eq!(external, cuts);
    assert_eq!(serialize_cuts(&own.doc, own.layer, &own.kept, &external), cuts_md, "the cuts file is stable");

    let back = parse_page(&chapter, file, &external);
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert_eq!(back.doc.text(), d.text());
    assert_eq!(back.doc.cuts().len(), 2);
    let (chapter2, cuts2) = serialize_page(&back.doc, back.layer, &back.kept, file);
    assert_eq!(chapter2, chapter);
    assert_eq!(cuts2, cuts);
}

#[test]
fn cut_14_cuts_with_no_trace_stay_with_the_project() {
    let mut d = Document::from_text("Chapter one.\n\nHe had avoided his landlady.\n");
    let file = "02.md";
    d.set_cut_source(Some(file.to_string()));
    let (id, _) = d.cut(at(&d, "his landlady"), None).unwrap();
    d.forget_spot(id).unwrap();
    let (_, cuts) = serialize_page(&d, false, &Preserved::default(), file);
    let back = parse_page("Chapter one.\n\nHe had avoided .\n", file, &cuts);
    let n = &back.doc.notes()[0];
    assert_eq!(n.source.as_deref(), Some(file));
    let (chapter, again) = serialize_page(&back.doc, false, &back.kept, file);
    assert_eq!(chapter, "Chapter one.\n\nHe had avoided .\n");
    assert_eq!(again.len(), 1);
}

#[test]
fn external_cuts_say_what_they_are() {
    let e = ExternalCut { file: "a.md".into(), note: "x".into(), records: vec![serde_json::json!({"kind":"stash","from":{"file":"a.md","at":{"block":0,"offset":1}}})] };
    assert!(e.has_spot());
    assert_eq!(e.text(), "x");
}

fn old_file(text: &str, ghosts: &[(usize, usize)]) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut ranges: Vec<(usize, usize)> = ghosts
        .iter()
        .map(|&(a, b)| {
            let (a, b) = (a % (chars.len() + 1), b % (chars.len() + 1));
            (a.min(b), a.max(b))
        })
        .filter(|(a, b)| a < b)
        .collect();
    ranges.sort();
    let mut kept: Vec<(usize, usize)> = Vec::new();
    for r in ranges {
        if kept.last().is_none_or(|l| l.1 < r.0) {
            kept.push(r);
        }
    }
    let mut records = String::new();
    for (a, b) in kept {
        let quote: String = chars[a..b.min(a + 16)].iter().collect();
        records.push_str(&serde_json::json!({"kind":"ghost","at":{"block":0,"offset":a,"length":b - a,"quote":quote}}).to_string());
        records.push('\n');
    }
    format!("{}\n\n<!-- dostoevsky:meta v1\n{records}-->\n", text.trim_end_matches('\n'))
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: std::env::var("PROPTEST_CASES").ok().and_then(|v| v.parse().ok()).unwrap_or(1000),
        ..ProptestConfig::default()
    })]

    #[test]
    fn cut_17_migration_is_lossless_and_idempotent(
        text in "[a-z .\n]{1,60}",
        ghosts in prop::collection::vec((any::<u16>(), any::<u16>()), 0..5),
    ) {
        prop_assume!(!text.trim().is_empty());
        let f = old_file(&text, &ghosts.iter().map(|&(a, b)| (a as usize, b as usize)).collect::<Vec<_>>());
        let l = parse(&f);
        l.doc.check_invariants().map_err(TestCaseError::fail)?;
        let shown = format!("{}\n", text.trim_end_matches('\n'));
        prop_assert_eq!(l.doc.text(), shown.clone(), "the page looks the same\n{}", f);
        let saved = l.serialize();
        let l2 = parse(&saved);
        prop_assert!(l2.warnings.is_empty(), "{:?}\n{}", l2.warnings, saved);
        // The body ends in one newline: blank lines next to a cut at the very end may shrink.
        let squash = |t: &str| t.split_whitespace().collect::<Vec<_>>().join(" ");
        prop_assert_eq!(squash(&l2.doc.text()), squash(&shown), "nothing moved or went\n{}", saved);
        // Exactly, unless a cut sits among the blank lines at the end.
        let text_after_last_cut = l2.doc.ghost_ranges().last().is_none_or(|r| !l2.doc.text_range(r.end..l2.doc.len()).trim().is_empty());
        if !body(&saved).trim().is_empty() && text_after_last_cut {
            let now = l2.doc.text();
            prop_assert_eq!(now.trim_end(), shown.trim_end(), "nothing moved or went\n{}", saved);
        }
        prop_assert_eq!(l2.serialize(), saved.clone(), "migration is done once");
        let ink = |s: &str| s.chars().filter(|c| !c.is_whitespace()).count();
        let cut_chars: usize = l2.doc.cuts().iter().map(|c| ink(&c.text)).sum();
        prop_assert_eq!(ink(body(&saved)) + cut_chars, ink(&text), "every char is kept or in a cut");
    }
}

#[test]
fn a_version_without_the_words_round_trips() {
    let mut d = Document::from_text("You pick a word, the the sentence moves on.\n");
    let t = d.text();
    let b = t.match_indices("the").nth(1).unwrap().0;
    let g = d.create_group(b..b + 3, Scope::Word).unwrap();
    d.add_variant(g, "whole", Source::Ai).unwrap();
    d.add_cut_version(g, Source::Ai).unwrap();
    for (active, look) in [(0, Look::Text), (2, Look::Text), (2, Look::Mark)] {
        d.set_presentation(Presentation { editing: true, look });
        d.set_active(g, active).unwrap();
        let f = save(&d);
        assert!(f.contains(r#"{"t":"","by":"ai","cut":true}"#), "{f}");
        let mut l = reload(&f);
        assert!(l.warnings.is_empty(), "{:?}", l.warnings);
        l.doc.set_presentation(Presentation { editing: true, look });
        let info = l.doc.group(g).unwrap();
        assert_eq!(info.active, active);
        assert!(info.variants[2].cut && info.variants[2].text == "the");
        assert_eq!(l.doc.text(), d.text());
        assert_eq!(l.doc.clean_text(), d.clean_text());
        let c = l.doc.cuts();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].version, Some(g));
    }
}

/// dsky-tzc: a sentence cut from the middle of a paragraph leaves one space in the saved body;
/// a reload gives the page back exactly, and Put Back the paragraph.
#[test]
fn a_cut_sentence_leaves_one_space_in_the_body() {
    for (text, cut, saved, side) in [
        ("He went up. Ghosted it. Then down.\n", "Ghosted it.", "He went up. Then down.\n", "after"),
        ("He went up. Ghosted it.\n\nNext.\n", "Ghosted it.", "He went up.\n\nNext.\n", "before"),
        ("It was very hot, and late.\n", "very hot", "It was, and late.\n", "before"),
        ("Ghosted it. Then down.\n", "Ghosted it.", "Then down.\n", "after"),
    ] {
        for look in [Some(Look::Mark), None] {
            let mut d = Document::from_text(text);
            d.cut(at(&d, cut), look).unwrap();
            let f = save(&d);
            assert_eq!(body(&f), saved, "{f}");
            assert!(f.contains(&format!("<!-- dostoevsky:overflow\n{cut}\n-->")), "the cut's text as it was\n{f}");
            assert!(f.contains(&format!("\"space\":\"{side}\"")), "{f}");
            let mut l = reload(&f);
            assert_eq!(l.doc.clean_text(), d.clean_text(), "{f}");
            if look.is_none() {
                assert_eq!(l.doc.snapshot(), d.snapshot(), "{f}");
            }
            let id = l.doc.cuts()[0].id;
            l.doc.put_back(id).unwrap();
            assert_eq!(l.doc.text(), text, "Put Back after a reload");
        }
    }
    // Two spaces stay two (only a lone one goes), and nothing changes on the next save.
    let mut d = Document::from_text("Up.  Ghosted it.  Down.\n");
    d.cut(at(&d, "Ghosted it."), None).unwrap();
    let f = save(&d);
    assert!(!f.contains("\"space\""), "{f}");
}

/// An outside editor already took the space out: a load doesn't put a second one in.
#[test]
fn a_cut_space_isnt_doubled_after_an_outside_edit() {
    let mut d = Document::from_text("He went up. Ghosted it. Then down.\n");
    d.cut(at(&d, "Ghosted it."), None).unwrap();
    let f = save(&d).replacen("He went up. Then down.", "He went up.  Then down.", 1);
    let l = parse(&f);
    assert_eq!(l.doc.clean_text(), "He went up. Then down.\n");
    assert!(!l.doc.text().contains("   "), "{:?}", l.doc.text());
}
