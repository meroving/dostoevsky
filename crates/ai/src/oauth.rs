use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use base64::Engine as _;
use l10n::tr;
use sha2::{Digest, Sha256};

use crate::client::AiError;

pub const AUTH_URL: &str = "https://openrouter.ai/auth";
pub const KEYS_URL: &str = "https://openrouter.ai/api/v1/auth/keys";

pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

fn random_bytes(n: usize) -> Vec<u8> {
    let mut b = vec![0u8; n];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).is_err() {
        // Not expected on macOS; still unpredictable enough for a one-off verifier.
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let mut h = Sha256::new();
        h.update(t.to_le_bytes());
        h.update(std::process::id().to_le_bytes());
        let d = h.finalize();
        for (i, x) in b.iter_mut().enumerate() {
            *x = d[i % d.len()];
        }
    }
    b
}

pub fn pkce() -> Pkce {
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let verifier = b64.encode(random_bytes(48));
    let challenge = b64.encode(Sha256::digest(verifier.as_bytes()));
    Pkce { verifier, challenge }
}

pub fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Without a callback, OpenRouter shows the code to paste.
pub fn auth_url(callback: Option<&str>, challenge: &str) -> String {
    let mut u = format!("{AUTH_URL}?code_challenge={}&code_challenge_method=S256", encode(challenge));
    if let Some(cb) = callback {
        u.push_str(&format!("&callback_url={}", encode(cb)));
    }
    u
}

pub struct Callback {
    listener: TcpListener,
    pub port: u16,
}

impl Callback {
    pub fn listen() -> std::io::Result<Callback> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        Ok(Callback { listener, port })
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/callback", self.port)
    }

    /// Other requests (a favicon) get a 404 and waiting goes on.
    pub fn wait(&self, cancel: &AtomicBool, timeout: Duration) -> Result<String, AiError> {
        let start = Instant::now();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(AiError::Cancelled);
            }
            if start.elapsed() > timeout {
                return Err(AiError::Network("timed out waiting for the browser".into()));
            }
            match self.listener.accept() {
                Ok((mut s, _)) => {
                    let _ = s.set_nonblocking(false);
                    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
                    let mut buf = [0u8; 8192];
                    let n = s.read(&mut buf).unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let path = req.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("").to_string();
                    match code_from_path(&path) {
                        Some(Ok(code)) => {
                            respond(&mut s, 200, tr!("Dostoevsky is connected to OpenRouter. You can close this tab."));
                            return Ok(code);
                        }
                        Some(Err(e)) => {
                            respond(&mut s, 200, tr!("OpenRouter didn't authorize Dostoevsky. You can close this tab."));
                            return Err(e);
                        }
                        None => respond(&mut s, 404, "Not found"),
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(80)),
                Err(e) => return Err(AiError::Network(e.to_string())),
            }
        }
    }
}

fn code_from_path(path: &str) -> Option<Result<String, AiError>> {
    let (p, q) = path.split_once('?').unwrap_or((path, ""));
    if p != "/callback" {
        return None;
    }
    let mut code = None;
    let mut error = None;
    for kv in q.split('&') {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        match k {
            "code" => code = Some(decode(v)),
            "error" => error = Some(decode(v)),
            _ => {}
        }
    }
    Some(match (code, error) {
        (Some(c), _) if !c.is_empty() => Ok(c),
        (_, Some(e)) => Err(AiError::Server(400, e)),
        _ => Err(AiError::BadReply("no code".into())),
    })
}

fn respond(s: &mut std::net::TcpStream, status: u16, msg: &str) {
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>Dostoevsky</title><body style=\"font:16px -apple-system,sans-serif;background:#070c1f;color:#f5cab3;display:grid;place-items:center;height:90vh\"><p>{msg}</p>"
    );
    let reason = if status == 200 { "OK" } else { "Not Found" };
    let _ = write!(s, "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
}

pub fn exchange(code: &str, verifier: &str) -> Result<String, AiError> {
    exchange_at(KEYS_URL, code, verifier)
}

pub fn exchange_at(url: &str, code: &str, verifier: &str) -> Result<String, AiError> {
    let body = serde_json::json!({"code": code.trim(), "code_verifier": verifier, "code_challenge_method": "S256"}).to_string();
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(20))).http_status_as_error(false).build().into();
    let mut resp = agent
        .post(url)
        .header("Content-Type", "application/json")
        .send(body.as_bytes())
        .map_err(|e| AiError::Network(e.to_string()))?;
    let code = resp.status().as_u16();
    let text = resp.body_mut().read_to_string().unwrap_or_default();
    if code == 400 || code == 403 {
        return Err(AiError::Auth);
    }
    if code >= 400 {
        return Err(AiError::Server(code, text));
    }
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| AiError::BadReply(text.clone()))?;
    v["key"].as_str().filter(|k| !k.is_empty()).map(str::to_string).ok_or(AiError::BadReply("no key".into()))
}

pub fn open_in_browser(url: &str) -> std::io::Result<()> {
    std::process::Command::new("open").arg(url).spawn().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_is_s256_of_the_verifier() {
        let p = pkce();
        assert!(p.verifier.len() >= 43);
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        assert_eq!(p.challenge, b64.encode(Sha256::digest(p.verifier.as_bytes())));
        assert_ne!(pkce().verifier, p.verifier);
    }

    #[test]
    fn the_auth_url() {
        let u = auth_url(Some("http://127.0.0.1:5000/callback"), "abc-_");
        assert_eq!(u, "https://openrouter.ai/auth?code_challenge=abc-_&code_challenge_method=S256&callback_url=http%3A%2F%2F127.0.0.1%3A5000%2Fcallback");
        assert!(!auth_url(None, "x").contains("callback_url"));
    }

    #[test]
    fn the_browser_comes_back_with_a_code() {
        let cb = Callback::listen().unwrap();
        let port = cb.port;
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /favicon.ico HTTP/1.1\r\n\r\n").unwrap();
            let mut r = String::new();
            let _ = s.read_to_string(&mut r);
            let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /callback?code=a%2Bb HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
            let mut r = String::new();
            let _ = s.read_to_string(&mut r);
            r
        });
        let code = cb.wait(&AtomicBool::new(false), Duration::from_secs(5)).unwrap();
        assert_eq!(code, "a+b");
        assert!(t.join().unwrap().contains("You can close this tab"));
    }

    #[test]
    fn cancelling_stops_waiting() {
        let cb = Callback::listen().unwrap();
        assert_eq!(cb.wait(&AtomicBool::new(true), Duration::from_secs(5)), Err(AiError::Cancelled));
        assert!(matches!(code_from_path("/callback?error=access_denied"), Some(Err(AiError::Server(400, _)))));
    }
}
