//! Panel tabs dragged between the two sides and along a header (crates/app/src/panels.rs).

use gpui_kit::{point, px, Bounds, MouseButton, Pixels, Point};

use super::Ui;
use crate::actions::{ToggleLayer, ToggleOverflow};
use crate::panels::{PanelTab, Side, TabLayout};
use crate::prefs::Prefs;

const DOC: &str = "Most of the friction in writing comes from choosing too early.\n\nYou pick a word, and the sentence moves on.\n";

fn layout(ui: &Ui) -> TabLayout {
    ui.read(|_, cx| Prefs::get(cx).panel_tabs.clone())
}

fn shown(ui: &Ui, side: Side) -> Option<PanelTab> {
    ui.read(|ws, cx| ws.shown_tab(side, cx))
}

fn bounds(ui: &mut Ui, tab: PanelTab) -> Bounds<Pixels> {
    ui.element(tab.id()).bounds()
}

/// Just inside a label's left edge: a drop here lands before it.
fn before(b: Bounds<Pixels>) -> Point<Pixels> {
    point(b.left() + px(3.), b.center().y)
}

#[test]
fn a_tab_dragged_to_the_other_header_lands_at_its_slot_and_shows_there() {
    let mut ui = Ui::open_editing(DOC);
    ui.action(ToggleOverflow);
    ui.keys("cmd-shift-a");
    assert_eq!(shown(&ui, Side::Left), Some(PanelTab::Alternatives));
    let from = bounds(&mut ui, PanelTab::Alternatives).center();
    let to = before(bounds(&mut ui, PanelTab::Comments));
    ui.drag_path(from, &[to], false, |ui| {
        assert!(ui.has_element(("tab-drop-header", 0usize)) && ui.has_element(("tab-drop-header", 1usize)), "both headers take it");
        assert!(ui.has_element("tab-drop-bar"), "where it would land");
    });
    let l = layout(&ui);
    assert_eq!(l.left, vec![PanelTab::Project, PanelTab::Review]);
    assert_eq!(l.right, vec![PanelTab::Cuts, PanelTab::Alternatives, PanelTab::Comments]);
    assert_eq!(shown(&ui, Side::Right), Some(PanelTab::Alternatives), "it shows on its new side");
    let drawn = ui.read(|ws, _| ws.tab_geometry.borrow().label(PanelTab::Alternatives)).expect("drawn in a header");
    assert_eq!(drawn, bounds(&mut ui, PanelTab::Alternatives), "the header keeps where each label is");
    assert!(ui.sidebar_open());
    assert!(ui.element("sidebar").bounds().left() > px(600.), "the Alternatives panel is on the right");
    assert!(!ui.has_element("tab-drop-bar"), "the targets go with the drag");
    let saved: Prefs = serde_json::from_slice(&std::fs::read(Prefs::path().unwrap()).unwrap()).unwrap();
    assert_eq!(saved.panel_tabs, l, "the layout is saved for every window");
}

#[test]
fn every_place_lights_up_as_soon_as_a_tab_lifts() {
    let mut ui = Ui::open_editing(DOC);
    ui.keys("cmd-shift-a");
    assert_eq!(shown(&ui, Side::Right), None);
    let from = bounds(&mut ui, PanelTab::Alternatives).center();
    // A few pixels: picked up, over its own header still.
    ui.drag_path(from, &[point(from.x + px(6.), from.y + px(2.))], false, |ui| {
        assert!(ui.has_element(("tab-drop-header", 0usize)), "its own header, to reorder");
        assert!(ui.has_element(("tab-drop-side", 1usize)), "the closed side, before the pointer gets there");
    });
    assert_eq!(layout(&ui), TabLayout::default(), "dropped where it was");
}

#[test]
fn a_tab_reorders_along_its_own_header() {
    let mut ui = Ui::open_editing(DOC);
    ui.keys("cmd-shift-a");
    let from = bounds(&mut ui, PanelTab::Review).center();
    let to = before(bounds(&mut ui, PanelTab::Alternatives));
    ui.drag_path(from, &[to], false, |_| {});
    assert_eq!(layout(&ui).left, vec![PanelTab::Project, PanelTab::Review, PanelTab::Alternatives]);
    assert_eq!(shown(&ui, Side::Left), Some(PanelTab::Review), "the tab picked up is the one showing");
    assert!(bounds(&mut ui, PanelTab::Review).left() < bounds(&mut ui, PanelTab::Alternatives).left());
}

#[test]
fn a_tab_dropped_over_a_closed_side_opens_it() {
    let mut ui = Ui::open_editing(DOC);
    ui.keys("cmd-shift-a");
    let from = bounds(&mut ui, PanelTab::Alternatives).center();
    ui.drag_path(from, &[point(px(1100.), px(400.))], false, |ui| {
        assert!(ui.has_element(("tab-drop-side", 1usize)));
    });
    assert_eq!(layout(&ui).right, vec![PanelTab::Cuts, PanelTab::Comments, PanelTab::Alternatives], "last on that side");
    assert_eq!(shown(&ui, Side::Right), Some(PanelTab::Alternatives));
}

#[test]
fn a_tab_dropped_on_the_page_goes_back() {
    let mut ui = Ui::open_editing(DOC);
    ui.action(ToggleOverflow);
    ui.keys("cmd-shift-a");
    let from = bounds(&mut ui, PanelTab::Alternatives).center();
    ui.drag_path(from, &[point(px(600.), px(400.))], false, |ui| {
        assert!(!ui.has_element("tab-drop-bar"), "no slot over the page");
    });
    assert_eq!(layout(&ui), TabLayout::default());
    assert_eq!(shown(&ui, Side::Left), Some(PanelTab::Alternatives));
}

