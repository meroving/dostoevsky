use std::path::{Path, PathBuf};

use gpui_kit::MouseButton;

use super::{TempDir, Ui};
use crate::actions::*;
use crate::prefs::Autosave;
use crate::project::Kind;

fn book() -> TempDir {
    let d = unchosen_book();
    std::fs::create_dir_all(d.0.join("notes")).unwrap();
    std::fs::rename(d.0.join("people.md"), d.0.join("notes/people.md")).unwrap();
    choose(&d, Kind::Book, Some("notes"));
    d
}

fn unchosen_book() -> TempDir {
    let d = TempDir::new("book");
    std::fs::create_dir_all(d.0.join("part-1")).unwrap();
    std::fs::create_dir_all(d.0.join("part-2")).unwrap();
    d.file("part-1/01-the-garret.md", "# The Garret\n\nOn an exceptionally hot evening.\n");
    d.file("part-1/02-marmeladov.md", "# Marmeladov\n\nRaskolnikov was not used to crowds.\n");
    d.file("part-1/03-the-letter.md", "# The Letter\n\nThe letter was in his pocket.\n");
    d.file("part-2/01-the-summons.md", "# The Summons\n\nHe lay for a long time.\n");
    d.file("people.md", "# People\n\nRazumihin: loud, kind.\n");
    d
}

fn choose(d: &TempDir, kind: Kind, notes: Option<&str>) {
    crate::project::write_settings(&d.0, &crate::project::Settings { kind, notes: notes.map(String::from), title: None, author: None, date: None }).unwrap();
}

fn path_of(ui: &Ui) -> Option<PathBuf> {
    ui.read(|ws, cx| ws.model.read(cx).path.clone())
}

fn name_of(ui: &Ui) -> String {
    path_of(ui).and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default()
}

fn names(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(d).unwrap().flatten() {
            if e.path().is_dir() {
                dirs.push(e.path());
            } else {
                out.push(e.path().strip_prefix(root).unwrap().to_string_lossy().into_owned());
            }
        }
    }
    out.sort();
    out
}

fn page_text(ui: &Ui) -> String {
    ui.read(|ws, cx| ws.page.read(cx).text().to_string())
}

fn summary(ui: &mut Ui) -> String {
    ui.element("project-summary").label().unwrap_or_default().to_string()
}

fn click_row(ui: &mut Ui, name: &str) {
    ui.click_id(gpui_kit::SharedString::from(format!("row-{name}")));
}

/// PROJ-1, PROJ-2
#[test]
fn a_numbered_folder_opens_as_a_book() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    assert_eq!(ui.read(|ws, _| ws.project_kind()), Some(Kind::Book));
    assert_eq!(name_of(&ui), "01-the-garret.md");
    assert!(ui.has_element("project"));
    assert!(ui.has_element("project-title"));
    assert!(ui.has_element("row-02-marmeladov.md"));
    assert!(ui.has_element("head-part 2"));
    assert!(summary(&mut ui).ends_with("· 4 chapters"), "{}", summary(&mut ui));
}

#[test]
fn the_list_under_a_sheet_does_not_take_clicks() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    ui.keys("cmd-,");
    click_row(&mut ui, "02-marmeladov.md");
    assert_eq!(name_of(&ui), "01-the-garret.md");
    assert!(ui.read(|ws, _| ws.sheet.is_none()));
}

/// PROJ-3
#[test]
fn the_header_names_the_folder_and_it_toggles_the_list() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    assert!(ui.has_element("project-name"));
    ui.keys("cmd-p cmd-p");
    assert!(!ui.has_element("project"));
    ui.click_id("project-name");
    assert!(ui.has_element("project"));
}

/// PROJ-3
#[test]
fn a_narrow_header_gives_up_the_folder_name_first() {
    use crate::workspace::header_path;
    let mut m = crate::model::DocModel::new(doc_model::Document::new());
    m.path = Some("/x/07-the-axe.md".into());
    let folder = "crime-and-punishment-second-draft";
    assert_eq!(header_path(folder, &m, 500.), (Some(folder.to_string()), "07-the-axe.md".to_string()));
    let (short, file) = header_path(folder, &m, 240.);
    assert!(short.as_deref().is_some_and(|s| s.ends_with('…') && s.len() < folder.len()), "{short:?}");
    assert_eq!(file, "07-the-axe.md");
    assert_eq!(header_path(folder, &m, 110.), (None, "07-the-axe.md".to_string()));
}

#[test]
fn a_single_file_has_no_project_controls() {
    let mut ui = Ui::open("# Hello\n");
    assert!(!ui.has_element("project-name"));
    ui.keys("cmd-p");
    assert!(!ui.has_element("project"));
    assert_eq!(ui.read(|ws, _| ws.project_kind()), None);
}

/// PROJ-4
#[test]
fn arrows_flip_through_the_list() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    ui.keys("cmd-p down");
    assert_eq!(name_of(&ui), "02-marmeladov.md");
    assert!(page_text(&ui).contains("Raskolnikov"));
    ui.keys("down down");
    assert_eq!(name_of(&ui), "01-the-summons.md", "the list runs on through parts");
    ui.keys("up");
    assert_eq!(name_of(&ui), "03-the-letter.md");
    ui.keys("enter");
    ui.type_text("Dear ");
    assert!(page_text(&ui).contains("Dear "));
    ui.keys("cmd-p escape");
    assert!(!ui.has_element("project"));
}

/// PROJ-4
#[test]
fn brackets_go_through_the_book_with_the_list_closed() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    ui.keys("cmd-p cmd-p cmd-]");
    assert_eq!(name_of(&ui), "02-marmeladov.md");
    ui.keys("cmd-[ cmd-[");
    assert_eq!(name_of(&ui), "01-the-garret.md");
    assert_eq!(ui.status().as_deref(), Some("That’s the first one"));
}

/// PROJ-5
#[test]
fn moving_between_files_keeps_everything() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    ui.select_text("exceptionally");
    ui.type_text("very");
    let edited = d.0.join("part-1/01-the-garret.md");
    click_row(&mut ui, "02-marmeladov.md");
    assert!(std::fs::read_to_string(&edited).unwrap().contains("very hot"), "saved on leaving");
    assert!(!ui.read(|ws, cx| ws.model.read(cx).dirty()));
    click_row(&mut ui, "01-the-garret.md");
    assert!(page_text(&ui).contains("very hot"));
    ui.undo();
    assert!(page_text(&ui).contains("exceptionally hot"), "undo came back with the file");
}

/// NFR-1. A folder made unwritable by its mode bits: Windows' read-only attribute doesn't stop
/// writes into a folder, so the test is Unix's.
#[test]
#[cfg(unix)]
fn a_save_that_fails_on_leaving_keeps_the_words() {
    use std::os::unix::fs::PermissionsExt;
    struct Writable(PathBuf);
    impl Drop for Writable {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        }
    }
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    ui.select_text("exceptionally");
    ui.type_text("very");
    let edited = d.0.join("part-1/01-the-garret.md");
    let part = Writable(d.0.join("part-1"));
    std::fs::set_permissions(&part.0, std::fs::Permissions::from_mode(0o555)).unwrap();
    click_row(&mut ui, "02-marmeladov.md");
    ui.answer_prompt("OK");
    drop(part);
    assert_eq!(name_of(&ui), "02-marmeladov.md", "the switch went ahead");
    assert!(std::fs::read_to_string(&edited).unwrap().contains("exceptionally"));
    assert_eq!(ui.read(|ws, cx| ws.parked_unsaved(cx)), 1, "the garret has unsaved edits again");
    click_row(&mut ui, "01-the-garret.md");
    assert!(page_text(&ui).contains("very hot"));
    assert!(ui.read(|ws, cx| ws.model.read(cx).dirty()));
    click_row(&mut ui, "02-marmeladov.md");
    assert!(std::fs::read_to_string(&edited).unwrap().contains("very hot"), "the next save goes through");
    assert_eq!(ui.read(|ws, cx| ws.parked_unsaved(cx)), 0);
}

