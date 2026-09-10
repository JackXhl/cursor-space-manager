//! Read-only discovery.
//!
//! Nothing in this module writes to the filesystem. It answers: is Cursor
//! installed, where is its data, how large is it, what is already a link, and
//! which volume could hold it instead.

use crate::core::model::*;
use crate::error::{AppError, AppResult};
use crate::platforms;
use crate::util::now_millis;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

/// Space the destination must keep free after the copy, so the target volume
/// does not become the next problem.
pub const TARGET_HEADROOM_BYTES: u64 = 10 * 1024 * 1024 * 1024;
/// Fixed slack added on top of the measured size to absorb growth between the
/// scan and the copy.
pub const COPY_SLACK_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Walks a directory and totals what it contains.
///
/// Symlinks and junctions are counted as entries but never followed: doing so
/// would double-count data or walk into an unrelated tree.
pub fn measure_directory(root: &Path, cancel: &AtomicBool) -> DirSize {
    let mut size = DirSize::default();
    if !root.exists() {
        return size;
    }

    let walker = walkdir::WalkDir::new(root).follow_links(false).min_depth(1);
    for entry in walker {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                size.error_count += 1;
                continue;
            }
        };

        let file_type = entry.file_type();
        if file_type.is_dir() {
            size.dir_count += 1;
            continue;
        }
        if file_type.is_symlink() {
            // The link itself occupies no meaningful space and its target is
            // accounted for wherever that target actually lives.
            continue;
        }

        match entry.metadata() {
            Ok(metadata) => {
                let logical = metadata.len();
                size.logical_bytes += logical;
                size.on_disk_bytes += platforms::allocated_size(entry.path(), &metadata);
                size.file_count += 1;
            }
            Err(_) => size.error_count += 1,
        }
    }

    size
}

/// Cheap pre-count used to size progress bars before the real copy starts.
pub fn count_entries(root: &Path, cancel: &AtomicBool) -> (u64, u64) {
    let size = measure_directory(root, cancel);
    (size.logical_bytes, size.file_count)
}

fn describe_link_state(state: &LinkState) -> Option<String> {
    match state {
        LinkState::Junction(target) => Some(format!("已经指向其他位置：{}", target.display())),
        LinkState::Symlink(target) => Some(format!("已经指向其他位置：{}", target.display())),
        LinkState::UnknownReparse(_) => {
            Some("这里有一个特殊链接，工具不会自动改动".into())
        }
        _ => None,
    }
}

pub struct ScanOptions {
    pub scan_id: String,
    /// Root ids the user asked to skip entirely.
    pub excluded_ids: Vec<String>,
}

