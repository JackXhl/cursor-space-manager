//! Shared domain types.
//!
//! Everything here crosses the IPC boundary, so field names are camelCase to
//! match the TypeScript definitions in `frontend/src/types`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// How Cursor was installed. Portable and machine-wide installs need different
/// handling than the common per-user install.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallKind {
    NotFound,
    PerUser,
    Machine,
    Portable,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunningProcess {
    pub pid: u32,
    pub name: String,
    pub exe_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInstallation {
    pub found: bool,
    pub kind: InstallKind,
    pub install_dir: Option<PathBuf>,
    pub exe_path: Option<PathBuf>,
    pub version: Option<String>,
    /// True when at least one Cursor process is alive. Migration must wait.
    pub running: bool,
    pub processes: Vec<RunningProcess>,
    /// Populated when several independent installs are detected; the user has
    /// to pick one before anything is migrated.
    pub conflicts: Vec<PathBuf>,
    pub notes: Vec<String>,
}

impl Default for AppInstallation {
    fn default() -> Self {
        Self {
            found: false,
            kind: InstallKind::NotFound,
            install_dir: None,
            exe_path: None,
            version: None,
            running: false,
            processes: Vec::new(),
            conflicts: Vec::new(),
            notes: Vec::new(),
        }
    }
}

/// What a path actually is on disk. Anything other than `Regular` means we must
/// not blindly rename or delete it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "target")]
pub enum LinkState {
    Missing,
    Regular,
    Junction(PathBuf),
    Symlink(PathBuf),
    /// A reparse point we do not understand (OneDrive placeholder, dedup, ...).
    UnknownReparse(String),
}

impl LinkState {
    pub fn is_link(&self) -> bool {
        matches!(
            self,
            LinkState::Junction(_) | LinkState::Symlink(_) | LinkState::UnknownReparse(_)
        )
    }

    pub fn target(&self) -> Option<&PathBuf> {
        match self {
            LinkState::Junction(p) | LinkState::Symlink(p) => Some(p),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirSize {
    /// Sum of file lengths.
    pub logical_bytes: u64,
    /// Allocated size where the platform can report it; falls back to logical.
    pub on_disk_bytes: u64,
    pub file_count: u64,
    pub dir_count: u64,
    /// Entries we could not read. A non-zero value makes any estimate a lower
    /// bound, and the UI has to say so.
    pub error_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Recommendation {
    Recommended,
    Optional,
    NotRecommended,
}

/// One migratable directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DataRoot {
    /// Stable identifier used by the journal and the UI selection state.
    pub id: String,
    pub label: String,
    pub purpose: String,
    pub path: PathBuf,
    pub resolved_path: PathBuf,
    pub link_state: LinkState,
    pub exists: bool,
    pub recommendation: Recommendation,
    pub reason: String,
    pub size: Option<DirSize>,
    pub volume: Option<String>,
    pub scan_errors: Vec<String>,
    /// Set when the path came from an explicit `--user-data-dir` style override
    /// rather than a well-known location.
    pub from_launch_argument: bool,
    /// A previously migrated copy of this directory, typically produced by a
    /// manual junction or an earlier run of this tool.
    #[serde(default)]
    pub prior_copy: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DriveKind {
    Fixed,
    Removable,
    Network,
    CdRom,
    RamDisk,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeInfo {
    /// Root path, e.g. `C:\` on Windows or `/` on Unix.
    pub mount: String,
    pub label: String,
    pub filesystem: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub kind: DriveKind,
    pub is_system: bool,
    /// False when the volume can never be a safe destination.
    pub eligible_target: bool,
    pub ineligible_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetSuggestion {
    pub volume: String,
    pub directory: PathBuf,
    pub free_bytes: u64,
    pub required_bytes: u64,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PriorSiteKind {
    RoamingMirror,
    DotCursor,
    ToolDefault,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PriorDisposition {
    /// Original path is already a link pointing here.
    Linked,
    /// Original path is still a real directory, so this is an unfinished copy.
    OrphanCopy,
    /// Original path is gone; only this copy remains.
    Leftover,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriorEntry {
    pub path: PathBuf,
    pub label: String,
    pub root_id: Option<String>,
    pub disposition: PriorDisposition,
    pub size: Option<DirSize>,
    pub source_path: Option<PathBuf>,
}

/// A directory that looks like a previous Cursor data migration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriorSite {
    pub path: PathBuf,
    pub kind: PriorSiteKind,
    pub volume: Option<String>,
    pub linked_count: u32,
    pub orphan_count: u32,
    pub leftover_count: u32,
    pub extra_count: u32,
    pub total_bytes: u64,
    pub entries: Vec<PriorEntry>,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScanPhase {
    Starting,
    Installation,
    Directories,
    Sizing,
    Volumes,
    Done,
    Cancelled,
    Failed,
}

/// Incremental scan event streamed to the UI so large directories stay
/// responsive instead of blocking on a single result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanProgress {
    pub scan_id: String,
    pub phase: ScanPhase,
    pub message: String,
    /// Which root this update belongs to, when the phase is per-root.
    pub root_id: Option<String>,
    pub size: Option<DirSize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub scan_id: String,
    pub scanned_at: i64,
    pub platform: String,
    pub installation: AppInstallation,
    pub roots: Vec<DataRoot>,
    pub volumes: Vec<VolumeInfo>,
    pub total_logical_bytes: u64,
    /// Bytes on the source volume that migrating the recommended roots frees,
    /// once the retained backup is finally removed.
    pub reclaimable_bytes: u64,
    pub suggested_target: Option<TargetSuggestion>,
    #[serde(default)]
    pub prior_migrations: Vec<PriorSite>,
    pub warnings: Vec<String>,
    pub duration_ms: u64,
}

/// Lifecycle of a migration. Persisted so an interrupted run can be
/// reconciled against the filesystem on the next launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MigrationState {
    PlanReady,
    Preflight,
    WaitingForCursorExit,
    Quiescent,
    Copying,
    Verifying,
    ReadyToCutover,
    CutoverIntentRecorded,
    SourceRenamed,
    LinkCreated,
    PostCheck,
    /// Success, but the original directory is still on the source volume.
    ActiveBackupRetained,
    CleanupEligible,
    Completed,
    Cancelled,
    /// Failed with the source left fully intact.
    FailedSafe,
    Recovering,
    PartialCutover,
    ManualIntervention,
    RollbackPreparing,
    RolledBack,
}

impl MigrationState {
    pub fn as_str(&self) -> &'static str {
        match self {
            MigrationState::PlanReady => "planReady",
            MigrationState::Preflight => "preflight",
            MigrationState::WaitingForCursorExit => "waitingForCursorExit",
            MigrationState::Quiescent => "quiescent",
            MigrationState::Copying => "copying",
            MigrationState::Verifying => "verifying",
            MigrationState::ReadyToCutover => "readyToCutover",
            MigrationState::CutoverIntentRecorded => "cutoverIntentRecorded",
            MigrationState::SourceRenamed => "sourceRenamed",
            MigrationState::LinkCreated => "linkCreated",
            MigrationState::PostCheck => "postCheck",
            MigrationState::ActiveBackupRetained => "activeBackupRetained",
            MigrationState::CleanupEligible => "cleanupEligible",
            MigrationState::Completed => "completed",
            MigrationState::Cancelled => "cancelled",
            MigrationState::FailedSafe => "failedSafe",
            MigrationState::Recovering => "recovering",
            MigrationState::PartialCutover => "partialCutover",
            MigrationState::ManualIntervention => "manualIntervention",
            MigrationState::RollbackPreparing => "rollbackPreparing",
            MigrationState::RolledBack => "rolledBack",
        }
    }

    /// Parses the string form persisted in the journal.
    pub fn from_wire(value: &str) -> Option<Self> {
        let state = match value {
            "planReady" => MigrationState::PlanReady,
            "preflight" => MigrationState::Preflight,
            "waitingForCursorExit" => MigrationState::WaitingForCursorExit,
            "quiescent" => MigrationState::Quiescent,
            "copying" => MigrationState::Copying,
            "verifying" => MigrationState::Verifying,
            "readyToCutover" => MigrationState::ReadyToCutover,
            "cutoverIntentRecorded" => MigrationState::CutoverIntentRecorded,
            "sourceRenamed" => MigrationState::SourceRenamed,
            "linkCreated" => MigrationState::LinkCreated,
            "postCheck" => MigrationState::PostCheck,
            "activeBackupRetained" => MigrationState::ActiveBackupRetained,
            "cleanupEligible" => MigrationState::CleanupEligible,
            "completed" => MigrationState::Completed,
            "cancelled" => MigrationState::Cancelled,
            "failedSafe" => MigrationState::FailedSafe,
            "recovering" => MigrationState::Recovering,
            "partialCutover" => MigrationState::PartialCutover,
            "manualIntervention" => MigrationState::ManualIntervention,
            "rollbackPreparing" => MigrationState::RollbackPreparing,
            "rolledBack" => MigrationState::RolledBack,
            _ => return None,
        };
        Some(state)
    }

    /// True once the source directory has been renamed away, meaning recovery
    /// has to reason about filesystem facts rather than just retrying.
    pub fn past_point_of_no_return(&self) -> bool {
        matches!(
            self,
            MigrationState::SourceRenamed
                | MigrationState::LinkCreated
                | MigrationState::PostCheck
                | MigrationState::ActiveBackupRetained
                | MigrationState::CleanupEligible
                | MigrationState::Completed
                | MigrationState::PartialCutover
        )
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            MigrationState::Completed
                | MigrationState::Cancelled
                | MigrationState::FailedSafe
                | MigrationState::RolledBack
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedRoot {
    pub root_id: String,
    pub label: String,
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub backup_path: PathBuf,
    pub staging_path: PathBuf,
    pub estimated_bytes: u64,
    pub estimated_files: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPlan {
    pub operation_id: String,
    pub created_at: i64,
    pub target_root: PathBuf,
    pub target_volume: String,
    pub source_volume: String,
    pub roots: Vec<PlannedRoot>,
    pub total_bytes: u64,
    pub total_files: u64,
    /// Worst-case space the destination must have free before we start.
    pub required_target_bytes: u64,
    pub blockers: Vec<PreflightIssue>,
    pub warnings: Vec<PreflightIssue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IssueSeverity {
    Blocker,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightIssue {
    pub code: String,
    pub severity: IssueSeverity,
    pub message: String,
    pub detail: Option<String>,
    pub root_id: Option<String>,
}

impl PreflightIssue {
    pub fn blocker(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            severity: IssueSeverity::Blocker,
            message: message.into(),
            detail: None,
            root_id: None,
        }
    }

    pub fn warning(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            severity: IssueSeverity::Warning,
            message: message.into(),
            detail: None,
            root_id: None,
        }
    }

    pub fn with_root(mut self, root_id: impl Into<String>) -> Self {
        self.root_id = Some(root_id.into());
        self
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationProgress {
    pub operation_id: String,
    pub state: MigrationState,
    pub root_id: Option<String>,
    pub message: String,
    pub processed_bytes: u64,
    pub total_bytes: u64,
    pub processed_files: u64,
    pub total_files: u64,
    pub bytes_per_second: u64,
    pub eta_seconds: Option<u64>,
    /// True while cancelling would still leave the source untouched.
    pub cancellable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RootOutcome {
    pub root_id: String,
    pub label: String,
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub backup_path: Option<PathBuf>,
    pub state: MigrationState,
    pub bytes: u64,
    pub files: u64,
    pub verified: bool,
    pub link_ok: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationResult {
    pub operation_id: String,
    pub state: MigrationState,
    pub started_at: i64,
    pub finished_at: i64,
    pub roots: Vec<RootOutcome>,
    pub source_free_before: u64,
    pub source_free_after: u64,
    /// Space actually returned to the source volume so far. Stays near zero
    /// until the retained backups are cleaned up.
    pub freed_bytes: i64,
    /// What cleaning up the retained backups would additionally free.
    pub pending_cleanup_bytes: u64,
    pub backups_retained: bool,
    pub messages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub operation_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub state: MigrationState,
    pub target_root: PathBuf,
    pub root_count: u64,
    pub total_bytes: u64,
    pub backups_retained: bool,
    pub error: Option<String>,
}
