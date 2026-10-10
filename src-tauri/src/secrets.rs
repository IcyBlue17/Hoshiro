//! API credentials live in the OS credential store (macOS Keychain / Windows Credential Manager /
//! Secret Service) as ONE JSON blob in ONE item, so the OS prompts at most once and a read is a
//! single call. The decoded blob is cached in memory; the keychain is never touched on the hot
//! dictation path. Nothing in this module ever logs a secret value.

use crate::types::Settings;
use parking_lot::Mutex;
use std::collections::BTreeMap;

pub const SERVICE: &str = "cn.icybit.hoshiro";
/// Keychain service used before the bundle id changed; read once to migrate. Can be removed
/// after a few releases.
pub const LEGACY_SERVICE: &str = "dev.hoshiro.app";
pub const ACCOUNT: &str = "credentials";

/// Flat map of `settings path -> secret value`; only non-empty values are kept.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secrets(pub BTreeMap<String, String>);

impl std::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Secrets({} entries, redacted)", self.0.len())
    }
}

impl Secrets {
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn to_blob(&self) -> String {
        serde_json::to_string(&self.0).unwrap_or_else(|_| "{}".into())
    }
    pub fn from_blob(blob: &str) -> Result<Self, String> {
        let map: BTreeMap<String, String> = serde_json::from_str(blob).map_err(|_| "keychain blob is not valid JSON".to_string())?;
        Ok(Self(map.into_iter().filter(|(_, v)| !v.is_empty()).collect()))
    }
}

/// Visit every secret field of `Settings` (the single source of truth for what is secret).
fn visit(s: &mut Settings, mut f: impl FnMut(&'static str, &mut String)) {
    f("asr.mimo.api_key", &mut s.asr.mimo.api_key);
    f("asr.cloudflare.api_key", &mut s.asr.cloudflare.api_key);
    f("asr.azure.api_key", &mut s.asr.azure.api_key);
    f("asr.google.api_key", &mut s.asr.google.api_key);
    f("asr.google.service_account_json", &mut s.asr.google.service_account_json);
    f("llm.gemini.api_key", &mut s.llm.gemini.api_key);
    f("llm.anthropic.api_key", &mut s.llm.anthropic.api_key);
    f("llm.openai.api_key", &mut s.llm.openai.api_key);
    f("llm.cloudflare.api_key", &mut s.llm.cloudflare.api_key);
}

/// Collect the non-empty secrets of `s`.
pub fn extract(s: &Settings) -> Secrets {
    let mut c = s.clone();
    let mut m = BTreeMap::new();
    visit(&mut c, |k, v| {
        if !v.trim().is_empty() {
            m.insert(k.to_string(), v.clone());
        }
    });
    Secrets(m)
}

/// Copy of `s` with every secret field blanked (safe to write to settings.json).
pub fn public_copy(s: &Settings) -> Settings {
    let mut c = s.clone();
    visit(&mut c, |_, v| v.clear());
    c
}

/// Fill secret fields from `secrets`. A non-empty value already present in `s` wins.
pub fn overlay(s: &mut Settings, secrets: &Secrets) {
    visit(s, |k, v| {
        if v.trim().is_empty() {
            if let Some(val) = secrets.0.get(k) {
                *v = val.clone();
            }
        }
    });
}

pub trait SecretStore: Send + Sync {
    /// `Ok(None)` when the item does not exist yet.
    fn get(&self) -> Result<Option<String>, String>;
    fn set(&self, blob: &str) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
}

pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    pub fn new(service: &str) -> Self {
        Self { service: service.to_string() }
    }
    pub fn system() -> Self {
        Self::new(SERVICE)
    }
    fn entry(&self) -> Result<keyring::Entry, String> {
        keyring::Entry::new(&self.service, ACCOUNT).map_err(|e| e.to_string())
    }
}

