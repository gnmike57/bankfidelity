//! Session Audit Logging and Snapshot Management
//! Supports financial compliance by tracking all manual and automated changes.

use crate::engine::history::ChangeRecord;
use crate::error::{AuditError, AuditResult};
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

#[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct SnapshotManifest {
    schema_version: u32,
    change_id: u64,
    object_path: PathBuf,
    evidence: crate::engine::history::SnapshotEvidence,
}

pub struct AuditLog {
    db: Option<Connection>,
    log_path: PathBuf,
    snapshots_dir: PathBuf,
}

impl AuditLog {
    /// Opens the audit log directory and initializes the current session's log file.
    ///
    /// # Errors
    /// Returns [`AuditError::Open`] if the snapshots directory cannot be created.
    pub fn open(audit_dir: impl AsRef<Path>) -> AuditResult<Self> {
        let audit_dir = audit_dir.as_ref().to_path_buf();
        let snapshots_dir = audit_dir.join("snapshots");
        fs::create_dir_all(&snapshots_dir)
            .map_err(|e| AuditError::open(snapshots_dir.display().to_string(), e))?;

        let db_path = audit_dir.join("audit.sqlite");

        Ok(Self {
            db: None,
            log_path: db_path,
            snapshots_dir,
        })
    }

    pub fn write(
        &mut self,
        record: &ChangeRecord,
        source: &Path,
        output: &Path,
        operator: &str,
        requires_visual_review: bool,
    ) -> AuditResult<()> {
        self.ensure_open()?;

        // Phase 7: Write structured native JSON for maximum precision and security.
        #[derive(serde::Serialize)]
        struct AuditEvent<'a> {
            version: &'static str,
            operator: &'a str,
            source_pdf: &'a Path,
            output_pdf: &'a Path,
            requires_visual_review: bool,
            #[serde(flatten)]
            record: &'a ChangeRecord,
        }

        let event = AuditEvent {
            version: "audit_v2_json",
            operator,
            source_pdf: source,
            output_pdf: output,
            requires_visual_review,
            record,
        };

        let json_line = serde_json::to_string(&event)
            .map_err(|error| AuditError::Write(std::io::Error::other(error.to_string())))?;
        let ts = record.timestamp.replace(':', "");
        let event_path = self
            .snapshots_dir
            .join(format!("{ts}-{}.audit.json", record.id));
        write_json_atomic(&event_path, &event)?;

        let timestamp = Utc::now().to_rfc3339();
        if let Err(error) = self.db.as_ref().unwrap().execute(
            "INSERT INTO audit_log (timestamp, action, details) VALUES (?1, ?2, ?3)",
            params![timestamp, "write", json_line],
        ) {
            if let Err(cleanup_error) = fs::remove_file(&event_path) {
                tracing::error!(
                    "[audit] failed to remove staged event {} after database failure: {}",
                    event_path.display(),
                    cleanup_error
                );
            }
            return Err(AuditError::Write(std::io::Error::other(error.to_string())));
        }

