//! The complete IPC surface.
//!
//! The frontend has no filesystem or shell permission of its own; everything it
//! can do is one of the commands below. Each one validates its input against
//! the current scan rather than trusting paths supplied by the webview.

use crate::core::executor::Executor;
use crate::core::journal::{Journal, JournalEvent};
use crate::core::model::*;
use crate::core::planner::{self, MigrationRequest, TargetProbe};
use crate::core::recovery::{self, RecoveryReport};
use crate::core::scanner::{self, ScanOptions};
use crate::error::{AppError, AppErrorWire, AppResult, CommandResult};
use crate::settings::{BuildInfo, Settings, SettingsLoad, SettingsStore};
use crate::util::redact_path;

use parking_lot::RwLock;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::UpdaterExt;

pub const EVENT_SCAN_PROGRESS: &str = "scan://progress";
pub const EVENT_SCAN_COMPLETE: &str = "scan://complete";
pub const EVENT_SCAN_FAILED: &str = "scan://failed";
pub const EVENT_MIGRATION_PROGRESS: &str = "migration://progress";
pub const EVENT_MIGRATION_COMPLETE: &str = "migration://complete";
pub const EVENT_MIGRATION_FAILED: &str = "migration://failed";
pub const EVENT_UPDATE_PROGRESS: &str = "update://progress";

pub struct AppState {
    pub journal: Arc<Journal>,
    pub settings: Arc<SettingsStore>,
    pub settings_load_message: Option<String>,
    scan_cancel: Arc<AtomicBool>,
    migration_cancel: Arc<AtomicBool>,
    scanning: Arc<AtomicBool>,
    migrating: Arc<AtomicBool>,
    latest_scan: RwLock<Option<ScanReport>>,
    active_plan: RwLock<Option<MigrationPlan>>,
}

impl AppState {
    pub fn new(
        journal: Arc<Journal>,
        settings: Arc<SettingsStore>,
        settings_load_message: Option<String>,
    ) -> Self {
        Self {
            journal,
            settings,
            settings_load_message,
            scan_cancel: Arc::new(AtomicBool::new(false)),
            migration_cancel: Arc::new(AtomicBool::new(false)),
            scanning: Arc::new(AtomicBool::new(false)),
            migrating: Arc::new(AtomicBool::new(false)),
            latest_scan: RwLock::new(None),
            active_plan: RwLock::new(None),
        }
    }

