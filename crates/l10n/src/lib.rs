//! The UI's languages. The English text is the key; catalogs are `locales/<code>.json`.
//! Keys must be string literals: `tests/catalogs.rs` reads them from the source.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lang {
    En,
    De,
    Es,
    Fr,
    It,
    PtBr,
    Ru,
}

impl Lang {
    /// In the order Settings lists them.
    pub const ALL: [Lang; 7] = [Lang::En, Lang::De, Lang::Es, Lang::Fr, Lang::It, Lang::PtBr, Lang::Ru];

    /// BCP 47; also the `locales/` file and `.lproj` folder names.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::De => "de",
            Lang::Es => "es",
            Lang::Fr => "fr",
            Lang::It => "it",
            Lang::PtBr => "pt-BR",
            Lang::Ru => "ru",
        }
    }

    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::De => "Deutsch",
            Lang::Es => "Español",
            Lang::Fr => "Français",
            Lang::It => "Italiano",
            Lang::PtBr => "Português (Brasil)",
            Lang::Ru => "Русский",
        }
    }

    pub fn from_code(code: &str) -> Option<Lang> {
        Lang::ALL.into_iter().find(|l| l.code().eq_ignore_ascii_case(code))
    }

    /// The catalog's text, `None` for English (the keys).
    fn source(self) -> Option<&'static str> {
        match self {
            Lang::En => None,
            Lang::De => Some(include_str!("../locales/de.json")),
            Lang::Es => Some(include_str!("../locales/es.json")),
            Lang::Fr => Some(include_str!("../locales/fr.json")),
            Lang::It => Some(include_str!("../locales/it.json")),
            Lang::PtBr => Some(include_str!("../locales/pt-BR.json")),
            Lang::Ru => Some(include_str!("../locales/ru.json")),
        }
    }

    pub fn plural_forms(self) -> usize {
        match self {
            Lang::Ru => 3,
            _ => 2,
        }
    }

    /// CLDR cardinal rules for whole numbers.
    pub fn plural_index(self, n: u64) -> usize {
        match self {
            Lang::En | Lang::De | Lang::Es | Lang::It => usize::from(n != 1),
            // 0 and 1 are singular in French and Brazilian Portuguese.
            Lang::Fr | Lang::PtBr => usize::from(n > 1),
            // один, два–четыре, пять: 1, 21, 101 · 2–4, 22–24 · 0, 5–20, 25–30, 111–114.
            Lang::Ru => {
                let (d, h) = (n % 10, n % 100);
                if d == 1 && h != 11 {
                    0
                } else if (2..=4).contains(&d) && !(12..=14).contains(&h) {
                    1
                } else {
                    2
                }
            }
        }
    }

    /// The one-form also serves other numbers (21 in Russian, 0 in French), so it must say `{n}`.
    pub fn one_form_is_shared(self) -> bool {
        matches!(self, Lang::Fr | Lang::PtBr | Lang::Ru)
    }
}

#[derive(Debug)]
pub enum Entry {
    Text(String),
    Forms(Vec<String>),
}

pub type Catalog = HashMap<String, Entry>;

/// Keys starting with `//` are comments.
pub fn parse(src: &str) -> Result<Catalog, String> {
    let v: serde_json::Value = serde_json::from_str(src).map_err(|e| e.to_string())?;
    let obj = v.as_object().ok_or("a catalog is a JSON object")?;
    let mut out = Catalog::new();
    for (k, v) in obj {
        if k.starts_with("//") {
            continue;
        }
        let entry = match v {
            serde_json::Value::String(s) => Entry::Text(s.clone()),
            serde_json::Value::Array(a) => Entry::Forms(
                a.iter().map(|f| f.as_str().map(str::to_string).ok_or(format!("{k:?}: forms are strings"))).collect::<Result<_, _>>()?,
            ),
            _ => return Err(format!("{k:?}: a translation is a string or an array of plural forms")),
        };
        out.insert(k.clone(), entry);
    }
    Ok(out)
}

fn catalog(lang: Lang) -> Option<&'static Catalog> {
    static CATALOGS: [OnceLock<Catalog>; Lang::ALL.len()] = [const { OnceLock::new() }; Lang::ALL.len()];
    let src = lang.source()?;
    let i = Lang::ALL.iter().position(|l| *l == lang)?;
    // A broken catalog shows English rather than nothing (the tests keep them valid).
    Some(CATALOGS[i].get_or_init(|| parse(src).unwrap_or_default()))
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

thread_local! {
    /// A test's own language, so it can't change what tests on other threads see.
    static THREAD: std::cell::Cell<Option<Lang>> = const { std::cell::Cell::new(None) };
}

/// The UI language. English until [`set`] is called, so tests see the keys.
pub fn current() -> Lang {
    THREAD.with(|t| t.get()).unwrap_or_else(|| Lang::ALL.get(CURRENT.load(Ordering::Relaxed) as usize).copied().unwrap_or(Lang::En))
}

pub fn set_for_thread(lang: Option<Lang>) {
    THREAD.with(|t| t.set(lang));
}

pub fn set(lang: Lang) {
    let i = Lang::ALL.iter().position(|l| *l == lang).unwrap_or(0);
    let _ = catalog(lang); // parse now, not in the middle of a frame
    CURRENT.store(i as u8, Ordering::Relaxed);
}

