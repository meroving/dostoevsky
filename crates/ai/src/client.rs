//! Blocking (ureq): the app runs it on a background executor, so no async runtime is started.

use std::io::{BufRead, BufReader, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use l10n::tr;
use serde_json::{json, Value};

use crate::service::{parse_models, ModelInfo, ServiceKind};

#[derive(Clone, Debug)]
pub struct Endpoint {
    pub kind: ServiceKind,
    /// `https://openrouter.ai/api/v1`: `/chat/completions` and `/models` are added.
    pub base: String,
    pub key: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct ChatRequest {
    pub model: String,
    pub system: String,
    pub user: String,
    /// (name, JSON schema): `response_format` where the server takes it; replies are checked anyway.
    pub schema: Option<(String, Value)>,
    pub max_tokens: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AiError {
    NotSetUp,
    /// The key was refused.
    Auth,
    NoCredits,
    /// Too many requests, or a free model's daily limit.
    RateLimited,
    /// The model declined the passage.
    Refused,
    /// `max_tokens` ran out before any answer: a reasoning model spent it all thinking.
    OutOfRoom,
    LocalNotRunning,
    Network(String),
    Server(u16, String),
    BadReply(String),
    Cancelled,
}

impl AiError {
    pub fn message(&self, service: ServiceKind) -> String {
        let s = service.label();
        match self {
            AiError::NotSetUp => tr!("No AI is set up yet.").into(),
            AiError::Auth => tr!("{s} didn't accept the key. Connect again in Preferences → AI.", s = s),
            AiError::NoCredits => tr!("{s}: out of credits.", s = s),
            // Free models and free tiers have daily limits.
            AiError::RateLimited if matches!(service, ServiceKind::OpenRouter | ServiceKind::Gemini | ServiceKind::Groq) => tr!("The limit for now is used up. Try again later or pick another model.").into(),
            AiError::RateLimited => tr!("{s} is busy. Try again in a moment.", s = s),
            AiError::Refused => tr!("The model declined this passage.").into(),
            AiError::OutOfRoom => tr!("The model spent its whole answer thinking. Try again or pick another model.").into(),
            AiError::LocalNotRunning => tr!("{s} isn't running.", s = s),
            AiError::Network(_) => tr!("No connection.").into(),
            AiError::Server(code, msg) if msg.is_empty() => tr!("{s} answered with an error ({code}).", s = s, code = code),
            AiError::Server(_, msg) => tr!("{s}: {message}", s = s, message = first_line(msg)),
            AiError::BadReply(_) => tr!("The model's answer couldn't be used.").into(),
            AiError::Cancelled => tr!("Stopped.").into(),
        }
    }
}

fn first_line(s: &str) -> String {
    let l = s.lines().next().unwrap_or("").trim();
    if l.chars().count() > 90 {
        l.chars().take(90).collect::<String>() + "…"
    } else {
        l.to_string()
    }
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .timeout_connect(Some(Duration::from_secs(8)))
        .http_status_as_error(false)
        .build()
        .into()
}

fn url(base: &str, path: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), path.trim_start_matches('/'))
}

fn transport_error(kind: ServiceKind, e: ureq::Error) -> AiError {
    let text = e.to_string();
    let refused = matches!(&e, ureq::Error::Io(io) if io.kind() == std::io::ErrorKind::ConnectionRefused)
        || text.to_lowercase().contains("connection refused");
    if refused && kind.local() {
        AiError::LocalNotRunning
    } else {
        AiError::Network(text)
    }
}

fn status_error(code: u16, body: &str) -> AiError {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let msg = v["error"]["message"].as_str().or_else(|| v["error"].as_str()).or_else(|| v["message"].as_str()).unwrap_or("").to_string();
    match code {
        401 | 403 => AiError::Auth,
        402 => AiError::NoCredits,
        429 => AiError::RateLimited,
        _ => AiError::Server(code, msg),
    }
}

/// Room for a reasoning model's thinking where `max_tokens` counts it too.
const THINKING_ROOM: u32 = 2000;

/// What a request carries beyond the messages; each is dropped when a server refuses it.
#[derive(Clone, Copy)]
struct Extras {
    schema: bool,
    effort: bool,
}

fn body_json(kind: ServiceKind, req: &ChatRequest, stream: bool, extras: Extras) -> Value {
    let mut b = json!({
        "model": req.model,
        "messages": [
            {"role": "system", "content": req.system},
            {"role": "user", "content": req.user},
        ],
        "stream": stream,
    });
    let openrouter = kind == ServiceKind::OpenRouter;
    let effort = extras.effort && kind.effort_knob();
    if let Some(n) = req.max_tokens {
        b["max_tokens"] = json!(if openrouter || effort { n + THINKING_ROOM } else { n });
    }
    if effort {
        // The lowest effort Gemini 3 Pro and gpt-oss take; a model without the knob refuses it
        // (Kimi K2 on Groq) and is asked again without.
        b["reasoning_effort"] = json!("low");
    }
    if openrouter {
        // Left alone, reasoning models (GLM, DeepSeek, Kimi…) can think through the whole
        // `max_tokens` and answer nothing. "low" is the lowest effort every one takes (some refuse
        // "none"); non-reasoning models ignore it. `exclude`: the thinking isn't streamed back.
        b["reasoning"] = json!({"effort": "low", "exclude": true});
    }
    if let (true, Some((name, schema))) = (extras.schema, &req.schema) {
        b["response_format"] = json!({"type": "json_schema", "json_schema": {"name": name, "strict": true, "schema": schema}});
    }
    b
}

/// Returns the whole reply; `cancel` is checked between pieces.
pub fn chat_stream(ep: &Endpoint, req: &ChatRequest, cancel: &AtomicBool, mut on_text: impl FnMut(&str)) -> Result<String, AiError> {
    let mut extras = Extras { schema: req.schema.is_some(), effort: ep.kind.effort_knob() };
    loop {
        match send(ep, req, cancel, extras, &mut on_text) {
            // Some servers refuse `response_format`: retry without it (the prompt describes the shape).
            Err(AiError::Server(400 | 422, msg)) if extras.schema && mentions_format(&msg) => extras.schema = false,
            Err(AiError::Server(400 | 422, msg)) if extras.effort && msg.to_lowercase().contains("reasoning") => extras.effort = false,
            r => return r,
        }
    }
}

pub fn chat(ep: &Endpoint, req: &ChatRequest, cancel: &AtomicBool) -> Result<String, AiError> {
    chat_stream(ep, req, cancel, |_| {})
}

fn mentions_format(msg: &str) -> bool {
    let m = msg.to_lowercase();
    // Gemini names its own field: "… at 'generation_config.response_schema'".
    m.contains("response_format") || m.contains("json_schema") || m.contains("response_schema") || m.contains("structured")
}

fn send(ep: &Endpoint, req: &ChatRequest, cancel: &AtomicBool, extras: Extras, on_text: &mut dyn FnMut(&str)) -> Result<String, AiError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(AiError::Cancelled);
    }
    let body = body_json(ep.kind, req, true, extras).to_string();
    let mut r = agent(Duration::from_secs(300))
        .post(&url(&ep.base, "chat/completions"))
        .header("Content-Type", "application/json")
        .header("Accept", "text/event-stream")
        .header("X-Title", "Dostoevsky");
    if let Some(k) = ep.key.as_deref().filter(|k| !k.is_empty()) {
        r = r.header("Authorization", &format!("Bearer {k}"));
    }
    let mut resp = r.send(body.as_bytes()).map_err(|e| transport_error(ep.kind, e))?;
    let code = resp.status().as_u16();
    let is_sse = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).is_some_and(|t| t.contains("event-stream"));
    if code >= 400 {
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        return Err(status_error(code, &text));
    }
    if !is_sse {
        // The server ignored `stream`: one JSON reply.
        let text = resp.body_mut().read_to_string().map_err(|e| AiError::Network(e.to_string()))?;
        let v: Value = serde_json::from_str(&text).map_err(|_| AiError::BadReply(first_line(&text)))?;
        if let Some(e) = v.get("error") {
            return Err(status_error(e["code"].as_u64().unwrap_or(500) as u16, &v.to_string()));
        }
        let choice = &v["choices"][0];
        check_finish(choice)?;
        let content = choice["message"]["content"].as_str().unwrap_or("").to_string();
        on_text(&content);
        return finish(content, choice["finish_reason"].as_str() == Some("length"));
    }
    read_sse(resp.body_mut().as_reader(), cancel, on_text)
}