impl SecretStore for KeyringStore {
    fn get(&self) -> Result<Option<String>, String> {
        match self.entry()?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    fn set(&self, blob: &str) -> Result<(), String> {
        self.entry()?.set_password(blob).map_err(|e| e.to_string())
    }
    fn delete(&self) -> Result<(), String> {
        self.entry()?.delete_credential().map_err(|e| e.to_string())
    }
}

enum State {
    Uninit,
    /// Keychain readable; holds what is currently stored there.
    Ready(Secrets),
    /// Keychain unreadable: never write it this session (could clobber unseen content); keep
    /// secrets in settings.json.
    Unavailable,
}

/// Credential store + in-memory cache.
pub struct Vault {
    store: Box<dyn SecretStore>,
    /// Pre-rename item, migrated into `store` the first time `store` turns out to be empty.
    legacy: Option<Box<dyn SecretStore>>,
    state: Mutex<State>,
}

impl Vault {
    pub fn new(store: Box<dyn SecretStore>) -> Self {
        Self { store, legacy: None, state: Mutex::new(State::Uninit) }
    }

    pub fn with_legacy(mut self, legacy: Box<dyn SecretStore>) -> Self {
        self.legacy = Some(legacy);
        self
    }

    /// Copy the legacy item into `store`; the legacy item is deleted only after the copy is
    /// verified. Returns the blob now held by `store`, or None if there was nothing to migrate.
    fn migrate_legacy(&self) -> Option<String> {
        let legacy = self.legacy.as_ref()?;
        let blob = legacy.get().ok().flatten()?;
        if self.store.set(&blob).is_err() || self.store.get().ok().flatten().as_deref() != Some(blob.as_str()) {
            log::warn!("credential store: migrating legacy item failed; will retry next launch");
            return None;
        }
        match legacy.delete() {
            Ok(()) => log::info!("credential store: migrated legacy item"),
            Err(e) => log::warn!("credential store: migrated, but could not remove legacy item ({e})"),
        }
        Some(blob)
    }

    fn ensure_init(&self, st: &mut State) {
        if !matches!(st, State::Uninit) {
            return;
        }
        let first = match self.store.get() {
            Ok(None) => Ok(self.migrate_legacy()),
            other => other,
        };
        *st = match first {
            Ok(None) => State::Ready(Secrets::default()),
            Ok(Some(blob)) => match Secrets::from_blob(&blob) {
                Ok(s) => {
                    log::info!("credential store: loaded {} secret(s)", s.len());
                    State::Ready(s)
                }
                Err(e) => {
                    log::warn!("credential store item unusable ({e}); falling back to settings.json");
                    State::Unavailable
                }
            },
            Err(e) => {
                log::warn!("credential store unavailable ({e}); falling back to settings.json");
                State::Unavailable
            }
        };
    }

    /// Secrets currently held in the credential store (empty when unavailable).
    pub fn stored(&self) -> Secrets {
        let mut st = self.state.lock();
        self.ensure_init(&mut st);
        match &*st {
            State::Ready(s) => s.clone(),
            _ => Secrets::default(),
        }
    }

    pub fn is_available(&self) -> bool {
        let mut st = self.state.lock();
        self.ensure_init(&mut st);
        matches!(&*st, State::Ready(_))
    }

    /// Make the keychain hold exactly `want` (write + read-back verification, skipped when the
    /// cache already matches). Returns true only if the keychain verifiably holds `want`.
    pub fn sync(&self, want: &Secrets) -> bool {
        let mut st = self.state.lock();
        self.ensure_init(&mut st);
        let State::Ready(cur) = &*st else { return false };
        if cur == want {
            return true;
        }
        let blob = want.to_blob();
        if let Err(e) = self.store.set(&blob) {
            log::warn!("credential store write failed ({e}); keeping secrets in settings.json");
            return false;
        }
        match self.store.get() {
            Ok(Some(back)) if Secrets::from_blob(&back).as_ref() == Ok(want) => {
                log::info!("credential store updated ({} secret(s))", want.len());
                *st = State::Ready(want.clone());
                true
            }
            _ => {
                log::warn!("credential store write could not be verified; keeping secrets in settings.json");
                false
            }
        }
    }
}

#[cfg(test)]
pub mod fake {
    use super::*;
    use std::sync::Arc;

