//! AI for Dostoevsky, no UI. Replies anchor to quoted sentences (anything that doesn't is
//! dropped) and come back as suggestions with ranges; nothing rewrites prose in place.

pub mod anchor;
pub mod client;
pub mod cost;
pub mod ideas;
pub mod lab;
pub mod oauth;
pub mod secrets;
pub mod segment;
pub mod service;
pub mod trim;

pub use client::{AiError, ChatRequest, Endpoint};
pub use service::{ModelInfo, ModelRef, Service, ServiceKind};

/// Models sometimes wrap the JSON in a code fence or a sentence.
pub fn json_object(reply: &str) -> Option<serde_json::Value> {
    let start = reply.find('{')?;
    let end = reply.rfind('}')?;
    if end < start {
        return None;
    }
    serde_json::from_str(&reply[start..=end]).ok()
}

#[cfg(test)]
mod tests {
    use super::json_object;

    #[test]
    fn finds_the_object_in_a_fenced_reply() {
        let v = json_object("Sure!\n```json\n{\"marks\": []}\n```").unwrap();
        assert!(v["marks"].as_array().unwrap().is_empty());
        assert!(json_object("no json here").is_none());
    }
}
