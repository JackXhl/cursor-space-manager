//! macOS and Linux implementation.
//!
//! The migration state machine is shared with Windows; only the path layout,
//! volume enumeration, link type (symlink instead of junction), and copy engine
//! differ.

use crate::core::model::{
    AppInstallation, DriveKind, InstallKind, LinkState, Recommendation, RunningProcess, VolumeInfo,
};
use crate::error::{AppError, AppResult};
use crate::platforms::{CandidateRoot, CopyOutcome};

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub fn platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

// ---------------------------------------------------------------------------
// Volumes
// ---------------------------------------------------------------------------

/// Mount points we never offer as a destination because they are virtual,
/// volatile, or owned by the system.
const EXCLUDED_MOUNT_PREFIXES: &[&str] = &[
    "/proc",
    "/sys",
    "/dev",
    "/run",
    "/snap",
    "/boot",
    "/var/lib/docker",
    "/System/Volumes/Preboot",
    "/System/Volumes/VM",
    "/System/Volumes/Update",
    "/System/Volumes/xarts",
    "/System/Volumes/iSCPreboot",
    "/System/Volumes/Hardware",
    "/private/var/vm",
];

/// Path-aware prefix test, so `/boot` does not also match `/bootcamp`.
fn under_prefix(mount: &str, prefix: &str) -> bool {
    mount == prefix
        || mount
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

const VIRTUAL_FILESYSTEMS: &[&str] = &[
    "proc",
    "sysfs",
    "devtmpfs",
    "devpts",
    "tmpfs",
    "cgroup",
    "cgroup2",
    "overlay",
    "squashfs",
    "autofs",
    "fusectl",
    "debugfs",
    "tracefs",
    "securityfs",
    "pstore",
    "bpf",
    "configfs",
    "mqueue",
    "hugetlbfs",
    "ramfs",
    "nsfs",
];

const NETWORK_FILESYSTEMS: &[&str] = &[
    "nfs",
    "nfs4",
    "cifs",
    "smbfs",
    "afpfs",
    "webdav",
    "sshfs",
    "fuse.sshfs",
];

#[derive(Debug, Clone)]
struct MountEntry {
    mount: String,
    filesystem: String,
    source: String,
}

#[cfg(target_os = "linux")]
fn read_mounts() -> Vec<MountEntry> {
    let mut entries = Vec::new();
    let Ok(text) = std::fs::read_to_string("/proc/self/mounts") else {
        return entries;
    };
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 3 {
            continue;
        }
        entries.push(MountEntry {
            // Mount points are octal escaped in /proc; spaces are the common case.
            mount: fields[1].replace("\\040", " "),
            filesystem: fields[2].to_string(),
            source: fields[0].to_string(),
        });
    }
    entries
}

#[cfg(target_os = "macos")]
fn read_mounts() -> Vec<MountEntry> {
    let mut entries = Vec::new();
    let Ok(output) = std::process::Command::new("/sbin/mount").output() else {
        return entries;
    };
    let text = String::from_utf8_lossy(&output.stdout);
    // Format: `/dev/disk1s1 on / (apfs, local, journaled)`
    for line in text.lines() {
        let Some((left, right)) = line.split_once(" on ") else {
            continue;
        };
        let Some((mount, rest)) = right.rsplit_once(" (") else {
            continue;
        };
        let filesystem = rest
            .trim_end_matches(')')
            .split(',')
            .next()
            .unwrap_or("")
            .trim();
        entries.push(MountEntry {
            mount: mount.to_string(),
            filesystem: filesystem.to_string(),
            source: left.to_string(),
        });
    }
    entries
}

fn statvfs_space(path: &Path) -> Option<(u64, u64)> {
    let output = std::process::Command::new("df")
        .arg("-kP")
        .arg(path.as_os_str())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().nth(1)?;
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 4 {
        return None;
    }
    let total = fields[1].parse::<u64>().ok()? * 1024;
    let available = fields[3].parse::<u64>().ok()? * 1024;
    Some((total, available))
}

