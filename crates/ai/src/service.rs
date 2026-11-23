use l10n::tr;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceKind {
    OpenRouter,
    OpenAi,
    Gemini,
    Mistral,
    Groq,
    Ollama,
    LmStudio,
    Custom,
}

impl ServiceKind {
    pub const ALL: [ServiceKind; 8] = [
        ServiceKind::OpenRouter,
        ServiceKind::OpenAi,
        ServiceKind::Gemini,
        ServiceKind::Mistral,
        ServiceKind::Groq,
        ServiceKind::Ollama,
        ServiceKind::LmStudio,
        ServiceKind::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ServiceKind::OpenRouter => "OpenRouter",
            ServiceKind::OpenAi => "OpenAI",
            ServiceKind::Gemini => "Google Gemini",
            ServiceKind::Mistral => "Mistral",
            ServiceKind::Groq => "Groq",
            ServiceKind::Ollama => "Ollama",
            ServiceKind::LmStudio => "LM Studio",
            ServiceKind::Custom => tr!("Custom"),
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            ServiceKind::OpenRouter => tr!("one account, many models"),
            ServiceKind::OpenAi => tr!("API key"),
            ServiceKind::Gemini => tr!("API key, has a free tier"),
            ServiceKind::Mistral => tr!("API key, strong beyond English"),
            ServiceKind::Groq => tr!("API key, very fast"),
            ServiceKind::Ollama | ServiceKind::LmStudio => tr!("on this Mac"),
            ServiceKind::Custom => tr!("any compatible address: llama.cpp, Jan, MLX…"),
        }
    }

    pub fn default_base_url(self) -> &'static str {
        match self {
            ServiceKind::OpenRouter => "https://openrouter.ai/api/v1",
            ServiceKind::OpenAi => "https://api.openai.com/v1",
            ServiceKind::Gemini => "https://generativelanguage.googleapis.com/v1beta/openai",
            ServiceKind::Mistral => "https://api.mistral.ai/v1",
            ServiceKind::Groq => "https://api.groq.com/openai/v1",
            ServiceKind::Ollama => "http://127.0.0.1:11434/v1",
            ServiceKind::LmStudio => "http://127.0.0.1:1234/v1",
            ServiceKind::Custom => "",
        }
    }

    pub fn local(self) -> bool {
        matches!(self, ServiceKind::Ollama | ServiceKind::LmStudio)
    }

    /// Custom may or may not use one.
    pub fn needs_key(self) -> bool {
        matches!(self, ServiceKind::OpenRouter | ServiceKind::OpenAi | ServiceKind::Gemini | ServiceKind::Mistral | ServiceKind::Groq)
    }

    /// Where a key is made, for the services that take a pasted one.
    pub fn key_page(self) -> Option<&'static str> {
        match self {
            ServiceKind::OpenAi => Some("https://platform.openai.com/api-keys"),
            ServiceKind::Gemini => Some("https://aistudio.google.com/apikey"),
            ServiceKind::Mistral => Some("https://console.mistral.ai/api-keys"),
            ServiceKind::Groq => Some("https://console.groq.com/keys"),
            _ => None,
        }
    }

    /// Takes `reasoning_effort` and counts the thinking in `max_tokens` (Gemini 3, gpt-oss on Groq).
    pub fn effort_knob(self) -> bool {
        matches!(self, ServiceKind::Gemini | ServiceKind::Groq)
    }

    /// The Keychain account its key is stored under.
    pub fn slug(self) -> &'static str {
        match self {
            ServiceKind::OpenRouter => "openrouter",
            ServiceKind::OpenAi => "openai",
            ServiceKind::Gemini => "gemini",
            ServiceKind::Mistral => "mistral",
            ServiceKind::Groq => "groq",
            ServiceKind::Ollama => "ollama",
            ServiceKind::LmStudio => "lmstudio",
            ServiceKind::Custom => "custom",
        }
    }
}

/// One of each kind.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Service {
    pub kind: ServiceKind,
    /// Empty: the kind's default address.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub base_url: String,
}

impl Service {
    pub fn new(kind: ServiceKind) -> Self {
        Service { kind, base_url: String::new() }
    }

    pub fn base(&self) -> &str {
        if self.base_url.trim().is_empty() {
            self.kind.default_base_url()
        } else {
            self.base_url.trim()
        }
    }

