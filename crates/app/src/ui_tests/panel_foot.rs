use crate::prefs::Autosave;
use crate::ui_tests::{TempDir, Ui};

const DOC: &str = "Dear M., the flat is cold.\n";

fn book() -> TempDir {
    let d = TempDir::new("foot-book");
    d.file("01-the-garret.md", "# The Garret\n\nOn an exceptionally hot evening.\n");
    d.file("02-marmeladov.md", "# Marmeladov\n\nRaskolnikov was not used to crowds.\n");
    crate::project::write_settings(&d.0, &crate::project::Settings { kind: crate::project::Kind::Book, notes: None, title: None, author: None, date: None }).unwrap();
    d
}

#[test]
fn foot_in_a_project() {
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    if !ui.read(|ws, _| ws.panel_open()) {
        ui.click_id("panel");
    }
    for id in ["panel-foot", "foot-open", "foot-save", "foot-finder", "foot-export", "foot-settings"] {
        assert!(ui.has_element(id), "{id}");
    }
    ui.click_id("switch-alternatives");
    assert!(ui.sidebar_open());
    assert!(ui.has_element("foot-settings") && ui.has_element("foot-export"), "the same floor under Alternatives");
    ui.click_id("foot-settings");
    assert_eq!(ui.sheet(), Some("prefs"));
    assert!(ui.has_element("nav-project"));
    ui.click_id("nav-project");
    assert!(ui.has_element("project-settings-name") && ui.has_element(("project-type", 0usize)));
}

#[test]
fn open_menu_lists_recent() {
    let d = book();
    let other = TempDir::new("foot-recent");
    let letter = other.file("letter-to-m.md", DOC);
    crate::files::add_recent(&d.0);
    crate::files::add_recent(&letter);
    let mut ui = Ui::open_folder(&d.0);
    if !ui.read(|ws, _| ws.panel_open()) {
        ui.click_id("panel");
    }
    ui.click_id("foot-open");
    let labels = ui.menu_labels();
    assert_eq!(labels.first().map(String::as_str), Some("Open…"));
    assert!(labels.contains(&"letter-to-m.md".to_string()), "{labels:?}");
    assert!(!labels.iter().any(|l| d.0.ends_with(l)), "not the project this window is on: {labels:?}");
    assert!(labels.contains(&"New Project…".to_string()));
    assert!(!labels.contains(&"Open Enclosing Folder".to_string()), "a project has its folder");
    assert!(ui.read(|ws, _| ws.context_menu.as_ref().is_some_and(|m| m.up)));
    let kind = ui.read(|ws, _| {
        ws.context_menu.as_ref().and_then(|m| m.items.iter().flatten().find(|it| it.label.as_ref() == "letter-to-m.md").and_then(|it| it.keys))
    });
    assert_eq!(kind, Some("file"));
}

/// The menu runs inside this window's update, while opening walks every window, this one too.
#[test]
fn open_menu_opens_a_recent_project_and_file() {
    let d = book();
    let stories = TempDir::new("foot-stories");
    stories.file("01-intro.md", "# Intro\n\nA story.\n");
    let other = TempDir::new("foot-recent-file");
    let letter = other.file("letter-to-m.md", DOC);
    crate::files::add_recent(&letter);
    crate::files::add_recent(&stories.0);
    let mut ui = Ui::open_folder(&d.0);
    if !ui.read(|ws, _| ws.panel_open()) {
        ui.click_id("panel");
    }
    let windows = |ui: &Ui| ui.read(|_, cx| crate::workspaces(cx).len());
    let before = windows(&ui);
    let name = stories.0.file_name().unwrap().to_string_lossy().into_owned();
    ui.click_id("foot-open");
    ui.menu(&name);
    assert_eq!(windows(&ui), before + 1, "the project opens in a window of its own");
    ui.click_id("foot-open");
    ui.menu("letter-to-m.md");
    assert_eq!(windows(&ui), before + 2);
}

