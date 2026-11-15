use crate::ui_tests::{MenuItem, Ui};

const DOC: &str = "Most of the friction in writing comes from choosing too early.\n";

fn item(label: &str, shortcut: Option<&str>) -> MenuItem {
    MenuItem { label: label.into(), enabled: true, badge: None, shortcut: shortcut.map(String::from) }
}

#[test]
fn ctx_1_word_menu() {
    let mut ui = Ui::open_editing(DOC);
    ui.right_click_text("friction");
    assert_eq!(ui.selection(), ui.find("friction"));
    assert_eq!(
        ui.menu_items(),
        [
            item("Alternatives for “friction”", Some("⇧⌘A")),
            item("AI alternatives for selection", Some("⌥⌘A")),
            item("Add Comment", Some("⌥⌘M")),
            item("Cut", Some("⌘X")),
            item("Cut Without a Trace", Some("⇧⌘X")),
            item("Copy", Some("⌘C")),
            item("Paste", Some("⌘V")),
            item("Look Up “friction”", None),
        ]
    );
}

/// WIN-17
#[test]
fn ctx_1_writing_menu() {
    let mut ui = Ui::open(DOC);
    ui.right_click_text("friction");
    assert_eq!(
        ui.menu_items(),
        [
            item("Alternatives for “friction”", Some("⇧⌘A")),
            item("Cut", Some("⌘X")),
            item("Copy", Some("⌘C")),
            item("Paste", Some("⌘V")),
            item("Look Up “friction”", None),
        ]
    );
    ui.menu("Alternatives for “friction”");
    assert!(ui.read(|ws, cx| ws.model.read(cx).layer), "the item went to editing");
    assert!(ui.sidebar_open());
    assert_eq!(ui.variants(ui.only_group()), ["friction"]);
}

/// WIN-17
#[test]
fn ctx_1_writing_menu_inside_a_group() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &["tension"]);
    ui.keys("cmd-d");
    ui.right_click_text("tension");
    assert_eq!(ui.menu_labels(), ["Open alternatives for “tension”", "Cut", "Copy", "Paste", "Look Up “tension”"]);
    ui.menu("Open alternatives for “tension”");
    assert!(ui.read(|ws, cx| ws.model.read(cx).layer));
    assert_eq!(ui.sidebar(), Some(g));
}

#[test]
fn ctx_1_selection_menu() {
    let mut ui = Ui::open(DOC);
    ui.select_text("choosing too early");
    ui.right_click_text("too");
    assert_eq!(ui.selection(), ui.find("choosing too early"), "a right-click inside keeps the selection");
    assert_eq!(ui.menu_labels()[0], "Alternatives for selection");
}

#[test]
fn ctx_1_alternatives_item() {
    let mut ui = Ui::open(DOC);
    ui.right_click_text("friction");
    ui.menu("Alternatives for “friction”");
    assert!(!ui.menu_open());
    assert!(ui.sidebar_open());
    assert_eq!(ui.variants(ui.only_group()), ["friction"]);
}

#[test]
fn ctx_1_inside_a_group_opens_it() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction in writing", &["drag in prose"]);
    ui.caret_at(0);
    ui.right_click_text("prose");
    assert_eq!(ui.selection(), ui.find("prose"), "the word is still selected, for Copy and Look Up");
    let labels = ui.menu_labels();
    assert_eq!(labels[0], "Open alternatives for “drag in prose”");
    assert!(!labels.iter().any(|l| l == "Alternatives for “prose”"), "{labels:?}");
    ui.menu("Open alternatives for “drag in prose”");
    assert_eq!(ui.sidebar(), Some(g));
    assert_eq!(ui.groups().len(), 1, "no nested group");

    let mut ui = Ui::open(DOC);
    ui.make_group("Most of the friction in writing comes", &["Most of the trouble in writing comes"]);
    ui.caret_at(0);
    ui.right_click_text("trouble");
    assert_eq!(ui.menu_labels()[0], "Open alternatives for “Most of the trouble in…”");
}

#[test]
fn ctx_1_inside_a_paragraph_offers_both() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("Most of the friction in writing comes from choosing too early.", &["A second version."]);
    assert_eq!(ui.visible_group(g).scope, doc_model::Scope::Paragraph);
    ui.caret_at(0);
    ui.right_click_text("second");
    assert_eq!(
        ui.menu_labels()[..5],
        ["Open alternatives for this paragraph", "Settle on this", "AI alternatives for this paragraph", "Alternatives for “second”", "AI alternatives for selection"]
    );
    ui.menu("Open alternatives for this paragraph");
    assert_eq!(ui.sidebar(), Some(g));
}

/// ALT-17
#[test]
fn ctx_1_settle_on_this() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &["drag"]);
    let other = ui.make_group("early", &["soon"]);
    ui.caret_at(0);
    ui.right_click_text("soon");
    ui.menu("Open alternatives for “soon”");
    assert_eq!(ui.sidebar(), Some(other));
    ui.right_click_text("drag");
    let items = ui.menu_items();
    assert_eq!(items[..2], [item("Open alternatives for “drag”", None), item("Settle on this", Some("⌥⌘↩"))]);
    ui.menu("Settle on this");
    assert!(!ui.menu_open());
    assert!(ui.text().contains("Most of the drag in writing"), "{}", ui.text());
    assert_eq!(ui.groups().iter().map(|v| v.id).collect::<Vec<_>>(), [other], "only the right-clicked group goes");
    assert_eq!(ui.sidebar(), Some(other), "the sidebar stays on its group");
    ui.undo();
    assert!(ui.groups().iter().any(|v| v.id == g), "one undo step brings it back");
}