/// Runs a full scan, reporting progress as each root is measured.
///
/// A failure on one directory downgrades that entry instead of aborting: a
/// permission error on the log folder should not hide a 20 GB extensions
/// directory.
pub fn run_scan(
    options: &ScanOptions,
    cancel: Arc<AtomicBool>,
    emit: &mut dyn FnMut(ScanProgress),
) -> AppResult<ScanReport> {
    let started = Instant::now();
    let mut warnings = Vec::new();

    emit(ScanProgress {
        scan_id: options.scan_id.clone(),
        phase: ScanPhase::Starting,
        message: "正在准备扫描".into(),
        root_id: None,
        size: None,
    });

    emit(ScanProgress {
        scan_id: options.scan_id.clone(),
        phase: ScanPhase::Installation,
        message: "正在检测 Cursor 安装状态".into(),
        root_id: None,
        size: None,
    });
    let installation = platforms::discover_installation();
    if !installation.found {
        warnings.push("未找到 Cursor 安装，仍会显示已知数据目录以便手动确认".to_string());
    }
    if !installation.conflicts.is_empty() {
        warnings.push(format!(
            "检测到 {} 个额外的安装位置",
            installation.conflicts.len()
        ));
    }

    emit(ScanProgress {
        scan_id: options.scan_id.clone(),
        phase: ScanPhase::Directories,
        message: "正在定位数据目录".into(),
        root_id: None,
        size: None,
    });

    let candidates: Vec<_> = platforms::candidate_roots()
        .into_iter()
        .filter(|c| !options.excluded_ids.contains(&c.id))
        .collect();

    let mut roots: Vec<DataRoot> = Vec::new();
    for candidate in candidates {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }

        let state = platforms::link_state(&candidate.path);
        let exists = !matches!(state, LinkState::Missing);
        let resolved = state
            .target()
            .cloned()
            .unwrap_or_else(|| candidate.path.clone());

        let mut recommendation = candidate.recommendation;
        let mut reason = candidate.reason.clone();
        let mut scan_errors = Vec::new();

        if !exists {
            recommendation = Recommendation::NotRecommended;
            reason = "目录不存在，无需迁移".to_string();
        } else if let Some(note) = describe_link_state(&state) {
            recommendation = Recommendation::NotRecommended;
            reason = note;
        }

        emit(ScanProgress {
            scan_id: options.scan_id.clone(),
            phase: ScanPhase::Sizing,
            message: format!("正在统计 {}", candidate.label),
            root_id: Some(candidate.id.clone()),
            size: None,
        });

        let size = if exists {
            let measured = measure_directory(&resolved, &cancel);
            if measured.error_count > 0 {
                scan_errors.push(format!(
                    "{} 个条目无法读取，统计结果为下限",
                    measured.error_count
                ));
            }
            Some(measured)
        } else {
            None
        };

        emit(ScanProgress {
            scan_id: options.scan_id.clone(),
            phase: ScanPhase::Sizing,
            message: format!("{} 统计完成", candidate.label),
            root_id: Some(candidate.id.clone()),
            size,
        });

        roots.push(DataRoot {
            id: candidate.id,
            label: candidate.label,
            purpose: candidate.purpose,
            // Attribute the bytes to the volume that actually stores them. For an
            // already-linked root that is the link target, not the original path.
            volume: platforms::volume_root_for(&resolved),
            path: candidate.path,
            resolved_path: resolved,
            link_state: state,
            exists,
            recommendation,
            reason,
            size,
            scan_errors,
            from_launch_argument: candidate.from_launch_argument,
            prior_copy: None,
        });
    }

    emit(ScanProgress {
        scan_id: options.scan_id.clone(),
        phase: ScanPhase::Volumes,
        message: "正在读取磁盘信息".into(),
        root_id: None,
        size: None,
    });
    let volumes = platforms::list_volumes();

    emit(ScanProgress {
        scan_id: options.scan_id.clone(),
        phase: ScanPhase::Directories,
        message: "正在查找先前迁移痕迹".into(),
        root_id: None,
        size: None,
    });
    let prior_migrations = crate::core::prior::discover(&roots, &cancel);
    crate::core::prior::attach_to_roots(&mut roots, &prior_migrations);
    if !prior_migrations.is_empty() {
        warnings.push(format!("发现 {} 处先前迁移位置", prior_migrations.len()));
    }

    let total_logical_bytes = roots
        .iter()
        .filter_map(|r| r.size.as_ref())
        .map(|s| s.logical_bytes)
        .sum();
    let reclaimable_bytes = roots
        .iter()
        .filter(|r| r.recommendation == Recommendation::Recommended && r.exists)
        .filter_map(|r| r.size.as_ref())
        .map(|s| s.logical_bytes)
        .sum();

    let suggested_target = suggest_target(&volumes, reclaimable_bytes, &prior_migrations);
    if suggested_target.is_none() {
        warnings.push("没有找到满足条件的目标磁盘".to_string());
    }

    emit(ScanProgress {
        scan_id: options.scan_id.clone(),
        phase: ScanPhase::Done,
        message: "扫描完成".into(),
        root_id: None,
        size: None,
    });

    Ok(ScanReport {
        scan_id: options.scan_id.clone(),
        scanned_at: now_millis(),
        platform: platforms::platform_name().to_string(),
        installation,
        roots,
        volumes,
        total_logical_bytes,
        reclaimable_bytes,
        suggested_target,
        prior_migrations,
        warnings,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

/// Space a destination volume must have free to accept `payload_bytes`.
pub fn required_target_bytes(payload_bytes: u64) -> u64 {
    let with_ratio = (payload_bytes as f64 * 1.15) as u64;
    let with_slack = payload_bytes.saturating_add(COPY_SLACK_BYTES);
    with_ratio.max(with_slack)
}

/// Reserve the destination should still have after the copy: 10% of capacity,
/// but never less than 10 GiB.
pub fn required_headroom(total_bytes: u64) -> u64 {
    (total_bytes / 10).max(TARGET_HEADROOM_BYTES)
}

/// Picks a destination. An existing migration site wins over an empty
/// `CursorData` folder, so a second run continues where the first left off.
pub fn suggest_target(
    volumes: &[VolumeInfo],
    payload_bytes: u64,
    prior_sites: &[PriorSite],
) -> Option<TargetSuggestion> {
    let required = required_target_bytes(payload_bytes);

    if let Some(directory) = crate::core::prior::preferred_directory(prior_sites) {
        if let Some(volume) = volumes.iter().find(|volume| {
            volume.eligible_target
                && platforms::volume_root_for(&directory)
                    .map(|mount| volume.mount.eq_ignore_ascii_case(&mount))
                    .unwrap_or(false)
        }) {
            let headroom = required_headroom(volume.total_bytes);
            if volume.free_bytes >= required.saturating_add(headroom) {
                return Some(TargetSuggestion {
                    volume: volume.mount.clone(),
                    directory,
                    free_bytes: volume.free_bytes,
                    required_bytes: required,
                    reason: "检测到先前迁移位置，后续目录建议继续放到这里".into(),
                });
            }
        }
    }

    let mut best: Option<&VolumeInfo> = None;
    for volume in volumes.iter().filter(|v| v.eligible_target) {
        let headroom = required_headroom(volume.total_bytes);
        if volume.free_bytes < required.saturating_add(headroom) {
            continue;
        }
        if best
            .map(|b| volume.free_bytes > b.free_bytes)
            .unwrap_or(true)
        {
            best = Some(volume);
        }
    }

    best.map(|volume| TargetSuggestion {
        volume: volume.mount.clone(),
        directory: platforms::default_target_directory(&volume.mount),
        free_bytes: volume.free_bytes,
        required_bytes: required,
        reason: format!(
            "{} 是可用空间最多的固定磁盘，复制后仍会保留足够余量",
            volume.mount
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(mount: &str, total: u64, free: u64, eligible: bool) -> VolumeInfo {
        VolumeInfo {
            mount: mount.into(),
            label: String::new(),
            filesystem: "NTFS".into(),
            total_bytes: total,
            free_bytes: free,
            kind: DriveKind::Fixed,
            is_system: false,
            eligible_target: eligible,
            ineligible_reason: None,
        }
    }

    const GB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn required_bytes_uses_the_larger_of_ratio_and_slack() {
        // Small payloads are dominated by the fixed slack.
        assert_eq!(required_target_bytes(GB), 3 * GB);
        // Large payloads are dominated by the 15% ratio.
        assert_eq!(
            required_target_bytes(100 * GB),
            (100.0 * GB as f64 * 1.15) as u64
        );
    }

    #[test]
    fn headroom_never_drops_below_the_floor() {
        assert_eq!(required_headroom(50 * GB), TARGET_HEADROOM_BYTES);
        assert_eq!(required_headroom(1000 * GB), 100 * GB);
    }

    #[test]
    fn suggestion_skips_volumes_without_headroom() {
        let volumes = vec![
            volume("D:\\", 200 * GB, 30 * GB, true),
            volume("E:\\", 500 * GB, 300 * GB, true),
        ];
        // 30 GB payload needs ~34.5 GB plus 20 GB headroom, so D: is out.
        let suggestion = suggest_target(&volumes, 30 * GB, &[]).expect("expected a target");
        assert_eq!(suggestion.volume, "E:\\");
    }

    #[test]
    fn ineligible_volumes_are_never_suggested() {
        let volumes = vec![volume("F:\\", 2000 * GB, 1900 * GB, false)];
        assert!(suggest_target(&volumes, GB, &[]).is_none());
    }

    #[test]
    fn measuring_a_missing_directory_is_empty_not_an_error() {
        let cancel = AtomicBool::new(false);
        let size = measure_directory(Path::new("./definitely-not-here-9f2a"), &cancel);
        assert_eq!(size.file_count, 0);
        assert_eq!(size.logical_bytes, 0);
        assert_eq!(size.error_count, 0);
    }

    #[test]
    fn measuring_counts_files_and_directories() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("nested")).unwrap();
        std::fs::write(temp.path().join("a.txt"), vec![b'x'; 100]).unwrap();
        std::fs::write(temp.path().join("nested/b.txt"), vec![b'y'; 200]).unwrap();

        let cancel = AtomicBool::new(false);
        let size = measure_directory(temp.path(), &cancel);
        assert_eq!(size.file_count, 2);
        assert_eq!(size.dir_count, 1);
        assert_eq!(size.logical_bytes, 300);
        assert!(size.on_disk_bytes >= size.logical_bytes);
    }
}