/// PROJ-5
#[test]
fn with_autosave_off_nothing_is_written_until_asked() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    ui.set_prefs(|p| p.autosave = Autosave::Off);
    ui.select_text("exceptionally");
    ui.type_text("very");
    let edited = d.0.join("part-1/01-the-garret.md");
    click_row(&mut ui, "02-marmeladov.md");
    assert!(std::fs::read_to_string(&edited).unwrap().contains("exceptionally"));
    assert_eq!(ui.read(|ws, cx| ws.parked_unsaved(cx)), 1);
    click_row(&mut ui, "01-the-garret.md");
    assert!(page_text(&ui).contains("very hot"), "the edit waited");
    assert!(ui.read(|ws, cx| ws.model.read(cx).dirty()));
    click_row(&mut ui, "02-marmeladov.md");
    ui.action(CloseWindow);
    ui.answer_prompt("Save All");
    assert!(std::fs::read_to_string(&edited).unwrap().contains("very hot"));
}

/// PROJ-6
#[test]
fn a_journal_opens_on_today_with_its_date() {
    let d = TempDir::new("journal");
    d.file("2020-01-02.md", "Rain all day. Good.\n");
    d.file("2020-01-01.md", "New year.\n");
    let mut ui = Ui::open_folder(&d.0);
    assert_eq!(ui.read(|ws, _| ws.project_kind()), Some(Kind::Journal));
    ui.keys("escape"); // the first-open sheet; the list under it doesn't take clicks
    let today = d.0.join(crate::project::Day::today().file_name());
    let date = today_heading(crate::project::DateHeading::Long);
    assert_eq!(path_of(&ui).as_ref(), Some(&today));
    assert_eq!(page_text(&ui), format!("# {date}\n\n"));
    ui.keys("cmd-p down");
    assert_eq!(name_of(&ui), "2020-01-02.md");
    assert_eq!(std::fs::read_to_string(&today).unwrap(), format!("# {date}\n\n"));
    assert_eq!(ui.element("row-2020-01-02.md").label(), Some("Rain all day. Good."));
    ui.keys("up enter");
    assert_eq!(path_of(&ui).as_ref(), Some(&today));
    ui.type_text("Slept badly.");
    click_row(&mut ui, "2020-01-01.md");
    assert!(std::fs::read_to_string(&today).unwrap().contains("Slept badly."));
}

fn journal(name: &str, date: Option<crate::project::DateHeading>) -> TempDir {
    let d = TempDir::new(name);
    d.file("2020-01-01.md", "New year.\n");
    crate::project::write_settings(&d.0, &crate::project::Settings { kind: Kind::Journal, notes: None, title: None, author: None, date }).unwrap();
    d
}

fn today_heading(form: crate::project::DateHeading) -> String {
    crate::project::Day::today().heading(form).unwrap()
}

/// PROJ-6
#[test]
fn a_new_day_starts_with_its_date_on_creation() {
    let d = journal("dated", None);
    let mut ui = Ui::open_folder(&d.0);
    let today = d.0.join(crate::project::Day::today().file_name());
    assert_eq!(path_of(&ui).as_ref(), Some(&today));
    let date = today_heading(crate::project::DateHeading::Long);
    assert_eq!(page_text(&ui), format!("# {date}\n\n"), "the page is dated before you type");
    ui.type_text("Slept");
    assert_eq!(page_text(&ui), format!("# {date}\n\nSlept"));
    ui.type_text(" badly.");
    ui.keys("cmd-z");
    assert!(!page_text(&ui).contains("Slept"), "the typing goes first");
    assert_eq!(page_text(&ui), format!("# {date}\n\n"));
    ui.keys("cmd-z");
    assert_eq!(page_text(&ui), "", "then the date");
    ui.keys("cmd-shift-z");
    ui.type_text("Rain.");
    click_row(&mut ui, "2020-01-01.md");
    assert_eq!(std::fs::read_to_string(&today).unwrap(), format!("# {date}\n\nRain."));
    ui.win.update(|ws, _, cx| ws.rescan(cx));
    ui.draw();
    assert_eq!(ui.element(gpui_kit::SharedString::from(format!("row-{}", crate::project::Day::today().file_name()))).label(), Some("Rain."), "the row leaves the date out");
}

/// PROJ-6
#[test]
fn a_day_already_written_gets_no_date() {
    let d = journal("written", None);
    d.file(&crate::project::Day::today().file_name(), "Already here.");
    let mut ui = Ui::open_folder(&d.0);
    ui.keys("cmd-down");
    ui.type_text(" More.");
    assert_eq!(page_text(&ui), "Already here. More.");
}

/// PROJ-6
#[test]
fn the_date_takes_the_form_project_settings_chose() {
    let d = journal("iso", Some(crate::project::DateHeading::Iso));
    let mut ui = Ui::open_folder(&d.0);
    assert_eq!(page_text(&ui), format!("# {}\n\n", today_heading(crate::project::DateHeading::Iso)));
    ui.type_text("x");
    assert_eq!(page_text(&ui), format!("# {}\n\nx", today_heading(crate::project::DateHeading::Iso)));

    let d = journal("chosen", None);
    let mut ui = Ui::open_folder(&d.0);
    ui.keys("cmd-,");
    ui.click_id("nav-project");
    assert!(ui.has_element(("settings-date", 0usize)), "a journal has the row");
    ui.click_id(("settings-date", 1usize));
    assert_eq!(settings_of(&d).and_then(|s| s.date), Some(crate::project::DateHeading::Short));
    ui.click_id(("settings-notes", 99usize));
    assert_eq!(settings_of(&d).and_then(|s| s.date), Some(crate::project::DateHeading::Short), "other choices keep it");
    ui.click_id(("settings-date", 3usize));
    ui.keys("escape");
    assert_eq!(page_text(&ui), format!("# {}\n\n", today_heading(crate::project::DateHeading::Long)), "a created page keeps its date");

    let d = journal("no-date", Some(crate::project::DateHeading::Off));
    let mut ui = Ui::open_folder(&d.0);
    assert_eq!(page_text(&ui), "");
    ui.type_text("x");
    assert_eq!(page_text(&ui), "x", "none: the page as typed");
}

#[test]
fn a_book_has_no_date_row() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    ui.click_id("project-kind");
    assert!(!ui.has_element(("settings-date", 0usize)));
}

/// PROJ-6
#[test]
fn brackets_follow_the_calendar_in_a_journal() {
    let d = TempDir::new("cal");
    d.file("2020-01-02.md", "b\n");
    d.file("2020-01-01.md", "a\n");
    let mut ui = Ui::open_folder(&d.0);
    ui.keys("cmd-[");
    assert_eq!(name_of(&ui), "2020-01-02.md");
    ui.keys("cmd-[");
    assert_eq!(name_of(&ui), "2020-01-01.md");
    ui.keys("cmd-]");
    assert_eq!(name_of(&ui), "2020-01-02.md");
}

/// PROJ-2, PROJ-7
#[test]
fn notes_sit_apart_from_the_book() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    assert!(!ui.has_element("Chapters") && !ui.has_element("Notes") && !ui.has_element("Find"), "no tabs");
    assert_eq!(ui.element("notes-section").label(), None);
    assert!(!ui.has_element("row-people.md"), "folded");
    ui.click_id("notes-section");
    click_row(&mut ui, "people.md");
    assert!(page_text(&ui).contains("Razumihin"));
    assert!(summary(&mut ui).ends_with("4 chapters"), "the book's count alone: {}", summary(&mut ui));
    assert!(ui.has_element("row-01-the-garret.md"), "the book and its notes in one list");
    ui.keys("cmd-]");
    assert_eq!(name_of(&ui), "people.md");
}

/// PROJ-7
#[test]
fn a_note_on_the_page_opens_the_notes() {
    let d = book();
    {
        let mut ui = Ui::open_folder(&d.0);
        ui.click_id("notes-section");
        click_row(&mut ui, "people.md");
        ui.click_id("notes-section");
        assert!(!ui.has_element("row-people.md"));
    }
    let mut ui = Ui::open_folder(&d.0);
    assert_eq!(name_of(&ui), "people.md");
    assert!(ui.has_element("row-people.md"));
}

