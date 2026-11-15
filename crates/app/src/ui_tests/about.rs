use std::time::Duration;

use gpui_kit::test::TestWindowExt;
use gpui_kit::{point, px, Bounds, ElementId, MouseButton, TestApp, TestAppWindow, WindowBounds, WindowOptions};

use crate::about::{self, About, Page};
use crate::ui_tests::Ui;

struct AboutUi {
    app: TestApp,
    win: TestAppWindow<About>,
}

impl AboutUi {
    fn open() -> AboutUi {
        let mut app = Ui::app();
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::new(point(px(0.), px(0.)), gpui_kit::size(px(about::SIZE.0), px(about::SIZE.1))))),
            ..Default::default()
        };
        let win = app.open_window_with_options(options, About::new);
        let mut ui = AboutUi { app, win };
        ui.draw();
        ui
    }

    fn draw(&mut self) {
        self.win.draw();
        self.app.run_until_parked();
    }

    fn has(&mut self, id: &'static str) -> bool {
        let id = ElementId::from(id);
        self.win.update(|_, window, _| window.try_find(id).is_some())
    }

    fn bounds(&mut self, id: &'static str) -> Bounds<gpui_kit::Pixels> {
        let id = ElementId::from(id);
        self.win.update(|_, window, _| window.try_find(id.clone()).unwrap_or_else(|| panic!("no {id:?}")).bounds())
    }

    fn click(&mut self, id: &'static str) {
        let at = self.bounds(id).center();
        self.win.simulate_click(at, MouseButton::Left);
        self.draw();
    }

    fn page(&self) -> Page {
        self.win.read(|a, _| a.page)
    }
}

#[test]
fn about_shows_version_links_and_acknowledgements_inside_the_window() {
    let mut ui = AboutUi::open();
    let window = Bounds::new(point(px(0.), px(0.)), gpui_kit::size(px(about::SIZE.0), px(about::SIZE.1)));
    for id in ["about-version", "about-site", "about-github", "about-support", "about-privacy", "about-thanks", "about-foot"] {
        let b = ui.bounds(id);
        assert!(b.origin.x >= px(0.) && b.origin.y >= px(28.) && b.right() <= window.right() && b.bottom() <= window.bottom(), "{id} at {b:?}");
    }
    let (site, privacy) = (ui.bounds("about-site"), ui.bounds("about-privacy"));
    assert!((site.origin.y - privacy.origin.y).abs() < px(1.), "one line of links: {site:?} {privacy:?}");
}

#[test]
fn about_version_click_copies_it() {
    let mut ui = AboutUi::open();
    ui.click("about-version");
    let copied = ui.app.read_from_clipboard().and_then(|c| c.text()).unwrap_or_default();
    assert_eq!(copied, about::version_details());
    assert!(copied.starts_with("Dostoevsky "), "{copied}");
    assert!(ui.win.read(|a, _| a.copied));
    ui.app.advance_clock(Duration::from_millis(1600));
    ui.draw();
    assert!(!ui.win.read(|a, _| a.copied));
}

#[test]
fn about_links_open_the_browser() {
    let mut ui = AboutUi::open();
    for (id, url) in [("about-site", about::SITE), ("about-github", about::GITHUB), ("about-support", about::SUPPORT), ("about-privacy", about::PRIVACY)] {
        ui.click(id);
        assert_eq!(ui.app.opened_url().as_deref(), Some(url), "{id}");
    }
}

#[test]
fn about_acknowledgements_and_back() {
    let mut ui = AboutUi::open();
    ui.click("about-thanks");
    assert_eq!(ui.page(), Page::Acknowledgements);
    assert!(ui.has("about-back"));
    // Room for the bundle's "Full licence texts…" button (30 px) and its margin.
    let rest = ui.bounds("about-rest");
    assert!(rest.bottom() + px(14. + 30. + 8.) <= px(about::SIZE.1), "{rest:?}");
    // A cargo run has no THIRD-PARTY-LICENSES.txt.
    assert!(!ui.has("about-licences"));
    ui.click("about-back");
    assert_eq!(ui.page(), Page::About);

    ui.click("about-thanks");
    ui.win.simulate_keystroke("escape");
    ui.draw();
    assert_eq!(ui.page(), Page::About);
    let before = ui.app.windows().len();
    ui.win.simulate_keystroke("escape");
    ui.app.run_until_parked();
    assert_eq!(ui.app.windows().len(), before - 1, "Esc on the first page closes About");
}

#[test]
fn about_closes_with_cmd_w() {
    let mut ui = AboutUi::open();
    let before = ui.app.windows().len();
    ui.win.simulate_keystroke("cmd-w");
    ui.app.run_until_parked();
    assert_eq!(ui.app.windows().len(), before - 1);
}

#[test]
fn about_menu_item_opens_one_window() {
    let mut app = Ui::app();
    app.update(|cx| cx.dispatch_action(&crate::actions::About));
    app.run_until_parked();
    assert_eq!(app.windows().len(), 1);
    app.update(|cx| cx.dispatch_action(&crate::actions::About));
    app.run_until_parked();
    assert_eq!(app.windows().len(), 1);
}
