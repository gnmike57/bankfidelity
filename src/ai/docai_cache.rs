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
//! Entries never expire automatically. Run `dual-core docai-cache prune` to
//! garbage-collect (future CLI subcommand). Until then, callers are
//! free to delete `audit/cache/docai/` whenever they want a clean slate.

use std::path::{Path, PathBuf};

use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::ai::document_ai::BankStatement;

const CACHE_FORMAT_VERSION: u32 = 1;

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
        std::fs::create_dir_all(&root)?;
        let mut hasher = Sha256::new();
        hasher.update(passphrase.as_bytes());
        let key_bytes = hasher.finalize();
        let cipher = ChaCha20Poly1305::new(&key_bytes);
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

#[cfg(test)]
mod tests {
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
}