/// WIN-6, ALT-15
#[test]
fn alternatives_take_the_slot_and_esc_steps_back() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    assert_eq!(ui.element("switch-project").selected(), Some(true));
    ui.select_text("exceptionally");
    ui.keys("cmd-shift-a");
    assert!(ui.has_element("sidebar"));
    assert!(!ui.has_element("project"));
    assert_eq!(ui.element("switch-alternatives").selected(), Some(true));
    ui.keys("escape");
    assert_eq!(ui.sidebar(), None, "the list of every group");
    ui.keys("escape");
    assert!(ui.has_element("project"));
    assert!(!ui.has_element("sidebar"));
    ui.keys("down");
    assert_eq!(name_of(&ui), "02-marmeladov.md");
    ui.keys("escape");
    assert!(!ui.has_element("project"));
}

/// PROJ-8
#[test]
fn a_new_chapter_lands_after_the_current_one() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    ui.keys("cmd-n");
    ui.type_text("the visit");
    ui.keys("enter");
    assert_eq!(name_of(&ui), "02-the-visit.md");
    assert_eq!(
        names(&d.0.join("part-1")),
        ["01-the-garret.md", "02-the-visit.md", "03-marmeladov.md", "04-the-letter.md"]
    );
    assert_eq!(page_text(&ui), "# the visit\n\n");
    ui.type_text("Words.");
    assert_eq!(page_text(&ui), "# the visit\n\nWords.");
    ui.keys("cmd-z");
    assert_eq!(page_text(&ui), "# the visit\n\n");
    ui.keys("cmd-z");
    assert_eq!(page_text(&ui), "");
}

/// PROJ-8
#[test]
fn cancelling_a_new_chapter_changes_nothing() {
    let d = book();
    let before = names(&d.0);
    let mut ui = Ui::open_folder(&d.0);
    ui.keys("cmd-n");
    ui.type_text("never mind");
    ui.keys("escape");
    assert_eq!(names(&d.0), before);
    assert_eq!(name_of(&ui), "01-the-garret.md");
}

fn field_hint(ui: &Ui) -> Option<String> {
    ui.read(|ws, cx| ws.project.as_ref()?.edit.as_ref().map(|e| e.field.read(cx).placeholder.to_string()))
}

/// PROJ-8
#[test]
fn the_new_chapter_line_adds_at_the_end() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    let last = ui.element("row-01-the-summons.md").bounds();
    let line = ui.element("add-chapter").bounds();
    assert!(line.top() >= last.bottom(), "under the last chapter");
    ui.click_id("add-chapter");
    assert!(!ui.has_element("add-chapter"), "the field takes its place");
    assert_eq!(field_hint(&ui).as_deref(), Some("name the chapter"));
    ui.type_text("the confession");
    ui.keys("enter");
    assert_eq!(name_of(&ui), "02-the-confession.md");
    assert_eq!(names(&d.0.join("part-2")), ["01-the-summons.md", "02-the-confession.md"]);
    assert_eq!(names(&d.0.join("part-1")).len(), 3, "part 1 untouched");
    let new = ui.element("row-02-the-confession.md").bounds();
    assert!(ui.element("add-chapter").bounds().top() >= new.bottom(), "the line follows the book's end");
    ui.keys("cmd-p down");
    assert_eq!(name_of(&ui), "02-the-confession.md");
}

/// PROJ-8
#[test]
fn the_new_chapter_line_cancels_and_cmd_n_still_goes_after_the_page() {
    let d = book();
    let before = names(&d.0);
    let mut ui = Ui::open_folder(&d.0);
    ui.click_id("add-chapter");
    ui.type_text("never mind");
    ui.keys("escape");
    assert_eq!(names(&d.0), before);
    assert!(ui.has_element("add-chapter"));
    ui.keys("cmd-n");
    assert_eq!(field_hint(&ui).as_deref(), Some("name the chapter"));
    assert!(!ui.has_element("add-chapter"));
}

/// PROJ-7
#[test]
fn a_new_note_goes_in_the_notes() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    assert!(!ui.has_element("row-people.md"), "notes start folded");
    ui.click_id("add-note"); // folded, they still take a new one
    assert_eq!(field_hint(&ui).as_deref(), Some("name the note"));
    ui.type_text("places");
    ui.keys("enter");
    assert_eq!(name_of(&ui), "places.md");
    assert!(d.0.join("notes/places.md").exists());
    assert_eq!(page_text(&ui), "# places\n\n");
    assert_eq!(names(&d.0.join("part-2")), ["01-the-summons.md"]);
}

#[test]
fn a_folder_adds_files_and_a_journal_needs_no_line() {
    let d = TempDir::new("plain-add");
    d.file("on-margins.md", "a\n");
    d.file("keep-the-words.md", "b\n");
    let mut ui = Ui::open_folder(&d.0);
    ui.keys("escape"); // the first-open sheet
    assert!(!ui.has_element("add-chapter"));
    ui.click_id("add-file");
    assert_eq!(field_hint(&ui).as_deref(), Some("name the file"));
    ui.type_text("outline");
    ui.keys("enter");
    assert_eq!(name_of(&ui), "outline.md");
    assert!(d.0.join("outline.md").exists());
    assert_eq!(page_text(&ui), "# outline\n\n");

    // With a subfolder the line still adds to the folder itself, and comes before the subfolder.
    d.file("drafts/some-stuff.md", "c\n");
    let mut ui = Ui::open_folder(&d.0);
    ui.keys("escape");
    assert!(ui.element("add-file").bounds().top() < ui.element("head-drafts").bounds().top());
    ui.click_id("add-file");
    ui.type_text("margins");
    ui.keys("enter");
    assert!(d.0.join("margins.md").exists() && !d.0.join("drafts/margins.md").exists());
    // and each subfolder has its own
    ui.click_id("add-file-drafts");
    ui.type_text("plan");
    ui.keys("enter");
    assert!(d.0.join("drafts/plan.md").exists());

    let j = TempDir::new("journal-add");
    j.file("2020-01-01.md", "a\n");
    let mut ui = Ui::open_folder(&j.0);
    assert!(!ui.has_element("add-chapter") && !ui.has_element("add-file"));
}

#[test]
fn new_in_a_journal_is_today() {
    let d = TempDir::new("new-j");
    d.file("2020-01-01.md", "a\n");
    let mut ui = Ui::open_folder(&d.0);
    click_row(&mut ui, "2020-01-01.md");
    ui.action(NewDocument);
    assert_eq!(name_of(&ui), crate::project::Day::today().file_name());
}

/// PROJ-9
#[test]
fn renaming_keeps_the_number_and_the_document() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    let from = d.0.join("part-1/01-the-garret.md");
    let b = ui.element("row-01-the-garret.md").bounds();
    ui.click(b.center(), MouseButton::Right);
    assert_eq!(ui.menu_labels()[0], "Rename");
    ui.menu("Rename");
    ui.type_text("the attic");
    ui.keys("enter");
    assert_eq!(name_of(&ui), "01-the-attic.md");
    assert!(!from.exists());
    ui.keys("cmd-z");
    assert_eq!(name_of(&ui), "01-the-garret.md");
    assert!(from.exists());
}

/// FILE-4
#[test]
fn backups_follow_a_rename() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    let from = d.0.join("part-1/01-the-garret.md");
    ui.type_text("x");
    ui.keys("cmd-s");
    let backups = |p: &Path| crate::files::backup_count(p);
    assert_eq!(backups(&from), 1);
    ui.read(|ws, cx| assert_eq!(ws.model.read(cx).path.as_ref(), Some(&from)));
    let to = d.0.join("part-1/01-the-attic.md");
    ui.win.update(|ws, window, cx| ws.rename_file(&from, "the attic", window, cx));
    ui.draw();
    assert_eq!((backups(&from), backups(&to)), (0, 1));
}