    #[derive(Default)]
    pub struct FakeState {
        pub blob: Option<String>,
        pub fail_get: bool,
        pub fail_set: bool,
        /// set() succeeds but nothing is persisted
        pub drop_writes: bool,
        pub sets: usize,
    }
    #[derive(Clone, Default)]
    pub struct FakeStore(pub Arc<Mutex<FakeState>>);
    impl SecretStore for FakeStore {
        fn get(&self) -> Result<Option<String>, String> {
            let s = self.0.lock();
            if s.fail_get { Err("denied".into()) } else { Ok(s.blob.clone()) }
        }
        fn set(&self, blob: &str) -> Result<(), String> {
            let mut s = self.0.lock();
            if s.fail_set {
                return Err("denied".into());
            }
            s.sets += 1;
            if !s.drop_writes {
                s.blob = Some(blob.to_string());
            }
            Ok(())
        }
        fn delete(&self) -> Result<(), String> {
            self.0.lock().blob = None;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;

    fn with_keys() -> Settings {
        let mut s = Settings::default();
        s.asr.mimo.api_key = "SEKRET-mimo".into();
        s.asr.google.service_account_json = "{\"private_key\":\"SEKRET-sa\"}".into();
        s.llm.openai.api_key = "SEKRET-oai".into();
        s
    }

    #[test]
    fn split_and_merge_roundtrip() {
        let s = with_keys();
        let sec = extract(&s);
        assert_eq!(sec.len(), 3);
        let public = public_copy(&s);
        assert!(extract(&public).is_empty());
        let mut back = public.clone();
        overlay(&mut back, &sec);
        assert_eq!(back, s);
    }

    #[test]
    fn public_json_has_no_secret_values() {
        let s = with_keys();
        let json = serde_json::to_string(&public_copy(&s)).unwrap();
        assert!(!json.contains("SEKRET"));
        assert!(!format!("{:?}", extract(&s)).contains("SEKRET"));
    }

    #[test]
    fn overlay_prefers_existing_non_empty_value() {
        let mut s = Settings::default();
        s.llm.openai.api_key = "file".into();
        let sec = Secrets([("llm.openai.api_key".to_string(), "kc".to_string())].into());
        overlay(&mut s, &sec);
        assert_eq!(s.llm.openai.api_key, "file");
    }

    #[test]
    fn sync_writes_only_on_change_and_verifies() {
        let fake = FakeStore::default();
        let v = Vault::new(Box::new(fake.clone()));
        let sec = extract(&with_keys());
        assert!(v.sync(&sec));
        assert!(v.sync(&sec));
        assert_eq!(fake.0.lock().sets, 1);
        assert_eq!(Secrets::from_blob(fake.0.lock().blob.as_ref().unwrap()).unwrap(), sec);
    }

    #[test]
    fn unverified_write_is_not_trusted() {
        let fake = FakeStore::default();
        fake.0.lock().drop_writes = true;
        let v = Vault::new(Box::new(fake));
        assert!(!v.sync(&extract(&with_keys())));
    }

    #[test]
    fn unreadable_store_is_never_written() {
        let fake = FakeStore::default();
        fake.0.lock().fail_get = true;
        let v = Vault::new(Box::new(fake.clone()));
        assert!(!v.is_available());
        assert!(!v.sync(&extract(&with_keys())));
        assert_eq!(fake.0.lock().sets, 0);
    }

    #[test]
    fn legacy_item_is_migrated_then_removed() {
        let (new, old) = (FakeStore::default(), FakeStore::default());
        let sec = extract(&with_keys());
        old.0.lock().blob = Some(sec.to_blob());
        let v = Vault::new(Box::new(new.clone())).with_legacy(Box::new(old.clone()));
        assert_eq!(v.stored(), sec);
        assert_eq!(Secrets::from_blob(new.0.lock().blob.as_ref().unwrap()).unwrap(), sec);
        assert!(old.0.lock().blob.is_none());
    }

    #[test]
    fn legacy_item_is_kept_when_copy_fails_and_ignored_when_new_exists() {
        let (new, old) = (FakeStore::default(), FakeStore::default());
        old.0.lock().blob = Some(extract(&with_keys()).to_blob());
        new.0.lock().fail_set = true;
        let v = Vault::new(Box::new(new.clone())).with_legacy(Box::new(old.clone()));
        assert!(v.stored().is_empty());
        assert!(old.0.lock().blob.is_some());

        let (new, old) = (FakeStore::default(), FakeStore::default());
        new.0.lock().blob = Some("{}".into());
        old.0.lock().blob = Some(extract(&with_keys()).to_blob());
        let v = Vault::new(Box::new(new)).with_legacy(Box::new(old.clone()));
        assert!(v.stored().is_empty());
        assert!(old.0.lock().blob.is_some());
    }
}
