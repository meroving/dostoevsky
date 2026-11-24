use std::time::Duration;

use crate::actions::{Export, RevertToSaved};
use crate::export_ui::Format;
use crate::ui_tests::{TempDir, Ui};

const DOC: &str = "Most of the friction in writing comes from choosing too early.\n\nEditors have always known this.\n";

fn read(p: &std::path::Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

#[test]
fn file_1_new_document() {
    let mut ui = Ui::open(DOC);
    ui.keys("cmd-n");
    assert_eq!(ui.window_count(), 2);
    assert_eq!(ui.documents()[1], (String::new(), None));
}

#[test]
fn file_1_save_and_save_as() {
    let dir = TempDir::new("file1");
    let mut ui = Ui::open(DOC);
    ui.keys("cmd-s");
    let first = dir.0.join("first.md");
    ui.choose_save_path(&first);
    assert_eq!(read(&first), DOC);
    assert_eq!(ui.path().as_deref(), Some(first.as_path()));
    assert_eq!(ui.title(), "first.md");

    ui.caret_at(0);
    ui.type_text("X");
    ui.keys("cmd-shift-s");
    let second = dir.0.join("second.md");
    ui.choose_save_path(&second);
    assert_eq!(read(&second), format!("X{DOC}"));
    assert_eq!(read(&first), DOC, "the first file is left as it was");
    assert_eq!(ui.title(), "second.md");
}

#[test]
fn file_1_revert_to_saved() {
    let dir = TempDir::new("revert");
    let mut ui = Ui::open_file(&dir.file("essay.md", DOC));
    ui.caret_at(0);
    ui.type_text("XYZ");
    ui.action(RevertToSaved);
    assert_eq!(ui.text(), DOC);
    assert!(!ui.dirty());
}

#[test]
fn file_1_open_most_recent() {
    let dir = TempDir::new("recent");
    let mut ui = Ui::open(DOC);
    ui.keys("cmd-s");
    ui.choose_save_path(&dir.0.join("older.md"));
    ui.keys("cmd-shift-s");
    ui.choose_save_path(&dir.0.join("newer.md"));
    ui.keys("cmd-shift-t");
    let docs = ui.documents();
    assert_eq!(docs.len(), 2, "{docs:?}");
    assert_eq!(docs[1].1.as_deref(), Some(dir.0.join("older.md").as_path()));
}

#[test]
fn file_1_extensions() {
    for name in ["a.md", "a.markdown", "a.txt"] {
        assert!(crate::files::is_markdown(std::path::Path::new(name)), "{name}");
    }
    assert!(!crate::files::is_markdown(std::path::Path::new("a.png")));
}

#[test]
fn file_2_plain_markdown_stays_plain() {
    let dir = TempDir::new("file2");
    let path = dir.file("plain.md", DOC);
    let mut ui = Ui::open_file(&path);
    ui.caret_at(0);
    ui.type_text("X");
    ui.keys("backspace cmd-s");
    assert_eq!(read(&path), DOC, "byte for byte, no comment block");

    ui.make_group("friction", &["tension"]);
    ui.keys("cmd-s");
    let saved = read(&path);
    assert!(saved.starts_with("Most of the tension in writing"), "{saved}");
    assert!(saved.contains("<!-- dostoevsky:meta v1"), "{saved}");
}

#[test]
fn file_3_autosave_after_idle() {
    let dir = TempDir::new("autosave");
    let path = dir.file("essay.md", DOC);
    let mut ui = Ui::open_file(&path);
    ui.caret_at(0);
    ui.type_text("X");
    assert!(ui.title().ends_with("— Edited"));
    std::thread::sleep(Duration::from_millis(2100));
    ui.advance(Duration::from_millis(600));
    assert_eq!(read(&path), format!("X{DOC}"));
    assert_eq!(ui.title(), "essay.md");

    ui.set_prefs(|p| p.autosave = crate::prefs::Autosave::Off);
    ui.type_text("Y");
    std::thread::sleep(Duration::from_millis(2100));
    ui.advance(Duration::from_millis(600));
    assert_eq!(read(&path), format!("X{DOC}"));
    assert!(ui.dirty());
}

#[test]
fn file_3_autosave_waits_for_sidebar_edits() {
    let dir = TempDir::new("autosave-sidebar");
    let path = dir.file("essay.md", DOC);
    let mut ui = Ui::open_file(&path);
    ui.action(crate::actions::ToggleLayer);
    ui.select_text("friction");
    ui.keys("cmd-shift-a");
    assert!(ui.sidebar_open());
    // Long enough after opening that a page-only idle timer would already have run out.
    std::thread::sleep(Duration::from_millis(2100));
    ui.advance(Duration::from_millis(600));
    ui.type_text("tension");
    ui.keys("enter");
    assert!(ui.text().starts_with("Most of the tension"));
    ui.advance(Duration::from_millis(600));
    assert!(ui.dirty(), "saved right after the sidebar edit, without a pause");
    assert!(read(&path).contains("friction in writing"));

    std::thread::sleep(Duration::from_millis(2100));
    ui.advance(Duration::from_millis(600));
    assert!(!ui.dirty());
    assert!(read(&path).contains("tension in writing"));
}

#[test]
fn file_4_backups_keep_the_last_20() {
    let dir = TempDir::new("backups");
    let path = dir.file("essay.md", DOC);
    let mut ui = Ui::open_file(&path);
    for _ in 0..23 {
        ui.caret_at(0);
        ui.type_text("X");
        ui.keys("cmd-s");
        std::thread::sleep(Duration::from_millis(3));
    }
    let backups = crate::files::support_dir().unwrap().join("Backups");
    let ring = std::fs::read_dir(&backups).unwrap().next().unwrap().unwrap().path();
    let mut saved: Vec<_> = std::fs::read_dir(&ring)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    saved.sort();
    assert_eq!(saved.len(), 20);
    assert_eq!(read(saved.last().unwrap()), read(&path), "the newest is the last save");
}

#[test]
fn file_4_crash_recovery() {
    let dir = TempDir::new("recovery");
    let path = dir.file("essay.md", DOC);
    let mut ui = Ui::open_file(&path);
    ui.caret_at(0);
    ui.type_text("Unsaved ");
    ui.advance(Duration::from_secs(5));
    let journals = crate::files::support_dir().unwrap().join("Recovery");
    let journal = std::fs::read_dir(&journals).unwrap().next().expect("a journal").unwrap().path();
    // Pretend it was left by an earlier run that crashed.
    std::fs::rename(&journal, journals.join("1-1.md")).unwrap();

    let mut next = Ui::open("");
    assert_eq!(next.with_app(crate::restore_recoveries), 1);
    let docs = next.documents();
    assert_eq!(docs[1], (format!("Unsaved {DOC}"), Some(path)));
    assert!(std::fs::read_dir(&journals).unwrap().next().is_none(), "the journal is cleared once reopened");
}

/// How long a change made outside may take to arrive. fseventsd answers a watcher starting on
/// its own within a second, but with the whole suite running (every test window watching a
/// folder) it took 33 seconds (2026-11-15). A test that passes waits only as long as it needs.
const WATCHER_SECS: f32 = 90.;

#[test]
fn file_5_outside_change_reloads() {
    let dir = TempDir::new("outside");
    let path = dir.file("essay.md", DOC);
    let mut ui = Ui::open_file(&path);
    std::thread::sleep(Duration::from_millis(300));
    std::fs::write(&path, "Changed in another editor.\n").unwrap();
    assert!(ui.wait_for(WATCHER_SECS, |ui| ui.text() == "Changed in another editor.\n"), "not reloaded: {:?}", ui.text());
    assert!(!ui.dirty());
    assert!(ui.status().is_some_and(|s| s.starts_with("Reloaded")));
}

/// The watcher starts on its own thread (fseventsd can take seconds to answer); a change made
/// before it's running is still noticed.
#[test]
fn file_5_change_while_the_watcher_starts_reloads() {
    let dir = TempDir::new("outside-early");
    let path = dir.file("essay.md", DOC);
    let mut ui = Ui::open_file(&path);
    std::fs::write(&path, "Changed at once.\n").unwrap();
    assert!(ui.wait_for(WATCHER_SECS, |ui| ui.text() == "Changed at once.\n"), "not reloaded: {:?}", ui.text());
    assert!(!ui.dirty());
}

#[test]
fn file_5_outside_change_with_local_edits_asks() {
    let dir = TempDir::new("outside2");
    let path = dir.file("essay.md", DOC);
    let mut ui = Ui::open_file(&path);
    ui.caret_at(0);
    ui.type_text("Mine. ");
    std::thread::sleep(Duration::from_millis(300));
    std::fs::write(&path, "Theirs.\n").unwrap();
    assert!(ui.wait_for(WATCHER_SECS, |ui| ui.app.has_pending_prompt()), "no Keep mine / Load theirs prompt");
    ui.answer_prompt("Load Theirs");
    assert_eq!(ui.text(), "Theirs.\n");

    ui.type_text("Mine again. ");
    std::fs::write(&path, "Theirs, twice.\n").unwrap();
    assert!(ui.wait_for(WATCHER_SECS, |ui| ui.app.has_pending_prompt()));
    ui.answer_prompt("Keep Mine");
    assert!(ui.text().contains("Mine again."));
    ui.keys("cmd-s");
    assert!(read(&path).contains("Mine again."), "the next save overwrites theirs");
}

/// Autosave waits for the answer: the words from outside stay on disk while the question is
/// open, and Load Theirs leaves the page and the file alike.
#[test]
fn file_5_autosave_never_writes_over_an_outside_change() {
    let dir = TempDir::new("outside-autosave");
    let path = dir.file("essay.md", DOC);
    let mut ui = Ui::open_file(&path);
    ui.caret_at(0);
    ui.type_text("Mine. ");
    std::fs::write(&path, "Theirs.\n").unwrap();
    assert!(ui.wait_for(WATCHER_SECS, |ui| ui.app.has_pending_prompt()), "no Keep mine / Load theirs prompt; on disk: {:?}", read(&path));
    // Longer than autosave's pause: the writer reads the question.
    ui.wait_for(3., |_| false);
    assert_eq!(read(&path), "Theirs.\n", "autosave wrote over the change from outside");
    ui.answer_prompt("Load Theirs");
    assert_eq!(ui.text(), "Theirs.\n");
    assert!(!ui.dirty());
    assert_eq!(read(&path), "Theirs.\n");
}

/// A save that comes before the watcher has said anything still finds the change and asks.
#[test]
fn file_5_a_save_before_the_watcher_notices_asks() {
    let dir = TempDir::new("outside-early-save");
    let path = dir.file("essay.md", DOC);
    let mut ui = Ui::open_file(&path);
    ui.caret_at(0);
    ui.type_text("Mine. ");
    std::fs::write(&path, "Theirs.\n").unwrap();
    ui.keys("cmd-s");
    assert_eq!(read(&path), "Theirs.\n", "the save wrote over the change from outside");
    assert!(ui.wait_for(2., |ui| ui.app.has_pending_prompt()), "no Keep mine / Load theirs prompt");
    ui.answer_prompt("Keep Mine");
    ui.keys("cmd-s");
    assert!(read(&path).starts_with("Mine. "), "Keep Mine lets the next save through");
}

/// The watcher reporting the same write while the question is open doesn't ask it again
/// (on a slow machine FSEvents comes after the save has asked).
#[test]
fn file_5_the_watcher_late_for_an_answered_change_asks_nothing() {
    let dir = TempDir::new("outside-late-watcher");
    let path = dir.file("essay.md", DOC);
    let mut ui = Ui::open_file(&path);
    ui.caret_at(0);
    ui.type_text("Mine. ");
    std::fs::write(&path, "Theirs.\n").unwrap();
    ui.keys("cmd-s");
    assert!(ui.wait_for(2., |ui| ui.app.has_pending_prompt()), "no Keep mine / Load theirs prompt");
    ui.update_window(|ws, _, cx| ws.disk_changed(cx));
    ui.answer_prompt("Keep Mine");
    assert!(!ui.app.has_pending_prompt(), "the same change asked about twice");
    ui.keys("cmd-s");
    assert!(read(&path).starts_with("Mine. "), "Keep Mine lets the next save through");
}

#[test]
fn file_6_detached_groups() {
    let saved = {
        let mut ui = Ui::open(DOC);
        ui.make_group("friction", &["tension"]);
        ui.make_group("Editors", &["Writers"]);
        ui.saved()
    };
    // Another editor rewrote the first sentence: its group can't be placed.
    let edited = saved.replacen("Most of the tension in writing comes from choosing too early.", "A different opening line entirely.", 1);
    let dir = TempDir::new("detached");
    let mut ui = Ui::open_file(&dir.file("essay.md", &edited));
    assert_eq!(ui.groups().len(), 1);
    assert!(ui.has_element("detached-banner"));
    ui.click_id("detached-banner");
    assert_eq!(ui.sheet(), Some("detached"));

    ui.keys("escape");
    ui.select_text("opening");
    ui.keys("cmd-shift-a escape");
    ui.click_id("detached-banner");
    let before = ui.groups().len();
    ui.select_text("different");
    ui.click_id(("attach", 0usize));
    assert_eq!(ui.groups().len(), before + 1, "attached to the selection");
    assert!(!ui.has_element("detached-banner"));
}

#[test]
fn file_6_discard_is_undoable() {
    let saved = {
        let mut ui = Ui::open(DOC);
        ui.make_group("friction", &["tension"]);
        ui.saved()
    };
    let edited = saved.replacen("Most of the tension in writing comes from choosing too early.", "A different opening line entirely.", 1);
    let dir = TempDir::new("discard");
    let mut ui = Ui::open_file(&dir.file("essay.md", &edited));
    ui.click_id("detached-banner");
    ui.click_id(("discard", 0usize));
    assert!(!ui.has_element(("discard", 0usize)));
    ui.keys("escape cmd-z");
    assert!(ui.has_element("detached-banner"), "undo brings the detached group back");
}

#[test]
fn file_7_source_view() {
    let mut ui = Ui::open(DOC);
    ui.make_group("friction", &["tension"]);
    ui.keys("alt-cmd-u");
    let text = ui.source_view().unwrap();
    assert_eq!(text, ui.saved());
    ui.click_id("copy");
    assert_eq!(ui.clipboard(), Some(ui.saved()));
    assert!(!ui.has_element("clean"), "the final text is Copy and Export's now");
}

#[test]
fn file_11_export_markdown() {
    let dir = TempDir::new("export");
    let mut ui = Ui::open_file(&dir.file("essay.md", DOC));
    ui.make_group("friction", &["tension"]);
    ui.select_text("Editors have always known this.");
    ui.keys("cmd-/");
    ui.action(Export);
    assert_eq!(ui.sheet(), Some("export"));
    ui.click_id(("export-format", Format::Markdown as usize));
    ui.click_id("export-save");
    let out = dir.0.join("essay (final).md");
    ui.choose_save_path(&out);
    ui.settle();
    assert_eq!(read(&out), "Most of the tension in writing comes from choosing too early.\n");
    assert_eq!(read(&dir.0.join("essay.md")), DOC, "the document itself is not saved");
    assert_eq!(ui.sheet(), None);
    assert_eq!(ui.status().as_deref(), Some("Exported essay (final).md"));
    assert!(ui.has_element("status-reveal"), "the status line offers Show in Finder");
}

#[test]
fn file_11_export_pdf_word_and_epub() {
    let dir = TempDir::new("export-formats");
    let mut ui = Ui::open_file(&dir.file("essay.md", "# The garret\n\nOn an exceptionally hot evening.\n"));
    ui.action(Export);
    ui.click_id(("export-format", Format::Pdf as usize));
    ui.settle();
    let (pages, image) = ui.export_form(|f| (f.preview.pdf.as_ref().map(|p| p.pages), f.preview.image.is_some())).unwrap();
    assert_eq!(pages, Some(1));
    assert!(image, "the first page is drawn");
    ui.click_id("export-save");
    let pdf = dir.0.join("essay.pdf");
    ui.choose_save_path(&pdf);
    ui.settle();
    assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));

    ui.action(Export);
    ui.click_id(("export-format", Format::Word as usize));
    assert_eq!(ui.export_form(|f| f.style), Some(export::Style::Manuscript), "Word starts as a manuscript");
    ui.click_id("export-save");
    let docx = dir.0.join("essay.docx");
    ui.choose_save_path(&docx);
    ui.settle();
    let parts = export::zip::read(&std::fs::read(&docx).unwrap()).unwrap();
    let doc = parts.iter().find(|(n, _)| n == "word/document.xml").map(|(_, b)| String::from_utf8_lossy(b).into_owned()).unwrap();
    assert!(doc.contains("On an exceptionally hot evening."));

    ui.action(Export);
    ui.click_id(("export-format", Format::Epub as usize));
    assert_eq!(ui.export_form(|f| f.style), Some(export::Style::Book));
    ui.click_id("export-save");
    let epub = dir.0.join("essay.epub");
    ui.choose_save_path(&epub);
    ui.settle();
    let parts = export::zip::read(&std::fs::read(&epub).unwrap()).unwrap();
    assert_eq!(parts[0].0, "mimetype");
    assert!(parts.iter().any(|(n, _)| n == "OEBPS/content.opf"));
}