/// PROJ-10
#[test]
fn trash_is_one_undo_away() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    let gone = d.0.join("part-1/01-the-garret.md");
    ui.keys("cmd-p cmd-backspace");
    assert!(!gone.exists());
    assert_eq!(name_of(&ui), "02-marmeladov.md");
    assert!(!ui.has_element("row-01-the-garret.md"));
    ui.keys("cmd-z");
    assert!(gone.exists());
    assert_eq!(name_of(&ui), "01-the-garret.md");
    assert!(page_text(&ui).contains("exceptionally"));
}

/// PROJ-10
#[test]
fn trashing_the_last_file_leaves_a_blank_page() {
    let d = TempDir::new("last");
    d.file("01-only.md", "Only.\n");
    let mut ui = Ui::open_folder(&d.0);
    ui.keys("cmd-p cmd-backspace");
    assert_eq!(page_text(&ui), "");
    assert_eq!(path_of(&ui), None);
    assert!(ui.has_element("first-chapter"));
}

/// PROJ-11
#[test]
fn dragging_a_chapter_reorders_the_files() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    ui.drag_row("row-03-the-letter.md", "row-02-marmeladov.md", true);
    assert_eq!(names(&d.0.join("part-1")), ["01-the-garret.md", "02-the-letter.md", "03-marmeladov.md"]);
    assert_eq!(name_of(&ui), "01-the-garret.md");
    ui.keys("cmd-z");
    assert_eq!(names(&d.0.join("part-1")), ["01-the-garret.md", "02-marmeladov.md", "03-the-letter.md"]);
}

/// PROJ-7, PROJ-11
#[test]
fn chapters_drag_into_the_notes_and_back() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    ui.click_id("notes-section");
    ui.drag_row("row-03-the-letter.md", "notes-section", false);
    assert!(d.0.join("notes/the-letter.md").exists(), "it leaves its number behind");
    assert_eq!(names(&d.0.join("part-1")), ["01-the-garret.md", "02-marmeladov.md"]);
    ui.drag_row("row-the-letter.md", "row-02-marmeladov.md", true);
    assert_eq!(names(&d.0.join("part-1")), ["01-the-garret.md", "02-the-letter.md", "03-marmeladov.md"]);
    assert!(!d.0.join("notes/the-letter.md").exists());
    ui.keys("cmd-z");
    assert!(d.0.join("notes/the-letter.md").exists());
    assert_eq!(names(&d.0.join("part-1")), ["01-the-garret.md", "02-marmeladov.md"]);
}

/// PROJ-7
#[test]
fn notes_have_a_line_in_each_of_their_folders() {
    let d = book();
    d.file("notes/research/fever.md", "a\n");
    let mut ui = Ui::open_folder(&d.0);
    ui.click_id("notes-section");
    assert!(ui.element("add-note").bounds().top() < ui.element("head-research").bounds().top(), "the notes' own line comes first");
    ui.click_id("add-note-research");
    ui.type_text("streets");
    ui.keys("enter");
    assert!(d.0.join("notes/research/streets.md").exists());
}

/// PROJ-7
#[test]
fn a_journal_trades_days_and_notes_by_dragging() {
    let d = TempDir::new("journal-notes");
    d.file("2020-01-01.md", "a\n");
    d.file("notes/ideas.md", "b\n");
    choose(&d, Kind::Journal, Some("notes"));
    let mut ui = Ui::open_folder(&d.0);
    ui.click_id("notes-section");
    ui.drag_row("row-ideas.md", "row-2020-01-01.md", false);
    assert!(!d.0.join("notes/ideas.md").exists());
    assert!(names(&d.0).iter().any(|n| n.ends_with(" ideas.md") && !n.starts_with("notes/")), "dated like Add to the Journal: {:?}", names(&d.0));
    ui.drag_row("row-2020-01-01.md", "notes-section", false);
    assert!(d.0.join("notes/2020-01-01.md").exists());
}

/// PROJ-11
#[test]
fn dragging_in_a_plain_folder_moves_between_folders_and_never_numbers() {
    let d = TempDir::new("plain");
    d.file("on-margins.md", "a\n");
    d.file("keep-the-words.md", "b\n");
    d.file("drafts/some-stuff.md", "c\n");
    let mut ui = Ui::open_folder(&d.0);
    assert_eq!(ui.read(|ws, _| ws.project_kind()), Some(Kind::Folder));
    ui.keys("escape"); // the first-open sheet; the list under it doesn't take drags
    // Its order is its own (newest first): a drop among the file's folder-mates moves nothing.
    ui.drag_row("row-on-margins.md", "row-keep-the-words.md", false);
    assert!(!ui.app.has_pending_prompt(), "no offer to number the files");
    assert_eq!(names(&d.0), ["drafts/some-stuff.md", "keep-the-words.md", "on-margins.md"]);

    click_row(&mut ui, "some-stuff.md");
    ui.drag_row("row-some-stuff.md", "row-on-margins.md", true);
    assert!(!ui.app.has_pending_prompt());
    assert!(d.0.join("some-stuff.md").exists() && !d.0.join("drafts/some-stuff.md").exists());
    assert_eq!(name_of(&ui), "some-stuff.md", "the page follows its file");
    assert_eq!(ui.read(|ws, _| ws.project_kind()), Some(Kind::Folder));
    assert!(!d.0.join(".dostoevsky").exists(), "a plain folder stays untouched");
    ui.keys("cmd-z");
    assert!(d.0.join("drafts/some-stuff.md").exists());
}

/// PROJ-1
#[test]
fn an_empty_folder_offers_a_start() {
    let d = TempDir::new("empty");
    let mut ui = Ui::open_folder(&d.0);
    assert_eq!(path_of(&ui), None);
    assert!(ui.has_element("first-chapter"));
    ui.click_id("todays-page");
    assert_eq!(ui.read(|ws, _| ws.project_kind()), Some(Kind::Journal));
    assert_eq!(name_of(&ui), crate::project::Day::today().file_name());
}

#[test]
fn an_untitled_first_chapter_has_no_heading() {
    let d = TempDir::new("empty-book");
    let mut ui = Ui::open_folder(&d.0);
    ui.click_id("first-chapter");
    assert_eq!(name_of(&ui), "01-untitled.md");
    assert_eq!(page_text(&ui), "");
}

#[test]
fn a_new_chapters_heading_reaches_the_file() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    ui.keys("cmd-n");
    ui.type_text("the visit");
    ui.keys("enter");
    ui.type_text("Words.");
    click_row(&mut ui, "01-the-garret.md");
    let saved = std::fs::read_to_string(d.0.join("part-1/02-the-visit.md")).unwrap();
    assert_eq!(saved, "# the visit\n\nWords.");
}

#[test]
fn words_on_a_blank_page_get_a_file_on_leaving() {
    let d = TempDir::new("blank");
    let mut ui = Ui::open_folder(&d.0);
    ui.type_text("Before there was a file.");
    ui.click_id("first-chapter");
    assert_eq!(name_of(&ui), "01-untitled.md");
    assert!(std::fs::read_to_string(d.0.join("untitled.md")).unwrap().contains("Before there was a file."));
}

#[test]
fn the_list_follows_the_disk() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    d.file("part-1/04-the-boulevard.md", "New.\n");
    ui.win.update(|ws, _, cx| ws.rescan(cx));
    ui.draw();
    assert!(ui.has_element("row-04-the-boulevard.md"));
}

#[test]
fn a_parked_file_changed_on_disk_reloads() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    click_row(&mut ui, "02-marmeladov.md");
    d.file("part-1/01-the-garret.md", "# Changed\n\nBy someone else.\n");
    click_row(&mut ui, "01-the-garret.md");
    assert!(page_text(&ui).contains("By someone else."));
}

/// PROJ-2
#[test]
fn the_folder_holds_only_the_users_files() {
    let d = book();
    let before = names(&d.0);
    let mut ui = Ui::open_folder(&d.0);
    ui.keys("cmd-p down down escape");
    assert_eq!(names(&d.0), before);
}