#[test]
fn alt_3_dots_open_the_sidebar() {
    let mut ui = Ui::open_editing(DOC);
    let g = ui.make_group("friction", &["tension"]);
    assert!(!ui.sidebar_open());
    ui.click_dots(g);
    assert_eq!(ui.sidebar(), Some(g));

    let p = ui.make_group("Most of the tension in writing comes from choosing too early.", &["A second version."]);
    ui.click_dots(p);
    assert_eq!(ui.sidebar(), Some(p));
}

#[test]
fn ctx_1_ai_item_opens_the_group() {
    let mut ui = Ui::open_editing(DOC);
    ui.right_click_text("friction");
    ui.menu("AI alternatives for selection");
    assert_eq!(ui.groups().len(), 1);
    assert!(ui.sidebar_open());
    assert!(ui.has_element("ideas-setup"));
}

#[test]
fn ctx_1_ai_item_inside_a_group() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction in writing", &["drag in prose"]);
    let other = ui.make_group("early", &["soon"]);
    ui.caret_at(0);
    ui.right_click_text("soon");
    ui.menu("Open alternatives for “soon”");
    assert_eq!(ui.sidebar(), Some(other));
    ui.right_click_text("prose");
    let labels = ui.menu_labels();
    assert_eq!(labels[..3], ["Open alternatives for “drag in prose”", "Settle on this", "AI alternatives for “drag in prose”"]);
    assert!(!labels.iter().any(|l| l == "AI alternatives for selection"), "{labels:?}");
    ui.menu("AI alternatives for “drag in prose”");
    assert_eq!(ui.groups().len(), 2, "no nested group");
    assert_eq!(ui.sidebar(), Some(g));
    assert!(ui.has_element("ideas-setup"));
}

#[test]
fn ctx_1_cut_copy_paste() {
    let mut ui = Ui::open_editing(DOC);
    ui.right_click_text("friction");
    ui.menu("Copy");
    assert_eq!(ui.clipboard().as_deref(), Some("friction"));

    ui.right_click_text("early");
    ui.menu("Cut");
    assert_eq!(ui.clipboard().as_deref(), Some("early"));
    assert_eq!(ui.ghosts(), [ui.find("early")], "a cut fades in place (CUT-1)");

    ui.right_click_text("writing");
    ui.menu("Paste");
    assert!(ui.text().contains("friction in early comes"), "the cut moved (CUT-2): {}", ui.text());
    assert!(ui.text().contains("choosing too."), "the space it left is tidied: {}", ui.text());

    ui.right_click_text("Most");
    ui.menu("Cut Without a Trace");
    assert_eq!(ui.notes(), ["Most"]);
}

#[test]
fn ctx_1_escape_closes_the_menu() {
    let mut ui = Ui::open(DOC);
    ui.right_click_text("friction");
    ui.keys("escape");
    assert!(!ui.menu_open());
    assert_eq!(ui.text(), DOC);
}

#[test]
fn ctx_2_variant_menu() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &["tension", "struggle"]);
    ui.caret_at(ui.visible_group(g).range.start);
    ui.keys("cmd-shift-a");

    ui.right_click_variant(0);
    assert_eq!(ui.menu_labels(), ["Edit", "Copy text", "Remove", "Add a version without it"]);
    ui.menu("Copy text");
    assert_eq!(ui.clipboard().as_deref(), Some("friction"));

    ui.right_click_variant(2);
    ui.menu("Remove");
    assert_eq!(ui.variants(g), ["friction", "tension"]);
}

#[test]
fn kb_1_shortcut_labels_include_context_bindings() {
    let mut ui = Ui::open(DOC);
    ui.keys("cmd-shift-k");
    let got: Vec<Option<String>> = ui.read(|_, cx| {
        use crate::actions::*;
        let actions: [Box<dyn gpui_kit::Action>; 5] =
            [Box::new(GhostToggle), Box::new(StashToOverflow), Box::new(CycleAtCaretNext), Box::new(Undo), Box::new(InsertOverflowNote)];
        actions.iter().map(|a| crate::context_menu::shortcut_label(a.as_ref(), cx)).collect()
    });
    assert_eq!(got, [Some("⌘/"), Some("⇧⌘X"), Some("⌥⌘↓"), Some("⌘Z"), Some("⌘↩")].map(|s| s.map(String::from)));
}

#[test]
fn ctx_open_menu_blocks_hover_beneath() {
    let mut ui = Ui::open(DOC);
    let g = ui.make_group("friction", &["tension"]);
    ui.keys("escape");
    ui.right_click_text("early");
    assert!(ui.menu_open());
    ui.hover_group(g);
    assert_eq!(ui.hovered_group(), None, "the page under an open menu stays still");
    ui.keys("escape");
    assert!(!ui.menu_open());
    ui.hover_group(g);
    assert_eq!(ui.hovered_group(), Some(g));
}
