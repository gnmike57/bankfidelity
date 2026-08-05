//! Undo / Redo + Change History System
//! Supports multi-page bank statements with visual snapshots.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotEvidence {
    pub sha256: String,
    pub size_bytes: u64,
    pub parent_sha256: Option<String>,
    pub created_at: String,
    pub manifest_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeRecord {
    pub id: u64,
    pub timestamp: String,
    pub page: usize,
    pub old_text: String,
    pub new_text: String,
    pub bbox: [f32; 4],
    pub description: String,
    pub snapshot_path: Option<PathBuf>,
    #[serde(default)]
    pub snapshot_evidence: Option<SnapshotEvidence>,
    pub provenance: String,
    pub obj_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChangeHistory {
    history: Vec<ChangeRecord>,
    current_index: usize, // Points to the next change to redo (index in history)
    #[serde(skip)]
    next_id: AtomicU64,
}

// Custom Clone implementation for ChangeHistory due to AtomicU64
impl Clone for ChangeHistory {
    fn clone(&self) -> Self {
        Self {
            history: self.history.clone(),
            current_index: self.current_index,
            next_id: AtomicU64::new(self.next_id.load(Ordering::SeqCst)),
        }
    }
}

impl Default for ChangeHistory {
    fn default() -> Self {
        Self::new()
    }
}

impl ChangeHistory {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
            current_index: 0,
            next_id: AtomicU64::new(1),
        }
    }

    pub fn current_index(&self) -> usize {
        self.current_index
    }

    pub fn push_change(
        &mut self,
        page: usize,
        old_text: String,
        new_text: String,
        bbox: [f32; 4],
        description: String,
    ) {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let timestamp = chrono::Utc::now().to_rfc3339();

        let record = ChangeRecord {
            id,
            timestamp,
            page,
            old_text,
            new_text,
            bbox,
            description,
            snapshot_path: None,
            snapshot_evidence: None,
            provenance: "Manual".into(),
            obj_id: None,
        };

        self.history.truncate(self.current_index);
        self.history.push(record);
        self.current_index = self.history.len();
    }

    pub fn create_record(
        &self,
        page: usize,
        old_text: String,
        new_text: String,
        bbox: [f32; 4],
        description: String,
        snapshot_path: Option<PathBuf>,
    ) -> ChangeRecord {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let timestamp = chrono::Utc::now().to_rfc3339();

        ChangeRecord {
            id,
            timestamp,
            page,
            old_text,
            new_text,
            bbox,
            description,
            snapshot_path,
            snapshot_evidence: None,
            provenance: "Manual".into(),
            obj_id: None,
        }
    }

    pub fn push_record(&mut self, record: ChangeRecord) {
        self.history.truncate(self.current_index);
        self.history.push(record);
        self.current_index = self.history.len();
    }

    /// Persist the current history to disk (atomic-ish: write tmp + rename).
    pub fn save_to_file(&self, path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, self.to_json_pretty_string())?;
        std::fs::rename(tmp, path)
    }

    /// Load history from a previously-saved JSON file.
    pub fn load_from_file(path: &std::path::Path) -> std::io::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let val: serde_json::Value = serde_json::from_str(&raw).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid history JSON: {e}"),
            )
        })?;
        let mut history = Self::new();
        if let Some(arr) = val.get("changes").and_then(|c| c.as_array()) {
            for entry in arr {
                if let Ok(rec) = serde_json::from_value::<ChangeRecord>(entry.clone()) {
                    history.history.push(rec);
                }
            }
            history.current_index = history.history.len();
            // Advance next_id beyond max(id) so future entries are unique.
            let max_id = history.history.iter().map(|r| r.id).max().unwrap_or(0);
            history.next_id = AtomicU64::new(max_id + 1);
        }
        Ok(history)
    }

    pub fn push_change_with_snapshot(
        &mut self,
        page: usize,
        old_text: String,
        new_text: String,
        bbox: [f32; 4],
        description: String,
        snapshot_path: PathBuf,
    ) -> ChangeRecord {
        let record = self.create_record(
            page,
            old_text,
            new_text,
            bbox,
            description,
            Some(snapshot_path),
        );
        self.push_record(record.clone());
        record
    }

    pub fn undo(&mut self) -> Option<ChangeRecord> {
        if self.current_index == 0 {
            return None;
        }

        self.current_index -= 1;
        Some(self.history[self.current_index].clone())
    }

    pub fn redo(&mut self) -> Option<ChangeRecord> {
        if self.current_index >= self.history.len() {
            return None;
        }

        let record = self.history[self.current_index].clone();
        self.current_index += 1;
        Some(record)
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "changes": self.history
        })
    }

    pub fn to_json_pretty_string(&self) -> String {
        serde_json::to_string_pretty(&self.to_json()).unwrap_or_default()
    }

    pub fn get_history(&self) -> Vec<ChangeRecord> {
        self.history.clone()
    }

    pub fn can_undo(&self) -> bool {
        self.current_index > 0
    }

    pub fn can_redo(&self) -> bool {
        self.current_index < self.history.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_monotonic_within_history() {
        let mut history = ChangeHistory::new();
        history.push_change(0, "old".into(), "new".into(), [0.0; 4], "desc".into());
        let id1 = history.get_history()[0].id;

        let rec2 = history.create_record(
            0,
            "old2".into(),
            "new2".into(),
            [0.0; 4],
            "desc2".into(),
            None,
        );
        let id2 = rec2.id;

        assert!(id2 > id1);
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn save_and_load_round_trip_preserves_records_and_id_counter() -> anyhow::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("history.json");

        let mut original = ChangeHistory::new();
        original.push_change(0, "old1".into(), "new1".into(), [0.0; 4], "first".into());
        original.push_change(
            1,
            "old2".into(),
            "new2".into(),
            [1.0, 2.0, 3.0, 4.0],
            "second".into(),
        );
        original.save_to_file(&path)?;

        let loaded = ChangeHistory::load_from_file(&path)?;
        assert_eq!(loaded.get_history().len(), 2);
        assert_eq!(loaded.get_history()[1].new_text, "new2");

        // Append after load -> id must be > existing max.
        let loaded_mut = loaded.clone();
        let new_record =
            loaded_mut.create_record(2, "x".into(), "y".into(), [0.0; 4], "third".into(), None);
        let max_old_id = loaded
            .get_history()
            .iter()
            .map(|r| r.id)
            .max()
            .unwrap_or_default();
        assert!(new_record.id > max_old_id);
        drop(loaded_mut);
        Ok(())
    }
}