#[test]
fn a_project_reopens_where_you_left_it() {
    let d = book();
    {
        let mut ui = Ui::open_folder(&d.0);
        ui.keys("cmd-]");
        ui.keys("cmd-p escape");
    }
    let ui = Ui::open_folder(&d.0);
    assert_eq!(name_of(&ui), "02-marmeladov.md");
    assert!(!ui.read(|ws, _| ws.panel_open()), "the list stays closed");
}

#[test]
fn menus_name_what_the_window_holds() {
    let prefs = crate::prefs::Prefs::default();
    let labels = |kind| {
        crate::actions::menus(&[], &prefs, kind, false)
            .into_iter()
            .flat_map(|m| m.items)
            .filter_map(|i| match i {
                gpui_kit::MenuItem::Action { name, .. } => Some(name.to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let book = labels(Some(Kind::Book));
    assert!(book.iter().any(|l| l == "New Chapter") && book.iter().any(|l| l == "Next Chapter"));
    let journal = labels(Some(Kind::Journal));
    assert!(journal.iter().any(|l| l == "Today’s Page") && journal.iter().any(|l| l == "Previous Day"));
    let single = labels(None);
    assert!(single.iter().any(|l| l == "Open Enclosing Folder"));
    assert!(!single.iter().any(|l| l == "Project"));
}

#[test]
fn a_file_can_grow_into_its_folder() {
    let d = book();
    let mut ui = Ui::open_file(&d.0.join("part-1/02-marmeladov.md"));
    ui.type_text("x");
    ui.action(OpenEnclosingFolder);
    assert_eq!(ui.read(|ws, _| ws.project_kind()), Some(Kind::Book));
    assert!(ui.has_element("row-01-the-garret.md"));
    assert!(page_text(&ui).starts_with('x'), "the page is untouched");
}

#[test]
#[cfg(unix)]
#[cfg_attr(not(target_os = "macos"), ignore = "the sandbox is the Mac App Store's")]
fn enclosing_folder_asks_under_the_sandbox() {
    use std::os::unix::fs::PermissionsExt;
    let d = book();
    let part = d.0.join("part-1");
    let mut ui = Ui::open_file(&part.join("02-marmeladov.md"));
    std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o311)).unwrap();
    crate::bookmarks::PRETEND_SANDBOX.set(true);
    crate::platform::FOLDER_ANSWER.with(|a| *a.borrow_mut() = Some(d.0.clone()));
    ui.action(OpenEnclosingFolder);
    crate::bookmarks::PRETEND_SANDBOX.set(false);
    std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(crate::platform::FOLDER_PANELS.with(|p| p.borrow().clone()), [part], "the panel starts at the folder");
    assert_eq!(ui.read(|ws, _| ws.project_kind()), Some(Kind::Book), "what was chosen opens");
}

fn find_in_project(ui: &mut Ui, query: &str) {
    ui.keys("cmd-shift-f");
    ui.type_text(query);
}

fn hits(ui: &Ui) -> Vec<(String, usize)> {
    ui.read(|ws, cx| ws.find_bar().map(|b| b.read(cx).files.iter().map(|h| (h.label.clone(), h.count)).collect()).unwrap_or_default())
}

/// PROJ-12
#[test]
fn find_reads_every_file() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    find_in_project(&mut ui, "letter");
    assert_eq!(hits(&ui), [("the letter".to_string(), 2)]);
    assert_eq!(ui.element("find-summary").label(), Some("2 in 1 chapter"));
    ui.keys("cmd-a");
    ui.type_text("razumihin");
    assert_eq!(hits(&ui), [("people".to_string(), 1)]);
    assert_eq!(ui.element("find-summary").label(), Some("1 in 1 note"));
    ui.click_id("opt-notes");
    assert!(hits(&ui).is_empty(), "notes left out");
    ui.click_id("opt-notes");
    ui.keys("cmd-a");
    ui.type_text("no such word");
    assert!(hits(&ui).is_empty());
    assert_eq!(ui.element("find-summary").label(), Some("nothing found"));
    assert_eq!(ui.element("find-count").label(), Some("nothing"));
}

/// PROJ-12
#[test]
fn find_has_a_scope() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    ui.keys("cmd-shift-f");
    ui.type_text("letter");
    assert_eq!(ui.element("scope-project").checked(), Some(true));
    assert!(ui.has_element("find-list"));
    ui.keys("escape");
    assert!(!ui.has_element("find-list"), "esc folds the list first");
    assert!(ui.find_state().is_some());
    ui.click_id("scope-file");
    assert_eq!(ui.element("find-summary").label(), Some("nothing in the garret"));
    assert!(ui.has_element("find-widen"), "the whole book is offered");
    ui.click_id("find-widen");
    assert_eq!(ui.element("scope-project").checked(), Some(true));
}

/// PROJ-12
#[test]
fn find_next_crosses_into_the_next_chapter() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    find_in_project(&mut ui, "the");
    let total = ui.read(|ws, cx| ws.find_bar().map(|b| b.read(cx).project_total())).unwrap_or(0);
    assert!(total > 2);
    ui.keys("cmd-g");
    assert_eq!(name_of(&ui), "01-the-garret.md");
    assert_eq!(ui.element("find-count").label(), Some(format!("1 / {total}").as_str()));
    ui.keys("cmd-g");
    assert_eq!(name_of(&ui), "03-the-letter.md", "marmeladov has none");
    assert_eq!(ui.element("find-count").label(), Some(format!("2 / {total}").as_str()));
    assert_eq!(page_text(&ui).chars().skip(ui.selection().start).take(3).collect::<String>().to_lowercase(), "the");
    ui.keys("cmd-shift-g");
    assert_eq!(name_of(&ui), "01-the-garret.md", "and back");
}

/// PROJ-12
#[test]
fn find_reaches_hidden_alternatives() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    ui.make_group("exceptionally", &["sweltering"]);
    ui.keys("cmd-]");
    find_in_project(&mut ui, "exceptionally");
    let kinds = ui.read(|ws, cx| ws.find_bar().map(|b| b.read(cx).files.iter().flat_map(|h| h.snippets.iter().map(|s| s.kind)).collect::<Vec<_>>()).unwrap_or_default());
    assert_eq!(kinds, [crate::find::SpotKind::Alternative]);
    ui.keys("cmd-g");
    assert_eq!(name_of(&ui), "01-the-garret.md");
    assert!(page_text(&ui).contains("sweltering"), "the page keeps its version");
    assert_eq!(ui.read(|ws, _| ws.sidebar.as_ref().and_then(|s| s.found).map(|f| f.1)), Some(0), "the original is marked");
    ui.keys("escape escape");
    assert!(ui.read(|ws, _| ws.sidebar.is_none()), "find puts the panel away again");
}

/// PROJ-12
#[test]
fn find_sees_what_isnt_saved_yet() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    ui.set_prefs(|p| p.autosave = Autosave::Off);
    ui.caret_at(0);
    ui.type_text("Zebra ");
    find_in_project(&mut ui, "zebra");
    assert_eq!(hits(&ui), [("the garret".to_string(), 1)]);
}

/// PROJ-12
#[test]
fn a_line_of_the_list_goes_there() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    find_in_project(&mut ui, "pocket");
    ui.click_id("hit-the letter-0");
    assert_eq!(name_of(&ui), "03-the-letter.md");
    assert_eq!(ui.find_state().map(|(n, ..)| n), Some(1));
    assert_eq!(page_text(&ui).chars().skip(ui.selection().start).take(6).collect::<String>(), "pocket");
    assert!(!ui.has_element("find-list"));
    ui.keys("escape");
    assert_eq!(ui.find_state(), None);
}

fn whole_book_text(ui: &mut Ui) -> Option<String> {
    if ui.sheet() != Some("export") {
        ui.keys("cmd-shift-e");
    }
    ui.click_id(("export-what", 1usize));
    ui.click_id(("export-format", crate::export_ui::Format::Markdown as usize));
    ui.settle();
    ui.export_form(|f| f.preview.text.clone()).flatten()
}

