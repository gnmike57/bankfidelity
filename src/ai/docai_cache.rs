//! Local cache for Document AI parses.
//!
//! Document AI is billed per-page. When the same PDF is parsed twice (the
//! user re-runs the workflow, batch processing, retries) we hit the cache
//! instead of the network.
//!
//! Layout:
//!
//! ```text
//! audit/cache/docai/<key>.json   // parsed BankStatement
//! audit/cache/docai/<key>.raw.json   // raw Document AI response
//! ```
//!
//! Where `<key> = sha256(pdf_bytes) :: ":" :: project_id :: ":" :: location :: ":" :: processor_id :: ":" :: processor_version`.
//!
//! Anything in any field of the key flips the hash; nothing collides.
//!
//! # Encryption key derivation (v2)
//!
//! Entry payloads are encrypted with ChaCha20-Poly1305 using a key derived
//! via **HKDF-SHA256** (`ikm = DUAL_CORE_PASSPHRASE`, random per-cache salt
//! persisted in `<root>/kdf.salt`, domain-separated `info`). Earlier versions
//! used bare `SHA256(passphrase)`; those entries cannot be decrypted under
//! the v2 key schedule and are treated as misses — i.e. upgrading invalidates
//! old caches once, by design.
//!
//! Entries never expire automatically. Run `dual-core docai-cache prune` to
//! garbage-collect (future CLI subcommand). Until then, callers are
//! free to delete `audit/cache/docai/` whenever they want a clean slate.

use std::path::{Path, PathBuf};

use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use hkdf::Hkdf;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::ai::document_ai::BankStatement;

/// v2: encryption keys are derived with salted HKDF-SHA256 instead of bare
/// SHA256(passphrase). v1 entries cannot be decrypted under the v2 key
/// schedule and are treated as misses — upgrading invalidates old caches
/// once, by design.
const CACHE_FORMAT_VERSION: u32 = 2;

/// Name of the per-cache salt file persisted beside the cached entries.
const KDF_SALT_FILE_NAME: &str = "kdf.salt";
/// Magic header for the salt file ("BKFS" = BankFidelity KDF Salt).
const KDF_SALT_MAGIC: [u8; 4] = *b"BKFS";
/// Salt file layout version so future KDF migrations can be detected.
const KDF_SALT_FILE_VERSION: u8 = 1;
/// Salt length in bytes (256-bit).
const KDF_SALT_LEN: usize = 32;
/// HKDF `info` binding: domain-separates this cache's key derivation.
const KDF_INFO: &[u8] = b"bankfidelity/docai-cache/v2";

/// On-disk cache entry. The `format_version` lets us bump the layout later
/// (e.g. add new fields to BankStatement) and treat older entries as misses.
#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    format_version: u32,
    key: String,
    written_at: String,
    statement: BankStatement,
}

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("cache I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("cache decode: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("encryption error: {0}")]
    Encryption(String),
    #[error(
        "DUAL_CORE_PASSPHRASE not set: refusing to derive cache encryption keys from an empty passphrase"
    )]
    EmptyPassphrase,
}

pub struct DocAiCache {
    root: PathBuf,
    cipher: ChaCha20Poly1305,
}

impl DocAiCache {
    /// Open (or create) the cache rooted at `audit/cache/docai/`.
    pub fn open_default(passphrase: &str) -> Result<Self, CacheError> {
        Self::open(
            PathBuf::from("audit").join("cache").join("docai"),
            passphrase,
        )
    }

    pub fn open(root: PathBuf, passphrase: &str) -> Result<Self, CacheError> {
        if passphrase.is_empty() {
            // Loud failure: never derive keys from an empty secret.
            return Err(CacheError::EmptyPassphrase);
        }
        std::fs::create_dir_all(&root)?;
        let salt = load_or_create_salt(&root)?;
        let hk = Hkdf::<Sha256>::new(Some(&salt[..]), passphrase.as_bytes());
        let mut key_bytes = [0u8; 32];
        hk.expand(KDF_INFO, &mut key_bytes)
            .map_err(|e| CacheError::Encryption(format!("HKDF expand failed: {e}")))?;
        let cipher = ChaCha20Poly1305::new((&key_bytes).into());
        Ok(Self { root, cipher })
    }

    /// Build the cache key. Key bytes are stable across runs as long as
    /// (file content, processor identity) are unchanged.
    pub fn make_key(
        pdf_path: &Path,
        project_id: &str,
        location: &str,
        processor_id: &str,
        processor_version: &str,
    ) -> Result<String, CacheError> {
        let bytes = std::fs::read(pdf_path)?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let content_hash = hex_lower(&hasher.finalize());

        let mut hasher = Sha256::new();
        hasher.update(content_hash.as_bytes());
        hasher.update(b":");
        hasher.update(project_id.as_bytes());
        hasher.update(b":");
        hasher.update(location.as_bytes());
        hasher.update(b":");
        hasher.update(processor_id.as_bytes());
        hasher.update(b":");
        hasher.update(processor_version.as_bytes());
        Ok(hex_lower(&hasher.finalize()))
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.root.join(format!("{key}.json"))
    }

