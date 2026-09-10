//! Turns a scan plus a user selection into a concrete, preflighted plan.
//!
//! Every reason a migration could go wrong is discovered here, before a single
//! byte is written. Blockers stop the run; warnings are surfaced for the user
//! to acknowledge individually.

use crate::core::model::*;
use crate::core::scanner::{required_headroom, required_target_bytes};
use crate::error::{AppError, AppResult};
use crate::platforms;
use crate::util::{is_inside, now_millis, paths_equal, timestamp_slug};

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Directory inside the target root that holds in-progress copies. Keeping it
/// on the destination volume makes promotion a rename rather than a second copy.
pub const STAGING_DIR: &str = ".csm-staging";
/// Suffix for the renamed-aside original directory.
pub const BACKUP_SUFFIX: &str = "csm-backup";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationRequest {
    pub root_ids: Vec<String>,
    pub target_root: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetProbe {
    pub path: PathBuf,
    pub writable: bool,
    pub volume: Option<String>,
    pub message: Option<String>,
}

pub fn backup_path_for(source: &Path, stamp: &str) -> PathBuf {
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "cursor-data".to_string());
    let parent = source.parent().unwrap_or(Path::new("."));
    parent.join(format!("{name}.{BACKUP_SUFFIX}-{stamp}"))
}

/// Confirms the destination directory can actually be written to.
///
/// A directory that exists and looks fine can still be read-only, or on a
/// filesystem that silently rejects our access pattern, so we create and remove
/// a real file rather than trusting metadata.
pub fn probe_target(path: &Path) -> TargetProbe {
    let volume = platforms::volume_root_for(path);
    if let Err(error) = std::fs::create_dir_all(path) {
        return TargetProbe {
            path: path.to_path_buf(),
            writable: false,
            volume,
            message: Some(format!("无法创建目标目录: {error}")),
        };
    }

    let probe_file = path.join(format!(".csm-write-probe-{}", timestamp_slug()));
    let result = std::fs::write(&probe_file, b"cursor-space-manager");
    let readable = result
        .as_ref()
        .ok()
        .and_then(|_| std::fs::read(&probe_file).ok())
        .map(|content| content == b"cursor-space-manager")
        .unwrap_or(false);
    let _ = std::fs::remove_file(&probe_file);

    match result {
        Ok(()) if readable => TargetProbe {
            path: path.to_path_buf(),
            writable: true,
            volume,
            message: None,
        },
        Ok(()) => TargetProbe {
            path: path.to_path_buf(),
            writable: false,
            volume,
            message: Some("目标目录写入后无法读回，可能是同步盘或有异常过滤驱动".into()),
        },
        Err(error) => TargetProbe {
            path: path.to_path_buf(),
            writable: false,
            volume,
            message: Some(format!("目标目录不可写: {error}")),
        },
    }
}

/// Returns true when the directory contains no entries we did not create.
pub(crate) fn target_slot_is_free(path: &Path) -> bool {
    match std::fs::read_dir(path) {
        Ok(mut entries) => entries.next().is_none(),
        // Not existing is the normal case and is fine.
        Err(_) => !path.exists(),
    }
}