pub fn list_volumes() -> Vec<VolumeInfo> {
    let mut volumes = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for entry in read_mounts() {
        if !seen.insert(entry.mount.clone()) {
            continue;
        }
        if EXCLUDED_MOUNT_PREFIXES
            .iter()
            .any(|p| under_prefix(&entry.mount, p))
        {
            continue;
        }
        let fs_lower = entry.filesystem.to_lowercase();
        if VIRTUAL_FILESYSTEMS.contains(&fs_lower.as_str()) && entry.mount != "/" {
            continue;
        }

        let kind = if NETWORK_FILESYSTEMS.contains(&fs_lower.as_str()) {
            DriveKind::Network
        } else if VIRTUAL_FILESYSTEMS.contains(&fs_lower.as_str()) {
            DriveKind::RamDisk
        } else if !cfg!(target_os = "macos")
            && (under_prefix(&entry.mount, "/media") || under_prefix(&entry.mount, "/mnt"))
        {
            // On Linux these are conventionally where removable and temporary
            // media land. On macOS every non-boot volume mounts under /Volumes,
            // including the internal second drive that is the whole point of
            // this tool, and telling them apart needs IOKit. So there we let the
            // user choose and rely on the write probe and preflight instead.
            DriveKind::Removable
        } else {
            DriveKind::Fixed
        };

        let (total_bytes, free_bytes) = statvfs_space(Path::new(&entry.mount)).unwrap_or((0, 0));
        if total_bytes == 0 && entry.mount != "/" {
            continue;
        }

        let is_system = entry.mount == "/";
        let (eligible, reason) = match kind {
            _ if is_system => (false, Some("系统卷，迁移到这里不会释放空间".to_string())),
            DriveKind::Network => (false, Some("网络位置不稳定，Cursor 可能打不开".to_string())),
            DriveKind::RamDisk => (false, Some("内存文件系统，重启后数据会丢失".to_string())),
            DriveKind::CdRom => (false, Some("只读介质".to_string())),
            DriveKind::Removable => (
                false,
                Some("可移动介质，拔出后 Cursor 会读不到数据".to_string()),
            ),
            _ if total_bytes == 0 => (false, Some("无法读取容量".to_string())),
            _ => (true, None),
        };

        volumes.push(VolumeInfo {
            mount: entry.mount.clone(),
            label: entry.source.clone(),
            filesystem: entry.filesystem.clone(),
            total_bytes,
            free_bytes,
            kind,
            is_system,
            eligible_target: eligible,
            ineligible_reason: reason,
        });
    }

    volumes.sort_by(|a, b| a.mount.cmp(&b.mount));
    volumes
}

/// Returns the longest mount point that is a prefix of `path`.
///
/// This reads the mount table directly rather than going through
/// [`list_volumes`]: the callers ask for a path's volume constantly, and
/// measuring free space for every mount each time would spawn a `df` per mount.
pub fn volume_root_for(path: &Path) -> Option<String> {
    let text = path.to_string_lossy().to_string();
    let mut best: Option<String> = None;
    for entry in read_mounts() {
        // Everything is under the root mount; anything else has to be a real
        // path-segment prefix.
        if entry.mount != "/" && !under_prefix(&text, &entry.mount) {
            continue;
        }
        if best
            .as_ref()
            .map(|b| entry.mount.len() > b.len())
            .unwrap_or(true)
        {
            best = Some(entry.mount);
        }
    }
    best.or_else(|| Some("/".to_string()))
}