    fn require_scan(&self) -> AppResult<ScanReport> {
        self.latest_scan
            .read()
            .clone()
            .ok_or_else(|| AppError::invalid("请先完成一次扫描"))
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupState {
    pub build: BuildInfo,
    pub settings: Settings,
    pub settings_message: Option<String>,
    pub platform: String,
    pub journal_path: PathBuf,
    pub settings_path: PathBuf,
    pub recovery: Vec<RecoveryReport>,
    pub pending_cleanup_operations: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationDetail {
    pub plan: MigrationPlan,
    pub state: MigrationState,
    pub roots: Vec<RootOutcome>,
    pub events: Vec<JournalEvent>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuiescenceReport {
    pub quiet: bool,
    pub processes: Vec<RunningProcess>,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    pub channel: String,
    pub update_available: bool,
    pub latest_version: Option<String>,
    pub release_notes: Option<String>,
    pub published_at: Option<String>,
    pub download_bytes: Option<u64>,
    /// Installing a desktop update always replaces the running binary.
    pub requires_restart: bool,
    pub checked_at: i64,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
    /// `download` while bytes are arriving, `install` after the archive is verified.
    pub phase: &'static str,
}

// ---------------------------------------------------------------------------
// Startup, settings, build info
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_startup_state(state: State<'_, AppState>) -> CommandResult<StartupState> {
    let recovery = recovery::diagnose_all(&state.journal).map_err(AppErrorWire)?;
    let pending = state
        .journal
        .operations_with_retained_backups()
        .map_err(AppErrorWire)?;

    Ok(StartupState {
        build: BuildInfo::current(),
        settings: state.settings.get(),
        settings_message: state.settings_load_message.clone(),
        platform: crate::platforms::platform_name().to_string(),
        journal_path: state.journal.path().to_path_buf(),
        settings_path: state.settings.path().to_path_buf(),
        recovery,
        pending_cleanup_operations: pending,
    })
}

#[tauri::command]
pub fn get_build_info() -> BuildInfo {
    BuildInfo::current()
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.get()
}

#[tauri::command]
pub fn save_settings(state: State<'_, AppState>, settings: Settings) -> CommandResult<Settings> {
    state.settings.save(settings).map_err(AppErrorWire)
}

#[tauri::command]
pub fn reset_settings(state: State<'_, AppState>) -> CommandResult<Settings> {
    state.settings.reset().map_err(AppErrorWire)
}

#[tauri::command]
pub fn reload_settings(state: State<'_, AppState>) -> SettingsLoad {
    SettingsStore::load(state.settings.path())
}

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn start_scan(app: AppHandle, state: State<'_, AppState>) -> CommandResult<String> {
    if state
        .scanning
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(AppErrorWire(AppError::invalid("扫描已在进行中")));
    }

    let scan_id = uuid::Uuid::new_v4().to_string();
    state.scan_cancel.store(false, Ordering::SeqCst);

    let options = ScanOptions {
        scan_id: scan_id.clone(),
        excluded_ids: state.settings.get().excluded_root_ids,
    };
    let cancel = Arc::clone(&state.scan_cancel);
    let scanning = Arc::clone(&state.scanning);
    let journal = Arc::clone(&state.journal);

    std::thread::spawn(move || {
        let emitter = app.clone();
        let mut emit = move |progress: ScanProgress| {
            let _ = emitter.emit(EVENT_SCAN_PROGRESS, progress);
        };

        let result = scanner::run_scan(&options, cancel, &mut emit);
        scanning.store(false, Ordering::SeqCst);

        match result {
            Ok(report) => {
                let _ = journal.record_scan(&report);
                if let Some(state) = app.try_state::<AppState>() {
                    *state.latest_scan.write() = Some(report.clone());
                }
                let _ = app.emit(EVENT_SCAN_COMPLETE, report);
            }
            Err(error) => {
                let _ = app.emit(
                    EVENT_SCAN_FAILED,
                    serde_json::json!({
                        "scanId": options.scan_id,
                        "code": error.code(),
                        "message": error.to_string(),
                    }),
                );
            }
        }
    });

    Ok(scan_id)
}

#[tauri::command]
pub fn cancel_scan(state: State<'_, AppState>) {
    state.scan_cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
pub fn get_latest_scan(state: State<'_, AppState>) -> CommandResult<Option<ScanReport>> {
    if let Some(report) = state.latest_scan.read().clone() {
        return Ok(Some(report));
    }
    // Falling back to the stored snapshot lets the window show meaningful data
    // immediately on launch, before the fresh scan finishes.
    state.journal.latest_scan().map_err(AppErrorWire)
}

#[tauri::command]
pub fn list_volumes() -> Vec<VolumeInfo> {
    crate::platforms::list_volumes()
}

#[tauri::command]
pub fn probe_target_directory(path: PathBuf) -> TargetProbe {
    planner::probe_target(&path)
}

#[tauri::command]
pub fn check_quiescence(state: State<'_, AppState>) -> QuiescenceReport {
    let processes = crate::platforms::running_processes();
    let quiet = processes.is_empty();
    let _ = state;
    QuiescenceReport {
        message: if quiet {
            "未检测到运行中的 Cursor 进程".to_string()
        } else {
            format!(
                "仍有 {} 个 Cursor 进程在运行，请完全退出后继续",
                processes.len()
            )
        },
        quiet,
        processes,
    }
}

// ---------------------------------------------------------------------------
// Planning
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn create_plan(
    state: State<'_, AppState>,
    request: MigrationRequest,
) -> CommandResult<MigrationPlan> {
    let report = state.require_scan().map_err(AppErrorWire)?;
    let plan = planner::build_plan(&report, &request).map_err(AppErrorWire)?;

    // The plan is only persisted once it is actually executable, so the history
    // does not fill up with rejected attempts.
    if plan.blockers.is_empty() {
        state
            .journal
            .create_operation(&plan)
            .map_err(AppErrorWire)?;
        *state.active_plan.write() = Some(plan.clone());
    }
    Ok(plan)
}

#[tauri::command]
pub fn get_active_plan(state: State<'_, AppState>) -> Option<MigrationPlan> {
    state.active_plan.read().clone()
}

// ---------------------------------------------------------------------------
// Migration
// ---------------------------------------------------------------------------

fn load_plan(state: &AppState, operation_id: &str) -> AppResult<MigrationPlan> {
    if let Some(plan) = state.active_plan.read().clone() {
        if plan.operation_id == operation_id {
            return Ok(plan);
        }
    }
    state.journal.plan(operation_id)
}

#[tauri::command]
pub fn start_migration(
    app: AppHandle,
    state: State<'_, AppState>,
    operation_id: String,
) -> CommandResult<()> {
    let plan = load_plan(&state, &operation_id).map_err(AppErrorWire)?;
    if !plan.blockers.is_empty() {
        return Err(AppErrorWire(AppError::unsafe_op(
            "还有问题需要先解决，暂时不能开始",
        )));
    }

    if state
        .migrating
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(AppErrorWire(AppError::invalid("正在搬家，请等当前这次完成")));
    }
    state.migration_cancel.store(false, Ordering::SeqCst);

    let journal = Arc::clone(&state.journal);
    let cancel = Arc::clone(&state.migration_cancel);
    let migrating = Arc::clone(&state.migrating);

    std::thread::spawn(move || {
        let emitter = app.clone();
        let mut emit = move |progress: MigrationProgress| {
            let _ = emitter.emit(EVENT_MIGRATION_PROGRESS, progress);
        };

        let executor = Executor::new(journal, cancel);
        let result = executor.run(&plan, &mut emit);
        migrating.store(false, Ordering::SeqCst);

        match result {
            Ok(outcome) => {
                let _ = app.emit(EVENT_MIGRATION_COMPLETE, outcome);
            }
            Err(error) => {
                let _ = app.emit(
                    EVENT_MIGRATION_FAILED,
                    serde_json::json!({
                        "operationId": plan.operation_id,
                        "code": error.code(),
                        "message": error.to_string(),
                    }),
                );
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub fn cancel_migration(state: State<'_, AppState>) {
    state.migration_cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
pub fn cleanup_backups(state: State<'_, AppState>, operation_id: String) -> CommandResult<u64> {
    let plan = load_plan(&state, &operation_id).map_err(AppErrorWire)?;
    let executor = Executor::new(Arc::clone(&state.journal), Arc::new(AtomicBool::new(false)));
    executor.cleanup_backups(&plan).map_err(AppErrorWire)
}

#[tauri::command]
pub fn undo_migration(
    state: State<'_, AppState>,
    operation_id: String,
) -> CommandResult<MigrationResult> {
    let plan = load_plan(&state, &operation_id).map_err(AppErrorWire)?;
    let executor = Executor::new(Arc::clone(&state.journal), Arc::new(AtomicBool::new(false)));
    executor.undo(&plan).map_err(AppErrorWire)
}

// ---------------------------------------------------------------------------
// History, recovery, diagnostics
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_history(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> CommandResult<Vec<HistoryEntry>> {
    state
        .journal
        .history(limit.unwrap_or(50))
        .map_err(AppErrorWire)
}

#[tauri::command]
pub fn get_operation(
    state: State<'_, AppState>,
    operation_id: String,
) -> CommandResult<OperationDetail> {
    let plan = state.journal.plan(&operation_id).map_err(AppErrorWire)?;
    Ok(OperationDetail {
        state: state
            .journal
            .operation_state(&operation_id)
            .map_err(AppErrorWire)?,
        roots: state
            .journal
            .root_outcomes(&operation_id)
            .map_err(AppErrorWire)?,
        events: state.journal.events(&operation_id).map_err(AppErrorWire)?,
        plan,
    })
}

#[tauri::command]
pub fn get_recovery_reports(state: State<'_, AppState>) -> CommandResult<Vec<RecoveryReport>> {
    recovery::diagnose_all(&state.journal).map_err(AppErrorWire)
}

#[tauri::command]
pub fn diagnose_operation(
    state: State<'_, AppState>,
    operation_id: String,
) -> CommandResult<RecoveryReport> {
    recovery::diagnose(&state.journal, &operation_id).map_err(AppErrorWire)
}

/// Builds a shareable report with user names stripped from every path.
#[tauri::command]
pub fn export_diagnostics(
    state: State<'_, AppState>,
    operation_id: Option<String>,
) -> CommandResult<String> {
    let build = BuildInfo::current();
    let mut document = serde_json::json!({
        "generatedAt": crate::util::now_millis(),
        "build": build,
        "platform": crate::platforms::platform_name(),
        "volumes": crate::platforms::list_volumes(),
    });

    if let Some(report) = state.latest_scan.read().clone() {
        document["scan"] = serde_json::json!({
            "scannedAt": report.scanned_at,
            "installationFound": report.installation.found,
            "installKind": report.installation.kind,
            "totalLogicalBytes": report.total_logical_bytes,
            "reclaimableBytes": report.reclaimable_bytes,
            "roots": report.roots.iter().map(|root| serde_json::json!({
                "id": root.id,
                "path": redact_path(&root.path),
                "exists": root.exists,
                "linkState": root.link_state,
                "recommendation": root.recommendation,
                "size": root.size,
                "scanErrors": root.scan_errors,
            })).collect::<Vec<_>>(),
        });
    }

    if let Some(operation_id) = operation_id {
        let plan = state.journal.plan(&operation_id).map_err(AppErrorWire)?;
        let events = state.journal.events(&operation_id).map_err(AppErrorWire)?;
        let outcomes = state
            .journal
            .root_outcomes(&operation_id)
            .map_err(AppErrorWire)?;
        document["operation"] = serde_json::json!({
            "operationId": operation_id,
            "createdAt": plan.created_at,
            "targetRoot": redact_path(&plan.target_root),
            "totalBytes": plan.total_bytes,
            "roots": plan.roots.iter().map(|root| serde_json::json!({
                "rootId": root.root_id,
                "source": redact_path(&root.source_path),
                "target": redact_path(&root.target_path),
                "bytes": root.estimated_bytes,
            })).collect::<Vec<_>>(),
            "outcomes": outcomes.iter().map(|outcome| serde_json::json!({
                "rootId": outcome.root_id,
                "state": outcome.state,
                "verified": outcome.verified,
                "linkOk": outcome.link_ok,
                "error": outcome.error,
            })).collect::<Vec<_>>(),
            "events": events.iter().map(|event| serde_json::json!({
                "at": event.at,
                "kind": event.kind,
                "action": event.action,
                "rootId": event.root_id,
                "ok": event.ok,
            })).collect::<Vec<_>>(),
        });
    }

    serde_json::to_string_pretty(&document).map_err(|error| AppErrorWire(AppError::Serde(error)))
}

fn updater_for(
    app: &AppHandle,
    channel: crate::settings::UpdateChannel,
) -> Result<tauri_plugin_updater::Updater, AppErrorWire> {
    let mut builder = app.updater_builder();
    if let Some(feed) = release_feed(channel) {
        builder = builder
            .endpoints(vec![feed])
            .map_err(AppErrorWire::from_display)?;
    }
    builder.build().map_err(AppErrorWire::from_display)
}

/// Release feed for a channel.
///
/// Both feeds are signed with the same key; the channel only decides which
/// manifest is consulted, so a preview build can never be served as stable.
fn release_feed(channel: crate::settings::UpdateChannel) -> Option<url::Url> {
    let name = match channel {
        crate::settings::UpdateChannel::Stable => return None, // configured default
        crate::settings::UpdateChannel::Preview => "preview.json",
    };
    url::Url::parse(&format!(
        "https://github.com/JackXhl/cursor-space-manager/releases/latest/download/{name}"
    ))
    .ok()
}

/// Checks the signed release feed.
///
/// A failure here is reported as a message rather than an error: not reaching
/// the update server is a normal condition and must never disturb the local
/// scan or the installed version.
#[tauri::command]
pub async fn check_for_updates(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<UpdateStatus, AppErrorWire> {
    let settings = state.settings.get();
    let build = BuildInfo::current();

    let mut status = UpdateStatus {
        current_version: build.version.clone(),
        channel: match settings.update_channel {
            crate::settings::UpdateChannel::Stable => "stable".into(),
            crate::settings::UpdateChannel::Preview => "preview".into(),
        },
        update_available: false,
        latest_version: None,
        release_notes: None,
        published_at: None,
        download_bytes: None,
        requires_restart: false,
        checked_at: crate::util::now_millis(),
        message: String::new(),
    };

    // An unsigned local build has no release to compare against, and saying
    // "up to date" would be a lie.
    if build.build_profile == "debug" {
        status.message = "开发版本不检查更新".into();
        return Ok(status);
    }

    let updater = match updater_for(&app, settings.update_channel) {
        Ok(updater) => updater,
        Err(error) => {
            status.message = format!("更新功能不可用：{error}");
            return Ok(status);
        }
    };

    match updater.check().await {
        Ok(Some(update)) => {
            status.update_available = true;
            status.requires_restart = true;
            status.latest_version = Some(update.version.clone());
            status.release_notes = update.body.clone();
            status.published_at = update.date.map(|date| date.to_string());
            status.download_bytes = update.raw_json.get("size").and_then(|value| value.as_u64());
            status.message = format!("发现新版本 {}", update.version);
        }
        Ok(None) => status.message = "当前已是最新版本".into(),
        Err(error) => {
            status.message = format!("检查更新失败，稍后可重试：{error}");
        }
    }

    Ok(status)
}

/// Downloads the signed package and hands it to the platform installer.
///
/// Windows launches NSIS/MSI and then exits this process. macOS/Linux replace
/// the bundle in place, so we restart afterwards. A move in progress is refused:
/// swapping the binary mid-cutover is how a recovery turns into two broken copies.
#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), AppErrorWire> {
    let build = BuildInfo::current();
    if build.build_profile == "debug" {
        return Err(AppErrorWire(AppError::invalid(
            "开发版本不能安装更新，请使用安装包运行",
        )));
    }
    if state.migrating.load(Ordering::SeqCst) {
        return Err(AppErrorWire(AppError::unsafe_op(
            "搬家还在进行，请等结束后再安装更新",
        )));
    }

    let settings = state.settings.get();
    let updater = updater_for(&app, settings.update_channel)?;
    let update = updater
        .check()
        .await
        .map_err(AppErrorWire::from_display)?
        .ok_or_else(|| AppErrorWire(AppError::not_found("当前已是最新版本")))?;

    let emitter = app.clone();
    let downloaded = AtomicU64::new(0);
    update
        .download_and_install(
            |chunk, total| {
                let next = downloaded.fetch_add(chunk as u64, Ordering::Relaxed) + chunk as u64;
                let _ = emitter.emit(
                    EVENT_UPDATE_PROGRESS,
                    UpdateProgress {
                        downloaded: next,
                        total,
                        phase: "download",
                    },
                );
            },
            || {
                let next = downloaded.load(Ordering::Relaxed);
                let _ = emitter.emit(
                    EVENT_UPDATE_PROGRESS,
                    UpdateProgress {
                        downloaded: next,
                        total: Some(next),
                        phase: "install",
                    },
                );
            },
        )
        .await
        .map_err(AppErrorWire::from_display)?;

    // Windows usually exits inside download_and_install after spawning the
    // installer. If we are still running (macOS/Linux, or a Windows fallback),
    // relaunch onto the new binary.
    app.restart();
}