#[test]
fn the_last_tab_out_closes_its_side_and_its_button_greys() {
    let mut ui = Ui::open_editing(DOC);
    for tab in [PanelTab::Cuts, PanelTab::Comments] {
        ui.win.update(|ws, w, cx| ws.move_tab(tab, Side::Left, None, w, cx));
        ui.draw();
    }
    assert!(layout(&ui).right.is_empty());
    assert_eq!(shown(&ui, Side::Right), None);
    assert_eq!(shown(&ui, Side::Left), Some(PanelTab::Comments), "the last one moved shows");
    ui.click_id("overflow-button");
    assert_eq!(shown(&ui, Side::Right), None, "an empty side doesn't open");
    assert_eq!(ui.element("overflow-button").label(), Some("Drag a tab here"));
    ui.action(ToggleOverflow);
    assert_eq!(shown(&ui, Side::Left), None, "⌃⌘C closes the Cuts' side, now the left");
    ui.action(ToggleOverflow);
    assert_eq!(shown(&ui, Side::Left), Some(PanelTab::Comments), "and opens it on the tab it showed");
}

#[test]
fn cuts_and_comments_on_two_sides_show_at_once() {
    let mut ui = Ui::open_editing(DOC);
    ui.win.update(|ws, w, cx| ws.move_tab(PanelTab::Comments, Side::Left, None, w, cx));
    ui.draw();
    ui.action(ToggleOverflow);
    assert_eq!(shown(&ui, Side::Left), Some(PanelTab::Comments));
    assert_eq!(shown(&ui, Side::Right), Some(PanelTab::Cuts));
    assert!(ui.has_element("comments-panel") && ui.has_element("overflow"));
    assert!(ui.element("comments-panel").bounds().right() < ui.element("overflow").bounds().left());
}

#[test]
fn the_panel_foot_goes_with_the_project_tab() {
    let mut ui = Ui::open(DOC);
    ui.keys("cmd-p");
    assert!(ui.has_element("file-panel"));
    assert!(ui.element("panel-foot").bounds().left() < px(100.));
    let from = bounds(&mut ui, PanelTab::Project).center();
    ui.drag_path(from, &[point(px(1100.), px(400.))], false, |_| {});
    assert_eq!(shown(&ui, Side::Right), Some(PanelTab::Project));
    assert!(ui.element("panel-foot").bounds().left() > px(800.), "the foot is on the right now");
    ui.action(ToggleOverflow);
    assert_eq!(shown(&ui, Side::Right), Some(PanelTab::Cuts), "Cuts over the Project tab, on the same side");
    assert!(ui.has_element("panel-foot"), "a panel on the Project's side has the foot");
    ui.action(ToggleOverflow);
    assert!(!ui.has_element("panel-foot"));
    ui.keys("cmd-p");
    assert_eq!(shown(&ui, Side::Right), Some(PanelTab::Cuts), "⌘P opens the Project tab's side, on the tab it showed");
}

#[test]
fn the_tab_menu_moves_a_tab_and_resets_the_layout() {
    let mut ui = Ui::open_editing(DOC);
    ui.keys("cmd-shift-a");
    let at = bounds(&mut ui, PanelTab::Alternatives).center();
    ui.click(at, MouseButton::Right);
    assert_eq!(ui.menu_labels(), vec!["Move to Right Panel", "Reset Panel Layout"]);
    ui.win.update(|ws, w, cx| ws.run_menu_label("Move to Right Panel", w, cx));
    ui.draw();
    assert_eq!(layout(&ui).right, vec![PanelTab::Cuts, PanelTab::Comments, PanelTab::Alternatives]);
    assert_eq!(shown(&ui, Side::Right), Some(PanelTab::Alternatives));
    let at = bounds(&mut ui, PanelTab::Alternatives).center();
    ui.click(at, MouseButton::Right);
    assert_eq!(ui.menu_labels()[0], "Move to Left Panel");
    ui.win.update(|ws, w, cx| ws.run_menu_label("Reset Panel Layout", w, cx));
    ui.draw();
    assert_eq!(layout(&ui), TabLayout::default());
    assert_eq!(shown(&ui, Side::Right), None, "nothing lives on the right but Cuts and Comments");
    let open: Vec<PanelTab> = PanelTab::ALL.into_iter().filter(|t| ui.read(|ws, cx| ws.tab_open(*t, cx))).collect();
    assert_eq!(open.len(), 1, "one tab a side: {open:?}");
}

#[test]
fn review_keeps_its_slot_and_leaves_writings_header() {
    let mut ui = Ui::open_editing(DOC);
    ui.win.update(|ws, w, cx| ws.move_tab(PanelTab::Review, Side::Right, Some(PanelTab::Cuts), w, cx));
    ui.draw();
    assert_eq!(shown(&ui, Side::Right), Some(PanelTab::Review));
    assert!(ui.has_element("switch-review"));
    ui.action(ToggleLayer);
    assert!(!ui.has_element("switch-review"), "writing has no Review");
    ui.action(ToggleOverflow);
    assert!(ui.has_element("tab-cuts") && !ui.has_element("switch-review"));
    assert_eq!(layout(&ui).right, vec![PanelTab::Review, PanelTab::Cuts, PanelTab::Comments], "it keeps its slot");
}