#[test]
fn save_button_and_its_dot() {
    let dir = TempDir::new("foot-save");
    let path = dir.file("letter.md", DOC);
    let mut ui = Ui::open_file(&path);
    ui.click_id("panel");
    assert!(!ui.has_element("foot-unsaved"));
    ui.caret_at(0);
    ui.type_text("X");
    ui.draw();
    assert!(!ui.has_element("foot-unsaved"), "autosave on: “— Edited” says it");
    ui.set_prefs(|p| p.autosave = Autosave::Off);
    ui.draw();
    assert!(ui.has_element("foot-unsaved"));
    ui.click_id("foot-save");
    assert!(!ui.dirty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), format!("X{DOC}"));
    assert!(!ui.has_element("foot-unsaved"));
}

#[test]
fn project_tab_with_no_project() {
    let dir = TempDir::new("foot-single");
    let path = dir.file("letter-to-m.md", DOC);
    let d = book();
    crate::files::add_recent(&d.0);
    let mut ui = Ui::open_file(&path);
    ui.click_id("panel");
    assert!(ui.has_element("file-panel"));
    let recent: Vec<String> = ui.read(|ws, _| ws.file_panel.as_ref().map(|f| f.recent.iter().map(|r| r.1.clone()).collect()).unwrap_or_default());
    assert!(ui.has_element(gpui_kit::SharedString::from("recent-0")));
    let name = d.0.file_name().unwrap().to_string_lossy().into_owned();
    assert!(recent.contains(&name), "{recent:?}");
    assert!(!recent.contains(&"letter-to-m.md".to_string()), "not the file on the page");
    assert!(ui.has_element("new-project"));
    assert!(ui.has_element("foot-open") && ui.has_element("foot-finder") && ui.has_element("foot-settings"));
    ui.click_id("foot-settings");
    assert_eq!(ui.sheet(), Some("prefs"));
    assert!(!ui.has_element("nav-project"), "no project, no Project section");
    ui.keys("escape");

    ui.click_id("folder-as-project");
    assert!(ui.read(|ws, _| ws.project.is_some()), "the folder became the window's project");
    assert!(!ui.has_element("file-panel") && ui.has_element("project"));
}

#[test]
fn project_tab_on_a_blank_page() {
    let mut ui = Ui::open("");
    ui.click_id("panel");
    assert!(ui.has_element("file-panel"));
    assert!(!ui.has_element("folder-as-project"));
    ui.click_id("foot-finder");
    assert!(ui.has_element("file-panel"), "nothing happens");
}

#[test]
fn export_button_in_a_book() {
    use crate::export_ui::{export_tip, Format};
    let d = book();
    let mut ui = Ui::open_folder(&d.0);
    if !ui.read(|ws, _| ws.panel_open()) {
        ui.click_id("panel");
    }
    let tip = |ui: &Ui| ui.read(|ws, cx| {
        let (in_book, key) = ws.export_tip_source(cx);
        export_tip(in_book, key.as_deref())
    });
    assert_eq!(tip(&ui), "Export… · whole book");
    ui.click_id("foot-export");
    assert_eq!(ui.sheet(), Some("export"));
    assert_eq!(ui.export_form(|f| f.whole_book), Some(true), "the whole book, the first time");
    ui.keys("escape");
    assert_eq!(ui.sheet(), None);

    ui.action(crate::actions::Export);
    assert_eq!(ui.export_form(|f| f.whole_book), Some(false));
    ui.click_id(("export-format", Format::Markdown as usize));
    ui.click_id("export-save");
    let out = d.0.join("the garret (final).md");
    ui.choose_save_path(&out);
    ui.settle();
    assert!(out.exists());
    assert_eq!(tip(&ui), "Export… · this chapter", "remembered: the chapter");
    ui.click_id("foot-export");
    assert_eq!(ui.export_form(|f| f.whole_book), Some(false), "what was chosen last");
}

#[test]
fn export_button_in_a_single_file() {
    let dir = TempDir::new("foot-export-single");
    let path = dir.file("letter-to-m.md", DOC);
    let mut ui = Ui::open_file(&path);
    ui.click_id("panel");
    let (in_book, key) = ui.read(|ws, cx| ws.export_tip_source(cx));
    assert_eq!(crate::export_ui::export_tip(in_book, key.as_deref()), "Export… · this page");
    ui.click_id("foot-export");
    assert_eq!(ui.sheet(), Some("export"));
    assert_eq!(ui.export_form(|f| f.whole_book), Some(false));

    let mut ui = Ui::open("Never saved.\n");
    ui.click_id("panel");
    ui.click_id("foot-export");
    assert_eq!(ui.sheet(), Some("export"));
}
