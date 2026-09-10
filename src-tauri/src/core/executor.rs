//! The migration state machine.
//!
//! Guarantees, in order of importance:
//!
//! 1. At every instant at least one complete copy of the data exists.
//! 2. Nothing is deleted until a byte-for-byte comparison has passed.
//! 3. Every filesystem action is journalled before and after, so an interrupted
//!    run can be reconciled against what is actually on disk.
//! 4. Directories are only removed after confirming they are not links.

use crate::core::journal::{Journal, StepAction};
use crate::core::model::*;
use crate::core::planner;
use crate::core::scanner;
use crate::core::verifier::{self, VerificationReport};
use crate::error::{AppError, AppResult};
use crate::platforms;
use crate::util::now_millis;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Cap on how many individual problem paths we keep in a report.
const REPORT_LIST_LIMIT: usize = 25;

pub type ProgressSink<'a> = &'a mut dyn FnMut(MigrationProgress);

/// Source of truth for "is the application we are migrating still running".
///
/// This is a seam rather than a direct call so the engine can be exercised
/// against scratch directories on a machine where Cursor happens to be open,
/// and so a future platform can supply its own detection.
pub trait ProcessGate: Send + Sync {
    fn running(&self) -> Vec<RunningProcess>;
}

/// The real gate: whatever the platform reports.
pub struct SystemProcessGate;

impl ProcessGate for SystemProcessGate {
    fn running(&self) -> Vec<RunningProcess> {
        platforms::running_processes()
    }
}

pub struct Executor {
    journal: Arc<Journal>,
    cancel: Arc<AtomicBool>,
    gate: Arc<dyn ProcessGate>,
}

/// Aggregate byte/file counters so progress stays monotonic across roots.
#[derive(Default, Clone, Copy)]
struct Counters {
    processed_bytes: u64,
    processed_files: u64,
    total_bytes: u64,
    total_files: u64,
}

fn free_bytes_for(path: &Path) -> u64 {
    let Some(root) = platforms::volume_root_for(path) else {
        return 0;
    };
    platforms::list_volumes()
        .into_iter()
        .find(|volume| volume.mount.eq_ignore_ascii_case(&root))
        .map(|volume| volume.free_bytes)
        .unwrap_or(0)
}

/// Deletes a directory tree, refusing outright if the path is a link.
///
/// This is the single most dangerous operation in the tool: recursing into a
/// junction would delete the migrated data the junction points at.
fn remove_directory_tree(path: &Path) -> AppResult<()> {
    match platforms::link_state(path) {
        LinkState::Missing => Ok(()),
        LinkState::Regular => {
            std::fs::remove_dir_all(path)?;
            Ok(())
        }
        other => Err(AppError::unsafe_op(format!(
            "{} 是链接 ({:?})，拒绝递归删除",
            path.display(),
            other
        ))),
    }
}

/// Moves a non-empty destination aside so the verified staging copy can take
/// its place. Used when a previous manual copy already occupies the slot.
fn park_occupied_target(path: &Path) -> AppResult<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }
    if planner::target_slot_is_free(path) {
        let _ = std::fs::remove_dir(path);
        return Ok(None);
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "target".into());
    let parked = path.with_file_name(format!(
        "{}.{}.csm-prior",
        name,
        crate::util::timestamp_slug()
    ));
    std::fs::rename(path, &parked)?;
    Ok(Some(parked))
}

impl Executor {
    pub fn new(journal: Arc<Journal>, cancel: Arc<AtomicBool>) -> Self {
        Self::with_gate(journal, cancel, Arc::new(SystemProcessGate))
    }

    pub fn with_gate(
        journal: Arc<Journal>,
        cancel: Arc<AtomicBool>,
        gate: Arc<dyn ProcessGate>,
    ) -> Self {
        Self {
            journal,
            cancel,
            gate,
        }
    }

