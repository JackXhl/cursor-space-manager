//! Durable record of what the tool intended to do and what actually happened.
//!
//! Every action that touches the filesystem writes an `intent` row first and a
//! `result` row afterwards. If the process dies in between, the next launch
//! sees an intent with no result and knows exactly which step to reconcile
//! against the real filesystem instead of guessing.

use crate::core::model::*;
use crate::error::{AppError, AppResult};
use crate::util::now_millis;

use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepKind {
    Intent,
    Result,
}

impl StepKind {
    fn as_str(&self) -> &'static str {
        match self {
            StepKind::Intent => "intent",
            StepKind::Result => "result",
        }
    }
}

/// Externally visible side effects. Each one is journalled around.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepAction {
    CreateStaging,
    CopyToStaging,
    VerifyStaging,
    PromoteStaging,
    RenameSourceToBackup,
    CreateLink,
    WriteProbe,
    RemoveBackup,
    RollbackRestoreSource,
    RollbackRemoveLink,
    RollbackRemoveTarget,
    CleanupStaging,
}

impl StepAction {
    fn as_str(&self) -> &'static str {
        match self {
            StepAction::CreateStaging => "createStaging",
            StepAction::CopyToStaging => "copyToStaging",
            StepAction::VerifyStaging => "verifyStaging",
            StepAction::PromoteStaging => "promoteStaging",
            StepAction::RenameSourceToBackup => "renameSourceToBackup",
            StepAction::CreateLink => "createLink",
            StepAction::WriteProbe => "writeProbe",
            StepAction::RemoveBackup => "removeBackup",
            StepAction::RollbackRestoreSource => "rollbackRestoreSource",
            StepAction::RollbackRemoveLink => "rollbackRemoveLink",
            StepAction::RollbackRemoveTarget => "rollbackRemoveTarget",
            StepAction::CleanupStaging => "cleanupStaging",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalEvent {
    pub id: i64,
    pub operation_id: String,
    pub root_id: Option<String>,
    pub at: i64,
    pub kind: StepKind,
    pub action: String,
    pub detail: Option<String>,
    pub ok: Option<bool>,
}

/// An action recorded as intended but never resolved. This is what recovery
/// works from.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DanglingIntent {
    pub operation_id: String,
    pub root_id: Option<String>,
    pub action: String,
    pub at: i64,
    pub detail: Option<String>,
}

pub struct Journal {
    connection: Mutex<Connection>,
    path: PathBuf,
}

impl Journal {
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        // WAL keeps the journal readable while a long migration is writing, and
        // FULL synchronous means an intent row survives a power loss.
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;

