use std::collections::BTreeMap;

use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};

const MAX_PER_WORD: usize = 12;
const MAX_STEERS: usize = 20;

#[derive(Default)]
pub struct Memory {
    pairs: BTreeMap<String, Vec<String>>,
    steers: Vec<Steer>,
}

/// `last` is a running count, not a clock.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Steer {
    text: String,
    uses: u32,
    last: u64,
}

impl Global for Memory {}

fn file() -> Option<std::path::PathBuf> {
    crate::files::support_dir().map(|d| d.join("pairings.json"))
}

fn steers_file() -> Option<std::path::PathBuf> {
    crate::files::support_dir().map(|d| d.join("steers.json"))
}

fn key(original: &str) -> String {
    original.trim().to_lowercase()
}

impl Memory {
    pub fn load(cx: &mut App) {
        let pairs = file()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let steers = steers_file()
            .and_then(|f| std::fs::read(f).ok())
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        cx.set_global(Memory { pairs, steers });
    }

    fn save_steers(&self) {
        if let Some(f) = steers_file() {
            if let Some(d) = f.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            if let Ok(s) = serde_json::to_string_pretty(&self.steers) {
                let _ = std::fs::write(f, s);
            }
        }
    }

    pub fn record_steer(cx: &mut App, steer: &str) {
        if !crate::prefs::Prefs::get(cx).remember_pairings {
            return;
        }
        let text = steer.trim().to_string();
        if text.is_empty() {
            return;
        }
        let m = cx.default_global::<Memory>();
        let now = m.steers.iter().map(|s| s.last).max().unwrap_or(0) + 1;
        match m.steers.iter_mut().find(|s| key(&s.text) == key(&text)) {
            Some(s) => {
                s.uses += 1;
                s.last = now;
                s.text = text;
            }
            None => m.steers.push(Steer { text, uses: 1, last: now }),
        }
        m.steers.sort_by(|a, b| b.uses.cmp(&a.uses).then(b.last.cmp(&a.last)));
        m.steers.truncate(MAX_STEERS);
        m.save_steers();
    }

    pub fn steers(cx: &App) -> Vec<String> {
        if !crate::prefs::Prefs::get(cx).remember_pairings {
            return Vec::new();
        }
        cx.try_global::<Memory>().map(|m| m.steers.iter().map(|s| s.text.clone()).collect()).unwrap_or_default()
    }

    pub fn steer_count(cx: &App) -> usize {
        cx.try_global::<Memory>().map_or(0, |m| m.steers.len())
    }

    fn save(&self) {
        if let Some(f) = file() {
            if let Some(d) = f.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            if let Ok(s) = serde_json::to_string_pretty(&self.pairs) {
                let _ = std::fs::write(f, s);
            }
        }
    }

    pub fn record(cx: &mut App, original: &str, variant: &str) {
        if !crate::prefs::Prefs::get(cx).remember_pairings {
            return;
        }
        let (k, v) = (key(original), variant.trim().to_string());
        if k.is_empty() || v.is_empty() || key(&v) == k || k.split_whitespace().count() > 4 {
            return;
        }
        let m = cx.default_global::<Memory>();
        let list = m.pairs.entry(k).or_default();
        list.retain(|x| key(x) != key(&v));
        list.insert(0, v);
        list.truncate(MAX_PER_WORD);
        m.save();
    }

    pub fn suggestions(cx: &App, original: &str, existing: &[String]) -> Vec<String> {
        if !crate::prefs::Prefs::get(cx).remember_pairings {
            return Vec::new();
        }
        cx.try_global::<Memory>()
            .and_then(|m| m.pairs.get(&key(original)))
            .map(|v| v.iter().filter(|s| !existing.iter().any(|e| key(e) == key(s))).take(6).cloned().collect())
            .unwrap_or_default()
    }

    pub fn count(cx: &App) -> usize {
        cx.try_global::<Memory>().map_or(0, |m| m.pairs.len())
    }

    pub fn clear(cx: &mut App) {
        let m = cx.default_global::<Memory>();
        m.pairs.clear();
        m.save();
        m.steers.clear();
        m.save_steers();
    }
}