#[test]
fn file_12_export_remembers_its_choices() {
    let dir = TempDir::new("export-memory");
    let mut ui = Ui::open_file(&dir.file("essay.md", DOC));
    ui.action(Export);
    ui.click_id(("export-format", Format::Pdf as usize));
    ui.click_id(("export-style", 2usize));
    ui.click_id("export-text");
    assert!(ui.export_form(|f| f.text_open).unwrap());
    ui.click_id(("export-align", 0usize));
    ui.click_id("export-save");
    ui.choose_save_path(&dir.0.join("essay.pdf"));
    ui.settle();
    ui.action(Export);
    let (style, text_align) = ui.export_form(|f| (f.style, f.text.align)).unwrap();
    assert_eq!(style, export::Style::Book);
    assert_eq!(text_align, export::Align::Left, "the Book style as changed, not as it starts");
    ui.click_id(("export-style", 1usize));
    ui.click_id("export-text");
    assert!(!ui.export_form(|f| f.text_open).unwrap(), "a manuscript's Text doesn't open");
}

#[test]
fn file_10_drop_a_file_to_open_it() {
    let dir = TempDir::new("drop");
    let dropped = dir.file("dropped.md", "Dropped in.\n");
    let mut ui = Ui::open(DOC);
    let at = ui.point_for(3);
    ui.drop_files(std::slice::from_ref(&dropped), at);
    let docs = ui.documents();
    assert!(docs.iter().any(|(t, p)| t == "Dropped in.\n" && p.as_deref() == Some(dropped.as_path())), "{docs:?}");
}

