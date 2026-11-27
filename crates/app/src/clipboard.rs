use doc_model::{CutId, Fragment};
use gpui_kit::*;

use crate::model::DocModel;
use crate::view::{Mode, TextView};

/// `change` is the pasteboard's change count then: ⌘V moves the cut only while it is unchanged.
#[derive(Clone)]
pub struct CutOnClipboard {
    /// A project swaps documents through one page model, so the model alone doesn't say which.
    pub uid: u64,
    pub id: CutId,
    change: Option<isize>,
    text: String,
}

#[derive(Default)]
struct Internal(Option<(String, Fragment)>, Option<CutOnClipboard>);

impl Global for Internal {}

/// Goes up whenever anything puts something on the pasteboard; `None` with no pasteboard (tests).
fn change_count() -> Option<isize> {
    #[cfg(all(target_os = "macos", not(test)))]
    {
        let pb = objc2_app_kit::NSPasteboard::generalPasteboard();
        Some(pb.changeCount())
    }
    #[cfg(any(not(target_os = "macos"), test))]
    {
        None
    }
}

pub fn write(view: &mut TextView, text: String, cx: &mut Context<TextView>) {
    let fragment = (view.mode == Mode::Page)
        .then(|| view.model.read(cx).doc.copy(view.selection()).ok())
        .flatten();
    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
    cx.set_global(Internal(fragment.map(|f| (text, f)), None));
}

pub fn write_cut(model: &Entity<DocModel>, id: CutId, cx: &mut App) {
    let Ok(fragment) = model.read(cx).doc.cut_fragment(id) else { return };
    let text = fragment.plain_text();
    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
    let cut = CutOnClipboard { uid: model.read(cx).uid, id, change: change_count(), text: text.clone() };
    cx.set_global(Internal(Some((text, fragment)), Some(cut)));
}

/// Doesn't check that the cut still exists: the window does.
pub fn cut_on_clipboard(cx: &App) -> Option<CutOnClipboard> {
    let c = cx.try_global::<Internal>()?.1.clone()?;
    let same = match (c.change, change_count()) {
        (Some(a), Some(b)) => a == b,
        _ => cx.read_from_clipboard().and_then(|i| i.text()).is_some_and(|t| t.replace("\r\n", "\n") == c.text),
    };
    same.then_some(c)
}

impl CutOnClipboard {
    pub fn is_in(&self, m: &DocModel) -> bool {
        m.uid == self.uid && m.doc.cut_fragment(self.id).is_ok_and(|f| f.plain_text() == self.text)
    }
}

pub fn forget_cut(cx: &mut App) {
    if let Some(i) = cx.try_global::<Internal>() {
        let keep = i.0.clone();
        cx.set_global(Internal(keep, None));
    }
}

pub fn paste(view: &mut TextView, cx: &mut Context<TextView>) {
    if view.mode == Mode::Page && cut_on_clipboard(cx).is_some() {
        cx.emit(crate::view::ViewEvent::PasteCut);
        return;
    }
    let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) else {
        return;
    };
    let text = text.replace("\r\n", "\n");
    let fragment = cx
        .try_global::<Internal>()
        .and_then(|i| i.0.as_ref())
        .filter(|(t, _)| *t == text)
        .map(|(_, f)| f.clone());
    if let Some(f) = fragment {
        if view.paste_fragment(&f, cx) {
            return;
        }
    }
    view.insert_text(&text, cx);
}

#[cfg(test)]
thread_local! {
    pub static RICH_HTML: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

pub fn write_rich(markdown: &str, html: &str, cx: &mut App) {
    #[cfg(all(target_os = "macos", not(test)))]
    {
        let _ = cx;
        rich_mac(markdown, html);
    }
    #[cfg(all(not(target_os = "macos"), not(test)))]
    if let Err(e) = rich_elsewhere(markdown, html) {
        // The text at least, as GPUI writes it.
        eprintln!("dostoevsky: couldn't put formatted text on the clipboard: {e}");
        cx.write_to_clipboard(ClipboardItem::new_string(markdown.to_string()));
    }
    #[cfg(test)]
    {
        cx.write_to_clipboard(ClipboardItem::new_string(markdown.to_string()));
        RICH_HTML.with(|r| *r.borrow_mut() = Some(html.to_string()));
    }
}

/// Off the Mac GPUI writes plain text only: arboard puts the HTML beside the Markdown, as
/// Windows' HTML Format (what Word and WordPad paste formatted) or X11's text/html, and the
/// Markdown as the text. One clipboard for the app's life: on X11 the app serves what it copied
/// for as long as it runs.
#[cfg(all(not(target_os = "macos"), not(test)))]
fn rich_elsewhere(markdown: &str, html: &str) -> Result<(), arboard::Error> {
    thread_local! {
        static CLIPBOARD: std::cell::RefCell<Option<arboard::Clipboard>> = const { std::cell::RefCell::new(None) };
    }
    CLIPBOARD.with(|c| {
        let mut c = c.borrow_mut();
        if c.is_none() {
            *c = Some(arboard::Clipboard::new()?);
        }
        let clipboard = c.as_mut().expect("made above");
        clipboard.set_html(html, Some(markdown))
    })
}

#[cfg(all(target_os = "macos", not(test)))]
fn rich_mac(markdown: &str, html: &str) {
    use objc2::AnyThread;
    use objc2_app_kit::{NSAttributedStringAppKitDocumentFormats, NSPasteboard, NSPasteboardTypeHTML, NSPasteboardTypeRTF, NSPasteboardTypeString};
    use objc2_foundation::{NSAttributedString, NSData, NSDictionary, NSRange, NSString};

    // A full document, so the HTML reader takes it as UTF-8.
    let doc = format!("<!DOCTYPE html><html><head><meta charset=\"utf-8\"></head><body>{html}</body></html>");
    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    // SAFETY: static pasteboard types; HTML read on the main thread (GPUI calls in on it).
    unsafe {
        pb.setString_forType(&NSString::from_str(html), NSPasteboardTypeHTML);
        let data = NSData::with_bytes(doc.as_bytes());
        if let Some(a) = NSAttributedString::initWithHTML_documentAttributes(NSAttributedString::alloc(), &data, None) {
            let attrs = NSDictionary::new();
            if let Some(rtf) = a.RTFFromRange_documentAttributes(NSRange::new(0, a.length()), &attrs) {
                pb.setData_forType(Some(&rtf), NSPasteboardTypeRTF);
            }
        }
        pb.setString_forType(&NSString::from_str(markdown), NSPasteboardTypeString);
    }
}