    pub fn get(&self, key: &str) -> Option<BankStatement> {
        let path = self.path_for(key);
        let file_data = std::fs::read(&path).ok()?;

        if file_data.len() < 12 {
            tracing::warn!("[docai_cache] file too short: {}", path.display());
            return None;
        }

        let nonce = Nonce::from_slice(&file_data[0..12]);
        let ciphertext = &file_data[12..];

        let plaintext = match self.cipher.decrypt(nonce, ciphertext) {
            Ok(pt) => pt,
            Err(e) => {
                tracing::warn!(
                    "[docai_cache] decryption failed for {}: {}",
                    path.display(),
                    e
                );
                return None;
            }
        };

        match serde_json::from_slice::<CacheEntry>(&plaintext) {
            Ok(entry) if entry.format_version == CACHE_FORMAT_VERSION => {
                tracing::debug!(cache.hit = true, cache.key = %key, "[docai_cache] hit");
                Some(entry.statement)
            }
            Ok(other) => {
                tracing::warn!(
                    cache.format_version = other.format_version,
                    "[docai_cache] entry has incompatible format_version, ignoring"
                );
                None
            }
            Err(e) => {
                tracing::warn!("[docai_cache] failed to decode {}: {}", path.display(), e);
                None
            }
        }
    }

    pub fn put(&self, key: &str, statement: &BankStatement) -> Result<(), CacheError> {
        let entry = CacheEntry {
            format_version: CACHE_FORMAT_VERSION,
            key: key.to_string(),
            written_at: chrono::Utc::now().to_rfc3339(),
            statement: statement.clone(),
        };
        let plaintext = serde_json::to_vec(&entry)?;

        let uuid = Uuid::new_v4();
        let nonce_bytes = &uuid.as_bytes()[0..12];
        let nonce = Nonce::from_slice(nonce_bytes);

        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext.as_slice())
            .map_err(|e| CacheError::Encryption(e.to_string()))?;

        // Store [12-byte nonce][ciphertext]
        let mut file_data = Vec::with_capacity(12 + ciphertext.len());
        file_data.extend_from_slice(nonce_bytes);
        file_data.extend_from_slice(&ciphertext);