        Ok(())
    }

    /// Stage 12 / Item #4: append an arbitrary single-line event to the
    /// audit log. The runtime uses this to record cascade invocations
    /// (which don't fit the `ChangeRecord` shape but still need an audit
    /// trail). The line is written verbatim with a trailing newline.
    ///
    /// # Errors
    /// Returns [`AuditError::Write`] if the log file cannot be opened or written.
    pub fn append_line(&mut self, line: &str) -> AuditResult<()> {
        self.ensure_open()?;
        let timestamp = Utc::now().to_rfc3339();
        self.db
            .as_ref()
            .unwrap()
            .execute(
                "INSERT INTO audit_log (timestamp, action, details) VALUES (?1, ?2, ?3)",
                params![timestamp, "append_line", line.trim()],
            )
            .map_err(|e| AuditError::Write(std::io::Error::other(e.to_string())))?;
        Ok(())
    }

    /// Lazily opens (creating if needed) the session log file in append mode.
    fn ensure_open(&mut self) -> AuditResult<()> {
        if self.db.is_none() {
            let conn = Connection::open(&self.log_path).map_err(|e| {
                AuditError::open(
                    self.log_path.display().to_string(),
                    std::io::Error::other(e.to_string()),
                )
            })?;

            conn.execute(
                "CREATE TABLE IF NOT EXISTS audit_log (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp TEXT NOT NULL,
                    action TEXT NOT NULL,
                    details TEXT NOT NULL
                )",
                [],
            )
            .map_err(|e| {
                AuditError::open(
                    self.log_path.display().to_string(),
                    std::io::Error::other(e.to_string()),
                )
            })?;

            // Automated Database Vacuuming: Prune records older than 30 days
            let _ = conn.execute(
                "DELETE FROM audit_log WHERE datetime(timestamp) < datetime('now', '-30 days')",
                [],
            );

            // Prune snapshot files older than 30 days
            if let Ok(entries) = std::fs::read_dir(&self.snapshots_dir) {
                let cutoff =
                    std::time::SystemTime::now() - std::time::Duration::from_secs(30 * 24 * 3600);
                for entry in entries.flatten() {
                    if let Ok(meta) = entry.metadata() {
                        if let Ok(modified) = meta.modified() {
                            if modified < cutoff {
                                let _ = std::fs::remove_file(entry.path());
                            }
                        }
                    }
                }
            }

            self.db = Some(conn);
        }
        Ok(())
    }

    pub fn create_content_addressed_snapshot(
        &self,
        change_id: u64,
        source: &Path,
        parent_source: Option<&Path>,
    ) -> AuditResult<(PathBuf, crate::engine::history::SnapshotEvidence)> {
        let (sha256, size_bytes) = sha256_file(source)?;
        let objects_dir = self.snapshots_dir.join("objects");
        fs::create_dir_all(&objects_dir)
            .map_err(|error| AuditError::snapshot(objects_dir.display().to_string(), error))?;
        let object_path = objects_dir.join(format!("{sha256}.pdf"));

        if object_path.exists() {
            verify_snapshot_file(&object_path, &sha256, size_bytes)?;
        } else {
            snapshot_link_or_copy(source, &object_path)?;
            verify_snapshot_file(&object_path, &sha256, size_bytes)?;
        }

        let parent_sha256 = match parent_source {
            Some(parent) => Some(sha256_file(parent)?.0),
            None => None,
        };
        let manifest_path = self
            .snapshots_dir
            .join(format!("{change_id}.snapshot.json"));
        let evidence = crate::engine::history::SnapshotEvidence {
            sha256,
            size_bytes,
            parent_sha256,
            created_at: Utc::now().to_rfc3339(),
            manifest_path: manifest_path.clone(),
        };
        let manifest = SnapshotManifest {
            schema_version: 1,
            change_id,
            object_path: object_path.clone(),
            evidence: evidence.clone(),
        };
        write_json_atomic(&manifest_path, &manifest)?;

        Ok((object_path, evidence))
    }

    pub fn verify_artifact_matches_snapshot(
        &self,
        artifact: &Path,
        evidence: &crate::engine::history::SnapshotEvidence,
    ) -> AuditResult<()> {
        verify_snapshot_file(artifact, &evidence.sha256, evidence.size_bytes)
    }

    pub fn verify_snapshot_record(&self, record: &ChangeRecord) -> AuditResult<()> {
        let path = record.snapshot_path.as_ref().ok_or_else(|| {
            AuditError::snapshot(
                format!("change {} snapshot path", record.id),
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "change record has no snapshot path",
                ),
            )
        })?;
        let evidence = record.snapshot_evidence.as_ref().ok_or_else(|| {
            AuditError::snapshot(
                format!("change {} snapshot evidence", record.id),
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "change record has no snapshot evidence",
                ),
            )
        })?;
        verify_snapshot_file(path, &evidence.sha256, evidence.size_bytes)?;

        let manifest_bytes = fs::read(&evidence.manifest_path).map_err(|error| {
            AuditError::snapshot(evidence.manifest_path.display().to_string(), error)
        })?;
        let manifest: SnapshotManifest =
            serde_json::from_slice(&manifest_bytes).map_err(|error| {
                AuditError::snapshot(
                    evidence.manifest_path.display().to_string(),
                    std::io::Error::new(std::io::ErrorKind::InvalidData, error),
                )
            })?;
        if manifest.change_id != record.id
            || manifest.object_path != *path
            || manifest.evidence != *evidence
        {
            return Err(AuditError::snapshot(
                evidence.manifest_path.display().to_string(),
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "snapshot manifest does not match change record",
                ),
            ));
        }
        Ok(())
    }

    pub fn snapshots_dir(&self) -> &Path {
        &self.snapshots_dir
    }

    /// Returns the legacy path where a snapshot for a specific change ID was stored.
    pub fn snapshot_path_for(&self, change_id: u64) -> PathBuf {
        self.snapshots_dir.join(format!("{change_id}.pdf"))
    }
}

