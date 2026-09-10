//! Startup reconciliation.
//!
//! The journal records what we meant to do. This module compares that against
//! what is actually on disk and decides what state each interrupted migration
//! is really in. It never repairs anything on its own: a wrong automatic guess
//! here is how data gets lost, so it produces a diagnosis and a proposed action
//! for the user to confirm.

use crate::core::journal::Journal;
use crate::core::model::*;
use crate::error::AppResult;
use crate::platforms;
use crate::util::paths_equal;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RootDisposition {
    /// Source is a normal directory and no target exists. Nothing happened.
    Untouched,
    /// A partial copy exists on the target; the source is intact.
    StagedOnly,
    /// Link and target are in place and the backup is still present.
    MigratedWithBackup,
    /// Link and target are in place; the backup is gone. Fully migrated.
    MigratedNoBackup,
    /// Source is missing and only the backup exists. The link was never made.
    SourceRenamedOnly,
    /// The path exists but is a link we did not create or cannot explain.
    UnexpectedLink,
    /// Nothing can be found. Requires a human.
    DataMissing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RootDiagnosis {
    pub root_id: String,
    pub label: String,
    pub disposition: RootDisposition,
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub backup_path: PathBuf,
    pub source_state: LinkState,
    pub target_exists: bool,
    pub backup_exists: bool,
    pub explanation: String,
    pub suggested_action: String,
    /// False when the tool must not act without explicit user instruction.
    pub safe_to_automate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryReport {
    pub operation_id: String,
    pub recorded_state: MigrationState,
    pub resolved_state: MigrationState,
    pub roots: Vec<RootDiagnosis>,
    /// Actions that were journalled as started but never finished.
    pub dangling_actions: Vec<String>,
    pub summary: String,
    pub requires_user_decision: bool,
}

fn diagnose_root(root: &PlannedRoot) -> RootDiagnosis {
    let source_state = platforms::link_state(&root.source_path);
    let target_exists = root.target_path.is_dir();
    let backup_exists = root.backup_path.exists();

    let points_at_target = source_state
        .target()
        .map(|target| paths_equal(target, &root.target_path))
        .unwrap_or(false);

    let (disposition, explanation, suggested_action, safe) =
        match (&source_state, target_exists, backup_exists) {
            (LinkState::Regular, false, false) => (
                RootDisposition::Untouched,
                "原来的文件夹完好，没有搬家的痕迹".to_string(),
                "不用处理，可以重新开始搬家".to_string(),
                true,
            ),
            (LinkState::Regular, true, false) => (
                RootDisposition::StagedOnly,
                "原来的文件夹完好，新磁盘上有一份还没用上的副本".to_string(),
                "确认不需要后，可以删掉新磁盘上的副本，再重新搬家".to_string(),
                true,
            ),
            (LinkState::Regular, _, true) => (
                RootDisposition::StagedOnly,
                "原来的文件夹完好，同时还留着一份备份，可能是上次还原留下的".to_string(),
                "确认原来的内容正常后，再手动清理备份".to_string(),
                false,
            ),
            (state, true, true) if state.is_link() && points_at_target => (
                RootDisposition::MigratedWithBackup,
                "搬家已经完成，原来的位置已接到新磁盘，备份还留着".to_string(),
                "确认 Cursor 正常后，清理备份就能腾出空间".to_string(),
                true,
            ),
            (state, true, false) if state.is_link() && points_at_target => (
                RootDisposition::MigratedNoBackup,
                "搬家已经完成，备份也清理过了".to_string(),
                "不用处理".to_string(),
                true,
            ),
            (state, _, _) if state.is_link() && !points_at_target => (
                RootDisposition::UnexpectedLink,
                format!(
                    "原来的位置指向了 {}，和这次搬家的目标不一致",
                    source_state
                        .target()
                        .map(|t| t.display().to_string())
                        .unwrap_or_else(|| "未知位置".into())
                ),
                "请先确认这个指向是怎么来的，工具不会擅自改动".to_string(),
                false,
            ),
            (LinkState::Missing, true, true) => (
                RootDisposition::SourceRenamedOnly,
                "原来的文件夹已改成备份，但还没接到新位置，Cursor 现在会找不到这些数据".to_string(),
                "建议先把备份恢复回去，然后再重新搬家".to_string(),
                false,
            ),
            (LinkState::Missing, false, true) => (
                RootDisposition::SourceRenamedOnly,
                "原来的文件夹已改成备份，新位置上却没有副本".to_string(),
                "建议马上把备份恢复回去".to_string(),
                false,
            ),
            (LinkState::Missing, true, false) => (
                RootDisposition::MigratedNoBackup,
                "原来的位置找不到了，但新磁盘上的副本是完整的".to_string(),
                "可以重新接到新位置，或让 Cursor 自己再生成一份".to_string(),
                false,
            ),
            (LinkState::Missing, false, false) => (
                RootDisposition::DataMissing,
                "原来的文件夹、新位置上的副本和备份都找不到".to_string(),
                "这个目录可能本来就没有；如果确认数据丢了，可以到设置里复制问题说明".to_string(),
                false,
            ),
            _ => (
                RootDisposition::UnexpectedLink,
                "现在磁盘上的情况和记录对不上".to_string(),
                "请先看清楚再动手".to_string(),
                false,
            ),
        };

    RootDiagnosis {
        root_id: root.root_id.clone(),
        label: root.label.clone(),
        disposition,
        source_path: root.source_path.clone(),
        target_path: root.target_path.clone(),
        backup_path: root.backup_path.clone(),
        source_state,
        target_exists,
        backup_exists,
        explanation,
        suggested_action,
        safe_to_automate: safe,
    }
}

pub fn diagnose(journal: &Journal, operation_id: &str) -> AppResult<RecoveryReport> {
    let plan = journal.plan(operation_id)?;
    let recorded_state = journal.operation_state(operation_id)?;
    let dangling = journal.dangling_intents(operation_id)?;

    let roots: Vec<RootDiagnosis> = plan.roots.iter().map(diagnose_root).collect();

    let any_needs_user = roots.iter().any(|r| !r.safe_to_automate);
    let all_migrated = !roots.is_empty()
        && roots.iter().all(|r| {
            matches!(
                r.disposition,
                RootDisposition::MigratedWithBackup | RootDisposition::MigratedNoBackup
            )
        });
    let all_untouched = roots
        .iter()
        .all(|r| matches!(r.disposition, RootDisposition::Untouched));

    let resolved_state = if all_migrated {
        if roots
            .iter()
            .any(|r| r.disposition == RootDisposition::MigratedWithBackup)
        {
            MigrationState::ActiveBackupRetained
        } else {
            MigrationState::Completed
        }
    } else if all_untouched {
        MigrationState::FailedSafe
    } else if any_needs_user {
        MigrationState::ManualIntervention
    } else {
        MigrationState::PartialCutover
    };

    let summary = match resolved_state {
        MigrationState::ActiveBackupRetained => {
            "上次搬家已经完成，备份还留着。确认无误后就可以清理".to_string()
        }
        MigrationState::Completed => "上次搬家已经完成".to_string(),
        MigrationState::FailedSafe => {
            "上次还没改到原来的文件，数据是完好的，可以重新开始".to_string()
        }
        MigrationState::ManualIntervention => {
            "上次搬家停在需要你确认的地方，请看下面的说明".to_string()
        }
        _ => "上次搬家只完成了一部分，请逐项看一下".to_string(),
    };

    Ok(RecoveryReport {
        operation_id: operation_id.to_string(),
        recorded_state,
        resolved_state,
        roots,
        dangling_actions: dangling
            .into_iter()
            .map(|intent| intent.action)
            .collect(),
        summary,
        requires_user_decision: any_needs_user,
    })
}

/// Diagnoses every operation that was not in a terminal state at last exit.
pub fn diagnose_all(journal: &Journal) -> AppResult<Vec<RecoveryReport>> {
    let mut reports = Vec::new();
    for operation_id in journal.unfinished_operations()? {
        match diagnose(journal, &operation_id) {
            Ok(report) => reports.push(report),
            // A record we can no longer parse must not block startup.
            Err(_) => continue,
        }
    }
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn planned(temp: &std::path::Path) -> PlannedRoot {
        PlannedRoot {
            root_id: "cache".into(),
            label: "缓存".into(),
            source_path: temp.join("source"),
            target_path: temp.join("target"),
            backup_path: temp.join("source.csm-backup-1"),
            staging_path: temp.join(".staging/cache"),
            estimated_bytes: 0,
            estimated_files: 0,
        }
    }

    #[test]
    fn an_intact_source_with_nothing_else_is_untouched() {
        let temp = tempfile::tempdir().unwrap();
        let root = planned(temp.path());
        std::fs::create_dir_all(&root.source_path).unwrap();

        let diagnosis = diagnose_root(&root);
        assert_eq!(diagnosis.disposition, RootDisposition::Untouched);
        assert!(diagnosis.safe_to_automate);
    }

    #[test]
    fn a_leftover_target_with_an_intact_source_is_safe_to_clean() {
        let temp = tempfile::tempdir().unwrap();
        let root = planned(temp.path());
        std::fs::create_dir_all(&root.source_path).unwrap();
        std::fs::create_dir_all(&root.target_path).unwrap();

        let diagnosis = diagnose_root(&root);
        assert_eq!(diagnosis.disposition, RootDisposition::StagedOnly);
        assert!(diagnosis.safe_to_automate);
    }

    #[test]
    fn a_renamed_source_without_a_link_demands_attention() {
        let temp = tempfile::tempdir().unwrap();
        let root = planned(temp.path());
        std::fs::create_dir_all(&root.backup_path).unwrap();
        std::fs::create_dir_all(&root.target_path).unwrap();

        let diagnosis = diagnose_root(&root);
        assert_eq!(diagnosis.disposition, RootDisposition::SourceRenamedOnly);
        assert!(!diagnosis.safe_to_automate);
    }

    #[test]
    fn everything_missing_is_reported_rather_than_silently_ignored() {
        let temp = tempfile::tempdir().unwrap();
        let diagnosis = diagnose_root(&planned(temp.path()));
        assert_eq!(diagnosis.disposition, RootDisposition::DataMissing);
        assert!(!diagnosis.safe_to_automate);
    }

    #[test]
    fn a_completed_migration_is_recognised_from_disk_alone() {
        let temp = tempfile::tempdir().unwrap();
        let root = planned(temp.path());
        std::fs::create_dir_all(&root.target_path).unwrap();
        std::fs::create_dir_all(&root.backup_path).unwrap();
        if platforms::create_directory_link(&root.source_path, &root.target_path).is_err() {
            return; // link creation may be denied in restricted environments
        }

        let diagnosis = diagnose_root(&root);
        assert_eq!(diagnosis.disposition, RootDisposition::MigratedWithBackup);
        assert!(diagnosis.safe_to_automate);
    }
}
