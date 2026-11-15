//! Every catalog has every key the source uses, with the same placeholders, and nothing else.
//! `L10N_MISSING=dir` also writes `<dir>/<code>.json` with the missing keys, for a translator.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use l10n::{parse, placeholders, Entry, Lang};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Text(String),
    Plural(String, String),
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

fn literal(s: &str) -> Option<(String, &str)> {
    let s = s.trim_start();
    let mut chars = s.strip_prefix('"')?.char_indices();
    let mut out = String::new();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Some((out, &s[i + 2..])),
            '\\' => match chars.next()?.1 {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                '\'' => out.push('\''),
                'u' => {
                    let mut hex = String::new();
                    for (_, c) in chars.by_ref() {
                        match c {
                            '{' => {}
                            '}' => break,
                            c => hex.push(c),
                        }
                    }
                    out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                }
                c => panic!("unsupported escape \\{c} in a key"),
            },
            c => out.push(c),
        }
    }
    None
}

fn keys_in(src: &str, file: &Path, keys: &mut BTreeMap<String, (Key, String)>) {
    // trc!("context", "English"): the key is `English#context`.
    let mut rest = src;
    while let Some(i) = rest.find("trc!(") {
        rest = &rest[i + 5..];
        let Some((ctx, after)) = literal(rest) else { continue };
        let after = after.trim_start().strip_prefix(',').expect("trc!(context, key)");
        let (key, _) = literal(after).expect("trc!: the key must be a literal");
        let id = format!("{key}#{ctx}");
        keys.insert(id, (Key::Text(key), file.display().to_string()));
    }
    for (mac, plural) in [("tr!(", false), ("trn!(", true)] {
        let mut rest = src;
        while let Some(i) = rest.find(mac) {
            let before = rest[..i].chars().next_back();
            rest = &rest[i + mac.len()..];
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue; // some_tr!(…)
            }
            if rest.trim_start().starts_with(')') || rest.trim_start().starts_with("$") {
                continue; // the macros' own definitions and docs
            }
            let where_ = file.display().to_string();
            let Some((one, after)) = literal(rest) else {
                panic!("{where_}: {mac}…) needs a string literal key: {}", &rest[..rest.len().min(60)]);
            };
            let key = if plural {
                let after = after.trim_start().strip_prefix(',').unwrap_or_else(|| panic!("{where_}: trn!({one:?}) needs its plural"));
                let (other, _) = literal(after).unwrap_or_else(|| panic!("{where_}: trn!({one:?}): the plural must be a literal"));
                Key::Plural(one.clone(), other)
            } else {
                Key::Text(one.clone())
            };
            if let Some((seen, at)) = keys.get(&one) {
                assert!(seen == &key, "{one:?} is used as both {seen:?} ({at}) and {key:?} ({where_})");
            } else {
                keys.insert(one, (key, where_));
            }
        }
    }
}

fn used_keys() -> BTreeMap<String, (Key, String)> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for c in std::fs::read_dir(crates).unwrap().flatten() {
        let src = c.path().join("src");
        if src.is_dir() && c.file_name() != "l10n" {
            rust_files(&src, &mut files);
        }
    }
    files.sort();
    let mut keys = BTreeMap::new();
    for f in files {
        keys_in(&std::fs::read_to_string(&f).unwrap(), &f, &mut keys);
    }
    keys
}

fn catalog_src(lang: Lang) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("locales").join(format!("{}.json", lang.code()));
    std::fs::read_to_string(p).unwrap()
}

#[test]
fn scanner_reads_literals() {
    let mut keys = BTreeMap::new();
    let src = r#"let e = trc!("variant", "Edit"); let a = tr!("Open…"); let b = tr!( "Say \"{x}\"", x = 1); let c = trn!("{n} word", "{n} words", n); mytr!("no");"#;
    keys_in(src, Path::new("x.rs"), &mut keys);
    let got: Vec<&Key> = keys.values().map(|(k, _)| k).collect();
    assert_eq!(
        got,
        [&Key::Text("Edit".into()), &Key::Text("Open…".into()), &Key::Text("Say \"{x}\"".into()), &Key::Plural("{n} word".into(), "{n} words".into())].into_iter().collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>()
    );
}