fn sha256_file(path: &Path) -> AuditResult<(String, u64)> {
    let mut file = File::open(path)
        .map_err(|error| AuditError::snapshot(path.display().to_string(), error))?;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| AuditError::snapshot(path.display().to_string(), error))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        total += read as u64;
    }
    Ok((format!("{:x}", hasher.finalize()), total))
}

fn verify_snapshot_file(path: &Path, expected_sha256: &str, expected_size: u64) -> AuditResult<()> {
    let (actual_sha256, actual_size) = sha256_file(path)?;
    if actual_sha256 != expected_sha256 || actual_size != expected_size {
        return Err(AuditError::snapshot(
            path.display().to_string(),
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "snapshot evidence mismatch: expected sha256={expected_sha256} size={expected_size}, actual sha256={actual_sha256} size={actual_size}"
                ),
            ),
        ));
    }
    Ok(())
}

fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T) -> AuditResult<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| AuditError::snapshot(parent.display().to_string(), error))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| AuditError::snapshot(parent.display().to_string(), error))?;
    serde_json::to_writer_pretty(temporary.as_file_mut(), value).map_err(|error| {
        AuditError::snapshot(
            path.display().to_string(),
            std::io::Error::new(std::io::ErrorKind::InvalidData, error),
        )
    })?;
    temporary
        .as_file_mut()
        .sync_all()
        .map_err(|error| AuditError::snapshot(path.display().to_string(), error))?;
    if path.exists() {
        fs::remove_file(path)
            .map_err(|error| AuditError::snapshot(path.display().to_string(), error))?;
    }
    temporary
        .persist(path)
        .map_err(|error| AuditError::snapshot(path.display().to_string(), error.error))?;
    Ok(())
}

/// Save an immutable snapshot of `source` at `dest`.
///
/// The compatibility name is retained for existing callers, but hard links
/// are intentionally forbidden: a snapshot must have independent storage so
/// later in-place edits to the source cannot rewrite historical evidence.
/// Bytes are copied to a temporary file in the destination directory, synced,
/// and then persisted to the final path.
///
/// Returns `Ok(false)` to preserve the historical return contract while
/// explicitly reporting that no hard link was created.
///
/// # Errors
/// Returns [`AuditError::Snapshot`] when the source cannot be opened, the
/// independent copy cannot be written and synced, or the destination cannot be
/// replaced.
pub fn snapshot_link_or_copy(source: &Path, dest: &Path) -> AuditResult<bool> {
    let parent = dest.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| AuditError::snapshot(parent.display().to_string(), error))?;

    let mut source_file = File::open(source)
        .map_err(|error| AuditError::snapshot(source.display().to_string(), error))?;
    let source_len = source_file
        .metadata()
        .map_err(|error| AuditError::snapshot(source.display().to_string(), error))?
        .len();
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| AuditError::snapshot(parent.display().to_string(), error))?;
    let copied = std::io::copy(&mut source_file, temporary.as_file_mut())
        .map_err(|error| AuditError::snapshot(dest.display().to_string(), error))?;
    if copied != source_len {
        return Err(AuditError::snapshot(
            dest.display().to_string(),
            std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!("snapshot copied {copied}/{source_len} bytes"),
            ),
        ));
    }
    temporary
        .as_file_mut()
        .sync_all()
        .map_err(|error| AuditError::snapshot(dest.display().to_string(), error))?;

    if dest.exists() {
        fs::remove_file(dest)
            .map_err(|error| AuditError::snapshot(dest.display().to_string(), error))?;
    }
    temporary
        .persist(dest)
        .map_err(|error| AuditError::snapshot(dest.display().to_string(), error.error))?;

    Ok(false)
}