/// PROJ-13
#[test]
fn whole_book_is_every_chapter_in_order() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    ui.select_text("Garret");
    ui.keys("cmd-/");
    let text = whole_book_text(&mut ui).expect("Export shows it");
    let at = |t: &str| text.find(t).unwrap_or_else(|| panic!("no {t:?} in {text}"));
    assert!(at("# Marmeladov") < at("# The Letter") && at("# The Letter") < at("# The Summons"));
    assert!(!text.contains("Razumihin: loud"), "notes stay out");
    assert!(!text.contains("Garret"), "ghosted text stays out, and the page's own state counts");
    assert!(!text.contains("dostoevsky:"), "no comments");
}

/// PROJ-13
#[test]
fn whole_book_copies_and_exports() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    let text = whole_book_text(&mut ui).unwrap();
    ui.click_id("export-copy");
    assert_eq!(ui.clipboard().map(|c| c.trim_end().to_string()), Some(text.trim_end().to_string()));
    ui.click_id("export-save");
    let out = d.0.parent().unwrap().join(format!("exported-{}.md", std::process::id()));
    ui.choose_save_path(&out);
    ui.settle();
    assert_eq!(std::fs::read_to_string(&out).unwrap().trim_end(), text.trim_end());
    std::fs::remove_file(out).unwrap();
}

/// PROJ-13
#[test]
fn only_a_book_has_a_whole_book() {
    let d = TempDir::new("no-whole");
    d.file("2020-01-01.md", "a\n");
    let mut ui = Ui::open_folder(&d.0);
    ui.keys("cmd-shift-e");
    ui.click_id(("export-what", 1usize));
    assert_eq!(ui.export_form(|f| f.whole_book), Some(false));
    let mut single = Ui::open("# One\n");
    single.keys("cmd-shift-e");
    single.click_id(("export-what", 1usize));
    assert_eq!(single.export_form(|f| f.whole_book), Some(false));
}

/// PROJ-15
#[test]
fn a_books_title_and_author_travel_with_it() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    ui.keys("cmd-shift-e");
    ui.click_id(("export-format", crate::export_ui::Format::Epub as usize));
    ui.click_id("export-title");
    ui.keys("cmd-a");
    ui.type_text("Crime and Punishment");
    ui.click_id("export-author");
    ui.keys("cmd-a");
    ui.type_text("Fyodor Dostoevsky");
    ui.click_id("export-save");
    let out = d.0.parent().unwrap().join(format!("titled-{}.epub", std::process::id()));
    ui.choose_save_path(&out);
    ui.settle();
    let s = crate::project::read_settings(&d.0).unwrap();
    assert_eq!((s.title.as_deref(), s.author.as_deref()), (Some("Crime and Punishment"), Some("Fyodor Dostoevsky")));
    let opf = export::zip::read(&std::fs::read(&out).unwrap()).unwrap().into_iter().find(|(n, _)| n == "OEBPS/content.opf").unwrap().1;
    assert!(String::from_utf8_lossy(&opf).contains("<dc:creator id=\"author\">Fyodor Dostoevsky</dc:creator>"));
    std::fs::remove_file(out).unwrap();
    ui.keys("cmd-shift-e");
    assert_eq!(ui.read(|ws, cx| match &ws.sheet {
        Some(crate::sheets::Sheet::Export(f)) => f.title.read(cx).field_text().to_string(),
        _ => String::new(),
    }), "Crime and Punishment");
}

/// NFR-1
#[test]
fn a_long_list_builds_what_shows_and_follows_the_page() {
    let d = TempDir::new("long");
    for i in 1..=130 {
        d.file(&format!("{i:03}-chapter.md"), "Short.\n");
    }
    let mut ui = Ui::open_folder(&d.0);
    assert!(ui.has_element("row-001-chapter.md"));
    assert!(!ui.has_element("row-130-chapter.md"), "far rows aren't built");
    ui.keys("cmd-p");
    for _ in 0..110 {
        ui.keys("down");
    }
    assert_eq!(name_of(&ui), "111-chapter.md");
    assert!(!ui.has_element("row-001-chapter.md"));
    let b = ui.element("row-111-chapter.md").bounds();
    assert_eq!(f32::from(b.size.height), 24., "three-digit numbers don't wrap");
    assert!(f32::from(b.bottom()) < 860. - 60., "the list followed the page: {b:?}");
}

fn settings_of(d: &TempDir) -> Option<crate::project::Settings> {
    crate::project::read_settings(&d.0)
}

fn right_click_row(ui: &mut Ui, name: &str) {
    let b = ui.element(gpui_kit::SharedString::from(format!("row-{name}"))).bounds();
    ui.click(b.center(), MouseButton::Right);
}

/// PROJ-14
#[test]
fn a_folder_opened_for_the_first_time_asks_what_it_is() {
    let d = unchosen_book();
    let before = names(&d.0);
    let mut ui = Ui::open_folder(&d.0);
    assert_eq!(ui.sheet(), Some("project-type"));
    assert_eq!(ui.element("kind-book").selected(), Some(true), "the guess is picked");
    assert_eq!(names(&d.0), before, "nothing written while asking");
    assert!(ui.has_element("row-people.md"), "a stray file is listed, not hidden");
    ui.click_id("first-open-confirm");
    assert_eq!(ui.sheet(), None);
    assert_eq!(settings_of(&d).map(|s| s.kind), Some(Kind::Book));
    assert_eq!(names(&d.0).len(), before.len() + 1, "only .dostoevsky was added");
    let again = Ui::open_folder(&d.0);
    assert_eq!(again.sheet(), None, "asked once");
    assert_eq!(again.read(|ws, _| ws.project_kind()), Some(Kind::Book));
}

/// PROJ-14
#[test]
fn first_open_can_move_the_strays_into_notes() {
    let d = unchosen_book();
    let mut ui = Ui::open_folder(&d.0);
    ui.click_id(("notes-folder", 0usize));
    ui.click_id(("loose", 1usize));
    ui.click_id("first-open-confirm");
    assert!(d.0.join("notes/people.md").exists());
    assert!(!d.0.join("people.md").exists());
    assert_eq!(settings_of(&d).and_then(|s| s.notes).as_deref(), Some("notes"));
    ui.click_id("notes-section");
    assert!(ui.has_element("row-people.md"));
}

/// PROJ-14
#[test]
fn not_now_writes_nothing_and_asks_again() {
    let d = unchosen_book();
    let before = names(&d.0);
    let mut ui = Ui::open_folder(&d.0);
    ui.click_id("first-open-cancel");
    assert_eq!(ui.sheet(), None);
    assert_eq!(names(&d.0), before);
    assert_eq!(Ui::open_folder(&d.0).sheet(), Some("project-type"));
}

/// PROJ-14
#[test]
fn a_plain_folders_choice_stays_out_of_the_folder() {
    let d = TempDir::new("essays");
    d.file("on-margins.md", "a\n");
    let mut ui = Ui::open_folder(&d.0);
    ui.click_id("kind-folder");
    ui.click_id("first-open-confirm");
    assert_eq!(names(&d.0), ["on-margins.md"]);
    assert_eq!(Ui::open_folder(&d.0).sheet(), None, "remembered in Application Support");
}

/// PROJ-7
#[test]
fn a_stray_file_joins_the_book_or_the_notes() {
    let d = book();
    d.file("outline.md", "# Outline\n\nThree acts.\n");
    let mut ui = Ui::open_folder(&d.0);
    assert!(ui.has_element("head-not in the book"));
    right_click_row(&mut ui, "outline.md");
    assert_eq!(ui.menu_labels()[..2], ["Add to the Book", "Move to Notes"]);
    ui.menu("Add to the Book");
    assert!(d.0.join("part-2/02-outline.md").exists());
    assert!(!ui.has_element("head-not in the book"));
    right_click_row(&mut ui, "02-marmeladov.md");
    ui.menu("Move to Notes");
    assert!(d.0.join("notes/marmeladov.md").exists());
    assert!(!d.0.join("part-1/02-marmeladov.md").exists());
}