pub fn build_plan(report: &ScanReport, request: &MigrationRequest) -> AppResult<MigrationPlan> {
    if request.root_ids.is_empty() {
        return Err(AppError::invalid("请至少选择一个要迁移的目录"));
    }

    let operation_id = uuid::Uuid::new_v4().to_string();
    let stamp = timestamp_slug();
    let target_root = request.target_root.clone();

    let mut blockers: Vec<PreflightIssue> = Vec::new();
    let mut warnings: Vec<PreflightIssue> = Vec::new();

    // -- Destination volume ------------------------------------------------
    let target_volume = platforms::volume_root_for(&target_root).unwrap_or_default();
    let volume_info = report
        .volumes
        .iter()
        .find(|v| v.mount.eq_ignore_ascii_case(&target_volume));

    match volume_info {
        None => blockers.push(PreflightIssue::blocker(
            "targetVolumeUnknown",
            format!("无法识别目标路径所在的磁盘: {}", target_root.display()),
        )),
        Some(volume) if !volume.eligible_target => {
            blockers.push(PreflightIssue::blocker(
                "targetVolumeIneligible",
                volume
                    .ineligible_reason
                    .clone()
                    .unwrap_or_else(|| "目标磁盘不适合作为迁移目标".into()),
            ));
        }
        Some(_) => {}
    }

    let probe = probe_target(&target_root);
    if !probe.writable {
        blockers.push(PreflightIssue::blocker(
            "targetNotWritable",
            probe
                .message
                .clone()
                .unwrap_or_else(|| "目标目录不可写".into()),
        ));
    }

    // -- Selected roots ----------------------------------------------------
    let mut planned: Vec<PlannedRoot> = Vec::new();
    let mut source_volumes: Vec<String> = Vec::new();

    for root_id in &request.root_ids {
        let Some(root) = report.roots.iter().find(|r| &r.id == root_id) else {
            blockers.push(PreflightIssue::blocker(
                "unknownRoot",
                format!("扫描结果中不存在目录 {root_id}"),
            ));
            continue;
        };

        if !root.exists {
            blockers.push(
                PreflightIssue::blocker("rootMissing", format!("{} 已不存在", root.label))
                    .with_root(root_id),
            );
            continue;
        }
        if root.link_state.is_link() {
            blockers.push(
                PreflightIssue::blocker(
                    "rootAlreadyLinked",
                    format!("{} 已经指向别处，不用再搬一次", root.label),
                )
                .with_root(root_id),
            );
            continue;
        }
        if root.recommendation == Recommendation::NotRecommended && root.id == "install-dir" {
            blockers.push(
                PreflightIssue::blocker(
                    "installDirNotMigratable",
                    "安装目录请留在原处，搬走后可能没法自动更新或正常卸载",
                )
                .with_root(root_id),
            );
            continue;
        }

        let target_path = match &root.prior_copy {
            Some(prior) if is_inside(prior, &target_root) || paths_equal(prior, &target_root) => {
                prior.clone()
            }
            _ => target_root.join(&root.id),
        };
        let using_prior_copy = root
            .prior_copy
            .as_ref()
            .is_some_and(|prior| paths_equal(&target_path, prior));
        let staging_path = target_root
            .join(STAGING_DIR)
            .join(&operation_id)
            .join(&root.id);
        let backup_path = backup_path_for(&root.path, &stamp);

        // A target inside the source (or the reverse) would make the copy
        // recurse into itself or make the junction point at its own contents.
        if is_inside(&target_root, &root.path) || is_inside(&root.path, &target_root) {
            blockers.push(
                PreflightIssue::blocker(
                    "targetOverlapsSource",
                    format!("目标目录与 {} 存在包含关系", root.label),
                )
                .with_root(root_id),
            );
            continue;
        }

        if !target_slot_is_free(&target_path) {
            if using_prior_copy {
                warnings.push(
                    PreflightIssue::warning(
                        "priorCopyPresent",
                        format!(
                            "{} 的目标位置已有先前副本，切换前会先放到旁边再写入校验过的新副本",
                            root.label
                        ),
                    )
                    .with_root(root_id)
                    .with_detail(target_path.display().to_string()),
                );
            } else {
                blockers.push(
                    PreflightIssue::blocker(
                        "targetSlotOccupied",
                        format!("{} 已存在且非空，请先清理", target_path.display()),
                    )
                    .with_root(root_id)
                    .with_detail("为避免覆盖历史数据，工具不会写入已有内容的目录"),
                );
                continue;
            }
        }

        // The cutover renames the source aside. That is only atomic, and only
        // instant, when the backup stays on the same volume.
        if !platforms::same_volume(&root.path, &backup_path) {
            blockers.push(
                PreflightIssue::blocker(
                    "backupCrossVolume",
                    format!("{} 的备份位置与源不在同一磁盘", root.label),
                )
                .with_root(root_id),
            );
            continue;
        }

        if platforms::same_volume(&root.path, &target_root) {
            warnings.push(
                PreflightIssue::warning(
                    "sameVolume",
                    format!("{} 与目标位于同一磁盘，迁移不会释放空间", root.label),
                )
                .with_root(root_id),
            );
        }

        if let Some(volume) = &root.volume {
            if !source_volumes.contains(volume) {
                source_volumes.push(volume.clone());
            }
        }

        let size = root.size.unwrap_or_default();
        if size.error_count > 0 {
            warnings.push(
                PreflightIssue::warning(
                    "partialScan",
                    format!("{} 有 {} 个条目无法读取", root.label, size.error_count),
                )
                .with_root(root_id)
                .with_detail("这些条目可能在复制阶段同样失败"),
            );
        }

        planned.push(PlannedRoot {
            root_id: root.id.clone(),
            label: root.label.clone(),
            source_path: root.path.clone(),
            target_path,
            backup_path,
            staging_path,
            estimated_bytes: size.logical_bytes,
            estimated_files: size.file_count,
        });
    }

    // Nesting between two selected roots would mean migrating the same data
    // twice and leaving the inner junction dangling.
    for outer in &planned {
        for inner in &planned {
            if outer.root_id != inner.root_id && is_inside(&inner.source_path, &outer.source_path) {
                blockers.push(PreflightIssue::blocker(
                    "nestedSelection",
                    format!("{} 位于 {} 内部，不能同时迁移", inner.label, outer.label),
                ));
            }
        }
    }

    let total_bytes: u64 = planned.iter().map(|p| p.estimated_bytes).sum();
    let total_files: u64 = planned.iter().map(|p| p.estimated_files).sum();
    let required = required_target_bytes(total_bytes);

    if let Some(volume) = volume_info {
        let headroom = required_headroom(volume.total_bytes);
        let needed = required.saturating_add(headroom);
        if volume.free_bytes < needed {
            blockers.push(
                PreflightIssue::blocker(
                    "targetInsufficientSpace",
                    format!(
                        "{} 可用 {}，本次至少需要 {}（含复制余量与保留空间）",
                        volume.mount,
                        crate::util::format_bytes(volume.free_bytes),
                        crate::util::format_bytes(needed)
                    ),
                )
                .with_detail("留出保留空间是为了避免目标盘随后被写满"),
            );
        }
    }

    // -- Runtime state -----------------------------------------------------
    if report.installation.running {
        warnings.push(PreflightIssue::warning(
            "cursorRunning",
            "Cursor 正在运行，执行前需要先完全退出",
        ));
    }
    if !report.installation.conflicts.is_empty() {
        warnings.push(PreflightIssue::warning(
            "multipleInstalls",
            "检测到多个 Cursor 安装，请确认迁移的是正在使用的那一个",
        ));
    }
    if planned.is_empty() && blockers.is_empty() {
        blockers.push(PreflightIssue::blocker("nothingToDo", "没有可迁移的目录"));
    }

    Ok(MigrationPlan {
        operation_id,
        created_at: now_millis(),
        target_root,
        target_volume,
        source_volume: source_volumes.join(", "),
        roots: planned,
        total_bytes,
        total_files,
        required_target_bytes: required,
        blockers,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_root(id: &str, path: PathBuf, bytes: u64) -> DataRoot {
        DataRoot {
            id: id.into(),
            label: id.into(),
            purpose: String::new(),
            resolved_path: path.clone(),
            volume: platforms::volume_root_for(&path),
            path,
            link_state: LinkState::Regular,
            exists: true,
            recommendation: Recommendation::Recommended,
            reason: String::new(),
            size: Some(DirSize {
                logical_bytes: bytes,
                on_disk_bytes: bytes,
                file_count: 1,
                dir_count: 0,
                error_count: 0,
            }),
            scan_errors: Vec::new(),
            from_launch_argument: false,
            prior_copy: None,
        }
    }

    fn sample_report(roots: Vec<DataRoot>, volumes: Vec<VolumeInfo>) -> ScanReport {
        ScanReport {
            scan_id: "scan".into(),
            scanned_at: 0,
            platform: platforms::platform_name().into(),
            installation: AppInstallation::default(),
            roots,
            volumes,
            total_logical_bytes: 0,
            reclaimable_bytes: 0,
            suggested_target: None,
            prior_migrations: Vec::new(),
            warnings: Vec::new(),
            duration_ms: 0,
        }
    }

    #[test]
    fn backup_name_sits_next_to_the_source() {
        let source = PathBuf::from("/tmp/data/Cache");
        let backup = backup_path_for(&source, "20260101-101010");
        assert_eq!(backup.parent(), source.parent());
        assert!(backup
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("Cache.csm-backup-"));
    }

    #[test]
    fn empty_selection_is_rejected() {
        let report = sample_report(Vec::new(), Vec::new());
        let request = MigrationRequest {
            root_ids: Vec::new(),
            target_root: std::env::temp_dir().join("csm-target"),
        };
        assert!(build_plan(&report, &request).is_err());
    }

    #[test]
    fn overlapping_target_and_source_is_a_blocker() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        let target = source.join("inside");

        let report = sample_report(vec![sample_root("a", source, 10)], Vec::new());
        let request = MigrationRequest {
            root_ids: vec!["a".into()],
            target_root: target,
        };
        let plan = build_plan(&report, &request).unwrap();
        assert!(plan
            .blockers
            .iter()
            .any(|b| b.code == "targetOverlapsSource"));
    }

    #[test]
    fn missing_root_is_a_blocker() {
        let report = sample_report(Vec::new(), Vec::new());
        let request = MigrationRequest {
            root_ids: vec!["ghost".into()],
            target_root: std::env::temp_dir().join("csm-target"),
        };
        let plan = build_plan(&report, &request).unwrap();
        assert!(plan.blockers.iter().any(|b| b.code == "unknownRoot"));
    }

    #[test]
    fn occupied_target_slot_is_a_blocker() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        std::fs::create_dir_all(&source).unwrap();
        let target = temp.path().join("target");
        std::fs::create_dir_all(target.join("a")).unwrap();
        std::fs::write(target.join("a").join("existing.txt"), b"keep me").unwrap();

        let report = sample_report(vec![sample_root("a", source, 10)], Vec::new());
        let request = MigrationRequest {
            root_ids: vec!["a".into()],
            target_root: target,
        };
        let plan = build_plan(&report, &request).unwrap();
        assert!(plan.blockers.iter().any(|b| b.code == "targetSlotOccupied"));
    }

    #[test]
    fn a_matching_prior_copy_is_reused_instead_of_blocked() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let cache = temp.path().join("cache");
        let prior = cache.join("Cursor-Roaming").join("logs");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&prior).unwrap();
        std::fs::write(prior.join("old.log"), b"previous copy").unwrap();

        let mut root = sample_root("roaming-logs", source, 10);
        root.prior_copy = Some(prior.clone());
        let report = sample_report(vec![root], Vec::new());
        let request = MigrationRequest {
            root_ids: vec!["roaming-logs".into()],
            target_root: cache,
        };
        let plan = build_plan(&report, &request).unwrap();
        assert!(
            !plan.blockers.iter().any(|b| b.code == "targetSlotOccupied"),
            "{:?}",
            plan.blockers
        );
        assert_eq!(plan.roots[0].target_path, prior);
        assert!(plan.warnings.iter().any(|w| w.code == "priorCopyPresent"));
    }

    #[test]
    fn write_probe_succeeds_on_a_normal_directory() {
        let temp = tempfile::tempdir().unwrap();
        let probe = probe_target(&temp.path().join("nested").join("target"));
        assert!(probe.writable, "{:?}", probe.message);
        // The probe must not leave anything behind.
        let leftovers: Vec<_> = std::fs::read_dir(temp.path().join("nested").join("target"))
            .unwrap()
            .collect();
        assert!(leftovers.is_empty());
    }
}
