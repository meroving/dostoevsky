use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::theme::{self, c};

pub fn scope_switch(id: &'static str, labels: [SharedString; 2], active: usize, on_change: impl Fn(&usize, &mut Window, &mut App) + 'static) -> AnyElement {
    let on_change = Rc::new(on_change);
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(10.))
        .text_size(px(11.))
        .line_height(px(16.))
        .children(labels.into_iter().enumerate().map(|(i, label)| {
            let on = i == active;
            let f = on_change.clone();
            div()
                .id((id, i))
                .role(Role::Tab)
                .aria_label(label.clone())
                .aria_selected(on)
                .cursor_pointer()
                .text_color(if on { c(theme::accent()) } else { c(theme::muted()) })
                .when(on, |d| d.border_b_1().border_color(c(theme::accent())))
                .when(!on, |d| d.hover(|s| s.text_color(c(theme::text()))))
                .on_click(move |_, w, cx| f(&i, w, cx))
                .child(label)
                .test_support()
        }))
        .test_support()
        .into_any_element()
}

pub fn section_header(id: impl Into<ElementId>, title: String, count: usize) -> AnyElement {
    div()
        .id(id)
        .flex()
        .justify_between()
        .mt(px(6.))
        .px(px(8.))
        .text_size(px(9.5))
        .line_height(px(15.))
        .text_color(c(theme::faint()))
        .child(div().min_w_0().truncate().child(title))
        .child(div().flex_none().child(count.to_string()))
        .test_support()
        .into_any_element()
}

pub fn fold_row(id: impl Into<ElementId>, label: String, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> AnyElement {
    div()
        .id(id)
        .px(px(8.))
        .text_size(px(11.))
        .line_height(px(16.))
        .cursor_pointer()
        .text_color(c(theme::muted()))
        .hover(|s| s.text_color(c(theme::text())))
        .on_click(on_click)
        .child(format!("› {label}"))
        .test_support()
        .into_any_element()
}