/// PROJ-7
#[test]
fn a_note_goes_back_to_the_book() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    ui.click_id("notes-section");
    right_click_row(&mut ui, "people.md");
    assert_eq!(ui.menu_labels()[0], "Move to the Book");
    ui.menu("Move to the Book");
    assert!(d.0.join("part-2/02-people.md").exists());
}

/// PROJ-7
#[test]
fn move_to_notes_makes_a_notes_folder_when_there_is_none() {
    let d = unchosen_book();
    choose(&d, Kind::Book, None);
    let mut ui = Ui::open_folder(&d.0);
    assert!(!ui.has_element("notes-section"), "no notes folder, no notes section");
    right_click_row(&mut ui, "people.md");
    ui.menu("Move to Notes");
    assert!(d.0.join("notes/people.md").exists());
    assert_eq!(settings_of(&d).and_then(|s| s.notes).as_deref(), Some("notes"));
    assert!(ui.has_element("notes-section"));
}

/// PROJ-15
#[test]
fn project_settings_change_the_type_and_rename_nothing() {
    let d = book();
    let before = names(&d.0);
    let mut ui = Ui::open_folder(&d.0);
    assert_eq!(ui.element("project-kind").label(), Some("book ▾"));
    ui.click_id("project-kind");
    assert_eq!(ui.sheet(), Some("prefs"));
    assert!(ui.has_element("project-settings-name"), "on the Project section");
    ui.click_id(("project-type", 2usize));
    assert_eq!(ui.read(|ws, _| ws.project_kind()), Some(Kind::Folder));
    assert_eq!(settings_of(&d).map(|s| s.kind), Some(Kind::Folder), "kept in the file that was there");
    assert!(!ui.has_element(("settings-notes", 99usize)), "a plain folder has no notes folder to choose");
    ui.keys("escape");
    assert_eq!(ui.sheet(), None);
    assert!(!ui.has_element("notes-section"));
    ui.click_id("project-kind");
    ui.click_id(("project-type", 0usize));
    ui.keys("escape");
    assert_eq!(ui.read(|ws, _| ws.project_kind()), Some(Kind::Book));
    assert!(ui.has_element("notes-section"), "a book again, notes/ is its notes again");
    assert_eq!(names(&d.0), before);
}

fn set_new_project_place(ui: &mut Ui, place: &Path) {
    ui.win.update(|ws, _, _| {
        if let Some(crate::sheets::Sheet::NewProject(f)) = &mut ws.sheet {
            f.place = place.to_path_buf();
        }
    });
}

/// PROJ-1
#[test]
fn new_project_makes_a_book() {
    let place = TempDir::new("writing");
    // A window with words in it stays; an empty one would make way for the project.
    let mut ui = Ui::open("Some words.");
    ui.keys("alt-cmd-n");
    assert_eq!(ui.sheet(), Some("new-project"));
    set_new_project_place(&mut ui, &place.0);
    ui.type_text("the idiot");
    ui.click_id("new-project-create");
    let root = place.0.join("the-idiot");
    assert_eq!(names(&root), [".dostoevsky", "01-untitled.md"]);
    assert!(root.join("notes").is_dir());
    assert_eq!(crate::project::read_settings(&root).map(|s| s.kind), Some(Kind::Book));
}

/// PROJ-1
#[test]
fn new_project_makes_a_journal_and_refuses_a_taken_name() {
    let place = TempDir::new("writing-j");
    std::fs::create_dir_all(place.0.join("diary")).unwrap();
    place.file("diary/x.md", "taken\n");
    let mut ui = Ui::open("Some words.");
    ui.keys("alt-cmd-n");
    set_new_project_place(&mut ui, &place.0);
    ui.click_id("new-journal");
    ui.click_id("new-project-name");
    ui.type_text("diary");
    ui.click_id("new-project-create");
    assert!(ui.has_element("new-project-error"));
    assert_eq!(names(&place.0.join("diary")), ["x.md"]);
    ui.click_id("new-project-name");
    ui.keys("cmd-a");
    ui.type_text("days");
    ui.keys("enter");
    let root = place.0.join("days");
    assert!(root.join(crate::project::Day::today().year.to_string()).is_dir());
    assert_eq!(crate::project::read_settings(&root).map(|s| s.kind), Some(Kind::Journal));
}

/// PROJ-1
#[test]
#[cfg(unix)]
#[cfg_attr(not(target_os = "macos"), ignore = "the sandbox is the Mac App Store's")]
fn new_project_asks_once_for_a_place_the_sandbox_hasnt_granted() {
    use std::os::unix::fs::PermissionsExt;
    let place = TempDir::new("writing-sandbox");
    let (locked, granted) = (place.0.join("locked"), place.0.join("granted"));
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::create_dir_all(&granted).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    let mut ui = Ui::open("Some words.");
    ui.keys("alt-cmd-n");
    set_new_project_place(&mut ui, &locked);
    ui.click_id("new-folder");
    ui.type_text("blog");
    ui.click_id("new-project-create");
    let panels = || crate::platform::FOLDER_PANELS.with(|p| p.borrow().clone());
    assert!(ui.has_element("new-project-error") && panels().is_empty(), "not sandboxed: it says so");

    crate::bookmarks::PRETEND_SANDBOX.set(true);
    crate::platform::FOLDER_ANSWER.with(|a| *a.borrow_mut() = Some(granted.clone()));
    ui.click_id("new-project-create");
    assert_eq!(panels(), std::slice::from_ref(&locked), "the panel starts where it couldn't write");
    assert!(!ui.has_element("new-project-error"));
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(names(&granted.join("blog")), ["untitled.md"]);
    assert_eq!(ui.sheet(), None);
    assert!(names(&locked).is_empty());
    assert_eq!(crate::bookmarks::last_place(), Some(granted.clone()));

    let mut ui = Ui::open("More words.");
    ui.keys("alt-cmd-n");
    ui.type_text("letters");
    ui.click_id("new-project-create");
    crate::bookmarks::PRETEND_SANDBOX.set(false);
    assert_eq!(panels().len(), 1, "not asked again");
    assert!(granted.join("letters").is_dir());
}

/// WIN-6
#[test]
fn the_switch_picks_a_side() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    ui.click_id("switch-alternatives");
    assert!(ui.sidebar_open() && !ui.has_element("project"));
    assert_eq!(ui.sidebar(), None);
    assert!(ui.has_element("alt-empty"), "no alternatives in this chapter yet");
    ui.click_id("switch-project");
    assert!(!ui.sidebar_open() && ui.has_element("project"));
    ui.keys("cmd-shift-a");
    assert!(ui.sidebar_open());
    ui.keys("cmd-p");
    assert!(!ui.sidebar_open() && ui.has_element("project"));
}

fn cuts_file(d: &TempDir) -> PathBuf {
    d.0.join(crate::project::CUTS_FILE)
}

fn chip(ui: &mut Ui, i: usize) -> String {
    ui.element(gpui_kit::ElementId::NamedInteger("origin".into(), i as u64)).label().map(String::from).unwrap_or_default()
}

/// CUT-9
#[test]
fn cut_14_a_chapters_cuts_live_in_cuts_md() {
    let d = book();
    {
        let mut ui = Ui::open_folder(&d.0);
        assert_eq!(name_of(&ui), "01-the-garret.md");
        ui.select_text("exceptionally hot");
        ui.keys("cmd-shift-x");
        assert_eq!(ui.notes(), ["exceptionally hot"]);
        ui.keys("cmd-s");
        let file = std::fs::read_to_string(cuts_file(&d)).unwrap();
        assert!(file.contains("<!-- dostoevsky:overflow\nexceptionally hot"), "{file}");
        assert!(file.contains("\"file\":\"part-1/01-the-garret.md\""), "{file}");
        let chapter = std::fs::read_to_string(d.0.join("part-1/01-the-garret.md")).unwrap();
        assert!(!chapter.contains("dostoevsky:overflow"), "{chapter}");
        let rows = ui.read(|ws, _| ws.project.as_ref().unwrap().tree.all().map(|e| e.path.clone()).collect::<Vec<_>>());
        assert!(!rows.contains(&cuts_file(&d)), "cuts.md is not a chapter");
    }
    let mut ui = Ui::open_folder(&d.0);
    assert_eq!(ui.notes(), ["exceptionally hot"], "the chapter has its cut back");
    ui.keys("ctrl-cmd-c");
    assert!(chip(&mut ui, 0).starts_with("from ¶"), "{}", chip(&mut ui, 0));

    click_row(&mut ui, "02-marmeladov.md");
    assert!(ui.notes().is_empty());
    ui.click_id(("cuts-scope", 1usize));
    assert_eq!(ui.notes(), ["exceptionally hot"]);
    assert!(ui.chip(0).is_some_and(|c| c.contains("garret")), "{:?}", ui.chip(0));
    ui.caret_at(0);
    ui.click_note(0);
    ui.keys("cmd-enter");
    assert!(page_text(&ui).starts_with("exceptionally hot"), "{}", page_text(&ui));
    assert!(ui.notes().is_empty(), "{:?}", ui.notes());
    ui.undo();
    assert!(!page_text(&ui).contains("exceptionally hot"));
    assert_eq!(ui.notes(), ["exceptionally hot"]);
}