fn check_finish(choice: &Value) -> Result<(), AiError> {
    match choice["finish_reason"].as_str() {
        Some("content_filter") | Some("refusal") => Err(AiError::Refused),
        _ => Ok(()),
    }
}

fn finish(text: String, cut_short: bool) -> Result<String, AiError> {
    if text.trim().is_empty() && cut_short {
        Err(AiError::OutOfRoom)
    } else if text.trim().is_empty() {
        Err(AiError::Refused)
    } else {
        Ok(text)
    }
}

pub fn read_sse(reader: impl Read, cancel: &AtomicBool, on_text: &mut dyn FnMut(&str)) -> Result<String, AiError> {
    let mut out = String::new();
    let mut refusal = false;
    let mut cut_short = false;
    for line in BufReader::new(reader).lines() {
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Cancelled);
        }
        let line = line.map_err(|e| AiError::Network(e.to_string()))?;
        let Some(data) = line.strip_prefix("data:") else { continue };
        let data = data.trim();
        if data == "[DONE]" {
            break;
        }
        let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
        if let Some(e) = v.get("error") {
            let code = e["code"].as_u64().unwrap_or(500) as u16;
            return Err(status_error(code, &json!({"error": e}).to_string()));
        }
        let choice = &v["choices"][0];
        if choice["delta"]["refusal"].as_str().is_some_and(|r| !r.is_empty()) {
            refusal = true;
        }
        if let Some(t) = choice["delta"]["content"].as_str() {
            if !t.is_empty() {
                out.push_str(t);
                on_text(t);
            }
        }
        check_finish(choice)?;
        if choice["finish_reason"].as_str() == Some("length") {
            cut_short = true;
        }
    }
    if refusal && out.trim().is_empty() {
        return Err(AiError::Refused);
    }
    finish(out, cut_short)
}