/// The first of `preferred` (macOS order: `en-US`, `pt-BR`…) with a catalog; English if none.
pub fn negotiate<S: AsRef<str>>(preferred: &[S]) -> Lang {
    for tag in preferred {
        let tag = tag.as_ref().replace('_', "-");
        let primary = tag.split('-').next().unwrap_or("");
        if let Some(l) = Lang::from_code(&tag).or_else(|| match primary {
            // Any Portuguese gets the Brazilian catalog, the only one.
            "pt" => Some(Lang::PtBr),
            p => Lang::from_code(p),
        }) {
            return l;
        }
    }
    Lang::En
}

/// `key` in the current language. Use [`tr!`], which the catalog test can see.
pub fn tr(key: &'static str) -> &'static str {
    match catalog(current()).and_then(|c| c.get(key)) {
        Some(Entry::Text(s)) => s,
        _ => key,
    }
}

/// `ctx_key` is `English#context`. Use [`trc!`].
pub fn trc(ctx_key: &'static str, key: &'static str) -> &'static str {
    match catalog(current()).and_then(|c| c.get(ctx_key)) {
        Some(Entry::Text(s)) => s,
        _ => key,
    }
}

/// Use [`trn!`].
pub fn plural(one: &'static str, other: &'static str, n: u64) -> &'static str {
    let lang = current();
    match catalog(lang).and_then(|c| c.get(one)) {
        Some(Entry::Forms(f)) if f.len() == lang.plural_forms() => &f[lang.plural_index(n)],
        _ => {
            if n == 1 {
                one
            } else {
                other
            }
        }
    }
}

pub fn fill(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('}').map(|j| (&after[..j], j)) {
            Some((name, j)) if args.iter().any(|(n, _)| *n == name) => {
                let (_, v) = args.iter().find(|(n, _)| *n == name).unwrap();
                out.push_str(&v.to_string());
                rest = &after[j + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The `{name}` placeholders in `s`, sorted.
pub fn placeholders(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        let after = &rest[i + 1..];
        match after.find('}') {
            Some(j) if after[..j].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && j > 0 => {
                out.push(&after[..j]);
                rest = &after[j + 1..];
            }
            _ => rest = after,
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// `tr!("English")` → `&'static str`; with `name = value` arguments → `String`.
#[macro_export]
macro_rules! tr {
    ($key:literal) => {
        $crate::tr($key)
    };
    ($key:literal, $($name:ident = $val:expr),+ $(,)?) => {
        $crate::fill($crate::tr($key), &[$((stringify!($name), &$val as &dyn ::std::fmt::Display)),+])
    };
}

/// `trc!("context", "English")`: the same English, translated differently; key `English#context`.
#[macro_export]
macro_rules! trc {
    ($ctx:literal, $key:literal) => {
        $crate::trc(concat!($key, "#", $ctx), $key)
    };
}

/// `trn!("{n} word", "{n} words", n)` → `String`, `{n}` and any `name = value` filled in.
#[macro_export]
macro_rules! trn {
    ($one:literal, $other:literal, $n:expr $(, $name:ident = $val:expr)* $(,)?) => {{
        let n = $n;
        $crate::fill(
            $crate::plural($one, $other, n as u64),
            &[("n", &n as &dyn ::std::fmt::Display) $(, (stringify!($name), &$val as &dyn ::std::fmt::Display))*],
        )
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn russian_plurals() {
        let forms: Vec<usize> = [0, 1, 2, 4, 5, 11, 12, 14, 21, 22, 25, 101, 111, 112, 122].iter().map(|n| Lang::Ru.plural_index(*n)).collect();
        assert_eq!(forms, [2, 0, 1, 1, 2, 2, 2, 2, 0, 1, 2, 0, 2, 2, 1]);
    }

    #[test]
    fn french_zero_is_singular() {
        assert_eq!((Lang::Fr.plural_index(0), Lang::Fr.plural_index(1), Lang::Fr.plural_index(2)), (0, 0, 1));
        assert_eq!((Lang::En.plural_index(0), Lang::En.plural_index(1)), (1, 0));
    }

    #[test]
    fn negotiation() {
        assert_eq!(negotiate(&["ru-RU", "en-US"]), Lang::Ru);
        assert_eq!(negotiate(&["ja-JP", "de-AT"]), Lang::De);
        assert_eq!(negotiate(&["pt-PT"]), Lang::PtBr);
        assert_eq!(negotiate(&["pt_BR"]), Lang::PtBr);
        assert_eq!(negotiate(&["ja", "zh-Hans"]), Lang::En);
        assert_eq!(negotiate::<&str>(&[]), Lang::En);
    }

    #[test]
    fn filling() {
        assert_eq!(fill("New {kind} in {where}", &[("kind", &"Chapter"), ("where", &3)]), "New Chapter in 3");
        assert_eq!(fill("{a} {unknown} {", &[("a", &1)]), "1 {unknown} {");
        assert_eq!(placeholders("{n} of {total}, {n} {} {x y}"), ["n", "total"]);
    }

    #[test]
    fn english_is_the_key() {
        assert_eq!(tr!("Not in any catalog"), "Not in any catalog");
        assert_eq!(trc!("menu", "Not in any catalog"), "Not in any catalog");
        assert_eq!(trn!("{n} thing", "{n} things", 1), "1 thing");
        assert_eq!(trn!("{n} thing", "{n} things", 3), "3 things");
        assert_eq!(trn!("{n} thing in {f}", "{n} things in {f}", 2, f = "a"), "2 things in a");
    }
}
