use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::*;
use l10n::{tr, trn};
use notify::Watcher as _;

use crate::files;
use crate::format;
use crate::model::{DocModel, ModelEvent};
use crate::workspace::Workspace;

/// An FSEvents watcher, stopped off the UI thread when dropped: stopping joins its run
/// loop's thread and waits on fseventsd, like starting it.
pub(crate) struct Watch(Option<notify::RecommendedWatcher>);

impl Drop for Watch {
    fn drop(&mut self) {
        if let Some(w) = self.0.take() {
            std::thread::spawn(move || drop(w));
        }
    }
}

/// Starts watching `dir` on a thread of its own. `FSEventStreamStart` waits for fseventsd's
/// reply, and a slow fseventsd (tens of seconds, seen 2026-11-14) must only delay noticing
/// outside changes, never freeze a window that is opening. A real thread, not the background
/// executor, which runs on the test thread in app tests. `None` if it can't watch.
pub(crate) fn start_watcher(
    dir: PathBuf,
    mode: notify::RecursiveMode,
    mut on_event: impl FnMut(notify::Event) + Send + 'static,
) -> impl std::future::Future<Output = Option<Watch>> {
    let started = crate::assist::off_thread(move || {
        let mut w = notify::recommended_watcher(move |e: notify::Result<notify::Event>| {
            if let Ok(e) = e {
                on_event(e);
            }
        })
        .ok()?;
        w.watch(&dir, mode).ok()?;
        Some(Watch(Some(w)))
    });
    async move { started.await.flatten() }
}

impl Workspace {
    pub fn watch_file(&mut self, cx: &mut Context<Self>) {
        let path = self.model.read(cx).path.clone();
        if path == self.watched {
            return;
        }
        self.watched = path.clone();
        self.watcher = None;
        let Some(path) = path else { return };
        let Some(dir) = path.parent().map(PathBuf::from) else { return };
        let (tx, rx) = smol::channel::unbounded::<()>();
        let name = path.file_name().map(|n| n.to_os_string());
        let start = start_watcher(dir, notify::RecursiveMode::NonRecursive, move |e| {
            if e.paths.iter().any(|p| p.file_name().map(|n| n.to_os_string()) == name) {
                let _ = tx.try_send(());
            }
        });
        let task = cx.spawn(async move |this, cx| {
            let Some(watch) = start.await else { return };
            let current = this.update(cx, |this, cx| {
                // Another file was opened while this one's watcher started.
                if this.watched.as_ref() != Some(&path) {
                    return false;
                }
                this.watcher = Some(watch);
                // A change made while fseventsd was slow to answer.
                this.disk_changed(cx);
                true
            });
            if !matches!(current, Ok(true)) {
                return;
            }
            while rx.recv().await.is_ok() {
                // Editors and sync tools write in bursts; wait for the last one.
                cx.background_executor().timer(Duration::from_millis(300)).await;
                while rx.try_recv().is_ok() {}
                if this.update(cx, |this, cx| this.disk_changed(cx)).is_err() {
                    break;
                }
            }
        });
        task.detach();
    }

    pub(crate) fn disk_changed(&mut self, cx: &mut Context<Self>) {
        let m = self.model.read(cx);
        let Some(path) = m.path.clone() else { return };
        // Our own save still on its way isn't a change from outside.
        files::wait_for_writes(Some(&path));
        let Ok(text) = std::fs::read_to_string(&path) else {
            if !path.exists() {
                self.flash_status(tr!("The file was moved or deleted. Save to write it again."), cx);
                self.model.update(cx, |m, _| m.disk_text = None);
            }
            return;
        };
        if m.disk_text.as_deref() == Some(text.as_str()) {
            return;
        }
        if !m.dirty() {
            self.reload_from(text, cx);
            self.flash_status(tr!("Reloaded: the file changed outside Dostoevsky"), cx);
            return;
        }
        self.pending_disk = Some(text);
        cx.notify();
    }

    /// Backs up what we had first, so loading theirs never loses ours.
    pub fn reload_from(&mut self, text: String, cx: &mut Context<Self>) {
        let m = self.model.read(cx);
        if let Some(path) = &m.path {
            if m.dirty() {
                let _ = files::backup(path, format::serialize(m).as_bytes());
            }
        }
        let loaded = self.load_page_text(&text, cx);
        let m = self.model.read(cx);
        let before = m.kept.detached.len();
        self.model.update(cx, |m, cx| {
            m.doc = loaded.doc;
            m.layer = loaded.layer;
            m.present();
            m.kept = loaded.kept;
            m.disk_text = Some(text);
            m.version += 1;
            m.saved_version = m.version;
            cx.emit(ModelEvent::Changed);
            cx.notify();
        });
        let after = self.model.read(cx).kept.detached.len();
        if after > before {
            self.flash_status(trn!("{n} alternative group lost its place", "{n} alternative groups lost their place", after - before), cx);
        }
    }

    /// False while the file holds words from outside that the writer hasn't seen: the change on
    /// disk is asked about (now, if the watcher hasn't said so yet) and nothing is written.
    /// Autosave would otherwise put the page over them unseen.
    pub(crate) fn may_write_over_file(&mut self, cx: &mut Context<Self>) -> bool {
        if self.disk_prompt {
            return false;
        }
        let m = self.model.read(cx);
        let (Some(path), Some(ours)) = (&m.path, &m.disk_text) else { return true };
        // Our own save still on its way isn't a change from outside.
        files::wait_for_writes(Some(path));
        let outside = std::fs::read_to_string(path).is_ok_and(|disk| disk != *ours);
        if outside {
            self.disk_changed(cx);
        }
        !outside
    }

    pub fn resolve_disk_change(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Another change that came in while the question is open is asked about next.
        if self.disk_prompt {
            return;
        }
        let Some(text) = self.pending_disk.take() else { return };
        self.disk_prompt = true;
        let title = self.model.read(cx).title();
        let rx = window.prompt(
            PromptLevel::Warning,
            &tr!("“{title}” changed on disk", title = title),
            Some(tr!("You have unsaved changes too. Keep yours (the next save overwrites the file), load the version on disk (yours goes to Backups), or open it next to yours to compare.")),
            &[tr!("Keep Mine"), tr!("Load Theirs"), tr!("Compare")],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.disk_prompt = false;
                cx.notify();
                match answer {
                    Err(_) => {}
                    Ok(0) => this.model.update(cx, |m, _| m.disk_text = Some(text)),
                    Ok(1) => this.reload_from(text, cx),
                    Ok(_) => {
                        let loaded = format::load(&text);
                        let mut theirs = DocModel::new(loaded.doc);
                        theirs.layer = loaded.layer;
                        theirs.present();
                        theirs.kept = loaded.kept;
                        theirs.suggested = this.model.read(cx).path.clone();
                        theirs.warnings.push(tr!("The version on disk, opened to compare. Saving asks for a new name.").into());
                        this.model.update(cx, |m, _| m.disk_text = Some(text));
                        crate::open_model(theirs, cx);
                    }
                }
            });
        })
        .detach();
    }
}