    pub fn short_address(&self) -> String {
        let b = self.base();
        let b = b.split("://").nth(1).unwrap_or(b);
        b.trim_end_matches('/').trim_end_matches("/v1").trim_end_matches("/api").to_string()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModelRef {
    pub service: ServiceKind,
    pub model: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    /// Dollars per token, when the service says.
    pub price_in: Option<f64>,
    pub price_out: Option<f64>,
}

impl ModelInfo {
    pub fn free(&self) -> bool {
        self.id.ends_with(":free") || (self.price_in == Some(0.) && self.price_out == Some(0.))
    }
}

pub fn parse_models(json: &Value) -> Vec<ModelInfo> {
    let price = |v: &Value| match v {
        Value::String(s) => s.parse::<f64>().ok(),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
    .filter(|p| *p >= 0.);
    let mut out: Vec<ModelInfo> = json["data"]
        .as_array()
        .or_else(|| json["models"].as_array())
        .map(|a| {
            a.iter()
                .filter_map(|m| {
                    // Gemini lists `models/gemini-…`; its chat endpoint takes the bare name too.
                    let id = m["id"].as_str().or_else(|| m["name"].as_str())?;
                    let id = id.strip_prefix("models/").unwrap_or(id).to_string();
                    let name = m["name"].as_str().filter(|n| !n.is_empty() && *n != id && !n.starts_with("models/")).map(clean_name).unwrap_or_else(|| id.clone());
                    Some(ModelInfo { name, price_in: price(&m["pricing"]["prompt"]), price_out: price(&m["pricing"]["completion"]), id })
                })
                .collect()
        })
        .unwrap_or_default();
    // Embedding, image, speech and safety models can't write; batch variants don't stream.
    const NOT_WRITERS: [&str; 14] =
        ["embed", "whisper", "tts", "dall-e", "imagen", "veo", "-image", "aqa", "guard", "moderation", "ocr", "transcribe", "native-audio", "realtime"];
    out.retain(|m| {
        let id = m.id.to_lowercase();
        !NOT_WRITERS.iter().any(|w| id.contains(w)) && !id.ends_with(":batch")
    });
    out.sort_by_key(|a| a.name.to_lowercase());
    out.dedup_by(|a, b| a.id == b.id);
    out
}

fn clean_name(n: &str) -> String {
    match n.split_once(": ") {
        Some((_, rest)) if !rest.is_empty() => rest.to_string(),
        _ => n.to_string(),
    }
}

/// A suggested model in the menu: what it's good for, and the id it resolved to on this service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggested {
    pub id: String,
    pub name: String,
    pub role: Role,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The Quick default.
    Quick,
    Quickest,
    /// The Deep default.
    Deep,
}

impl Suggested {
    pub fn blurb(&self) -> &'static str {
        match self.role {
            Role::Quick => tr!("quick, good with words"),
            Role::Quickest => tr!("quickest and cheapest"),
            Role::Deep => tr!("best judgement, slower"),
        }
    }
}

pub const SONNET: &str = "anthropic/claude-sonnet-5.5";
pub const OPUS: &str = "anthropic/claude-opus-5.5";
pub const HAIKU: &str = "anthropic/claude-haiku-5.5";

/// Each service's picks, in menu order: ids in order of preference, where `#` is a version
/// (`3`, `2.5`), `*` anything and `!word` leaves out ids with that word; a pattern takes the
/// newest listed id it matches (a preview of a newer version beats an older release).
fn picks(kind: ServiceKind) -> &'static [(Role, &'static [&'static str])] {
    match kind {
        ServiceKind::OpenRouter => &[
            (Role::Quick, &[SONNET, "anthropic/claude-sonnet-#"]),
            (Role::Quickest, &[HAIKU, "anthropic/claude-haiku-#"]),
            (Role::Deep, &[OPUS, "anthropic/claude-opus-#"]),
        ],
        ServiceKind::OpenAi => &[(Role::Quick, &["gpt-#-mini"]), (Role::Quickest, &["gpt-#-nano"]), (Role::Deep, &["gpt-#"])],
        // `-latest` aliases follow Google's newest Flash and Pro.
        ServiceKind::Gemini => &[
            (Role::Quick, &["gemini-flash-latest", "gemini-#-flash*!lite"]),
            (Role::Quickest, &["gemini-flash-lite-latest", "gemini-#-flash-lite*"]),
            (Role::Deep, &["gemini-pro-latest", "gemini-#-pro*"]),
        ],
        ServiceKind::Mistral => &[
            (Role::Quick, &["mistral-small-latest", "mistral-small-#*"]),
            (Role::Quickest, &["ministral-8b-latest", "ministral-3b-latest"]),
            (Role::Deep, &["mistral-large-latest", "mistral-medium-latest", "mistral-large-#*"]),
        ],
        ServiceKind::Groq => &[
            (Role::Quick, &["openai/gpt-oss-120b", "llama-#-70b-versatile"]),
            (Role::Quickest, &["llama-#-8b-instant", "openai/gpt-oss-20b"]),
            (Role::Deep, &["moonshotai/kimi-k#*", "openai/gpt-oss-120b", "llama-#-70b-versatile"]),
        ],
        _ => &[],
    }
}

/// `#` matches a version (digits and dots), `*` anything.
fn glob(p: &[u8], s: &[u8]) -> bool {
    match p.first() {
        None => s.is_empty(),
        Some(b'*') => (0..=s.len()).any(|i| glob(&p[1..], &s[i..])),
        Some(b'#') => {
            let n = s.iter().take_while(|c| c.is_ascii_digit() || **c == b'.').count();
            s.first().is_some_and(u8::is_ascii_digit) && (1..=n).any(|i| glob(&p[1..], &s[i..]))
        }
        Some(c) => s.first() == Some(c) && glob(&p[1..], &s[1..]),
    }
}

fn pick(wanted: &[&str], models: &[ModelInfo]) -> Option<String> {
    if models.is_empty() {
        // Before the list arrives: a known id, never a guess at a pattern.
        return wanted.first().filter(|w| !w.contains(['#', '*'])).map(|w| w.to_string());
    }
    wanted.iter().find_map(|w| {
        if !w.contains(['#', '*']) {
            return models.iter().find(|m| m.id == *w).map(|m| m.id.clone());
        }
        let mut parts = w.split('!');
        let w = parts.next().unwrap_or_default();
        let without: Vec<&str> = parts.collect();
        let head = &w[..w.find(['#', '*']).unwrap_or(w.len())];
        models
            .iter()
            .filter(|m| glob(w.as_bytes(), m.id.as_bytes()) && !without.iter().any(|x| m.id.contains(x)))
            // The highest version, then a release before a preview, then the latest dated one.
            .max_by(|a, b| {
                let key = |id: &str| (version(&id[head.len()..]).unwrap_or(0.), !id.contains("preview"));
                let (ka, kb) = (key(&a.id), key(&b.id));
                ka.0.total_cmp(&kb.0).then(ka.1.cmp(&kb.1)).then(a.id.cmp(&b.id))
            })
            .map(|m| m.id.clone())
    })
}

/// `3.5` in `3.5-flash`, `2` in `k2-instruct`.
fn version(s: &str) -> Option<f64> {
    let digits: String = s.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit() || *c == '.').collect();
    digits.trim_end_matches('.').parse().ok()
}