#[test]
fn file_13_word_alternatives_as_comments() {
    let dir = TempDir::new("export-comments");
    // Tall enough that Word's options all show without scrolling the column.
    let mut ui = Ui::open_file_sized(&dir.file("essay.md", DOC), 1180., 1100.);
    ui.make_group("friction", &["tension", "hesitation"]);
    assert!(ui.text().contains("Most of the hesitation in writing"));
    ui.action(Export);
    ui.click_id(("export-format", Format::Word as usize));
    ui.click_id(("export-alternatives", 1usize));
    assert_eq!(ui.export_form(|f| f.comments), Some(true));
    ui.click_id("export-save");
    let docx = dir.0.join("essay.docx");
    ui.choose_save_path(&docx);
    ui.settle();
    let parts = export::zip::read(&std::fs::read(&docx).unwrap()).unwrap();
    let part = |name: &str| parts.iter().find(|(n, _)| n == name).map(|(_, b)| String::from_utf8_lossy(b).into_owned());
    let doc = part("word/document.xml").unwrap();
    let start = doc.find("<w:commentRangeStart w:id=\"0\"/>").expect("the place is marked");
    let end = doc.find("<w:commentRangeEnd w:id=\"0\"/>").unwrap();
    assert!(doc[start..end].contains("hesitation"), "the chosen version is in the text");
    let comments = part("word/comments.xml").unwrap();
    assert!(comments.contains("· friction") && comments.contains("· tension") && !comments.contains("· hesitation"), "{comments}");

    ui.action(Export);
    ui.click_id(("export-alternatives", 0usize));
    ui.click_id("export-save");
    ui.choose_save_path(&docx);
    ui.settle();
    let parts = export::zip::read(&std::fs::read(&docx).unwrap()).unwrap();
    assert!(!parts.iter().any(|(n, _)| n == "word/comments.xml"));
}

#[test]
fn file_13_print_without_the_sheet() {
    let mut ui = Ui::open(DOC);
    ui.action(crate::actions::Print);
    ui.settle();
    assert_eq!(ui.sheet(), None);
}