#[test]
fn cut_14_drawer_md_becomes_cuts_md() {
    let d = book();
    d.file(
        crate::project::DRAWER_FILE,
        "<!-- dostoevsky:overflow\nA typed scrap.\n\nOn an exceptionally\n-->\n\n<!-- dostoevsky:meta v1\n{\"kind\":\"stash\",\"note\":1,\"quote\":\"On an exceptionally\",\"from\":{\"file\":\"part-1/01-the-garret.md\",\"quote\":\"On an exceptionally hot\"}}\n-->\n",
    );
    let mut ui = Ui::open_folder(&d.0);
    assert_eq!(ui.notes(), ["On an exceptionally"], "the garret's scrap is its cut now");
    ui.keys("ctrl-cmd-c");
    ui.click_id(("cuts-scope", 1usize));
    assert_eq!(ui.notes(), ["On an exceptionally", "A typed scrap."], "and the typed one is of no file");
    ui.select_text("hot evening");
    ui.keys("cmd-x");
    ui.keys("cmd-s");
    assert!(cuts_file(&d).exists());
    assert!(!d.0.join(crate::project::DRAWER_FILE).exists(), "drawer.md is cuts.md now");
    let file = std::fs::read_to_string(cuts_file(&d)).unwrap();
    assert!(file.contains("A typed scrap.") && file.contains("hot evening") && file.contains("On an exceptionally"), "{file}");
}

#[test]
fn cut_14_cuts_of_no_file_move_with_one_undo() {
    let d = book();
    d.file(crate::project::DRAWER_FILE, "<!-- dostoevsky:overflow\nA typed scrap.\n-->\n");
    let mut ui = Ui::open_folder(&d.0);
    ui.keys("ctrl-cmd-c");
    ui.click_id(("cuts-scope", 1usize));
    assert_eq!(ui.notes(), ["A typed scrap."]);
    ui.double_click_note(0);
    ui.keys("cmd-down");
    ui.type_text("!");
    ui.keys("escape");
    assert_eq!(ui.notes(), ["A typed scrap.!"]);
    ui.caret_at(0);
    let to = ui.point_for(ui.find("On an").start);
    ui.drag_note(0, to, false);
    assert!(page_text(&ui).contains("A typed scrap.!"), "{}", page_text(&ui));
    assert!(ui.notes().is_empty());
    ui.undo();
    assert_eq!(ui.notes(), ["A typed scrap.!"], "one ⌘Z brings it back");
    assert!(!page_text(&ui).contains("A typed scrap"));
}

#[test]
fn cut_14_a_files_own_cuts_move_to_the_projects() {
    let d = book();
    d.file(
        "part-1/03-the-letter.md",
        "# The Letter\n\nThe letter was in his pocket.\n\n<!-- dostoevsky:overflow\nHis mother wrote it.\n\nSecond scrap.\n-->\n",
    );
    let mut ui = Ui::open_folder(&d.0);
    click_row(&mut ui, "03-the-letter.md");
    ui.keys("ctrl-cmd-c");
    assert_eq!(ui.notes(), ["His mother wrote it.", "Second scrap."]);
    assert_eq!(ui.file_notes().len(), 2);
    assert!(ui.has_element("file-scraps"));
    ui.click_id("move-file-scraps");
    assert!(ui.file_notes().is_empty());
    assert!(!ui.has_element("file-scraps"));
    assert_eq!(ui.notes(), ["His mother wrote it.", "Second scrap."], "the same cuts, the project's now");
    ui.keys("cmd-s");
    let letter = std::fs::read_to_string(d.0.join("part-1/03-the-letter.md")).unwrap();
    assert!(!letter.contains("dostoevsky:overflow"), "{letter}");
    assert!(std::fs::read_to_string(cuts_file(&d)).unwrap().contains("His mother wrote it."));
    ui.undo();
    assert_eq!(ui.file_notes().len(), 2);
}

/// PROJ-12
#[test]
fn cut_14_find_reaches_the_cuts() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    ui.select_text("exceptionally hot");
    ui.keys("cmd-shift-x");
    ui.keys("ctrl-cmd-c");
    assert!(!ui.drawer_open());
    find_in_project(&mut ui, "exceptionally");
    ui.keys("cmd-g");
    assert!(ui.drawer_open());
    assert!(ui.read(|ws, _| ws.drawer.as_ref().and_then(|d| d.found.clone())).is_some(), "its cut is marked");
    assert_eq!(name_of(&ui), "01-the-garret.md", "the page stays");
}

/// PROJ-11
#[test]
fn cut_14_cuts_follow_their_files_when_chapters_move() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    click_row(&mut ui, "02-marmeladov.md");
    ui.select_text("not used to crowds");
    ui.keys("cmd-shift-x");
    click_row(&mut ui, "03-the-letter.md");
    ui.select_text("in his pocket");
    ui.keys("cmd-shift-x");
    ui.keys("cmd-s");
    ui.drag_row("row-03-the-letter.md", "row-02-marmeladov.md", true);
    assert_eq!(names(&d.0.join("part-1")), ["01-the-garret.md", "02-the-letter.md", "03-marmeladov.md"]);
    let file = std::fs::read_to_string(cuts_file(&d)).unwrap();
    assert!(file.contains("\"file\":\"part-1/03-marmeladov.md\""), "{file}");
    assert!(file.contains("\"file\":\"part-1/02-the-letter.md\""), "{file}");
    assert!(!file.contains("03-the-letter"), "{file}");
    let p = ui.point_for(3);
    ui.click(p, MouseButton::Left);
    ui.undo();
    assert!(page_text(&ui).contains("in his pocket"));
    ui.keys("cmd-s");
    let file = std::fs::read_to_string(cuts_file(&d)).unwrap();
    assert!(!file.contains("in his pocket"), "{file}");
    assert!(file.contains("not used to crowds"), "{file}");
}

/// CUT-14
#[test]
fn cut_2_moves_across_chapters() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    ui.keys("cmd-d");
    ui.select_text("exceptionally ");
    ui.keys("cmd-x");
    assert_eq!(ui.ghosts().len(), 1);
    click_row(&mut ui, "02-marmeladov.md");
    let at = ui.find("crowds").start;
    ui.caret_at(at);
    ui.keys("cmd-v");
    assert!(ui.text().contains("used to exceptionally crowds"), "{}", ui.text());
    let garret = ui.read(|ws, _| {
        let p = ws.project.as_ref().unwrap();
        p.parked.values().find(|q| q.model.path.as_ref().is_some_and(|p| p.ends_with("01-the-garret.md"))).map(|q| q.model.doc.cuts().len())
    });
    assert_eq!(garret, Some(0), "the cut left the first chapter");

    ui.keys("cmd-z");
    assert!(!ui.text().contains("exceptionally"), "{}", ui.text());
    click_row(&mut ui, "01-the-garret.md");
    assert_eq!(ui.ghosts().len(), 1, "and came back to its spot in one ⌘Z");
}