pub fn same_volume(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let resolve = |path: &Path| -> Option<u64> {
        let mut current = path.to_path_buf();
        // A path that does not exist yet still lives on its parent's device.
        loop {
            if let Ok(meta) = std::fs::metadata(&current) {
                return Some(meta.dev());
            }
            if !current.pop() {
                return None;
            }
        }
    };
    match (resolve(a), resolve(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Links
// ---------------------------------------------------------------------------

pub fn link_state(path: &Path) -> LinkState {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return LinkState::Missing;
    };
    if metadata.file_type().is_symlink() {
        return match std::fs::read_link(path) {
            Ok(target) => LinkState::Symlink(target),
            Err(_) => LinkState::UnknownReparse("无法读取符号链接目标".into()),
        };
    }
    LinkState::Regular
}

pub fn create_directory_link(link: &Path, target: &Path) -> AppResult<()> {
    if !target.is_dir() {
        return Err(AppError::invalid(format!(
            "链接目标不存在: {}",
            target.display()
        )));
    }
    if std::fs::symlink_metadata(link).is_ok() {
        return Err(AppError::invalid(format!(
            "链接位置已被占用: {}",
            link.display()
        )));
    }
    std::os::unix::fs::symlink(target, link)?;
    Ok(())
}

pub fn remove_directory_link(link: &Path) -> AppResult<()> {
    if !link_state(link).is_link() {
        return Err(AppError::unsafe_op(format!(
            "{} 不是链接，拒绝按链接方式删除",
            link.display()
        )));
    }
    // Removing the symlink itself never touches the directory it points at.
    std::fs::remove_file(link)?;
    Ok(())
}

/// Unix reports the allocated block count directly in the stat we already have.
pub fn allocated_size(_path: &Path, metadata: &std::fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.blocks() * 512
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

pub fn running_processes() -> Vec<RunningProcess> {
    let system = sysinfo::System::new_all();
    let mut result = Vec::new();
    for (pid, process) in system.processes() {
        let name = process.name().to_string_lossy().to_string();
        let matches = name.eq_ignore_ascii_case("cursor") || name.eq_ignore_ascii_case("Cursor");
        if !matches {
            continue;
        }
        result.push(RunningProcess {
            pid: pid.as_u32(),
            name,
            exe_path: process.exe().map(|p| p.to_path_buf()),
        });
    }
    result
}

pub fn discover_installation() -> AppInstallation {
    let mut installation = AppInstallation::default();
    let mut candidates: Vec<(InstallKind, PathBuf, PathBuf)> = Vec::new();

    if cfg!(target_os = "macos") {
        let app = PathBuf::from("/Applications/Cursor.app");
        candidates.push((
            InstallKind::Machine,
            app.clone(),
            app.join("Contents").join("MacOS").join("Cursor"),
        ));
        if let Some(home) = home_dir() {
            let user_app = home.join("Applications").join("Cursor.app");
            candidates.push((
                InstallKind::PerUser,
                user_app.clone(),
                user_app.join("Contents").join("MacOS").join("Cursor"),
            ));
        }
    } else {
        for dir in ["/usr/share/cursor", "/opt/cursor", "/opt/Cursor"] {
            let path = PathBuf::from(dir);
            candidates.push((InstallKind::Machine, path.clone(), path.join("cursor")));
        }
        if let Some(home) = home_dir() {
            let local = home.join(".local").join("share").join("cursor");
            candidates.push((InstallKind::PerUser, local.clone(), local.join("cursor")));
        }
    }

    let processes = running_processes();
    for process in &processes {
        if let Some(exe) = &process.exe_path {
            if let Some(dir) = exe.parent() {
                if !candidates.iter().any(|(_, d, _)| d == dir) {
                    candidates.push((InstallKind::Portable, dir.to_path_buf(), exe.clone()));
                }
            }
        }
    }

    let found: Vec<(InstallKind, PathBuf, PathBuf)> = candidates
        .into_iter()
        .filter(|(_, dir, exe)| dir.exists() && (exe.exists() || dir.extension().is_some()))
        .collect();

    if found.is_empty() {
        installation.running = !processes.is_empty();
        installation.processes = processes;
        return installation;
    }

    let (kind, dir, exe) = found[0].clone();
    installation.found = true;
    installation.kind = kind;
    installation.install_dir = Some(dir);
    installation.exe_path = Some(exe);
    installation.conflicts = found.iter().skip(1).map(|(_, d, _)| d.clone()).collect();
    installation.running = !processes.is_empty();
    installation.processes = processes;
    installation
}

pub fn candidate_roots() -> Vec<CandidateRoot> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };

    let base = if cfg!(target_os = "macos") {
        home.join("Library")
            .join("Application Support")
            .join("Cursor")
    } else {
        home.join(".config").join("Cursor")
    };

    let mut roots = vec![
        CandidateRoot::new(
            "roaming-cache",
            "网络与渲染缓存",
            "Chromium 网络请求与渲染缓存",
            base.join("Cache"),
            Recommendation::Recommended,
            "纯缓存数据，体积增长快，迁移风险最低",
        ),
        CandidateRoot::new(
            "roaming-gpucache",
            "GPU 着色器缓存",
            "GPU 着色器编译缓存",
            base.join("GPUCache"),
            Recommendation::Recommended,
            "可随时重建的缓存",
        ),
        CandidateRoot::new(
            "roaming-code-cache",
            "脚本编译缓存",
            "JavaScript 字节码缓存",
            base.join("Code Cache"),
            Recommendation::Recommended,
            "可随时重建的缓存",
        ),
        CandidateRoot::new(
            "roaming-logs",
            "运行日志",
            "主进程与扩展宿主日志",
            base.join("logs"),
            Recommendation::Recommended,
            "只增不减的日志，迁移后不影响功能",
        ),
        CandidateRoot::new(
            "roaming-workspace-storage",
            "工作区状态",
            "每个工作区的编辑器状态与索引",
            base.join("User").join("workspaceStorage"),
            Recommendation::Recommended,
            "通常是用户数据目录中最大的部分",
        ),
        CandidateRoot::new(
            "roaming-global-storage",
            "全局存储",
            "扩展全局状态与聊天记录数据库",
            base.join("User").join("globalStorage"),
            Recommendation::Optional,
            "里面有 Cursor 正在使用的数据，搬家前必须完全退出 Cursor",
        ),
        CandidateRoot::new(
            "roaming-cached-data",
            "扩展宿主编译缓存",
            "扩展宿主的代码缓存",
            base.join("CachedData"),
            Recommendation::Optional,
            "可重建的编译缓存",
        ),
        CandidateRoot::new(
            "roaming-snapshots",
            "崩溃快照",
            "Chromium 崩溃与会话快照",
            base.join("snapshots"),
            Recommendation::Optional,
            "体积可能很大，不影响日常编辑",
        ),
        CandidateRoot::new(
            "dot-extensions",
            "已安装扩展",
            "扩展本体与依赖",
            home.join(".cursor").join("extensions"),
            Recommendation::Recommended,
            "扩展目录通常占用数 GB",
        ),
        CandidateRoot::new(
            "dot-ai-tracking",
            "AI 会话追踪",
            "AI 功能的本地追踪数据",
            home.join(".cursor").join("ai-tracking"),
            Recommendation::Optional,
            "本地分析数据",
        ),
        CandidateRoot::new(
            "dot-worktrees",
            "工作树",
            "Cursor 管理的 git worktree",
            home.join(".cursor").join("worktrees"),
            Recommendation::Optional,
            "与项目索引配套",
        ),
    ];

    if cfg!(target_os = "macos") {
        roots.push(CandidateRoot::new(
            "install-dir",
            "安装目录",
            "Cursor 程序本体",
            PathBuf::from("/Applications/Cursor.app"),
            Recommendation::NotRecommended,
            "请留在原处，搬走后可能没法自动更新",
        ));
    }

    roots
}

pub fn default_target_directory(volume: &str) -> PathBuf {
    Path::new(volume).join("CursorData")
}

pub fn cursor_data_bases() -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if let Some(home) = home_dir() {
        if cfg!(target_os = "macos") {
            bases.push(
                home.join("Library")
                    .join("Application Support")
                    .join("Cursor"),
            );
        } else {
            bases.push(home.join(".config").join("Cursor"));
        }
        bases.push(home.join(".cursor"));
    }
    bases
}