pub struct AuditLogParser;

impl AuditLogParser {
    /// Parses an audit log file into a list of [`ChangeRecord`]s.
    ///
    /// # Errors
    /// Returns [`AuditError::Read`] if the file cannot be opened or a line
    /// cannot be read.
    pub fn parse_file(path: &Path) -> AuditResult<Vec<ChangeRecord>> {
        // Try parsing as SQLite first
        if let Ok(conn) =
            rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        {
            if let Ok(mut stmt) =
                conn.prepare("SELECT details FROM audit_log WHERE action = 'write'")
            {
                let records_iter = stmt.query_map([], |row| {
                    let details: String = row.get(0)?;
                    Ok(details)
                });

                if let Ok(iter) = records_iter {
                    let mut records = Vec::new();
                    for details in iter.flatten() {
                        if let Ok(record) = serde_json::from_str::<ChangeRecord>(&details) {
                            records.push(record);
                        }
                    }
                    if !records.is_empty() {
                        return Ok(records);
                    }
                }
            }
        }

        // Fallback to legacy flat file parsing
        let file = File::open(path).map_err(|e| AuditError::read(path.display().to_string(), e))?;
        let reader = BufReader::new(file);
        let mut records = Vec::new();

        for line in reader.lines() {
            let line = line.map_err(|e| AuditError::read(path.display().to_string(), e))?;
            if !line.starts_with("audit_v1") {
                continue;
            }

            if let Some(record) = Self::parse_line(&line) {
                records.push(record);
            }
        }

        Ok(records)
    }