/// The menu's suggested rows for a service, resolved against its list (each id once).
pub fn suggested(kind: ServiceKind, models: &[ModelInfo]) -> Vec<Suggested> {
    let mut out: Vec<Suggested> = Vec::new();
    for (role, wanted) in picks(kind) {
        if let Some(id) = pick(wanted, models).filter(|id| !out.iter().any(|s| s.id == *id)) {
            let name = models.iter().find(|m| m.id == id && m.name != id).map(|m| m.name.clone()).unwrap_or_else(|| pretty_id(&id));
            out.push(Suggested { id, name, role: *role });
        }
    }
    out
}

/// A readable name for a bare id: `gemini-2.5-flash` → Gemini 2.5 Flash,
/// `moonshotai/kimi-k2-instruct-0905` → Kimi K2, `openai/gpt-oss-120b` → GPT OSS 120B.
pub fn pretty_id(id: &str) -> String {
    let base = id.rsplit('/').next().unwrap_or(id);
    base.split('-')
        .filter(|t| !t.is_empty() && !matches!(*t, "latest" | "instruct" | "versatile" | "it"))
        // Dates and build numbers: 0905, 001, 2025.
        .filter(|t| !(t.chars().all(|c| c.is_ascii_digit()) && (t.len() >= 4 || t.starts_with('0'))))
        .map(|t| match t {
            "gpt" | "oss" => t.to_uppercase(),
            _ => {
                let mut cs: Vec<char> = t.chars().collect();
                cs[0] = cs[0].to_ascii_uppercase();
                // 120b → 120B, e4b → E4B.
                if cs.len() > 1 && cs[cs.len() - 1] == 'b' && cs[cs.len() - 2].is_ascii_digit() {
                    let last = cs.len() - 1;
                    cs[last] = 'B';
                }
                cs.into_iter().collect()
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

/// Readable names and newest first, for services whose lists are bare ids.
pub fn polish(kind: ServiceKind, mut models: Vec<ModelInfo>) -> Vec<ModelInfo> {
    if kind.local() || kind == ServiceKind::Custom || kind == ServiceKind::OpenRouter {
        return models;
    }
    for m in &mut models {
        if m.name == m.id {
            m.name = pretty_id(&m.id);
        }
    }
    // Family by family (the name without its numbers); in each, an alias first, then the newest.
    let family = |n: &str| n.split(' ').filter(|t| !t.starts_with(|c: char| c.is_ascii_digit())).collect::<Vec<_>>().join(" ").to_lowercase();
    models.sort_by(|a, b| {
        let (va, vb) = (version(&a.name).unwrap_or(f64::INFINITY), version(&b.name).unwrap_or(f64::INFINITY));
        family(&a.name).cmp(&family(&b.name)).then(vb.total_cmp(&va)).then(a.id.cmp(&b.id))
    });
    models
}

/// The largest free model by the size in its name (`29b`), otherwise the first.
pub fn free_model(models: &[ModelInfo]) -> Option<&ModelInfo> {
    let free: Vec<&ModelInfo> = models.iter().filter(|m| m.free()).collect();
    free.iter().copied().filter(|m| size_in_name(&m.id) > 0.).max_by(|a, b| size_in_name(&a.id).total_cmp(&size_in_name(&b.id))).or_else(|| free.first().copied())
}

fn size_in_name(id: &str) -> f64 {
    id.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '.')
        .filter_map(|t| t.strip_suffix('b').and_then(|n| n.parse::<f64>().ok()))
        .fold(0., f64::max)
}

/// (Quick, Deep).
pub fn default_models(kind: ServiceKind, models: &[ModelInfo]) -> (Option<String>, Option<String>) {
    let role = |r: Role| picks(kind).iter().find(|(x, _)| *x == r).and_then(|(_, w)| pick(w, models));
    let (q, d) = (role(Role::Quick).or_else(|| role(Role::Quickest)), role(Role::Deep));
    if q.is_some() || d.is_some() {
        return (q.clone().or_else(|| d.clone()), d.or(q));
    }
    if kind == ServiceKind::OpenRouter {
        return (None, None);
    }
    let first = models.first().map(|m| m.id.clone());
    let size = |m: &ModelInfo| size_in_name(&m.id);
    if models.len() > 1 {
        let q = models.iter().min_by(|a, b| size(a).total_cmp(&size(b))).map(|m| m.id.clone());
        let d = models.iter().max_by(|a, b| size(a).total_cmp(&size(b))).map(|m| m.id.clone());
        (q, d)
    } else {
        (first.clone(), first)
    }
}

pub fn display_name(r: &ModelRef, models: &[ModelInfo]) -> String {
    match models.iter().find(|m| m.id == r.model) {
        Some(m) => m.name.clone(),
        // The list isn't here yet (or no longer has it).
        None if r.service.local() || r.service == ServiceKind::Custom => r.model.clone(),
        None => pretty_id(&r.model),
    }
}

/// The model menu's search: every word of `query` is in the name or the id, ignoring case.
pub fn matches_query(name: &str, id: &str, query: &str) -> bool {
    query.split_whitespace().all(|w| find_ignoring_case(name, w).is_some() || find_ignoring_case(id, w).is_some())
}

/// Where the words of `query` first appear in `text`, as sorted byte ranges that don't overlap.
pub fn query_ranges(text: &str, query: &str) -> Vec<std::ops::Range<usize>> {
    let mut out: Vec<std::ops::Range<usize>> = query.split_whitespace().filter_map(|w| find_ignoring_case(text, w)).collect();
    out.sort_by_key(|r| (r.start, std::cmp::Reverse(r.end)));
    let mut merged: Vec<std::ops::Range<usize>> = Vec::new();
    for r in out {
        match merged.last_mut() {
            Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
            _ => merged.push(r),
        }
    }
    merged
}

fn find_ignoring_case(text: &str, word: &str) -> Option<std::ops::Range<usize>> {
    let fold = |c: char| c.to_lowercase().next().unwrap_or(c);
    let want: Vec<char> = word.chars().map(fold).collect();
    if want.is_empty() {
        return None;
    }
    text.char_indices().find_map(|(i, _)| {
        let mut end = i;
        let mut chars = text[i..].chars();
        for w in &want {
            let c = chars.next()?;
            if fold(c) != *w {
                return None;
            }
            end += c.len_utf8();
        }
        Some(i..end)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn search_words_in_any_order_in_name_or_id() {
        assert!(matches_query("Claude Sonnet 5.5", SONNET, "son 5"));
        assert!(matches_query("Claude Sonnet 5.5", SONNET, "5 SON"));
        assert!(matches_query("Google: Gemini 3 Flash", "google/gemini-3-flash", "google flash"));
        assert!(matches_query("Google: Gemini 3 Flash Lite", "google/gemini-3-flash-lite", "flash-lite"));
        assert!(matches_query("Gemma 4 27B (free)", "google/gemma-4-27b-it:free", "free"));
        assert!(!matches_query("Claude Opus 5.5", OPUS, "clade"));
        assert!(matches_query("Claude Opus 5.5", OPUS, "  "), "an empty search matches everything");
    }

    #[test]
    fn search_ranges_light_each_word_once() {
        assert_eq!(query_ranges("Claude Sonnet 5.5", "son 5"), vec![7..10, 14..15]);
        assert_eq!(query_ranges("Google: Gemini", "gem gemini"), vec![8..14], "overlaps merge");
        assert_eq!(query_ranges("Ёлка Модель", "модель"), vec![9..21], "byte ranges, any case");
        assert!(query_ranges("qwen3:14b", "llama").is_empty());
    }

    #[test]
    fn reads_openrouter_models_with_prices() {
        let v = json!({"data": [
            {"id": "anthropic/claude-sonnet-5.5", "name": "Anthropic: Claude Sonnet 5.5", "pricing": {"prompt": "0.000002", "completion": "0.00001"}},
            {"id": "meta-llama/llama-4-scout:free", "name": "Meta: Llama 4 Scout (free)", "pricing": {"prompt": "0", "completion": "0"}},
            {"id": "openai/text-embedding-3", "name": "Embeddings"}
        ]});
        let m = parse_models(&v);
        assert_eq!(m.len(), 2);
        assert_eq!(m[0].name, "Claude Sonnet 5.5");
        assert_eq!(m[0].price_in, Some(0.000002));
        assert!(m[1].free());
        assert_eq!(free_model(&m).unwrap().id, "meta-llama/llama-4-scout:free");
        let v = json!({"data": [
            {"id": "liquid/lfm-3-1.7b:free", "pricing": {"prompt": "0", "completion": "0"}},
            {"id": "xingchen-agi/xing4.0-29b-a4b:free", "pricing": {"prompt": "0", "completion": "0"}},
            {"id": "anthropic/claude-opus-5.5:batch"}
        ]});
        let m = parse_models(&v);
        assert_eq!(m.len(), 2, "batch variants are left out");
        assert_eq!(free_model(&m).unwrap().id, "xingchen-agi/xing4.0-29b-a4b:free", "the largest free model");
    }

    #[test]
    fn reads_local_models_and_picks_by_size() {
        let v = json!({"object": "list", "data": [{"id": "qwen3:14b"}, {"id": "llama3.1:8b"}]});
        let m = parse_models(&v);
        assert_eq!(m.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["llama3.1:8b", "qwen3:14b"]);
        let (q, d) = default_models(ServiceKind::Ollama, &m);
        assert_eq!(q.as_deref(), Some("llama3.1:8b"));
        assert_eq!(d.as_deref(), Some("qwen3:14b"));
    }

    #[test]
    fn gemini_lists_bare_ids_and_only_models_that_write() {
        let v = json!({"object": "list", "data": [
            {"id": "models/gemini-3-flash", "object": "model", "owned_by": "google"},
            {"id": "models/gemini-flash-latest", "object": "model"},
            {"id": "models/gemini-pro-latest", "object": "model"},
            {"id": "models/gemini-embedding-001"},
            {"id": "models/imagen-4.0-generate-001"},
            {"id": "models/veo-3.0-generate-001"},
            {"id": "models/gemini-2.5-flash-image"},
            {"id": "models/gemini-2.5-flash-preview-tts"},
            {"id": "models/gemini-2.5-flash-native-audio-latest"}
        ]});
        let m = parse_models(&v);
        assert_eq!(m.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["gemini-3-flash", "gemini-flash-latest", "gemini-pro-latest"]);
        assert_eq!(m[0].name, "gemini-3-flash");
        let (q, d) = default_models(ServiceKind::Gemini, &m);
        assert_eq!((q.as_deref(), d.as_deref()), (Some("gemini-flash-latest"), Some("gemini-pro-latest")));
    }

    #[test]
    fn presets_fall_back_through_their_families() {
        let ids = |ids: &[&str]| ids.iter().map(|id| ModelInfo { id: id.to_string(), name: id.to_string(), price_in: None, price_out: None }).collect::<Vec<_>>();
        let m = ids(&["gemini-2.5-flash", "gemini-3-flash", "gemini-3.5-flash-lite", "gemini-3.5-flash", "gemini-3-pro-preview", "gemini-2.5-pro"]);
        let (q, d) = default_models(ServiceKind::Gemini, &m);
        assert_eq!((q.as_deref(), d.as_deref()), (Some("gemini-3.5-flash"), Some("gemini-3-pro-preview")), "the newest, never Lite");
        let m = ids(&["llama-3.3-70b-versatile", "moonshotai/kimi-k2-instruct", "moonshotai/kimi-k2-instruct-0905", "openai/gpt-oss-120b"]);
        let (q, d) = default_models(ServiceKind::Groq, &m);
        assert_eq!((q.as_deref(), d.as_deref()), (Some("openai/gpt-oss-120b"), Some("moonshotai/kimi-k2-instruct-0905")));
        let m = ids(&["mistral-small-latest", "codestral-latest"]);
        let (q, d) = default_models(ServiceKind::Mistral, &m);
        assert_eq!((q.as_deref(), d.as_deref()), (Some("mistral-small-latest"), Some("mistral-small-latest")), "Deep falls back to Quick");
        let (q, d) = default_models(ServiceKind::Mistral, &[]);
        assert_eq!((q.as_deref(), d.as_deref()), (Some("mistral-small-latest"), Some("mistral-large-latest")), "before the list arrives");
        let m = ids(&["some-new-model"]);
        assert_eq!(default_models(ServiceKind::Groq, &m).0.as_deref(), Some("some-new-model"), "nothing known: the size rule");
    }

    #[test]
    fn every_kind_has_its_own_slug_and_keyed_ones_a_page() {
        let slugs: std::collections::HashSet<_> = ServiceKind::ALL.iter().map(|k| k.slug()).collect();
        assert_eq!(slugs.len(), ServiceKind::ALL.len());
        for k in ServiceKind::ALL {
            assert_eq!(k.key_page().is_some(), k.needs_key() && k != ServiceKind::OpenRouter, "{k:?}");
            assert!(k == ServiceKind::Custom || k.default_base_url().starts_with("http"), "{k:?}");
        }
    }

    #[test]
    fn patterns_take_versions_and_anything() {
        let g = |p: &str, s: &str| glob(p.as_bytes(), s.as_bytes());
        assert!(g("gpt-#", "gpt-5.1") && g("gpt-#", "gpt-5") && !g("gpt-#", "gpt-5-mini") && !g("gpt-#", "gpt-image"));
        assert!(g("gpt-#-mini", "gpt-5-mini") && !g("gpt-#-mini", "gpt-mini"));
        assert!(g("moonshotai/kimi-k#*", "moonshotai/kimi-k2-instruct-0905"));
        assert!(g("gemini-#-pro*", "gemini-3-pro-preview") && !g("gemini-#-pro*", "gemini-pro-latest"));
    }

    #[test]
    fn readable_names_for_bare_ids() {
        assert_eq!(pretty_id("gemini-2.5-flash"), "Gemini 2.5 Flash");
        assert_eq!(pretty_id("gemini-flash-lite-latest"), "Gemini Flash Lite");
        assert_eq!(pretty_id("gemini-2.0-flash-001"), "Gemini 2.0 Flash");
        assert_eq!(pretty_id("moonshotai/kimi-k2-instruct-0905"), "Kimi K2");
        assert_eq!(pretty_id("openai/gpt-oss-120b"), "GPT OSS 120B");
        assert_eq!(pretty_id("llama-3.3-70b-versatile"), "Llama 3.3 70B");
        assert_eq!(pretty_id("gemma-3n-e4b-it"), "Gemma 3n E4B");
        assert_eq!(pretty_id("qwen/qwen3-32b"), "Qwen3 32B");
        assert_eq!(pretty_id("mistral-small-latest"), "Mistral Small");
        assert_eq!(pretty_id(OPUS), "Claude Opus 5.5");
    }

    #[test]
    fn preset_lists_read_newest_first_family_by_family() {
        let ids = ["gemini-2.0-flash", "gemini-2.5-pro", "gemini-3-flash", "gemini-flash-latest", "gemini-2.5-flash", "gemma-3-4b-it", "gemma-3-27b-it"];
        let m = polish(ServiceKind::Gemini, ids.iter().map(|id| ModelInfo { id: id.to_string(), name: id.to_string(), price_in: None, price_out: None }).collect());
        let names: Vec<&str> = m.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Gemini Flash", "Gemini 3 Flash", "Gemini 2.5 Flash", "Gemini 2.0 Flash", "Gemini 2.5 Pro", "Gemma 3 27B", "Gemma 3 4B"]);
        let local = polish(ServiceKind::Ollama, vec![ModelInfo { id: "qwen3:14b".into(), name: "qwen3:14b".into(), price_in: None, price_out: None }]);
        assert_eq!(local[0].name, "qwen3:14b", "local names are left alone");
    }

    #[test]
    fn suggested_rows_resolve_against_the_list() {
        let ids = |ids: &[&str]| ids.iter().map(|id| ModelInfo { id: id.to_string(), name: pretty_id(id), price_in: None, price_out: None }).collect::<Vec<_>>();
        let m = ids(&["gemini-flash-latest", "gemini-pro-latest", "gemini-flash-lite-latest", "gemini-2.5-flash"]);
        let s = suggested(ServiceKind::Gemini, &m);
        let rows: Vec<(&str, &str, Role)> = s.iter().map(|s| (s.id.as_str(), s.name.as_str(), s.role)).collect();
        assert_eq!(rows, [("gemini-flash-latest", "Gemini Flash", Role::Quick), ("gemini-flash-lite-latest", "Gemini Flash Lite", Role::Quickest), ("gemini-pro-latest", "Gemini Pro", Role::Deep)]);
        // Groq without Kimi: Deep falls to gpt-oss-120b, already the Quick row, so it isn't listed twice.
        let s = suggested(ServiceKind::Groq, &ids(&["openai/gpt-oss-120b", "llama-3.1-8b-instant"]));
        assert_eq!(s.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["openai/gpt-oss-120b", "llama-3.1-8b-instant"]);
        let s = suggested(ServiceKind::OpenAi, &ids(&["gpt-4o", "gpt-5", "gpt-5.1", "gpt-5-mini", "gpt-5-nano", "gpt-image-1"]));
        assert_eq!(s.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["gpt-5-mini", "gpt-5-nano", "gpt-5.1"]);
        assert_eq!(suggested(ServiceKind::OpenRouter, &[]).iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), [SONNET, HAIKU, OPUS], "known before the list");
        assert!(suggested(ServiceKind::Ollama, &ids(&["llama3.2:3b"])).is_empty());
    }

    #[test]
    fn openrouter_defaults_are_sonnet_and_opus() {
        let (q, d) = default_models(ServiceKind::OpenRouter, &[]);
        assert_eq!((q.as_deref(), d.as_deref()), (Some(SONNET), Some(OPUS)));
    }

    #[test]
    fn short_address() {
        assert_eq!(Service::new(ServiceKind::Ollama).short_address(), "127.0.0.1:11434");
        let s = Service { kind: ServiceKind::Custom, base_url: "https://example.com/api/v1/".into() };
        assert_eq!(s.base(), "https://example.com/api/v1/");
        assert_eq!(s.short_address(), "example.com");
    }

    #[test]
    fn display_names() {
        let r = ModelRef { service: ServiceKind::OpenRouter, model: OPUS.into() };
        assert_eq!(display_name(&r, &[]), "Claude Opus 5.5");
        let r = ModelRef { service: ServiceKind::Ollama, model: "qwen3:14b".into() };
        assert_eq!(display_name(&r, &[]), "qwen3:14b");
    }
}
