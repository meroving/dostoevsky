//! Keys in the macOS Keychain, the Secret Service on Linux, the Credential Manager on Windows;
//! `DOSTOEVSKY_SECRETS=memory` (or `cfg(test)`) keeps them in memory.

use std::cell::RefCell;
use std::collections::HashMap;

const KEYCHAIN_SERVICE: &str = "Dostoevsky AI";

thread_local! {
    /// Per thread, so tests running in parallel don't see each other's keys.
    static MEMORY: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

fn in_memory() -> bool {
    cfg!(test) || std::env::var("DOSTOEVSKY_SECRETS").is_ok_and(|v| v == "memory")
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
    use keyring_core::{CredentialStore, Entry, Error};
    use std::sync::{Arc, Mutex};

    /// Opened on first use and kept; a failure isn't kept, so a keyring started later is found.
    fn store() -> Result<Arc<CredentialStore>, String> {
        static STORE: Mutex<Option<Arc<CredentialStore>>> = Mutex::new(None);
        let mut kept = STORE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = kept.as_ref() {
            return Ok(s.clone());
        }
        let s = open()?;
        *kept = Some(s.clone());
        Ok(s)
    }

    #[cfg(target_os = "linux")]
    fn open() -> Result<Arc<CredentialStore>, String> {
        // Without a session bus or a Secret Service, zbus's own message names a socket path.
        let s: Arc<CredentialStore> = zbus_secret_service_keyring_store::Store::new().map_err(|e| {
            eprintln!("dostoevsky: no Secret Service: {e}");
            l10n::tr!("no keyring is running (GNOME Keyring or KWallet)").to_string()
        })?;
        Ok(s)
    }

    #[cfg(windows)]
    fn open() -> Result<Arc<CredentialStore>, String> {
        let s: Arc<CredentialStore> = windows_native_keyring_store::Store::new().map_err(|e| e.to_string())?;
        Ok(s)
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    fn open() -> Result<Arc<CredentialStore>, String> {
        Err("no keychain on this system".into())
    }

    fn entry(service: &str, account: &str) -> Result<Entry, String> {
        Entry::new_in_store(&store()?, service, account).map_err(|e| e.to_string())
    }

    pub fn get(service: &str, account: &str) -> Option<String> {
        let read = entry(service, account).and_then(|e| match e.get_password() {
            Err(Error::NoEntry) => Ok(None),
            r => r.map(Some).map_err(|e| e.to_string()),
        });
        read.unwrap_or_else(|e| {
            eprintln!("dostoevsky: couldn't read the {account} key: {e}");
            None
        })
    }

    pub fn set(service: &str, account: &str, secret: &str) -> Result<(), String> {
        entry(service, account)?.set_password(secret).map_err(|e| e.to_string())
    }

    pub fn delete(service: &str, account: &str) {
        if let Ok(e) = entry(service, account) {
            let _ = e.delete_credential();
        }
    }
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

    /// `cargo test -p ai -- --ignored system_keychain`: on Linux it needs a running Secret
    /// Service (GNOME Keyring or KWallet), on the Mac it asks for the login keychain.
    #[test]
    #[ignore = "touches the system's keychain"]
    fn system_keychain_round_trip() {
        use super::keychain;
        let (service, account) = ("Dostoevsky AI test", "round-trip");
        keychain::delete(service, account);
        assert_eq!(keychain::get(service, account), None);
        keychain::set(service, account, "sk-ключ-1").unwrap();
        assert_eq!(keychain::get(service, account).as_deref(), Some("sk-ключ-1"));
        keychain::set(service, account, "sk-2").unwrap();
        assert_eq!(keychain::get(service, account).as_deref(), Some("sk-2"), "replaced, not added");
        keychain::delete(service, account);
        assert_eq!(keychain::get(service, account), None);
    }
}