    fn parse_line(line: &str) -> Option<ChangeRecord> {
        // audit_v1 ts=... page=... id=... old=... new=... op=... prov=... desc=... snap=... bbox=[...] in=... out=... review=...
        let mut id = None;
        let mut timestamp = None;
        let mut page = None;
        let mut old_text = None;
        let mut new_text = None;
        let mut bbox = None;
        let mut provenance = "Manual".to_string();
        let mut description = String::new();
        let mut snapshot_path = None;

        // Simple state machine parser
        let mut rest = line.trim();
        if !rest.starts_with("audit_v1 ") {
            return None;
        }
        rest = &rest["audit_v1 ".len()..];

        while !rest.is_empty() {
            rest = rest.trim_start();
            let eq_idx = match rest.find('=') {
                Some(idx) => idx,
                None => break,
            };
            let key = &rest[..eq_idx];
            rest = &rest[eq_idx + 1..];

            // If it's a JSON string, use serde to parse it
            if rest.starts_with('"') {
                let mut end_idx = 1;
                let mut escaped = false;
                while end_idx < rest.len() {
                    let c = rest.as_bytes()[end_idx];
                    if escaped {
                        escaped = false;
                    } else if c == b'\\' {
                        escaped = true;
                    } else if c == b'"' {
                        end_idx += 1;
                        break;
                    }
                    end_idx += 1;
                }

                let val_str = &rest[..end_idx];
                rest = &rest[end_idx..];

                if key == "old" {
                    old_text = serde_json::from_str::<String>(val_str).ok();
                } else if key == "new" {
                    new_text = serde_json::from_str::<String>(val_str).ok();
                } else if key == "desc" {
                    description = serde_json::from_str::<String>(val_str).unwrap_or_default();
                } else if key == "snap" {
                    let s = serde_json::from_str::<String>(val_str).unwrap_or_default();
                    if !s.is_empty() {
                        snapshot_path = Some(PathBuf::from(s));
                    }
                }
            } else {
                // Read until space
                let end_idx = rest.find(' ').unwrap_or(rest.len());
                let val_str = &rest[..end_idx];
                rest = &rest[end_idx..];

                match key {
                    "id" => id = val_str.parse().ok(),
                    "ts" => timestamp = Some(val_str.to_string()),
                    "page" => page = val_str.parse().ok(),
                    "prov" => provenance = val_str.to_string(),
                    "bbox" => {
                        let clean = val_str.trim_matches(|c| c == '[' || c == ']');
                        let parts: Vec<f32> =
                            clean.split(',').filter_map(|p| p.parse().ok()).collect();
                        if parts.len() == 4 {
                            bbox = Some([parts[0], parts[1], parts[2], parts[3]]);
                        }
                    }
                    _ => {}
                }
            }
        }

        Some(ChangeRecord {
            id: id?,
            timestamp: timestamp?,
            page: page?,
            old_text: old_text?,
            new_text: new_text?,
            bbox: bbox?,
            description,
            snapshot_path,
            snapshot_evidence: None,
            provenance,
            obj_id: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn roundtrip_records() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let mut audit = AuditLog::open(dir.path())?;

        let rec1 = ChangeRecord {
            id: 123,
            timestamp: "ts".into(),
            page: 1,
            old_text: "foo".into(),
            new_text: "bar".into(),
            bbox: [0.0, 1.0, 2.0, 3.0],
            description: "Adjustment".into(),
            snapshot_path: Some(PathBuf::from("audit/snapshots/123.pdf")),
            snapshot_evidence: None,
            provenance: "DocumentAI".into(),
            obj_id: None,
        };

        audit.write(&rec1, Path::new("in"), Path::new("out"), "test", false)?;

        let conn = rusqlite::Connection::open(&audit.log_path)?;
        let mut stmt =
            conn.prepare("SELECT details FROM audit_log WHERE action = 'write' LIMIT 1")?;
        let details: String = stmt.query_row([], |row| row.get(0))?;

        let v: serde_json::Value = serde_json::from_str(&details)?;
        assert_eq!(v["id"], 123);
        assert_eq!(v["old_text"], "foo");
        assert_eq!(v["description"], "Adjustment");
        assert_eq!(v["provenance"], "DocumentAI");

        let parsed = AuditLogParser::parse_file(&audit.log_path).map_err(|e| anyhow::anyhow!(e))?;
        assert_eq!(parsed.len(), 1);
        Ok(())
    }

    #[test]
    fn value_containing_key_prefix() -> anyhow::Result<()> {
        let line = r#"audit_v1 ts=20260526t120000Z page=0 id=456 old="text with id= inside" new="text with ts= inside" op=test prov=Manual bbox=[0,0,0,0] in="in" out="out" review=false"#;
        let rec =
            AuditLogParser::parse_line(line).ok_or_else(|| anyhow::anyhow!("parse failed"))?;
        assert_eq!(rec.id, 456);
        assert_eq!(rec.old_text, "text with id= inside");
        assert_eq!(rec.new_text, "text with ts= inside");
        Ok(())
    }

    #[test]
    fn snapshot_is_an_independent_immutable_copy() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let source = dir.path().join("source.pdf");
        let payload = b"%PDF-1.7\nfake snapshot content";
        std::fs::write(&source, payload)?;

        let dest = dir.path().join("snapshots").join("123.pdf");
        let was_hard_link = snapshot_link_or_copy(&source, &dest)?;
        assert!(!was_hard_link, "audit snapshots must never be hard linked");
        assert_eq!(std::fs::read(&dest)?, payload);

        std::fs::write(&source, b"modified source bytes")?;
        assert_eq!(
            std::fs::read(&dest)?,
            payload,
            "later source rewrites must not mutate historical evidence"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_ne!(
                std::fs::metadata(&source)?.ino(),
                std::fs::metadata(&dest)?.ino(),
                "source and snapshot must use independent inodes"
            );
        }
        Ok(())
    }

    #[test]
    fn content_addressed_snapshot_records_and_verifies_evidence() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let audit = AuditLog::open(dir.path().join("audit"))?;
        let parent = dir.path().join("parent.pdf");
        let source = dir.path().join("edited.pdf");
        std::fs::write(&parent, b"parent bytes")?;
        std::fs::write(&source, b"edited immutable bytes")?;

        let (object_path, evidence) =
            audit.create_content_addressed_snapshot(42, &source, Some(&parent))?;
        let expected_object_name = format!("{}.pdf", evidence.sha256);
        assert_eq!(
            object_path.file_name().and_then(|name| name.to_str()),
            Some(expected_object_name.as_str())
        );
        assert_eq!(evidence.size_bytes, b"edited immutable bytes".len() as u64);
        assert!(evidence.parent_sha256.is_some());
        assert!(evidence.manifest_path.is_file());

        let record = ChangeRecord {
            id: 42,
            timestamp: Utc::now().to_rfc3339(),
            page: 0,
            old_text: "old".into(),
            new_text: "new".into(),
            bbox: [0.0; 4],
            description: "snapshot test".into(),
            snapshot_path: Some(object_path.clone()),
            snapshot_evidence: Some(evidence),
            provenance: "test".into(),
            obj_id: None,
        };
        audit.verify_snapshot_record(&record)?;

        std::fs::write(&source, b"later live-output rewrite")?;
        audit.verify_snapshot_record(&record)?;
        assert_eq!(std::fs::read(&object_path)?, b"edited immutable bytes");
        Ok(())
    }

