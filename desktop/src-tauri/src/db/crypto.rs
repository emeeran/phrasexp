//! At-rest encryption for sensitive DB columns.
//!
//! AES-256-GCM with a data-encryption key (DEK) stored in the OS keychain (Secret Service on
//! Linux, Keychain on macOS, Credential Manager on Windows). Encrypted values live inline in the
//! existing TEXT columns as `enc1:<base64(nonce12 ‖ ciphertext ‖ tag)>`, so the envelope rides
//! through raw `body_json` copies (`phrases` ↔ `phrase_versions`) untouched. Readers branch on
//! the `enc1:` prefix; writers encrypt-or-not by context.
//!
//! Threat model: protects the DB file at rest (disk theft, backups, cloud sync, other local
//! users). Does NOT protect against an attacker running code as the user — the OS keychain and
//! decrypted RAM are reachable — the standard browser-password-manager model.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::{rngs::OsRng, RngCore};
use zeroize::Zeroizing;

/// Inline envelope prefix for encrypted values. Doubles as a version tag (`enc1` → future `enc2`).
pub const ENVELOPE_PREFIX: &str = "enc1:";

const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const SERVICE: &str = "com.phrasexp.app";
const USER: &str = "phrase-dek";

/// Minimal secret-store abstraction so the keychain is mockable in unit tests.
pub trait SecretStore {
    /// `Ok(None)` = no entry yet; `Err(())` = the store is unavailable (e.g. no Secret Service).
    fn get(&self) -> Result<Option<String>, ()>;
    fn set(&self, value: &str) -> Result<(), ()>;
}

/// OS keychain backing store.
pub struct KeyringStore(keyring::Entry);

impl KeyringStore {
    /// Fallible: on a platform/backend with no keychain (e.g. headless Linux with no Secret
    /// Service), `Entry::new` returns an error.
    pub fn new() -> Result<Self, keyring::Error> {
        Ok(Self(keyring::Entry::new(SERVICE, USER)?))
    }
}

impl SecretStore for KeyringStore {
    fn get(&self) -> Result<Option<String>, ()> {
        match self.0.get_password() {
            Ok(s) => Ok(Some(s)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(()),
        }
    }

    fn set(&self, value: &str) -> Result<(), ()> {
        self.0.set_password(value).map_err(|_| ())
    }
}

/// Holds the in-memory DEK (zeroized on drop). `dek == None` means encryption is unavailable —
/// writes fall back to plaintext and reads of `enc1:` values degrade (locked).
pub struct Crypto {
    dek: Option<Zeroizing<Vec<u8>>>,
}

impl Crypto {
    /// Load the DEK from the store; generate + persist one on first run. If the store is
    /// unavailable, returns a `Crypto` with no DEK (encryption disabled — app still works for
    /// plaintext rows).
    pub fn init(store: &dyn SecretStore) -> Self {
        match Self::load_or_create(store) {
            Some(dek) => Crypto { dek: Some(dek) },
            None => Crypto { dek: None },
        }
    }

    /// Encryption disabled (no DEK). Used when the OS keychain isn't available at all.
    pub fn disabled() -> Self {
        Crypto { dek: None }
    }

    fn load_or_create(store: &dyn SecretStore) -> Option<Zeroizing<Vec<u8>>> {
        match store.get() {
            Ok(Some(b64)) => match B64.decode(b64.as_bytes()) {
                Ok(bytes) if bytes.len() == KEY_LEN => Some(Zeroizing::new(bytes)),
                // Wrong shape / corrupt → treat as no key; fall through to generate.
                _ => Self::generate(store),
            },
            Ok(None) => Self::generate(store),
            Err(()) => None, // store unavailable → can't key
        }
    }

    fn generate(store: &dyn SecretStore) -> Option<Zeroizing<Vec<u8>>> {
        let mut key = vec![0u8; KEY_LEN];
        OsRng.fill_bytes(&mut key);
        let encoded = B64.encode(&key);
        match store.set(&encoded) {
            Ok(()) => Some(Zeroizing::new(key)),
            Err(()) => None, // store writable check failed → no key
        }
    }