pub fn list_models(ep: &Endpoint) -> Result<Vec<ModelInfo>, AiError> {
    let mut r = agent(Duration::from_secs(20)).get(&url(&ep.base, "models"));
    if let Some(k) = ep.key.as_deref().filter(|k| !k.is_empty()) {
        r = r.header("Authorization", &format!("Bearer {k}"));
    }
    let mut resp = r.call().map_err(|e| transport_error(ep.kind, e))?;
    let code = resp.status().as_u16();
    let text = resp.body_mut().read_to_string().map_err(|e| AiError::Network(e.to_string()))?;
    if code >= 400 {
        return Err(status_error(code, &text));
    }
    let v: Value = serde_json::from_str(&text).map_err(|_| AiError::BadReply(first_line(&text)))?;
    Ok(parse_models(&v))
}

/// Returns how long the first piece took.
pub fn ping(ep: &Endpoint, model: &str) -> Result<Duration, AiError> {
    let t = Instant::now();
    let mut first = None;
    let req = ChatRequest {
        model: model.to_string(),
        system: "Reply with one word.".into(),
        user: "Say ok.".into(),
        schema: None,
        max_tokens: Some(16),
    };
    chat_stream(ep, &req, &AtomicBool::new(false), |_| {
        first.get_or_insert(t.elapsed());
    })?;
    Ok(first.unwrap_or(t.elapsed()))
}