    #[test]
    fn snapshot_verification_rejects_tamper_and_missing_objects() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let audit = AuditLog::open(dir.path().join("audit"))?;
        let source = dir.path().join("edited.pdf");
        std::fs::write(&source, b"verified bytes")?;
        let (object_path, evidence) = audit.create_content_addressed_snapshot(7, &source, None)?;
        let record = ChangeRecord {
            id: 7,
            timestamp: Utc::now().to_rfc3339(),
            page: 0,
            old_text: "old".into(),
            new_text: "new".into(),
            bbox: [0.0; 4],
            description: "snapshot test".into(),
            snapshot_path: Some(object_path.clone()),
            snapshot_evidence: Some(evidence),
            provenance: "test".into(),
            obj_id: None,
        };

        std::fs::write(&object_path, b"tampered")?;
        assert!(audit.verify_snapshot_record(&record).is_err());
        std::fs::remove_file(&object_path)?;
        assert!(audit.verify_snapshot_record(&record).is_err());
        Ok(())
    }

    #[test]
    fn parse_file_missing_returns_read_error() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let missing = dir.path().join("does_not_exist.log");
        let err = AuditLogParser::parse_file(&missing)
            .map_err(|e| anyhow::anyhow!(e))
            .unwrap_err();
        assert!(
            matches!(
                err.downcast_ref::<AuditError>(),
                Some(AuditError::Read { .. })
            ),
            "expected AuditError::Read, got {err:?}"
        );
        // The error message should carry the offending path for diagnosis.
        assert!(err.to_string().contains("does_not_exist.log"));
        Ok(())
    }

    #[test]
    fn snapshot_link_overwrites_existing_destination() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let source = dir.path().join("source.pdf");
        std::fs::write(&source, b"new content")?;
        let dest = dir.path().join("snapshots").join("456.pdf");
        std::fs::create_dir_all(dest.parent().ok_or_else(|| anyhow::anyhow!("No parent"))?)?;
        std::fs::write(&dest, b"OLD STALE CONTENT")?;

        snapshot_link_or_copy(&source, &dest)?;
        assert_eq!(std::fs::read(&dest)?, b"new content");
        Ok(())
    }
}