        let journal = Self {
            connection: Mutex::new(connection),
            path: path.to_path_buf(),
        };
        journal.migrate()?;
        Ok(journal)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn migrate(&self) -> AppResult<()> {
        let connection = self.connection.lock();
        connection.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS schema_meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS scans (
                scan_id     TEXT PRIMARY KEY,
                created_at  INTEGER NOT NULL,
                report_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS operations (
                operation_id       TEXT PRIMARY KEY,
                created_at         INTEGER NOT NULL,
                updated_at         INTEGER NOT NULL,
                state              TEXT NOT NULL,
                plan_json          TEXT NOT NULL,
                target_root        TEXT NOT NULL,
                total_bytes        INTEGER NOT NULL DEFAULT 0,
                source_free_before INTEGER NOT NULL DEFAULT 0,
                source_free_after  INTEGER NOT NULL DEFAULT 0,
                backups_retained   INTEGER NOT NULL DEFAULT 1,
                error              TEXT
            );

            CREATE TABLE IF NOT EXISTS operation_roots (
                operation_id TEXT NOT NULL,
                root_id      TEXT NOT NULL,
                label        TEXT NOT NULL,
                source_path  TEXT NOT NULL,
                target_path  TEXT NOT NULL,
                backup_path  TEXT NOT NULL,
                staging_path TEXT NOT NULL,
                state        TEXT NOT NULL,
                bytes        INTEGER NOT NULL DEFAULT 0,
                files        INTEGER NOT NULL DEFAULT 0,
                verified     INTEGER NOT NULL DEFAULT 0,
                link_ok      INTEGER NOT NULL DEFAULT 0,
                error        TEXT,
                PRIMARY KEY (operation_id, root_id),
                FOREIGN KEY (operation_id) REFERENCES operations(operation_id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS events (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                operation_id TEXT NOT NULL,
                root_id      TEXT,
                at           INTEGER NOT NULL,
                kind         TEXT NOT NULL,
                action       TEXT NOT NULL,
                detail       TEXT,
                ok           INTEGER,
                FOREIGN KEY (operation_id) REFERENCES operations(operation_id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_events_operation ON events(operation_id, id);
            CREATE INDEX IF NOT EXISTS idx_operations_state ON operations(state);
            "#,
        )?;
        connection.execute(
            "INSERT OR REPLACE INTO schema_meta (key, value) VALUES ('version', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )?;
        Ok(())
    }

    // -- scans -------------------------------------------------------------

    pub fn record_scan(&self, report: &ScanReport) -> AppResult<()> {
        let json = serde_json::to_string(report)?;
        let connection = self.connection.lock();
        connection.execute(
            "INSERT OR REPLACE INTO scans (scan_id, created_at, report_json) VALUES (?1, ?2, ?3)",
            params![report.scan_id, report.scanned_at, json],
        )?;
        // Old snapshots have no value once a newer one exists.
        connection.execute(
            "DELETE FROM scans WHERE scan_id NOT IN (
                 SELECT scan_id FROM scans ORDER BY created_at DESC LIMIT 10
             )",
            [],
        )?;
        Ok(())
    }

    pub fn latest_scan(&self) -> AppResult<Option<ScanReport>> {
        let connection = self.connection.lock();
        let json: Option<String> = connection
            .query_row(
                "SELECT report_json FROM scans ORDER BY created_at DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        match json {
            Some(json) => Ok(Some(serde_json::from_str(&json)?)),
            None => Ok(None),
        }
    }

    // -- operations --------------------------------------------------------

    pub fn create_operation(&self, plan: &MigrationPlan) -> AppResult<()> {
        let json = serde_json::to_string(plan)?;
        let mut connection = self.connection.lock();
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO operations
                 (operation_id, created_at, updated_at, state, plan_json, target_root, total_bytes)
             VALUES (?1, ?2, ?2, ?3, ?4, ?5, ?6)",
            params![
                plan.operation_id,
                plan.created_at,
                MigrationState::PlanReady.as_str(),
                json,
                plan.target_root.to_string_lossy(),
                plan.total_bytes as i64,
            ],
        )?;
        for root in &plan.roots {
            transaction.execute(
                "INSERT INTO operation_roots
                     (operation_id, root_id, label, source_path, target_path, backup_path,
                      staging_path, state, bytes, files)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    plan.operation_id,
                    root.root_id,
                    root.label,
                    root.source_path.to_string_lossy(),
                    root.target_path.to_string_lossy(),
                    root.backup_path.to_string_lossy(),
                    root.staging_path.to_string_lossy(),
                    MigrationState::PlanReady.as_str(),
                    root.estimated_bytes as i64,
                    root.estimated_files as i64,
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn set_operation_state(&self, operation_id: &str, state: MigrationState) -> AppResult<()> {
        let connection = self.connection.lock();
        connection.execute(
            "UPDATE operations SET state = ?2, updated_at = ?3 WHERE operation_id = ?1",
            params![operation_id, state.as_str(), now_millis()],
        )?;
        Ok(())
    }

    pub fn set_operation_error(&self, operation_id: &str, error: &str) -> AppResult<()> {
        let connection = self.connection.lock();
        connection.execute(
            "UPDATE operations SET error = ?2, updated_at = ?3 WHERE operation_id = ?1",
            params![operation_id, error, now_millis()],
        )?;
        Ok(())
    }

    pub fn set_space_snapshot(&self, operation_id: &str, before: u64, after: u64) -> AppResult<()> {
        let connection = self.connection.lock();
        connection.execute(
            "UPDATE operations SET source_free_before = ?2, source_free_after = ?3, updated_at = ?4
             WHERE operation_id = ?1",
            params![operation_id, before as i64, after as i64, now_millis()],
        )?;
        Ok(())
    }

    pub fn set_backups_retained(&self, operation_id: &str, retained: bool) -> AppResult<()> {
        let connection = self.connection.lock();
        connection.execute(
            "UPDATE operations SET backups_retained = ?2, updated_at = ?3 WHERE operation_id = ?1",
            params![operation_id, retained as i64, now_millis()],
        )?;
        Ok(())
    }

    pub fn set_root_state(
        &self,
        operation_id: &str,
        root_id: &str,
        state: MigrationState,
    ) -> AppResult<()> {
        let connection = self.connection.lock();
        connection.execute(
            "UPDATE operation_roots SET state = ?3 WHERE operation_id = ?1 AND root_id = ?2",
            params![operation_id, root_id, state.as_str()],
        )?;
        Ok(())
    }

    // The columns are what they are; bundling them into a struct here would only
    // move the same list one level away from the SQL that consumes it.
    #[allow(clippy::too_many_arguments)]
    pub fn update_root_outcome(
        &self,
        operation_id: &str,
        root_id: &str,
        bytes: u64,
        files: u64,
        verified: bool,
        link_ok: bool,
        error: Option<&str>,
    ) -> AppResult<()> {
        let connection = self.connection.lock();
        connection.execute(
            "UPDATE operation_roots
             SET bytes = ?3, files = ?4, verified = ?5, link_ok = ?6, error = ?7
             WHERE operation_id = ?1 AND root_id = ?2",
            params![
                operation_id,
                root_id,
                bytes as i64,
                files as i64,
                verified as i64,
                link_ok as i64,
                error
            ],
        )?;
        Ok(())
    }

    // -- intent / result ---------------------------------------------------

    /// Records that we are about to perform `action`. Must be called *before*
    /// the filesystem is touched.
    pub fn record_intent(
        &self,
        operation_id: &str,
        root_id: Option<&str>,
        action: StepAction,
        detail: Option<&str>,
    ) -> AppResult<i64> {
        let connection = self.connection.lock();
        connection.execute(
            "INSERT INTO events (operation_id, root_id, at, kind, action, detail)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                operation_id,
                root_id,
                now_millis(),
                StepKind::Intent.as_str(),
                action.as_str(),
                detail
            ],
        )?;
        Ok(connection.last_insert_rowid())
    }

    /// Records the observed outcome of `action`, read back from the filesystem
    /// rather than assumed from the call returning.
    pub fn record_result(
        &self,
        operation_id: &str,
        root_id: Option<&str>,
        action: StepAction,
        ok: bool,
        detail: Option<&str>,
    ) -> AppResult<()> {
        let connection = self.connection.lock();
        connection.execute(
            "INSERT INTO events (operation_id, root_id, at, kind, action, detail, ok)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                operation_id,
                root_id,
                now_millis(),
                StepKind::Result.as_str(),
                action.as_str(),
                detail,
                ok as i64
            ],
        )?;
        Ok(())
    }

    pub fn events(&self, operation_id: &str) -> AppResult<Vec<JournalEvent>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT id, operation_id, root_id, at, kind, action, detail, ok
             FROM events WHERE operation_id = ?1 ORDER BY id",
        )?;
        let rows = statement.query_map(params![operation_id], |row| {
            let kind: String = row.get(4)?;
            Ok(JournalEvent {
                id: row.get(0)?,
                operation_id: row.get(1)?,
                root_id: row.get(2)?,
                at: row.get(3)?,
                kind: if kind == "intent" {
                    StepKind::Intent
                } else {
                    StepKind::Result
                },
                action: row.get(5)?,
                detail: row.get(6)?,
                ok: row.get::<_, Option<i64>>(7)?.map(|v| v != 0),
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Intents with no matching later result, newest first.
    pub fn dangling_intents(&self, operation_id: &str) -> AppResult<Vec<DanglingIntent>> {
        let events = self.events(operation_id)?;
        let mut dangling: Vec<DanglingIntent> = Vec::new();

        for event in &events {
            match event.kind {
                StepKind::Intent => dangling.push(DanglingIntent {
                    operation_id: event.operation_id.clone(),
                    root_id: event.root_id.clone(),
                    action: event.action.clone(),
                    at: event.at,
                    detail: event.detail.clone(),
                }),
                StepKind::Result => {
                    if let Some(index) = dangling.iter().rposition(|intent| {
                        intent.action == event.action && intent.root_id == event.root_id
                    }) {
                        dangling.remove(index);
                    }
                }
            }
        }

        dangling.reverse();
        Ok(dangling)
    }

    // -- reads -------------------------------------------------------------

    pub fn plan(&self, operation_id: &str) -> AppResult<MigrationPlan> {
        let connection = self.connection.lock();
        let json: String = connection
            .query_row(
                "SELECT plan_json FROM operations WHERE operation_id = ?1",
                params![operation_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| AppError::not_found(format!("找不到迁移记录 {operation_id}")))?;
        Ok(serde_json::from_str(&json)?)
    }

    pub fn operation_state(&self, operation_id: &str) -> AppResult<MigrationState> {
        let connection = self.connection.lock();
        let state: String = connection
            .query_row(
                "SELECT state FROM operations WHERE operation_id = ?1",
                params![operation_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| AppError::not_found(format!("找不到迁移记录 {operation_id}")))?;
        MigrationState::from_wire(&state)
            .ok_or_else(|| AppError::invalid(format!("未知的迁移状态 {state}")))
    }

    pub fn root_outcomes(&self, operation_id: &str) -> AppResult<Vec<RootOutcome>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT root_id, label, source_path, target_path, backup_path, state,
                    bytes, files, verified, link_ok, error
             FROM operation_roots WHERE operation_id = ?1 ORDER BY rowid",
        )?;
        let rows = statement.query_map(params![operation_id], |row| {
            let state: String = row.get(5)?;
            let backup: String = row.get(4)?;
            Ok(RootOutcome {
                root_id: row.get(0)?,
                label: row.get(1)?,
                source_path: PathBuf::from(row.get::<_, String>(2)?),
                target_path: PathBuf::from(row.get::<_, String>(3)?),
                backup_path: if backup.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(backup))
                },
                state: MigrationState::from_wire(&state)
                    .unwrap_or(MigrationState::ManualIntervention),
                bytes: row.get::<_, i64>(6)? as u64,
                files: row.get::<_, i64>(7)? as u64,
                verified: row.get::<_, i64>(8)? != 0,
                link_ok: row.get::<_, i64>(9)? != 0,
                error: row.get(10)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn history(&self, limit: u32) -> AppResult<Vec<HistoryEntry>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT o.operation_id, o.created_at, o.updated_at, o.state, o.target_root,
                    o.total_bytes, o.backups_retained, o.error,
                    (SELECT COUNT(*) FROM operation_roots r WHERE r.operation_id = o.operation_id)
             FROM operations o ORDER BY o.created_at DESC LIMIT ?1",
        )?;
        let rows = statement.query_map(params![limit], |row| {
            let state: String = row.get(3)?;
            Ok(HistoryEntry {
                operation_id: row.get(0)?,
                created_at: row.get(1)?,
                updated_at: row.get(2)?,
                state: MigrationState::from_wire(&state)
                    .unwrap_or(MigrationState::ManualIntervention),
                target_root: PathBuf::from(row.get::<_, String>(4)?),
                total_bytes: row.get::<_, i64>(5)? as u64,
                root_count: row.get::<_, i64>(8)? as u64,
                backups_retained: row.get::<_, i64>(6)? != 0,
                error: row.get(7)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Operations that were still running when the process last exited.
    pub fn unfinished_operations(&self) -> AppResult<Vec<String>> {
        let terminal = [
            MigrationState::Completed.as_str(),
            MigrationState::Cancelled.as_str(),
            MigrationState::FailedSafe.as_str(),
            MigrationState::RolledBack.as_str(),
        ];
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT operation_id FROM operations
             WHERE state NOT IN (?1, ?2, ?3, ?4) ORDER BY created_at DESC",
        )?;
        let rows = statement.query_map(
            params![terminal[0], terminal[1], terminal[2], terminal[3]],
            |row| row.get::<_, String>(0),
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Completed operations whose backups are still on the source volume.
    pub fn operations_with_retained_backups(&self) -> AppResult<Vec<String>> {
        let connection = self.connection.lock();
        let mut statement = connection.prepare(
            "SELECT operation_id FROM operations
             WHERE backups_retained = 1
               AND state IN (?1, ?2)
             ORDER BY created_at DESC",
        )?;
        let rows = statement.query_map(
            params![
                MigrationState::ActiveBackupRetained.as_str(),
                MigrationState::CleanupEligible.as_str()
            ],
            |row| row.get::<_, String>(0),
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_plan() -> MigrationPlan {
        MigrationPlan {
            operation_id: "op-1".into(),
            created_at: 1000,
            target_root: PathBuf::from("/target"),
            target_volume: "/".into(),
            source_volume: "/".into(),
            roots: vec![PlannedRoot {
                root_id: "cache".into(),
                label: "缓存".into(),
                source_path: PathBuf::from("/source/Cache"),
                target_path: PathBuf::from("/target/cache"),
                backup_path: PathBuf::from("/source/Cache.csm-backup-1"),
                staging_path: PathBuf::from("/target/.csm-staging/op-1/cache"),
                estimated_bytes: 4096,
                estimated_files: 3,
            }],
            total_bytes: 4096,
            total_files: 3,
            required_target_bytes: 8192,
            blockers: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn journal() -> (Journal, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::open(&dir.path().join("journal.sqlite")).unwrap();
        (journal, dir)
    }

    #[test]
    fn plan_round_trips_through_storage() {
        let (journal, _dir) = journal();
        let plan = sample_plan();
        journal.create_operation(&plan).unwrap();

        let loaded = journal.plan("op-1").unwrap();
        assert_eq!(loaded.roots.len(), 1);
        assert_eq!(loaded.roots[0].root_id, "cache");
        assert_eq!(
            journal.operation_state("op-1").unwrap(),
            MigrationState::PlanReady
        );
    }

    #[test]
    fn an_intent_without_a_result_is_reported_as_dangling() {
        let (journal, _dir) = journal();
        journal.create_operation(&sample_plan()).unwrap();

        journal
            .record_intent(
                "op-1",
                Some("cache"),
                StepAction::RenameSourceToBackup,
                None,
            )
            .unwrap();
        let dangling = journal.dangling_intents("op-1").unwrap();
        assert_eq!(dangling.len(), 1);
        assert_eq!(dangling[0].action, "renameSourceToBackup");

        journal
            .record_result(
                "op-1",
                Some("cache"),
                StepAction::RenameSourceToBackup,
                true,
                None,
            )
            .unwrap();
        assert!(journal.dangling_intents("op-1").unwrap().is_empty());
    }

    #[test]
    fn results_only_close_the_matching_root() {
        let (journal, _dir) = journal();
        journal.create_operation(&sample_plan()).unwrap();

        journal
            .record_intent("op-1", Some("a"), StepAction::CreateLink, None)
            .unwrap();
        journal
            .record_intent("op-1", Some("b"), StepAction::CreateLink, None)
            .unwrap();
        journal
            .record_result("op-1", Some("a"), StepAction::CreateLink, true, None)
            .unwrap();

        let dangling = journal.dangling_intents("op-1").unwrap();
        assert_eq!(dangling.len(), 1);
        assert_eq!(dangling[0].root_id.as_deref(), Some("b"));
    }

    #[test]
    fn unfinished_operations_exclude_terminal_states() {
        let (journal, _dir) = journal();
        journal.create_operation(&sample_plan()).unwrap();
        assert_eq!(
            journal.unfinished_operations().unwrap(),
            vec!["op-1".to_string()]
        );

        journal
            .set_operation_state("op-1", MigrationState::Completed)
            .unwrap();
        assert!(journal.unfinished_operations().unwrap().is_empty());
    }

    #[test]
    fn history_reports_root_counts_and_retention() {
        let (journal, _dir) = journal();
        journal.create_operation(&sample_plan()).unwrap();
        journal
            .set_operation_state("op-1", MigrationState::ActiveBackupRetained)
            .unwrap();

        let history = journal.history(10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].root_count, 1);
        assert!(history[0].backups_retained);
        assert_eq!(
            journal.operations_with_retained_backups().unwrap(),
            vec!["op-1".to_string()]
        );
    }

    #[test]
    fn scan_snapshots_keep_only_the_most_recent() {
        let (journal, _dir) = journal();
        for index in 0..12 {
            let report = ScanReport {
                scan_id: format!("scan-{index}"),
                scanned_at: index as i64,
                platform: "test".into(),
                installation: AppInstallation::default(),
                roots: Vec::new(),
                volumes: Vec::new(),
                total_logical_bytes: 0,
                reclaimable_bytes: 0,
                suggested_target: None,
                prior_migrations: Vec::new(),
                warnings: Vec::new(),
                duration_ms: 0,
            };
            journal.record_scan(&report).unwrap();
        }
        let latest = journal.latest_scan().unwrap().unwrap();
        assert_eq!(latest.scan_id, "scan-11");
    }
}
