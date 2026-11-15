use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::*;
use l10n::tr;
use serde_json::Value;

pub const TEMPLATE: &str = r#"// Dostoevsky key bindings. Saved changes apply immediately.
// Each section has an optional "context" (Page, TextView, Note, Field, Workspace, Sheet)
// and "bindings": keystrokes → action name. Use null to remove a default binding.
// Action names: see the shortcuts sheet (⌘⇧K) or the Help menu.
[
  {
    "context": "Page",
    "bindings": {
      // "cmd-k": "page::GhostToggle"
    }
  }
]
"#;

pub fn path() -> Option<PathBuf> {
    // Tests run on the default bindings only.
    if cfg!(test) {
        return None;
    }
    if let Ok(p) = std::env::var("DOSTOEVSKY_KEYMAP") {
        return Some(PathBuf::from(p));
    }
    directories::BaseDirs::new().map(|b| b.home_dir().join(".config").join("dostoevsky").join("keymap.json"))
}

fn strip_jsonc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_str = false;
    let mut esc = false;
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if in_str {
            out.push(ch);
            if esc {
                esc = false;
            } else if ch == '\\' {
                esc = true;
            } else if ch == '"' {
                in_str = false;
            }
            continue;
        }
        match ch {
            '"' => {
                in_str = true;
                out.push(ch);
            }
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            _ => out.push(ch),
        }
    }
    let mut cleaned = String::with_capacity(out.len());
    let b: Vec<char> = out.chars().collect();
    for (i, ch) in b.iter().enumerate() {
        if *ch == ',' {
            let next = b[i + 1..].iter().find(|c| !c.is_whitespace());
            if matches!(next, Some('}') | Some(']')) {
                continue;
            }
        }
        cleaned.push(*ch);
    }
    cleaned
}

pub struct Parsed {
    pub bindings: Vec<KeyBinding>,
    /// (keystrokes, context) pairs whose default binding is removed.
    pub removed: Vec<(String, Option<String>)>,
    pub errors: Vec<String>,
}

pub fn parse(text: &str, cx: &App) -> Parsed {
    let mut p = Parsed { bindings: Vec::new(), removed: Vec::new(), errors: Vec::new() };
    let json: Value = match serde_json::from_str(&strip_jsonc(text)) {
        Ok(v) => v,
        Err(e) => {
            p.errors.push(format!("keymap.json: {e}"));
            return p;
        }
    };
    let Some(sections) = json.as_array() else {
        p.errors.push(tr!("keymap.json: expected a list of sections").into());
        return p;
    };
    for sec in sections {
        let context = sec.get("context").and_then(Value::as_str).map(String::from);
        let predicate = match &context {
            Some(c) => match KeyBindingContextPredicate::parse(c) {
                Ok(pr) => Some(Rc::new(pr)),
                Err(e) => {
                    p.errors.push(tr!("context “{context}”: {error}", context = c, error = e));
                    continue;
                }
            },
            None => None,
        };
        let Some(bindings) = sec.get("bindings").and_then(Value::as_object) else { continue };
        for (keys, action) in bindings {
            let (name, args) = match action {
                Value::Null => {
                    p.removed.push((keys.clone(), context.clone()));
                    continue;
                }
                Value::String(s) => (s.clone(), None),
                Value::Array(a) if !a.is_empty() => match a[0].as_str() {
                    Some(s) => (s.to_string(), a.get(1).cloned()),
                    None => {
                        p.errors.push(tr!("{keys}: bad action", keys = keys));
                        continue;
                    }
                },
                _ => {
                    p.errors.push(tr!("{keys}: bad action", keys = keys));
                    continue;
                }
            };
            let action = match cx.build_action(&name, args) {
                Ok(a) => a,
                Err(e) => {
                    p.errors.push(format!("{keys}: {e}"));
                    continue;
                }
            };
            match KeyBinding::load(keys, action, predicate.clone(), false, None, &DummyKeyboardMapper) {
                Ok(b) => p.bindings.push(b),
                Err(e) => p.errors.push(format!("{keys}: {e}")),
            }
        }
    }
    p
}

/// Returns the keymap's problems.
pub fn apply(cx: &mut App) -> Vec<String> {
    let text = path().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    apply_text(&text, cx)
}

/// Empty text binds the defaults only.
pub fn apply_text(text: &str, cx: &mut App) -> Vec<String> {
    let parsed = if text.trim().is_empty() {
        Parsed { bindings: Vec::new(), removed: Vec::new(), errors: Vec::new() }
    } else {
        parse(text, cx)
    };
    cx.clear_key_bindings();
    let removed = parsed.removed;
    let defaults = crate::actions::default_bindings_spec()
        .into_iter()
        .filter(|(keys, ctx, _)| !removed.iter().any(|(k, c)| k == keys && c.as_deref() == *ctx))
        .map(|(keys, ctx, action)| KeyBinding::load(keys, action, ctx.and_then(|c| KeyBindingContextPredicate::parse(c).ok()).map(Rc::new), false, None, &DummyKeyboardMapper))
        .filter_map(Result::ok)
        .collect::<Vec<_>>();
    cx.bind_keys(defaults);
    cx.bind_keys(parsed.bindings);
    parsed.errors
}

pub fn ensure_file() -> Option<PathBuf> {
    let p = path()?;
    if !p.exists() {
        std::fs::create_dir_all(p.parent()?).ok()?;
        std::fs::write(&p, TEMPLATE).ok()?;
    }
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::strip_jsonc;

    #[test]
    fn comments_and_trailing_commas() {
        let s = "[ // c\n {\"a\": \"x // not a comment\",}, ]";
        let v: serde_json::Value = serde_json::from_str(&strip_jsonc(s)).unwrap();
        assert_eq!(v[0]["a"], "x // not a comment");
    }
}
