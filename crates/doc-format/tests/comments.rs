use doc_format::{parse, parse_cuts, parse_page, serialize, serialize_cuts, serialize_page, LoadedDoc, Preserved};
use doc_model::{Document, Scope, Source};

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

fn on_page(d: &Document) -> Vec<(String, String)> {
    d.comments().into_iter().map(|c| (d.text_range(c.range), c.text)).collect()
}

const PAGE: &str = "The Garret\n\nHe had avoided his landlady. His garret was more like a cupboard than a room.\n";

/// COM-12
#[test]
fn a_comment_is_a_record_and_the_body_is_the_text() {
    let mut d = Document::from_text(PAGE);
    d.add_comment(at(&d, "more like a cupboard"), "Cupboard or closet?", Some("2026-11-12T10:00:00Z".into())).unwrap();
    let f = save(&d);
    assert!(f.starts_with(PAGE), "{f}");
    let line = f.lines().find(|l| l.contains("\"kind\":\"comment\"")).unwrap();
    assert_eq!(
        line,
        "{\"kind\":\"comment\",\"at\":{\"block\":1,\"offset\":44,\"before\":\" His garret was \",\"after\":\" than a room.\",\"length\":20,\"quote\":\"more like a cupboard\"},\"text\":\"Cupboard or closet?\",\"created\":\"2026-11-12T10:00:00Z\"}"
    );
    let l = reload(&f);
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    assert_eq!(on_page(&l.doc), vec![("more like a cupboard".into(), "Cupboard or closet?".into())]);
    assert_eq!(l.serialize(), f);
}

/// COM-12
#[test]
fn comments_follow_outside_edits() {
    let mut d = Document::from_text(PAGE);
    d.add_comment(at(&d, "more like a cupboard"), "?", None).unwrap();
    let f = save(&d);
    let moved = f.replacen("He had avoided his landlady. ", "He had, as usual, avoided his landlady. ", 1);
    assert_eq!(on_page(&reload(&moved).doc)[0].0, "more like a cupboard");
    let edited = f.replacen("more like a cupboard than", "more like a cuboard than", 1);
    assert_eq!(on_page(&reload(&edited).doc)[0].0, "more like a cuboard");
    let gone = f.replacen("His garret was more like a cupboard than a room.", "Gone.", 1);
    let l = reload(&gone);
    assert!(l.doc.comments().is_empty());
    assert_eq!(l.warnings.len(), 1, "{:?}", l.warnings);
}

/// COM-8, COM-12
#[test]
fn comments_in_versions_round_trip() {
    let mut d = Document::from_text(PAGE);
    let g = d.create_group(at(&d, "more like a cupboard than a room"), Scope::Sentence).unwrap();
    d.add_comment(at(&d, "cupboard"), "inside", None).unwrap();
    d.add_variant_and_show(g, "a closet", Source::Me).unwrap();
    d.add_comment(at(&d, "closet"), "the other", None).unwrap();
    let f = save(&d);
    assert_eq!(f.matches("\"parent\"").count(), 2, "{f}");
    let mut l = reload(&f);
    assert_eq!(on_page(&l.doc), vec![("closet".into(), "the other".into())]);
    let g = l.doc.all_group_ids()[0];
    l.doc.cycle(g, 1).unwrap();
    assert_eq!(on_page(&l.doc), vec![("cupboard".into(), "inside".into())]);
}

/// COM-9, COM-12
#[test]
fn comments_in_cuts_round_trip() {
    let mut d = Document::from_text(PAGE);
    d.add_comment(at(&d, "more like a cupboard"), "in the cut", None).unwrap();
    let (id, _) = d.cut(at(&d, "His garret was more like a cupboard than a room."), None).unwrap();
    assert_eq!(d.cut_info(id).unwrap().comments.len(), 1);
    let f = save(&d);
    assert!(f.contains("\"at\":{\"note\":0"), "{f}");
    let mut l = reload(&f);
    let cut = l.doc.cuts()[0].clone();
    assert_eq!(cut.comments, vec!["in the cut".to_string()]);
    l.doc.put_back(cut.id).unwrap();
    assert_eq!(on_page(&l.doc), vec![("more like a cupboard".into(), "in the cut".into())]);
}

#[test]
fn unknown_comment_fields_are_kept() {
    let f = "Body friction here.\n\n<!-- dostoevsky:meta v1\n\
{\"kind\":\"comment\",\"at\":{\"block\":0,\"offset\":5,\"before\":\"Body \",\"after\":\" here.\",\"length\":8,\"quote\":\"friction\"},\"text\":\"hi\",\"color\":\"red\"}\n\
-->\n";
    let l = reload(f);
    assert_eq!(on_page(&l.doc), vec![("friction".into(), "hi".into())]);
    assert_eq!(l.serialize(), f);
}

#[test]
fn an_authors_name_is_kept() {
    let mut d = Document::from_text(PAGE);
    d.add_comment_by(at(&d, "garret"), "Attic?", None, Some("Anna".into())).unwrap();
    let f = save(&d);
    assert!(f.contains("\"author\":\"Anna\""), "{f}");
    let l = reload(&f);
    assert_eq!(l.doc.comments()[0].author.as_deref(), Some("Anna"));
}

/// COM-12
#[test]
fn a_chapters_cut_takes_its_comments_to_the_cuts_file() {
    let mut d = Document::from_text("Chapter one.\n\nHe had avoided his landlady.\n");
    d.add_comment(at(&d, "landlady"), "Name her?", None).unwrap();
    d.add_comment(at(&d, "Chapter one"), "Stays in the chapter", None).unwrap();
    d.cut(at(&d, "his landlady"), None).unwrap();
    let file = "01.md";
    let (chapter, cuts) = serialize_page(&d, false, &Preserved::default(), file);
    assert!(chapter.contains("Stays in the chapter"));
    assert!(!chapter.contains("Name her?"), "{chapter}");
    let cuts_md = serialize_cuts(&Document::new(), false, &Preserved::default(), &cuts);
    assert!(cuts_md.contains("Name her?"), "{cuts_md}");
    let (_, external) = parse_cuts(&cuts_md);
    let back = parse_page(&chapter, file, &external);
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert_eq!(back.doc.cuts()[0].comments, vec!["Name her?".to_string()]);
    assert_eq!(on_page(&back.doc), vec![("Chapter one".into(), "Stays in the chapter".into())]);
}