#[test]
fn english_plurals_agree_on_placeholders() {
    for (key, at) in used_keys().values() {
        if let Key::Plural(one, other) = key {
            let (a, b) = (placeholders(one), placeholders(other));
            let extra: Vec<_> = a.iter().filter(|p| !b.contains(p)).collect();
            assert!(extra.is_empty(), "{at}: {one:?} has {extra:?} that {other:?} lacks");
        }
    }
}

#[test]
fn every_catalog_is_complete_and_current() {
    let keys = used_keys();
    let mut problems = Vec::new();
    let dump = std::env::var_os("L10N_MISSING").map(PathBuf::from);
    for lang in Lang::ALL.into_iter().filter(|l| *l != Lang::En) {
        let code = lang.code();
        let cat = match parse(&catalog_src(lang)) {
            Ok(c) => c,
            Err(e) => {
                problems.push(format!("{code}.json: {e}"));
                continue;
            }
        };
        let mut missing = BTreeMap::new();
        for (id, (key, at)) in &keys {
            let wanted = match key {
                Key::Text(k) => placeholders(k),
                Key::Plural(one, other) => {
                    let mut p = placeholders(one);
                    p.extend(placeholders(other));
                    p.sort_unstable();
                    p.dedup();
                    p
                }
            };
            match (key, cat.get(id)) {
                (_, None) => {
                    let v = match key {
                        Key::Text(k) => serde_json::Value::String(k.clone()),
                        Key::Plural(one, other) => serde_json::json!([one, other]),
                    };
                    missing.insert(id.clone(), v);
                }
                (Key::Text(_), Some(Entry::Text(t))) => {
                    if t.trim().is_empty() {
                        problems.push(format!("{code}: {id:?} is empty"));
                    } else if placeholders(t) != wanted {
                        problems.push(format!("{code}: {id:?} → {t:?} has placeholders {:?}, the key {wanted:?}", placeholders(t)));
                    }
                }
                (Key::Plural(..), Some(Entry::Forms(f))) => {
                    if f.len() != lang.plural_forms() {
                        problems.push(format!("{code}: {id:?} has {} forms, {code} needs {}", f.len(), lang.plural_forms()));
                    }
                    for (i, form) in f.iter().enumerate() {
                        let p = placeholders(form);
                        if let Some(bad) = p.iter().find(|x| !wanted.contains(x)) {
                            problems.push(format!("{code}: {id:?} form {i} has {{{bad}}}, which the key hasn't"));
                        }
                        // A form serving several numbers must name the number: {n}, or {count} when grouped.
                        let Key::Plural(_, other) = key else { unreachable!() };
                        if i > 0 || lang.one_form_is_shared() {
                            if let Some(lack) = placeholders(other).into_iter().find(|x| !p.contains(x)) {
                                problems.push(format!("{code}: {id:?} form {i} ({form:?}) must say {{{lack}}}: it serves more than one number"));
                            }
                        }
                    }
                }
                (Key::Text(_), Some(Entry::Forms(_))) => problems.push(format!("{code}: {id:?} is plain text in the source ({at}), plural forms in the catalog")),
                (Key::Plural(..), Some(Entry::Text(_))) => problems.push(format!("{code}: {id:?} is plural in the source ({at}), a single string in the catalog")),
            }
        }
        if !missing.is_empty() {
            problems.push(format!("{code}.json lacks {} keys, e.g. {:?}", missing.len(), missing.keys().take(5).collect::<Vec<_>>()));
            if let Some(dir) = &dump {
                std::fs::create_dir_all(dir).unwrap();
                let obj: serde_json::Map<String, serde_json::Value> = missing.into_iter().collect();
                std::fs::write(dir.join(format!("{code}.json")), serde_json::to_string_pretty(&obj).unwrap()).unwrap();
            }
        }
        let stale: Vec<&String> = cat.keys().filter(|k| !keys.contains_key(*k)).collect();
        if !stale.is_empty() {
            problems.push(format!("{code}.json has {} keys no source uses: {:?}", stale.len(), stale.iter().take(8).collect::<Vec<_>>()));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