    fn check_cancelled(&self) -> AppResult<()> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        Ok(())
    }

    /// Confirms nothing is holding the sources open.
    ///
    /// We never kill anything: a forced exit is exactly how a half-written
    /// SQLite file happens.
    pub fn assert_quiescent(&self, plan: &MigrationPlan) -> AppResult<()> {
        let processes = self.gate.running();
        if !processes.is_empty() {
            let names: Vec<String> = processes
                .iter()
                .map(|p| format!("{} (PID {})", p.name, p.pid))
                .collect();
            return Err(AppError::unsafe_op(format!(
                "Cursor 仍在运行，请先完全退出：{}",
                names.join("、")
            )));
        }

        for root in &plan.roots {
            match platforms::processes_locking(&root.source_path) {
                Ok(holders) if !holders.is_empty() => {
                    let names: Vec<String> = holders
                        .iter()
                        .map(|p| format!("{} (PID {})", p.name, p.pid))
                        .collect();
                    return Err(AppError::unsafe_op(format!(
                        "{} 仍被占用：{}",
                        root.label,
                        names.join("、")
                    )));
                }
                Ok(_) => {}
                // Lock detection is a safety net, not a gate. If the API itself
                // fails we continue: the copy and verification still protect us.
                Err(_) => {}
            }
        }
        Ok(())
    }

    /// Runs the whole migration. Returns a result even for failures, so the UI
    /// can always show what state each directory ended up in.
    pub fn run(&self, plan: &MigrationPlan, progress: ProgressSink) -> AppResult<MigrationResult> {
        let started_at = now_millis();
        let source_free_before = plan
            .roots
            .first()
            .map(|root| free_bytes_for(&root.source_path))
            .unwrap_or(0);

        let mut counters = Counters {
            total_bytes: plan.total_bytes,
            total_files: plan.total_files,
            ..Default::default()
        };
        let mut messages: Vec<String> = Vec::new();

        self.journal
            .set_operation_state(&plan.operation_id, MigrationState::Preflight)?;
        progress(MigrationProgress {
            operation_id: plan.operation_id.clone(),
            state: MigrationState::Preflight,
            root_id: None,
            message: "正在复核执行条件".to_string(),
            processed_bytes: counters.processed_bytes,
            total_bytes: counters.total_bytes,
            processed_files: counters.processed_files,
            total_files: counters.total_files,
            bytes_per_second: 0,
            eta_seconds: None,
            cancellable: true,
        });

        if let Err(error) = self.assert_quiescent(plan) {
            self.fail_safe(plan, &error)?;
            return Err(error);
        }

        // -- Phase 1: copy and verify. The source is untouched throughout, so
        // any failure here is recoverable by simply cleaning up the target.
        let mut staged: Vec<StagedRoot> = Vec::new();
        for root in &plan.roots {
            match self.stage_root(plan, root, &mut counters, progress) {
                Ok(result) => {
                    counters.processed_bytes = counters
                        .processed_bytes
                        .max(staged.iter().map(|s| s.bytes).sum::<u64>() + result.bytes);
                    staged.push(result);
                }
                Err(error) => {
                    let cancelled = matches!(error, AppError::Cancelled);
                    self.cleanup_after_staging_failure(plan, &staged);
                    if cancelled {
                        self.journal
                            .set_operation_state(&plan.operation_id, MigrationState::Cancelled)?;
                    } else {
                        self.fail_safe(plan, &error)?;
                    }
                    return Err(error);
                }
            }
        }

        // -- Phase 2: cutover. From the first rename onward, cancellation is no
        // longer offered; failures roll back instead.
        self.journal
            .set_operation_state(&plan.operation_id, MigrationState::ReadyToCutover)?;

        let mut outcomes: Vec<RootOutcome> = Vec::new();
        let mut cutover_failed = false;

        for stage in &staged {
            let root = plan
                .roots
                .iter()
                .find(|r| r.root_id == stage.root_id)
                .expect("staged root must exist in plan");

            match self.cutover_root(plan, root, stage, progress) {
                Ok(outcome) => outcomes.push(outcome),
                Err(error) => {
                    cutover_failed = true;
                    messages.push(format!("{} 切换失败: {error}", root.label));
                    let recovered = self.rollback_root(plan, root);
                    outcomes.push(RootOutcome {
                        root_id: root.root_id.clone(),
                        label: root.label.clone(),
                        source_path: root.source_path.clone(),
                        target_path: root.target_path.clone(),
                        backup_path: None,
                        state: if recovered.is_ok() {
                            MigrationState::RolledBack
                        } else {
                            MigrationState::ManualIntervention
                        },
                        bytes: stage.bytes,
                        files: stage.files,
                        verified: true,
                        link_ok: false,
                        error: Some(error.to_string()),
                    });
                    if let Err(rollback_error) = recovered {
                        messages.push(format!(
                            "{} 自动还原没有完成，需要你来处理：{rollback_error}",
                            root.label
                        ));
                    }
                    break;
                }
            }
        }

        // Staging directories have served their purpose either way.
        let staging_root = plan
            .target_root
            .join(planner::STAGING_DIR)
            .join(&plan.operation_id);
        let _ = self.journal.record_intent(
            &plan.operation_id,
            None,
            StepAction::CleanupStaging,
            Some(&staging_root.to_string_lossy()),
        );
        let cleanup_ok = remove_directory_tree(&staging_root).is_ok();
        let _ = self.journal.record_result(
            &plan.operation_id,
            None,
            StepAction::CleanupStaging,
            cleanup_ok,
            None,
        );

        let succeeded = !cutover_failed
            && outcomes
                .iter()
                .all(|outcome| outcome.state == MigrationState::ActiveBackupRetained);

        let final_state = if succeeded {
            MigrationState::ActiveBackupRetained
        } else if outcomes
            .iter()
            .any(|o| o.state == MigrationState::ManualIntervention)
        {
            MigrationState::ManualIntervention
        } else {
            MigrationState::PartialCutover
        };

        self.journal
            .set_operation_state(&plan.operation_id, final_state)?;
        self.journal
            .set_backups_retained(&plan.operation_id, succeeded)?;

        let source_free_after = plan
            .roots
            .first()
            .map(|root| free_bytes_for(&root.source_path))
            .unwrap_or(0);
        self.journal.set_space_snapshot(
            &plan.operation_id,
            source_free_before,
            source_free_after,
        )?;

        let pending_cleanup_bytes = outcomes
            .iter()
            .filter(|o| o.backup_path.is_some())
            .map(|o| o.bytes)
            .sum();

        if succeeded {
            messages.push(
                "源目录已重命名为备份保留，确认 Cursor 一切正常后再清理，届时才会真正释放空间"
                    .to_string(),
            );
        }

        Ok(MigrationResult {
            operation_id: plan.operation_id.clone(),
            state: final_state,
            started_at,
            finished_at: now_millis(),
            roots: outcomes,
            source_free_before,
            source_free_after,
            freed_bytes: source_free_after as i64 - source_free_before as i64,
            pending_cleanup_bytes,
            backups_retained: succeeded,
            messages,
        })
    }

    // -- phase 1 -----------------------------------------------------------

    fn stage_root(
        &self,
        plan: &MigrationPlan,
        root: &PlannedRoot,
        counters: &mut Counters,
        progress: ProgressSink,
    ) -> AppResult<StagedRoot> {
        let operation_id = &plan.operation_id;
        let root_id = root.root_id.as_str();
        self.check_cancelled()?;

        self.journal
            .set_operation_state(operation_id, MigrationState::Copying)?;
        self.journal
            .set_root_state(operation_id, root_id, MigrationState::Copying)?;

        self.journal.record_intent(
            operation_id,
            Some(root_id),
            StepAction::CreateStaging,
            Some(&root.staging_path.to_string_lossy()),
        )?;
        std::fs::create_dir_all(&root.staging_path)?;
        self.journal.record_result(
            operation_id,
            Some(root_id),
            StepAction::CreateStaging,
            root.staging_path.is_dir(),
            None,
        )?;

        // -- copy
        self.journal.record_intent(
            operation_id,
            Some(root_id),
            StepAction::CopyToStaging,
            Some(&root.source_path.to_string_lossy()),
        )?;

        let base_bytes = counters.processed_bytes;
        let base_files = counters.processed_files;
        let started = Instant::now();
        let mut last_emit = Instant::now();

        let copy_result = {
            let mut on_progress = |bytes: u64, files: u64| {
                if last_emit.elapsed() < std::time::Duration::from_millis(250) {
                    return;
                }
                last_emit = Instant::now();
                let elapsed = started.elapsed().as_secs_f64().max(0.001);
                let rate = (bytes as f64 / elapsed) as u64;
                let done = base_bytes + bytes;
                let remaining = counters.total_bytes.saturating_sub(done);
                progress(MigrationProgress {
                    operation_id: operation_id.clone(),
                    state: MigrationState::Copying,
                    root_id: Some(root_id.to_string()),
                    message: format!("正在复制 {}", root.label),
                    processed_bytes: done,
                    total_bytes: counters.total_bytes,
                    processed_files: base_files + files,
                    total_files: counters.total_files,
                    bytes_per_second: rate,
                    eta_seconds: if rate > 0 {
                        Some(remaining / rate.max(1))
                    } else {
                        None
                    },
                    cancellable: true,
                });
            };
            platforms::copy_tree(
                &root.source_path,
                &root.staging_path,
                Arc::clone(&self.cancel),
                &mut on_progress,
            )
        };

        let outcome = match copy_result {
            Ok(outcome) => {
                self.journal.record_result(
                    operation_id,
                    Some(root_id),
                    StepAction::CopyToStaging,
                    true,
                    Some(&format!("{} 字节 / {} 文件", outcome.bytes, outcome.files)),
                )?;
                outcome
            }
            Err(error) => {
                self.journal.record_result(
                    operation_id,
                    Some(root_id),
                    StepAction::CopyToStaging,
                    false,
                    Some(&error.to_string()),
                )?;
                return Err(error);
            }
        };

        // -- verify
        self.check_cancelled()?;
        self.journal
            .set_operation_state(operation_id, MigrationState::Verifying)?;
        self.journal
            .set_root_state(operation_id, root_id, MigrationState::Verifying)?;
        self.journal
            .record_intent(operation_id, Some(root_id), StepAction::VerifyStaging, None)?;

        progress(MigrationProgress {
            operation_id: operation_id.clone(),
            state: MigrationState::Verifying,
            root_id: Some(root_id.to_string()),
            message: format!("正在核对 {}", root.label),
            processed_bytes: base_bytes + outcome.bytes,
            total_bytes: counters.total_bytes,
            processed_files: base_files + outcome.files,
            total_files: counters.total_files,
            bytes_per_second: 0,
            eta_seconds: None,
            cancellable: true,
        });

        let mut report = self.verify_pair(&root.source_path, &root.staging_path)?;
        report.corrupt_databases = verifier::check_databases(&root.staging_path, &self.cancel);
        if !report.corrupt_databases.is_empty() {
            report.ok = false;
        }
        verifier::truncate_lists(&mut report, REPORT_LIST_LIMIT);

        self.journal.record_result(
            operation_id,
            Some(root_id),
            StepAction::VerifyStaging,
            report.ok,
            Some(&serde_json::to_string(&report)?),
        )?;

        if !report.ok {
            return Err(AppError::unsafe_op(format!(
                "{} 校验未通过：缺失 {}、不一致 {}、不可读 {}、数据库异常 {}",
                root.label,
                report.missing.len(),
                report.mismatched.len(),
                report.unreadable.len(),
                report.corrupt_databases.len()
            )));
        }

        // -- promote staging to its final location (same volume, so a rename)
        self.journal.record_intent(
            operation_id,
            Some(root_id),
            StepAction::PromoteStaging,
            Some(&root.target_path.to_string_lossy()),
        )?;
        if let Some(parent) = root.target_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let parked = park_occupied_target(&root.target_path)?;
        let promote = std::fs::rename(&root.staging_path, &root.target_path);
        if promote.is_err() {
            if let Some(parked_path) = parked.as_ref() {
                let _ = std::fs::rename(parked_path, &root.target_path);
            }
        }
        let promoted = promote.is_ok() && root.target_path.is_dir();
        self.journal.record_result(
            operation_id,
            Some(root_id),
            StepAction::PromoteStaging,
            promoted,
            promote.as_ref().err().map(|e| e.to_string()).as_deref(),
        )?;
        promote?;

        counters.processed_bytes = base_bytes + outcome.bytes;
        counters.processed_files = base_files + outcome.files;

        self.journal.update_root_outcome(
            operation_id,
            root_id,
            outcome.bytes,
            outcome.files,
            true,
            false,
            None,
        )?;
        self.journal
            .set_root_state(operation_id, root_id, MigrationState::ReadyToCutover)?;

        Ok(StagedRoot {
            root_id: root.root_id.clone(),
            bytes: outcome.bytes,
            files: outcome.files,
            verification: report,
        })
    }

    fn verify_pair(&self, source: &Path, target: &Path) -> AppResult<VerificationReport> {
        let started = Instant::now();
        let source_manifest = verifier::build_manifest(source, true, &self.cancel, &mut |_, _| {})?;
        let target_manifest = verifier::build_manifest(target, true, &self.cancel, &mut |_, _| {})?;
        let mut report = verifier::compare(&source_manifest, &target_manifest);
        report.duration_ms = started.elapsed().as_millis() as u64;
        Ok(report)
    }

    /// Removes what we created when staging fails. The source has not been
    /// touched at this point, so this can never lose data.
    fn cleanup_after_staging_failure(&self, plan: &MigrationPlan, staged: &[StagedRoot]) {
        for stage in staged {
            if let Some(root) = plan.roots.iter().find(|r| r.root_id == stage.root_id) {
                let _ = remove_directory_tree(&root.target_path);
            }
        }
        let staging_root = plan
            .target_root
            .join(planner::STAGING_DIR)
            .join(&plan.operation_id);
        let _ = remove_directory_tree(&staging_root);
    }

    fn fail_safe(&self, plan: &MigrationPlan, error: &AppError) -> AppResult<()> {
        self.journal
            .set_operation_state(&plan.operation_id, MigrationState::FailedSafe)?;
        self.journal
            .set_operation_error(&plan.operation_id, &error.to_string())?;
        Ok(())
    }

    // -- phase 2 -----------------------------------------------------------

    fn cutover_root(
        &self,
        plan: &MigrationPlan,
        root: &PlannedRoot,
        stage: &StagedRoot,
        progress: ProgressSink,
    ) -> AppResult<RootOutcome> {
        let operation_id = &plan.operation_id;
        let root_id = root.root_id.as_str();

        progress(MigrationProgress {
            operation_id: operation_id.clone(),
            state: MigrationState::CutoverIntentRecorded,
            root_id: Some(root_id.to_string()),
            message: format!("正在切换 {}", root.label),
            processed_bytes: plan.total_bytes,
            total_bytes: plan.total_bytes,
            processed_files: plan.total_files,
            total_files: plan.total_files,
            bytes_per_second: 0,
            eta_seconds: None,
            cancellable: false,
        });

        self.journal
            .set_operation_state(operation_id, MigrationState::CutoverIntentRecorded)?;

        // Nothing may have appeared in the source's place while we copied.
        if platforms::link_state(&root.source_path).is_link() {
            return Err(AppError::unsafe_op(format!(
                "{} 在复制期间变成了链接，已停止切换",
                root.label
            )));
        }
        if root.backup_path.exists() {
            return Err(AppError::unsafe_op(format!(
                "备份位置已存在: {}",
                root.backup_path.display()
            )));
        }

        // -- rename source aside (atomic, same volume)
        self.journal.record_intent(
            operation_id,
            Some(root_id),
            StepAction::RenameSourceToBackup,
            Some(&root.backup_path.to_string_lossy()),
        )?;
        let rename = std::fs::rename(&root.source_path, &root.backup_path);
        let renamed = rename.is_ok() && root.backup_path.exists() && !root.source_path.exists();
        self.journal.record_result(
            operation_id,
            Some(root_id),
            StepAction::RenameSourceToBackup,
            renamed,
            rename.as_ref().err().map(|e| e.to_string()).as_deref(),
        )?;
        rename?;
        self.journal
            .set_root_state(operation_id, root_id, MigrationState::SourceRenamed)?;

        // -- create the link
        self.journal.record_intent(
            operation_id,
            Some(root_id),
            StepAction::CreateLink,
            Some(&root.target_path.to_string_lossy()),
        )?;
        let link = platforms::create_directory_link(&root.source_path, &root.target_path);
        let observed = platforms::link_state(&root.source_path);
        let link_ok = link.is_ok() && observed.is_link();
        self.journal.record_result(
            operation_id,
            Some(root_id),
            StepAction::CreateLink,
            link_ok,
            link.as_ref().err().map(|e| e.to_string()).as_deref(),
        )?;
        link?;
        if !link_ok {
            return Err(AppError::unsafe_op(format!(
                "{} 的链接创建后未能验证",
                root.label
            )));
        }
        self.journal
            .set_root_state(operation_id, root_id, MigrationState::LinkCreated)?;

        // -- prove the link actually reaches writable storage
        self.journal
            .record_intent(operation_id, Some(root_id), StepAction::WriteProbe, None)?;
        let probe = self.write_probe(&root.source_path, &root.target_path);
        self.journal.record_result(
            operation_id,
            Some(root_id),
            StepAction::WriteProbe,
            probe.is_ok(),
            probe.as_ref().err().map(|e| e.to_string()).as_deref(),
        )?;
        probe?;

        self.journal
            .set_root_state(operation_id, root_id, MigrationState::ActiveBackupRetained)?;
        self.journal.update_root_outcome(
            operation_id,
            root_id,
            stage.bytes,
            stage.files,
            true,
            true,
            None,
        )?;

        Ok(RootOutcome {
            root_id: root.root_id.clone(),
            label: root.label.clone(),
            source_path: root.source_path.clone(),
            target_path: root.target_path.clone(),
            backup_path: Some(root.backup_path.clone()),
            state: MigrationState::ActiveBackupRetained,
            bytes: stage.bytes,
            files: stage.files,
            verified: true,
            link_ok: true,
            error: None,
        })
    }

    /// Writes through the link and confirms the bytes land in the target.
    ///
    /// This catches a link that resolves but points somewhere unwritable, which
    /// metadata checks alone would miss.
    fn write_probe(&self, link: &Path, target: &Path) -> AppResult<()> {
        let name = format!(".csm-probe-{}", uuid::Uuid::new_v4());
        let via_link = link.join(&name);
        let via_target = target.join(&name);
        let payload = b"cursor-space-manager-probe";

        std::fs::write(&via_link, payload)?;
        let landed = std::fs::read(&via_target)
            .map(|content| content == payload)
            .unwrap_or(false);
        let _ = std::fs::remove_file(&via_link);

        if !landed {
            return Err(AppError::unsafe_op(
                "通过链接写入的数据未出现在目标目录，链接可能指向了别处",
            ));
        }
        Ok(())
    }

    // -- rollback ----------------------------------------------------------

    /// Puts a single root back the way it was, as far as the filesystem allows.
    pub fn rollback_root(&self, plan: &MigrationPlan, root: &PlannedRoot) -> AppResult<()> {
        let operation_id = &plan.operation_id;
        let root_id = root.root_id.as_str();
        self.journal
            .set_root_state(operation_id, root_id, MigrationState::RollbackPreparing)?;

        // Remove the link first so the original name is free again.
        if platforms::link_state(&root.source_path).is_link() {
            self.journal.record_intent(
                operation_id,
                Some(root_id),
                StepAction::RollbackRemoveLink,
                None,
            )?;
            let removed = platforms::remove_directory_link(&root.source_path);
            self.journal.record_result(
                operation_id,
                Some(root_id),
                StepAction::RollbackRemoveLink,
                removed.is_ok(),
                removed.as_ref().err().map(|e| e.to_string()).as_deref(),
            )?;
            removed?;
        }

        // Restore the backup only if the original name is genuinely vacant.
        if root.backup_path.exists() {
            if root.source_path.exists() {
                return Err(AppError::unsafe_op(format!(
                    "{} 已存在，无法自动恢复备份 {}",
                    root.source_path.display(),
                    root.backup_path.display()
                )));
            }
            self.journal.record_intent(
                operation_id,
                Some(root_id),
                StepAction::RollbackRestoreSource,
                Some(&root.source_path.to_string_lossy()),
            )?;
            let restored = std::fs::rename(&root.backup_path, &root.source_path);
            self.journal.record_result(
                operation_id,
                Some(root_id),
                StepAction::RollbackRestoreSource,
                restored.is_ok(),
                restored.as_ref().err().map(|e| e.to_string()).as_deref(),
            )?;
            restored?;
        }

        // Only now is the copy on the target redundant.
        self.journal.record_intent(
            operation_id,
            Some(root_id),
            StepAction::RollbackRemoveTarget,
            None,
        )?;
        let removed = remove_directory_tree(&root.target_path);
        self.journal.record_result(
            operation_id,
            Some(root_id),
            StepAction::RollbackRemoveTarget,
            removed.is_ok(),
            removed.as_ref().err().map(|e| e.to_string()).as_deref(),
        )?;

        self.journal
            .set_root_state(operation_id, root_id, MigrationState::RolledBack)?;
        Ok(())
    }

    /// User-initiated undo of a finished migration.
    pub fn undo(&self, plan: &MigrationPlan) -> AppResult<MigrationResult> {
        self.assert_quiescent(plan)?;
        self.journal
            .set_operation_state(&plan.operation_id, MigrationState::RollbackPreparing)?;

        let mut messages = Vec::new();
        let mut outcomes = Vec::new();

        for root in &plan.roots {
            let backup_present = root.backup_path.exists();
            let result = if backup_present {
                self.rollback_root(plan, root)
            } else {
                // Without a backup, the only copy lives at the target. Moving it
                // back is the correct undo; a rename across volumes is not
                // possible, so this is a copy followed by a verified delete.
                self.restore_from_target(plan, root)
            };

            match result {
                Ok(()) => outcomes.push(RootOutcome {
                    root_id: root.root_id.clone(),
                    label: root.label.clone(),
                    source_path: root.source_path.clone(),
                    target_path: root.target_path.clone(),
                    backup_path: None,
                    state: MigrationState::RolledBack,
                    bytes: root.estimated_bytes,
                    files: root.estimated_files,
                    verified: true,
                    link_ok: false,
                    error: None,
                }),
                Err(error) => {
                    messages.push(format!("{} 撤销失败: {error}", root.label));
                    outcomes.push(RootOutcome {
                        root_id: root.root_id.clone(),
                        label: root.label.clone(),
                        source_path: root.source_path.clone(),
                        target_path: root.target_path.clone(),
                        backup_path: Some(root.backup_path.clone()),
                        state: MigrationState::ManualIntervention,
                        bytes: root.estimated_bytes,
                        files: root.estimated_files,
                        verified: false,
                        link_ok: false,
                        error: Some(error.to_string()),
                    });
                }
            }
        }

        let all_done = outcomes
            .iter()
            .all(|o| o.state == MigrationState::RolledBack);
        let state = if all_done {
            MigrationState::RolledBack
        } else {
            MigrationState::ManualIntervention
        };
        self.journal
            .set_operation_state(&plan.operation_id, state)?;
        self.journal
            .set_backups_retained(&plan.operation_id, false)?;

        Ok(MigrationResult {
            operation_id: plan.operation_id.clone(),
            state,
            started_at: now_millis(),
            finished_at: now_millis(),
            roots: outcomes,
            source_free_before: 0,
            source_free_after: 0,
            freed_bytes: 0,
            pending_cleanup_bytes: 0,
            backups_retained: false,
            messages,
        })
    }

    /// Copies migrated data back to the source volume when no backup remains.
    fn restore_from_target(&self, plan: &MigrationPlan, root: &PlannedRoot) -> AppResult<()> {
        if !root.target_path.is_dir() {
            return Err(AppError::not_found(format!(
                "目标目录不存在，无法撤销: {}",
                root.target_path.display()
            )));
        }

        let restore_staging = root
            .source_path
            .parent()
            .ok_or_else(|| AppError::invalid("源路径没有父目录"))?
            .join(format!(".csm-restore-{}", uuid::Uuid::new_v4()));

        platforms::copy_tree(
            &root.target_path,
            &restore_staging,
            Arc::clone(&self.cancel),
            &mut |_, _| {},
        )?;

        let report = self.verify_pair(&root.target_path, &restore_staging)?;
        if !report.ok {
            let _ = remove_directory_tree(&restore_staging);
            return Err(AppError::unsafe_op("回迁数据校验未通过，已保留原有链接"));
        }

        if platforms::link_state(&root.source_path).is_link() {
            platforms::remove_directory_link(&root.source_path)?;
        }
        if root.source_path.exists() {
            let _ = remove_directory_tree(&restore_staging);
            return Err(AppError::unsafe_op(format!(
                "{} 已存在，无法回迁",
                root.source_path.display()
            )));
        }

        std::fs::rename(&restore_staging, &root.source_path)?;

        // The source is whole again, so the target copy is now the redundant one.
        let _ = remove_directory_tree(&root.target_path);
        let _ = self.journal.set_root_state(
            &plan.operation_id,
            &root.root_id,
            MigrationState::RolledBack,
        );
        Ok(())
    }

    // -- cleanup -----------------------------------------------------------

    /// Deletes the retained backups. This is the step that actually frees space
    /// on the source volume, and it is always explicit.
    pub fn cleanup_backups(&self, plan: &MigrationPlan) -> AppResult<u64> {
        let mut freed = 0u64;
        let cancel = AtomicBool::new(false);

        for root in &plan.roots {
            if !root.backup_path.exists() {
                continue;
            }

            // Re-establish the invariant instead of trusting the journal: the
            // live path must be a link pointing at a target that really holds
            // the data before we delete the only other copy.
            let state = platforms::link_state(&root.source_path);
            let Some(target) = state.target() else {
                return Err(AppError::unsafe_op(format!(
                    "{} 当前不是链接，为安全起见不会删除备份",
                    root.source_path.display()
                )));
            };
            if !crate::util::paths_equal(target, &root.target_path) {
                return Err(AppError::unsafe_op(format!(
                    "{} 指向 {}，与预期的 {} 不一致，不会删除备份",
                    root.source_path.display(),
                    target.display(),
                    root.target_path.display()
                )));
            }
            if !root.target_path.is_dir() {
                return Err(AppError::unsafe_op(format!(
                    "目标目录 {} 不可用，不会删除备份",
                    root.target_path.display()
                )));
            }

            let size = scanner::measure_directory(&root.backup_path, &cancel);

            self.journal.record_intent(
                &plan.operation_id,
                Some(&root.root_id),
                StepAction::RemoveBackup,
                Some(&root.backup_path.to_string_lossy()),
            )?;
            let removed = remove_directory_tree(&root.backup_path);
            self.journal.record_result(
                &plan.operation_id,
                Some(&root.root_id),
                StepAction::RemoveBackup,
                removed.is_ok(),
                removed.as_ref().err().map(|e| e.to_string()).as_deref(),
            )?;
            removed?;

            freed += size.logical_bytes;
            self.journal.set_root_state(
                &plan.operation_id,
                &root.root_id,
                MigrationState::Completed,
            )?;
        }

        self.journal
            .set_operation_state(&plan.operation_id, MigrationState::Completed)?;
        self.journal
            .set_backups_retained(&plan.operation_id, false)?;
        Ok(freed)
    }
}

struct StagedRoot {
    root_id: String,
    bytes: u64,
    files: u64,
    #[allow(dead_code)]
    verification: VerificationReport,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_to_recursively_delete_a_link() {
        let temp = tempfile::tempdir().unwrap();
        let real = temp.path().join("real");
        let link = temp.path().join("link");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("keep.txt"), b"important").unwrap();

        if platforms::create_directory_link(&link, &real).is_err() {
            // Unprivileged symlink creation can be denied; the guard below is
            // what matters and is covered by the regular-directory case.
            return;
        }

        let error = remove_directory_tree(&link).unwrap_err();
        assert!(matches!(error, AppError::Unsafe(_)));
        assert!(real.join("keep.txt").exists(), "linked data must survive");
    }

    #[test]
    fn removes_a_plain_directory() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("plain");
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        std::fs::write(dir.join("nested/file.txt"), b"x").unwrap();

        remove_directory_tree(&dir).unwrap();
        assert!(!dir.exists());
    }

    #[test]
    fn removing_a_missing_directory_is_a_no_op() {
        let temp = tempfile::tempdir().unwrap();
        assert!(remove_directory_tree(&temp.path().join("nope")).is_ok());
    }
}