pub fn local_running(base: &str) -> bool {
    let Some(host) = base.split("://").nth(1).and_then(|h| h.split('/').next()) else { return false };
    let Ok(mut addrs) = std::net::ToSocketAddrs::to_socket_addrs(host) else { return false };
    addrs.any(|a| std::net::TcpStream::connect_timeout(&a, Duration::from_millis(250)).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    #[test]
    fn sse_pieces_and_done() {
        let s = "data: {\"choices\":[{\"delta\":{\"content\":\"mar\"}}]}\n\n: keep-alive\ndata: {\"choices\":[{\"delta\":{\"content\":\"gin\"}}]}\n\ndata: [DONE]\n";
        let mut pieces = Vec::new();
        let out = read_sse(s.as_bytes(), &AtomicBool::new(false), &mut |t| pieces.push(t.to_string())).unwrap();
        assert_eq!(out, "margin");
        assert_eq!(pieces, ["mar", "gin"]);
    }

    #[test]
    fn sse_refusal_and_errors() {
        let s = "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"content_filter\"}]}\n";
        assert_eq!(read_sse(s.as_bytes(), &AtomicBool::new(false), &mut |_| {}), Err(AiError::Refused));
        let s = "data: {\"error\":{\"code\":402,\"message\":\"Insufficient credits\"}}\n";
        assert_eq!(read_sse(s.as_bytes(), &AtomicBool::new(false), &mut |_| {}), Err(AiError::NoCredits));
        assert_eq!(read_sse("data: [DONE]\n".as_bytes(), &AtomicBool::new(false), &mut |_| {}), Err(AiError::Refused));
        // All of `max_tokens` went on thinking (GLM-5.3 on OpenRouter): not a refusal.
        let s = "data: {\"choices\":[{\"delta\":{\"content\":\"\"},\"finish_reason\":\"length\"}]}\n";
        assert_eq!(read_sse(s.as_bytes(), &AtomicBool::new(false), &mut |_| {}), Err(AiError::OutOfRoom));
    }

    #[test]
    fn openrouter_gets_low_reasoning_and_room_to_think() {
        let req = ChatRequest { model: "m".into(), max_tokens: Some(200), ..Default::default() };
        let all = Extras { schema: true, effort: true };
        let b = body_json(ServiceKind::OpenRouter, &req, true, all);
        assert_eq!(b["reasoning"], json!({"effort": "low", "exclude": true}));
        assert_eq!(b["max_tokens"], json!(200 + THINKING_ROOM));
        let b = body_json(ServiceKind::OpenAi, &req, true, all);
        assert!(b.get("reasoning").is_none() && b.get("reasoning_effort").is_none());
        assert_eq!(b["max_tokens"], json!(200));
        for kind in [ServiceKind::Gemini, ServiceKind::Groq] {
            let b = body_json(kind, &req, true, all);
            assert_eq!(b["reasoning_effort"], json!("low"), "{kind:?}");
            assert_eq!(b["max_tokens"], json!(200 + THINKING_ROOM));
            let b = body_json(kind, &req, true, Extras { schema: true, effort: false });
            assert!(b.get("reasoning_effort").is_none());
            assert_eq!(b["max_tokens"], json!(200));
        }
        assert!(body_json(ServiceKind::Mistral, &req, true, all).get("reasoning_effort").is_none(), "Mistral has no knob");
    }

    #[test]
    fn status_codes() {
        assert_eq!(status_error(401, ""), AiError::Auth);
        assert_eq!(status_error(429, ""), AiError::RateLimited);
        assert_eq!(status_error(500, "{\"error\":{\"message\":\"boom\"}}"), AiError::Server(500, "boom".into()));
    }

    fn serve(replies: Vec<String>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1", l.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for reply in replies {
                let (mut s, _) = l.accept().unwrap();
                let mut buf = vec![0u8; 65536];
                let mut req = Vec::new();
                loop {
                    let n = s.read(&mut buf).unwrap();
                    req.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&req).to_string();
                    if let Some(i) = text.find("\r\n\r\n") {
                        let len = text[..i]
                            .lines()
                            .find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                            .unwrap_or(0);
                        if req.len() >= i + 4 + len {
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                seen.push(String::from_utf8_lossy(&req).to_string());
                s.write_all(reply.as_bytes()).unwrap();
            }
            seen
        });
        (base, h)
    }

    fn http(status: &str, ctype: &str, body: &str) -> String {
        format!("HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
    }

    #[test]
    fn streams_from_a_server_and_sends_the_key_and_schema() {
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"{\\\"marks\\\":\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"[]}\"}}]}\n\ndata: [DONE]\n\n";
        let (base, h) = serve(vec![http("200 OK", "text/event-stream", sse)]);
        let ep = Endpoint { kind: ServiceKind::OpenRouter, base, key: Some("sk-test".into()) };
        let req = ChatRequest { model: "m".into(), system: "s".into(), user: "u".into(), schema: Some(("marks".into(), json!({"type": "object"}))), max_tokens: None };
        let out = chat(&ep, &req, &AtomicBool::new(false)).unwrap();
        assert_eq!(out, "{\"marks\":[]}");
        let seen = h.join().unwrap();
        assert!(seen[0].starts_with("POST /v1/chat/completions"));
        assert!(seen[0].to_lowercase().contains("authorization: bearer sk-test"));
        assert!(seen[0].contains("\"response_format\""));
    }

    #[test]
    fn retries_without_response_format_when_refused() {
        let (base, h) = serve(vec![
            http("400 Bad Request", "application/json", "{\"error\":{\"message\":\"response_format is not supported\"}}"),
            http("200 OK", "application/json", "{\"choices\":[{\"message\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}"),
        ]);
        let ep = Endpoint { kind: ServiceKind::Ollama, base, key: None };
        let req = ChatRequest { model: "m".into(), schema: Some(("x".into(), json!({}))), ..Default::default() };
        assert_eq!(chat(&ep, &req, &AtomicBool::new(false)).unwrap(), "ok");
        let seen = h.join().unwrap();
        assert!(seen[0].contains("response_format"));
        assert!(!seen[1].contains("response_format"));
    }

    #[test]
    fn retries_without_reasoning_effort_then_without_the_schema() {
        let (base, h) = serve(vec![
            http("400 Bad Request", "application/json", r#"{"error":{"message":"`reasoning_effort` is not supported with this model"}}"#),
            http("400 Bad Request", "application/json", r#"{"error":{"message":"Invalid JSON payload received. Unknown name \"additionalProperties\" at 'generation_config.response_schema'"}}"#),
            http("200 OK", "application/json", r#"{"choices":[{"message":{"content":"ok"},"finish_reason":"stop"}]}"#),
        ]);
        let ep = Endpoint { kind: ServiceKind::Groq, base, key: Some("gsk".into()) };
        let req = ChatRequest { model: "m".into(), schema: Some(("x".into(), json!({}))), ..Default::default() };
        assert_eq!(chat(&ep, &req, &AtomicBool::new(false)).unwrap(), "ok");
        let seen = h.join().unwrap();
        assert!(seen[0].contains("reasoning_effort") && seen[0].contains("response_format"));
        assert!(!seen[1].contains("reasoning_effort") && seen[1].contains("response_format"));
        assert!(!seen[2].contains("reasoning_effort") && !seen[2].contains("response_format"));
    }

    #[test]
    fn models_and_auth_errors() {
        let (base, h) = serve(vec![
            http("200 OK", "application/json", "{\"data\":[{\"id\":\"llama3.1:8b\"}]}"),
            http("401 Unauthorized", "application/json", "{\"error\":{\"message\":\"bad key\"}}"),
        ]);
        let ep = Endpoint { kind: ServiceKind::Ollama, base, key: None };
        assert_eq!(list_models(&ep).unwrap()[0].id, "llama3.1:8b");
        assert_eq!(list_models(&ep), Err(AiError::Auth));
        h.join().unwrap();
    }

    #[test]
    fn a_local_server_that_isnt_running() {
        // Bind and drop to get a port nobody listens on.
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let base = format!("http://127.0.0.1:{port}/v1");
        assert!(!local_running(&base));
        let ep = Endpoint { kind: ServiceKind::Ollama, base, key: None };
        assert_eq!(list_models(&ep), Err(AiError::LocalNotRunning));
    }
}
