//! Keys in the macOS Keychain; `DOSTOEVSKY_SECRETS=memory` (or `cfg(test)`) keeps them in memory.

use std::cell::RefCell;
use std::collections::HashMap;

const KEYCHAIN_SERVICE: &str = "Dostoevsky AI";

thread_local! {
    /// Per thread, so tests running in parallel don't see each other's keys.
    static MEMORY: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

fn in_memory() -> bool {
    cfg!(test) || std::env::var("DOSTOEVSKY_SECRETS").is_ok_and(|v| v == "memory") || !cfg!(target_os = "macos")
}

pub fn get(account: &str) -> Option<String> {
    if in_memory() {
        return MEMORY.with(|m| m.borrow().get(account).cloned());
    }
    keychain::get(KEYCHAIN_SERVICE, account)
}

pub fn set(account: &str, secret: &str) -> Result<(), String> {
    if in_memory() {
        MEMORY.with(|m| m.borrow_mut().insert(account.to_string(), secret.to_string()));
        return Ok(());
    }
    keychain::set(KEYCHAIN_SERVICE, account, secret)
}

pub fn delete(account: &str) {
    if in_memory() {
        MEMORY.with(|m| m.borrow_mut().remove(account));
        return;
    }
    keychain::delete(KEYCHAIN_SERVICE, account);
}

#[cfg(target_os = "macos")]
mod keychain {
    use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};

    pub fn get(service: &str, account: &str) -> Option<String> {
        get_generic_password(service, account).ok().and_then(|b| String::from_utf8(b).ok())
    }

    pub fn set(service: &str, account: &str, secret: &str) -> Result<(), String> {
        set_generic_password(service, account, secret.as_bytes()).map_err(|e| e.to_string())
    }

    pub fn delete(service: &str, account: &str) {
        let _ = delete_generic_password(service, account);
    }
}

#[cfg(not(target_os = "macos"))]
mod keychain {
    pub fn get(_: &str, _: &str) -> Option<String> {
        None
    }
    pub fn set(_: &str, _: &str, _: &str) -> Result<(), String> {
        Err("no keychain".into())
    }
    pub fn delete(_: &str, _: &str) {}
}

#[cfg(test)]
mod tests {
    #[test]
    fn memory_round_trip() {
        super::set("test-account", "sk-1").unwrap();
        assert_eq!(super::get("test-account").as_deref(), Some("sk-1"));
        super::delete("test-account");
        assert_eq!(super::get("test-account"), None);
    }
}