    /// Test-only constructor that bypasses the keychain entirely.
    #[cfg(test)]
    pub fn for_testing(dek: Option<[u8; KEY_LEN]>) -> Self {
        Crypto {
            dek: dek.map(|k| Zeroizing::new(k.to_vec())),
        }
    }

    /// True when a DEK is loaded (encryption/decryption active).
    pub fn is_available(&self) -> bool {
        self.dek.is_some()
    }

    fn cipher(&self) -> Option<Aes256Gcm> {
        let dek = self.dek.as_ref()?;
        // dek length is validated to KEY_LEN on load; from_slice would panic otherwise.
        Some(Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&dek[..])))
    }

    /// Encrypt bytes into a `enc1:<base64>` envelope. Returns `None` when no DEK is loaded
    /// (caller should store plaintext instead).
    pub fn encrypt(&self, plaintext: &[u8]) -> Option<String> {
        let cipher = self.cipher()?;
        let mut nonce_bytes = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        // AES-GCM appends the 16-byte auth tag to the ciphertext.
        let ct = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext)
            .ok()?;
        let mut blob = Vec::with_capacity(NONCE_LEN + ct.len());
        blob.extend_from_slice(&nonce_bytes);
        blob.extend_from_slice(&ct);
        Some(format!("{ENVELOPE_PREFIX}{}", B64.encode(&blob)))
    }

    /// Convenience for string values (body_json, ai_api_key, clipboard text).
    pub fn encrypt_str(&self, plaintext: &str) -> Option<String> {
        self.encrypt(plaintext.as_bytes())
    }

    /// Decrypt a full `enc1:<base64>` envelope value. `s` MUST already be prefixed. Returns
    /// `None` on any failure (no DEK, bad base64, truncated, or failed AEAD auth).
    pub fn decrypt(&self, enveloped: &str) -> Option<Vec<u8>> {
        let b64 = enveloped.strip_prefix(ENVELOPE_PREFIX)?;
        let cipher = self.cipher()?;
        let blob = B64.decode(b64.as_bytes()).ok()?;
        if blob.len() < NONCE_LEN + 16 {
            return None; // truncated (need at least nonce + 16-byte tag)
        }
        let (nonce_bytes, ct) = blob.split_at(NONCE_LEN);
        cipher.decrypt(Nonce::from_slice(nonce_bytes), ct).ok()
    }

    /// Decrypt a prefixed envelope to a UTF-8 string. `None` if not prefixed, locked, or invalid.
    pub fn decrypt_str(&self, enveloped: &str) -> Option<String> {
        if !enveloped.starts_with(ENVELOPE_PREFIX) {
            return None;
        }
        String::from_utf8(self.decrypt(enveloped)?).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// In-memory SecretStore for testing (Mutex so it's &dyn-safe under interior mutability).
    struct MockStore {
        inner: Mutex<Option<String>>,
        writable: bool,
    }

    impl MockStore {
        fn new() -> Self {
            Self {
                inner: Mutex::new(None),
                writable: true,
            }
        }
        fn unwritable() -> Self {
            Self {
                inner: Mutex::new(None),
                writable: false,
            }
        }
    }

    impl SecretStore for MockStore {
        fn get(&self) -> Result<Option<String>, ()> {
            Ok(self.inner.lock().unwrap().clone())
        }
        fn set(&self, value: &str) -> Result<(), ()> {
            if !self.writable {
                return Err(());
            }
            *self.inner.lock().unwrap() = Some(value.to_string());
            Ok(())
        }
    }

    const DEK: [u8; KEY_LEN] = [0x42; KEY_LEN];

    #[test]
    fn round_trip_bytes() {
        let c = Crypto::for_testing(Some(DEK));
        let pt = b"hello world \x00 binary";
        let envelope = c.encrypt(pt).unwrap();
        assert!(envelope.starts_with(ENVELOPE_PREFIX));
        assert_eq!(c.decrypt(&envelope).unwrap(), pt);
    }

    #[test]
    fn round_trip_string() {
        let c = Crypto::for_testing(Some(DEK));
        let envelope = c
            .encrypt_str("[{\"type\":\"text\",\"text\":\"sk-abc123\"}]")
            .unwrap();
        assert_eq!(
            c.decrypt_str(&envelope).unwrap(),
            "[{\"type\":\"text\",\"text\":\"sk-abc123\"}]"
        );
    }

    #[test]
    fn two_encryptions_differ_due_to_random_nonce() {
        let c = Crypto::for_testing(Some(DEK));
        let a = c.encrypt(b"same").unwrap();
        let b = c.encrypt(b"same").unwrap();
        assert_ne!(a, b); // random nonce → different ciphertext
        assert_eq!(c.decrypt(&a).unwrap(), b"same");
        assert_eq!(c.decrypt(&b).unwrap(), b"same");
    }

    #[test]
    fn tampered_ciphertext_fails_auth() {
        let c = Crypto::for_testing(Some(DEK));
        let envelope = c.encrypt(b"secret").unwrap();
        let b64 = &envelope[ENVELOPE_PREFIX.len()..];
        let mut blob = B64.decode(b64.as_bytes()).unwrap();
        blob[NONCE_LEN] ^= 0xff; // flip a ciphertext byte
        let tampered = format!("{ENVELOPE_PREFIX}{}", B64.encode(&blob));
        assert_eq!(c.decrypt(&tampered), None); // GCM auth fails
    }

    #[test]
    fn truncated_blob_is_none() {
        let c = Crypto::for_testing(Some(DEK));
        // nonce only (no room for a 16-byte tag)
        let blob = vec![0u8; NONCE_LEN];
        let envelope = format!("{ENVELOPE_PREFIX}{}", B64.encode(&blob));
        assert_eq!(c.decrypt(&envelope), None);
    }

    #[test]
    fn decrypt_non_prefixed_is_none() {
        let c = Crypto::for_testing(Some(DEK));
        assert_eq!(c.decrypt_str("not enveloped"), None);
        assert_eq!(c.decrypt_str("[{\"type\":\"text\"}]"), None);
    }

    #[test]
    fn wrong_key_fails_auth() {
        let a = Crypto::for_testing(Some(DEK));
        let envelope = a.encrypt(b"secret").unwrap();
        let b = Crypto::for_testing(Some([0x99; KEY_LEN]));
        assert_eq!(b.decrypt(&envelope), None);
    }

    #[test]
    fn no_dek_encrypt_returns_none() {
        let c = Crypto::for_testing(None);
        assert!(!c.is_available());
        assert_eq!(c.encrypt(b"x"), None);
        // decrypt of an envelope with no DEK → locked (None)
        let other = Crypto::for_testing(Some(DEK));
        let env = other.encrypt(b"x").unwrap();
        assert_eq!(c.decrypt(&env), None);
    }

    #[test]
    fn init_generates_and_persists_then_reads_back() {
        let store = MockStore::new();
        let c1 = Crypto::init(&store);
        assert!(c1.is_available());
        let envelope = c1.encrypt(b"data").unwrap();
        // Second init reuses the persisted DEK (same key → decrypts).
        let c2 = Crypto::init(&store);
        assert!(c2.is_available());
        assert_eq!(c2.decrypt(&envelope).unwrap(), b"data");
    }

    #[test]
    fn init_with_unwritable_store_disables_encryption() {
        let store = MockStore::unwritable();
        let c = Crypto::init(&store);
        assert!(!c.is_available()); // can't persist a generated key → disabled
    }

    #[test]
    fn init_rejects_corrupt_key_and_regenerates() {
        // A persisted value of the wrong length must not be used as the DEK.
        let store = MockStore::new();
        store.inner.lock().unwrap().replace(B64.encode(b"tooshort"));
        let c = Crypto::init(&store);
        assert!(c.is_available()); // regenerated + persisted
        assert!(c.encrypt(b"x").is_some());
    }
}