pub fn well_known_prior_search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for volume in list_volumes() {
        if volume.kind != DriveKind::Fixed {
            continue;
        }
        let mount = PathBuf::from(&volume.mount);
        roots.push(mount.join("cache"));
        roots.push(mount.join("CursorData"));
        roots.push(mount.join("Cursor"));
    }
    if let Some(home) = home_dir() {
        roots.push(home.join("CursorData"));
    }
    roots
}

/// Unix has no Restart Manager. Process presence is the signal we can rely on
/// without asking for extra privileges.
pub fn processes_locking(_root: &Path) -> AppResult<Vec<RunningProcess>> {
    Ok(running_processes())
}

// ---------------------------------------------------------------------------
// Copying
// ---------------------------------------------------------------------------

/// Recursive copy that preserves the tree but never follows symlinks out of it.
pub fn copy_tree(
    source: &Path,
    destination: &Path,
    cancel: Arc<AtomicBool>,
    progress: &mut dyn FnMut(u64, u64),
) -> AppResult<CopyOutcome> {
    std::fs::create_dir_all(destination)?;

    let mut bytes = 0u64;
    let mut files = 0u64;
    let mut last_report = std::time::Instant::now();

    for entry in walkdir::WalkDir::new(source).follow_links(false) {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let Ok(relative) = entry.path().strip_prefix(source) else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = destination.join(relative);
        let file_type = entry.file_type();

        if file_type.is_dir() {
            std::fs::create_dir_all(&target)?;
        } else if file_type.is_symlink() {
            // Recreate the link rather than duplicating what it points at.
            if let Ok(link_target) = std::fs::read_link(entry.path()) {
                let _ = std::fs::remove_file(&target);
                std::os::unix::fs::symlink(link_target, &target)?;
            }
        } else if file_type.is_file() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let copied = std::fs::copy(entry.path(), &target)?;
            bytes += copied;
            files += 1;
            if last_report.elapsed() >= std::time::Duration::from_millis(400) {
                progress(bytes, files);
                last_report = std::time::Instant::now();
            }
        }
    }

    progress(bytes, files);
    Ok(CopyOutcome {
        bytes,
        files,
        raw_status: 0,
        log: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_matching_respects_path_segments() {
        assert!(under_prefix("/boot", "/boot"));
        assert!(under_prefix("/boot/efi", "/boot"));
        // The bug this guards against: excluding /boot must not exclude /bootcamp.
        assert!(!under_prefix("/bootcamp", "/boot"));
        assert!(!under_prefix("/bootcamp/data", "/boot"));
    }

    #[test]
    fn a_path_resolves_to_the_deepest_containing_mount() {
        // Whatever the machine looks like, the root always contains everything
        // and the answer must be a prefix of the path we asked about.
        let resolved = volume_root_for(Path::new("/tmp")).expect("root always matches");
        assert!(
            resolved == "/" || under_prefix("/tmp", &resolved),
            "unexpected mount {resolved} for /tmp"
        );
    }

    #[test]
    fn copying_recreates_links_instead_of_following_them() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("huge.bin"), vec![1u8; 4096]).unwrap();
        std::os::unix::fs::symlink(&outside, source.join("link")).unwrap();
        std::fs::write(source.join("real.txt"), b"real").unwrap();

        let destination = temp.path().join("destination");
        let outcome = copy_tree(
            &source,
            &destination,
            Arc::new(AtomicBool::new(false)),
            &mut |_, _| {},
        )
        .unwrap();

        // Only the real file is copied; the link is reproduced as a link, so the
        // 4 KiB behind it is not duplicated.
        assert_eq!(outcome.files, 1);
        assert_eq!(outcome.bytes, 4);
        assert!(std::fs::symlink_metadata(destination.join("link"))
            .unwrap()
            .file_type()
            .is_symlink());
    }
}
