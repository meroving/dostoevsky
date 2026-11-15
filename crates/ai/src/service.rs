use l10n::tr;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceKind {
    OpenRouter,
    OpenAi,
    Ollama,
    LmStudio,
    Custom,
}

impl ServiceKind {
    pub const ALL: [ServiceKind; 5] = [ServiceKind::OpenRouter, ServiceKind::OpenAi, ServiceKind::Ollama, ServiceKind::LmStudio, ServiceKind::Custom];

    pub fn label(self) -> &'static str {
        match self {
            ServiceKind::OpenRouter => "OpenRouter",
            ServiceKind::OpenAi => "OpenAI",
            ServiceKind::Ollama => "Ollama",
            ServiceKind::LmStudio => "LM Studio",
            ServiceKind::Custom => tr!("Custom"),
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            ServiceKind::OpenRouter => tr!("one account, many models"),
            ServiceKind::OpenAi => tr!("API key"),
            ServiceKind::Ollama | ServiceKind::LmStudio => tr!("on this Mac"),
            ServiceKind::Custom => tr!("any compatible address"),
        }
    }

    pub fn default_base_url(self) -> &'static str {
        match self {
            ServiceKind::OpenRouter => "https://openrouter.ai/api/v1",
            ServiceKind::OpenAi => "https://api.openai.com/v1",
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
        matches!(self, ServiceKind::OpenRouter | ServiceKind::OpenAi)
    }

    /// The Keychain account its key is stored under.
    pub fn slug(self) -> &'static str {
        match self {
            ServiceKind::OpenRouter => "openrouter",
            ServiceKind::OpenAi => "openai",
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
                    let id = m["id"].as_str().or_else(|| m["name"].as_str())?.to_string();
                    let name = m["name"].as_str().filter(|n| !n.is_empty() && *n != id).map(clean_name).unwrap_or_else(|| id.clone());
                    Some(ModelInfo { name, price_in: price(&m["pricing"]["prompt"]), price_out: price(&m["pricing"]["completion"]), id })
                })
                .collect()
        })
        .unwrap_or_default();
    // Embedding and image models can't write; batch variants don't stream.
    out.retain(|m| {
        let id = m.id.to_lowercase();
        !id.contains("embed") && !id.contains("whisper") && !id.contains("tts") && !id.contains("dall-e") && !id.ends_with(":batch")
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Suggested {
    pub id: &'static str,
    pub name: &'static str,
}

impl Suggested {
    pub fn blurb(&self) -> &'static str {
        match self.id {
            SONNET => tr!("quick, good with words"),
            HAIKU => tr!("quickest and cheapest"),
            OPUS => tr!("best judgement, slower"),
            _ => "",
        }
    }
}

pub const SONNET: &str = "anthropic/claude-sonnet-5.5";
pub const OPUS: &str = "anthropic/claude-opus-5.5";
pub const HAIKU: &str = "anthropic/claude-haiku-5.5";

pub fn suggested(kind: ServiceKind) -> &'static [Suggested] {
    const OR: [Suggested; 3] = [
        Suggested { id: SONNET, name: "Claude Sonnet 5.5" },
        Suggested { id: HAIKU, name: "Claude Haiku 5.5" },
        Suggested { id: OPUS, name: "Claude Opus 5.5" },
    ];
    match kind {
        ServiceKind::OpenRouter => &OR,
        _ => &[],
    }
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
    let has = |id: &str| models.is_empty() || models.iter().any(|m| m.id == id);
    match kind {
        ServiceKind::OpenRouter => (has(SONNET).then(|| SONNET.to_string()), has(OPUS).then(|| OPUS.to_string())),
        _ => {
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
    }
}

pub fn display_name(r: &ModelRef, models: &[ModelInfo]) -> String {
    if let Some(s) = suggested(r.service).iter().find(|s| s.id == r.model) {
        return s.name.to_string();
    }
    models.iter().find(|m| m.id == r.model).map(|m| m.name.clone()).unwrap_or_else(|| r.model.clone())
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