        let path = self.path_for(key);
        // Atomic-ish write: tmp + rename. Important on Windows because
        // PyMuPDF's `pymupdf.open` from another thread can read in parallel.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &file_data)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Loads the per-cache KDF salt from `<root>/kdf.salt`, creating a fresh
/// random salt on first use. The file carries a magic header and layout
/// version so future KDF migrations can be detected explicitly.
///
/// An unrecognized or corrupt salt file is regenerated, which invalidates
/// existing entries (they become undecryptable misses).
fn load_or_create_salt(root: &Path) -> Result<[u8; KDF_SALT_LEN], CacheError> {
    let path = root.join(KDF_SALT_FILE_NAME);
    if let Ok(data) = std::fs::read(&path) {
        let header_len = KDF_SALT_MAGIC.len() + 1;
        if data.len() == header_len + KDF_SALT_LEN
            && data[..KDF_SALT_MAGIC.len()] == KDF_SALT_MAGIC
            && data[KDF_SALT_MAGIC.len()] == KDF_SALT_FILE_VERSION
        {
            let mut salt = [0u8; KDF_SALT_LEN];
            salt.copy_from_slice(&data[header_len..]);
            return Ok(salt);
        }
        tracing::warn!(
            "[docai_cache] unrecognized {} header; regenerating (existing entries will be invalidated)",
            path.display()
        );
    }
    let mut salt = [0u8; KDF_SALT_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    let mut file_data = Vec::with_capacity(KDF_SALT_MAGIC.len() + 1 + KDF_SALT_LEN);
    file_data.extend_from_slice(&KDF_SALT_MAGIC);
    file_data.push(KDF_SALT_FILE_VERSION);
    file_data.extend_from_slice(&salt);
    std::fs::write(&path, &file_data)?;
    tracing::info!(
        "[docai_cache] generated new per-cache KDF salt at {}",
        path.display()
    );
    Ok(salt)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::engine::model::Transaction;
    use tempfile::tempdir;

    fn sample_statement() -> BankStatement {
        use rust_decimal_macros::dec;
        BankStatement {
            total_pages: 1,
            transactions: vec![Transaction {
                page: 0,
                line_on_page: 0,
                date: "01/01/2026".into(),
                raw_text: "Test".into(),
                debit: Some(dec!(100.0)),
                credit: None,
                running_balance: Some(dec!(100.0)),
                bbox: None,
                field_bboxes: Default::default(),
                provenance: crate::engine::model::Provenance::Computed,
                category: None,
                canonical: Default::default(),
            }],
            opening_balance: dec!(0.0),
            closing_balance: dec!(100.0),
            account_number: None,
            bank_name: None,
        }
    }

    #[test]
    fn roundtrip_through_cache() {
        use rust_decimal_macros::dec;
        let dir = tempdir().unwrap();
        let cache = DocAiCache::open(dir.path().to_path_buf(), "testpass").unwrap();
        let stmt = sample_statement();
        cache.put("key1", &stmt).unwrap();
        let got = cache.get("key1").unwrap();
        assert_eq!(got.total_pages, stmt.total_pages);
        assert_eq!(got.transactions.len(), 1);
        assert_eq!(got.transactions[0].debit, Some(dec!(100.0)));
        assert_eq!(got.account_number.as_deref(), None);
    }

    #[test]
    fn miss_returns_none() {
        let dir = tempdir().unwrap();
        let cache = DocAiCache::open(dir.path().to_path_buf(), "testpass").unwrap();
        assert!(cache.get("nonexistent").is_none());
    }

    #[test]
    fn key_changes_when_processor_changes() {
        let dir = tempdir().unwrap();
        let pdf = dir.path().join("test.pdf");
        std::fs::write(&pdf, b"%PDF-1.4 hello world").unwrap();
        let k1 = DocAiCache::make_key(&pdf, "p1", "us", "proc1", "v1").unwrap();
        let k2 = DocAiCache::make_key(&pdf, "p1", "us", "proc2", "v1").unwrap();
        let k3 = DocAiCache::make_key(&pdf, "p1", "us", "proc1", "v2").unwrap();
        assert_ne!(k1, k2, "different processor id must change the key");
        assert_ne!(k1, k3, "different processor version must change the key");
    }

    #[test]
    fn key_changes_when_pdf_content_changes() {
        let dir = tempdir().unwrap();
        let pdf = dir.path().join("test.pdf");
        std::fs::write(&pdf, b"%PDF-1.4 v1").unwrap();
        let k1 = DocAiCache::make_key(&pdf, "p", "us", "proc", "v").unwrap();
        std::fs::write(&pdf, b"%PDF-1.4 v2").unwrap();
        let k2 = DocAiCache::make_key(&pdf, "p", "us", "proc", "v").unwrap();
        assert_ne!(k1, k2);
    }

    #[test]
    fn corrupt_entry_is_treated_as_miss() {
        let dir = tempdir().unwrap();
        let cache = DocAiCache::open(dir.path().to_path_buf(), "testpass").unwrap();
        std::fs::write(dir.path().join("badkey.json"), "{not json").unwrap();
        assert!(cache.get("badkey").is_none());
    }

    #[test]
    fn empty_passphrase_is_rejected_loudly() {
        let dir = tempdir().unwrap();
        let err = match DocAiCache::open(dir.path().to_path_buf(), "") {
            Err(e) => e,
            Ok(_) => panic!("empty passphrase must fail fast"),
        };
        assert!(
            matches!(err, CacheError::EmptyPassphrase),
            "empty passphrase must fail fast, got: {err:?}"
        );
    }

    #[test]
    fn wrong_passphrase_cannot_decrypt_entries() {
        let dir = tempdir().unwrap();
        let stmt = sample_statement();
        {
            let cache = DocAiCache::open(dir.path().to_path_buf(), "alpha").unwrap();
            cache.put("key1", &stmt).unwrap();
        }
        let reopened = DocAiCache::open(dir.path().to_path_buf(), "beta").unwrap();
        assert!(
            reopened.get("key1").is_none(),
            "cache opened with the wrong passphrase must not decrypt entries"
        );
    }

    #[test]
    fn same_passphrase_reopens_with_persisted_salt() {
        let dir = tempdir().unwrap();
        let stmt = sample_statement();
        {
            let cache = DocAiCache::open(dir.path().to_path_buf(), "stable-pass").unwrap();
            cache.put("key1", &stmt).unwrap();
        }
        let reopened = DocAiCache::open(dir.path().to_path_buf(), "stable-pass").unwrap();
        assert!(
            reopened.get("key1").is_some(),
            "reopening with the same passphrase must reuse the persisted salt"
        );
    }

    #[test]
    fn salt_file_is_versioned_and_persisted_beside_cache() {
        let dir = tempdir().unwrap();
        let _ = DocAiCache::open(dir.path().to_path_buf(), "pw").unwrap();
        let data = std::fs::read(dir.path().join(KDF_SALT_FILE_NAME)).expect("kdf.salt must exist");
        assert_eq!(&data[0..4], &KDF_SALT_MAGIC, "salt file must carry magic");
        assert_eq!(
            data[4], KDF_SALT_FILE_VERSION,
            "salt file must be versioned"
        );
        assert_eq!(data.len(), 4 + 1 + KDF_SALT_LEN);
    }
}
