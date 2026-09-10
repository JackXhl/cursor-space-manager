//! Windows implementation: volumes, NTFS reparse points, Cursor discovery,
//! Restart Manager lock detection, and robocopy-driven tree copying.

use crate::core::model::{
    AppInstallation, DriveKind, InstallKind, LinkState, Recommendation, RunningProcess, VolumeInfo,
};
use crate::error::{AppError, AppResult};
use crate::platforms::{CandidateRoot, CopyOutcome};

use std::ffi::{c_void, OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetCompressedFileSizeW, GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives,
    GetVolumeInformationW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::RestartManager::{
    RmEndSession, RmGetList, RmRegisterResources, RmStartSession, RM_PROCESS_INFO,
};
use windows_sys::Win32::System::IO::DeviceIoControl;

const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;

const FSCTL_GET_REPARSE_POINT: u32 = 0x0009_00A8;
const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00A4;
const FSCTL_DELETE_REPARSE_POINT: u32 = 0x0009_00AC;

const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;
const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;

const MAXIMUM_REPARSE_DATA_BUFFER_SIZE: usize = 16 * 1024;

const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;
const DRIVE_REMOTE: u32 = 4;
const DRIVE_CDROM: u32 = 5;
const DRIVE_RAMDISK: u32 = 6;

/// Restart Manager caps session keys at `CCH_RM_SESSION_KEY` wide chars.
const CCH_RM_SESSION_KEY: usize = 32;
/// Registering every file in a multi-gigabyte cache would take longer than the
/// copy itself, so we sample the files most likely to be held open.
const MAX_RM_FILES: usize = 512;

pub fn platform_name() -> &'static str {
    "windows"
}

fn to_wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(std::iter::once(0)).collect()
}

fn wide_to_string(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    OsString::from_wide(&buffer[..end])
        .to_string_lossy()
        .into_owned()
}

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

// ---------------------------------------------------------------------------
// Volumes
// ---------------------------------------------------------------------------

pub fn list_volumes() -> Vec<VolumeInfo> {
    let system_root = env_path("SystemDrive")
        .map(|p| p.to_string_lossy().to_uppercase())
        .unwrap_or_else(|| "C:".to_string());
    let system_letter = system_root.chars().next().unwrap_or('C');

    let mask = unsafe { GetLogicalDrives() };
    let mut volumes = Vec::new();

    for index in 0..26u32 {
        if mask & (1 << index) == 0 {
            continue;
        }
        let letter = (b'A' + index as u8) as char;
        let root = format!("{letter}:\\");
        let wide = to_wide(OsStr::new(&root));

        let drive_type = unsafe { GetDriveTypeW(wide.as_ptr()) };
        let kind = match drive_type {
            DRIVE_FIXED => DriveKind::Fixed,
            DRIVE_REMOVABLE => DriveKind::Removable,
            DRIVE_REMOTE => DriveKind::Network,
            DRIVE_CDROM => DriveKind::CdRom,
            DRIVE_RAMDISK => DriveKind::RamDisk,
            _ => DriveKind::Unknown,
        };

        let mut label_buf = [0u16; 261];
        let mut fs_buf = [0u16; 64];
        let mut serial = 0u32;
        let mut max_component = 0u32;
        let mut flags = 0u32;
        let ok = unsafe {
            GetVolumeInformationW(
                wide.as_ptr(),
                label_buf.as_mut_ptr(),
                label_buf.len() as u32,
                &mut serial,
                &mut max_component,
                &mut flags,
                fs_buf.as_mut_ptr(),
                fs_buf.len() as u32,
            )
        };
        // A CD drive with no disc fails here; the volume is still worth listing
        // so the UI can explain why it is not selectable.
        let (label, filesystem) = if ok != 0 {
            (wide_to_string(&label_buf), wide_to_string(&fs_buf))
        } else {
            (String::new(), String::new())
        };

        let mut free_to_caller = 0u64;
        let mut total = 0u64;
        let mut total_free = 0u64;
        let space_ok = unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut free_to_caller,
                &mut total,
                &mut total_free,
            )
        };
        let (total_bytes, free_bytes) = if space_ok != 0 {
            (total, free_to_caller)
        } else {
            (0, 0)
        };

        let is_system = letter.eq_ignore_ascii_case(&system_letter);
        let (eligible, reason) = target_eligibility(kind, is_system, &filesystem, total_bytes);

        volumes.push(VolumeInfo {
            mount: root,
            label,
            filesystem,
            total_bytes,
            free_bytes,
            kind,
            is_system,
            eligible_target: eligible,
            ineligible_reason: reason,
        });
    }

    volumes
}

fn target_eligibility(
    kind: DriveKind,
    is_system: bool,
    filesystem: &str,
    total_bytes: u64,
) -> (bool, Option<String>) {
    if is_system {
        return (false, Some("系统盘，迁移到这里不会释放空间".into()));
    }
    match kind {
        DriveKind::Removable => {
            return (false, Some("可移动磁盘，拔出后 Cursor 会无法启动".into()))
        }
        DriveKind::Network => return (false, Some("网络位置不稳定，Cursor 可能打不开".into())),
        DriveKind::CdRom => return (false, Some("光驱，不可写入".into())),
        DriveKind::RamDisk => return (false, Some("内存盘，重启后数据会丢失".into())),
        DriveKind::Unknown => return (false, Some("无法识别的磁盘类型".into())),
        DriveKind::Fixed => {}
    }
    if total_bytes == 0 {
        return (false, Some("无法读取容量，可能没有介质".into()));
    }
    if !filesystem.eq_ignore_ascii_case("NTFS") {
        return (
            false,
            Some(format!(
                "这块盘的格式是 {filesystem}，搬家需要用电脑里的普通本地硬盘"
            )),
        );
    }
    (true, None)
}

pub fn volume_root_for(path: &Path) -> Option<String> {
    let text = path.to_string_lossy();
    let bytes = text.as_bytes();
    // UNC paths have no drive letter and can never host a junction target.
    if bytes.len() >= 2 && bytes[1] == b':' {
        let letter = (bytes[0] as char).to_ascii_uppercase();
        if letter.is_ascii_alphabetic() {
            return Some(format!("{letter}:\\"));
        }
    }
    None
}

pub fn same_volume(a: &Path, b: &Path) -> bool {
    match (volume_root_for(a), volume_root_for(b)) {
        (Some(x), Some(y)) => x.eq_ignore_ascii_case(&y),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Reparse points
// ---------------------------------------------------------------------------

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

fn open_reparse_handle(path: &Path, write: bool) -> AppResult<OwnedHandle> {
    let wide = to_wide(path.as_os_str());
    let access = if write {
        GENERIC_READ | GENERIC_WRITE
    } else {
        GENERIC_READ
    };
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(AppError::Io(std::io::Error::last_os_error()));
    }
    Ok(OwnedHandle(handle))
}

/// Strips the NT namespace prefixes so paths can be shown and compared as the
/// user would write them.
fn normalize_substitute_name(raw: &str) -> String {
    let trimmed = raw
        .strip_prefix(r"\??\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .or_else(|| raw.strip_prefix(r"\??\").map(|rest| rest.to_string()))
        .or_else(|| raw.strip_prefix(r"\\?\").map(|rest| rest.to_string()))
        .unwrap_or_else(|| raw.to_string());
    trimmed.trim_end_matches('\\').to_string()
}

pub fn link_state(path: &Path) -> LinkState {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return LinkState::Missing,
    };
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    use std::os::windows::fs::MetadataExt;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
        return LinkState::Regular;
    }

    let handle = match open_reparse_handle(path, false) {
        Ok(h) => h,
        Err(_) => return LinkState::UnknownReparse("无法读取重解析点".into()),
    };

    let mut buffer = vec![0u8; MAXIMUM_REPARSE_DATA_BUFFER_SIZE];
    let mut returned = 0u32;
    let ok = unsafe {
        DeviceIoControl(
            handle.0,
            FSCTL_GET_REPARSE_POINT,
            std::ptr::null(),
            0,
            buffer.as_mut_ptr() as *mut c_void,
            buffer.len() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 || (returned as usize) < 8 {
        return LinkState::UnknownReparse("重解析点数据不可读".into());
    }

    let tag = u32::from_le_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
    let read_u16 =
        |offset: usize| u16::from_le_bytes([buffer[offset], buffer[offset + 1]]) as usize;

    // Header is 8 bytes; the per-tag path fields follow immediately.
    let (name_offset, subst_offset, subst_len) = match tag {
        IO_REPARSE_TAG_MOUNT_POINT => (16usize, read_u16(8), read_u16(10)),
        IO_REPARSE_TAG_SYMLINK => (20usize, read_u16(8), read_u16(10)),
        other => return LinkState::UnknownReparse(format!("0x{other:08X}")),
    };

    let start = name_offset + subst_offset;
    let end = start + subst_len;
    if end > returned as usize || subst_len % 2 != 0 {
        return LinkState::UnknownReparse("重解析点路径长度异常".into());
    }
    let wide: Vec<u16> = buffer[start..end]
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let target = PathBuf::from(normalize_substitute_name(&String::from_utf16_lossy(&wide)));

    if tag == IO_REPARSE_TAG_MOUNT_POINT {
        LinkState::Junction(target)
    } else {
        LinkState::Symlink(target)
    }
}

/// Creates an NTFS junction at `link` pointing to `target`.
///
/// A junction is used rather than a symlink because it needs no developer mode
/// or elevation, and Cursor resolves it transparently.
pub fn create_directory_link(link: &Path, target: &Path) -> AppResult<()> {
    if !target.is_dir() {
        return Err(AppError::invalid(format!(
            "链接目标不存在: {}",
            target.display()
        )));
    }
    if link.exists() || std::fs::symlink_metadata(link).is_ok() {
        return Err(AppError::invalid(format!(
            "链接位置已被占用: {}",
            link.display()
        )));
    }

    let absolute = std::fs::canonicalize(target)?;
    let absolute_text = absolute.to_string_lossy().to_string();
    let clean = absolute_text
        .strip_prefix(r"\\?\")
        .unwrap_or(&absolute_text)
        .trim_end_matches('\\')
        .to_string();

    let substitute: Vec<u16> = format!(r"\??\{clean}").encode_utf16().collect();
    let print: Vec<u16> = clean.encode_utf16().collect();

    let subst_bytes = substitute.len() * 2;
    let print_bytes = print.len() * 2;
    // Both names are stored NUL terminated inside a single path buffer.
    let path_buffer_bytes = subst_bytes + 2 + print_bytes + 2;
    let reparse_data_length = 8 + path_buffer_bytes;
    let total = 8 + reparse_data_length;

    let mut buffer = vec![0u8; total];
    buffer[0..4].copy_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
    buffer[4..6].copy_from_slice(&(reparse_data_length as u16).to_le_bytes());
    buffer[6..8].copy_from_slice(&0u16.to_le_bytes());
    buffer[8..10].copy_from_slice(&0u16.to_le_bytes());
    buffer[10..12].copy_from_slice(&(subst_bytes as u16).to_le_bytes());
    buffer[12..14].copy_from_slice(&((subst_bytes + 2) as u16).to_le_bytes());
    buffer[14..16].copy_from_slice(&(print_bytes as u16).to_le_bytes());

    let mut cursor = 16usize;
    for unit in &substitute {
        buffer[cursor..cursor + 2].copy_from_slice(&unit.to_le_bytes());
        cursor += 2;
    }
    cursor += 2; // NUL
    for unit in &print {
        buffer[cursor..cursor + 2].copy_from_slice(&unit.to_le_bytes());
        cursor += 2;
    }

    std::fs::create_dir(link)?;

    let result = (|| -> AppResult<()> {
        let handle = open_reparse_handle(link, true)?;
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                handle.0,
                FSCTL_SET_REPARSE_POINT,
                buffer.as_ptr() as *const c_void,
                buffer.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(AppError::Io(std::io::Error::last_os_error()));
        }
        Ok(())
    })();

    if result.is_err() {
        // Leaving an empty directory behind would look like a half-migrated
        // state on the next launch, so undo the placeholder.
        let _ = std::fs::remove_dir(link);
    }
    result
}

/// Removes a junction without touching the data it points at.
pub fn remove_directory_link(link: &Path) -> AppResult<()> {
    let state = link_state(link);
    if !state.is_link() {
        return Err(AppError::unsafe_op(format!(
            "{} 不是链接，拒绝按链接方式删除",
            link.display()
        )));
    }

    {
        let handle = open_reparse_handle(link, true)?;
        // FSCTL_DELETE_REPARSE_POINT takes a header-only buffer.
        let mut buffer = [0u8; 8];
        buffer[0..4].copy_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                handle.0,
                FSCTL_DELETE_REPARSE_POINT,
                buffer.as_ptr() as *const c_void,
                buffer.len() as u32,
                std::ptr::null_mut(),
                0,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(AppError::Io(std::io::Error::last_os_error()));
        }
    }

    std::fs::remove_dir(link)?;
    Ok(())
}

fn compressed_file_size(path: &Path, logical: u64) -> u64 {
    let wide = to_wide(path.as_os_str());
    let mut high = 0u32;
    let low = unsafe { GetCompressedFileSizeW(wide.as_ptr(), &mut high) };
    if low == u32::MAX {
        return logical;
    }
    ((high as u64) << 32) | low as u64
}

/// Space a file actually consumes.
///
/// The exact answer needs an extra syscall per file, which is too expensive
/// across a cache tree with hundreds of thousands of entries. For ordinary
/// files, rounding up to the cluster size is exact; only compressed and sparse
/// files need the real call.
pub fn allocated_size(path: &Path, metadata: &std::fs::Metadata) -> u64 {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_SPARSE_FILE: u32 = 0x0000_0200;
    const FILE_ATTRIBUTE_COMPRESSED: u32 = 0x0000_0800;
    const CLUSTER_BYTES: u64 = 4096;

    let logical = metadata.file_size();
    if metadata.file_attributes() & (FILE_ATTRIBUTE_SPARSE_FILE | FILE_ATTRIBUTE_COMPRESSED) != 0 {
        return compressed_file_size(path, logical);
    }
    logical.div_ceil(CLUSTER_BYTES) * CLUSTER_BYTES
}

// ---------------------------------------------------------------------------
// Cursor discovery
// ---------------------------------------------------------------------------

fn read_product_version(install_dir: &Path) -> Option<String> {
    let manifest = install_dir
        .join("resources")
        .join("app")
        .join("package.json");
    let text = std::fs::read_to_string(manifest).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    value.get("version")?.as_str().map(|s| s.to_string())
}

pub fn discover_installation() -> AppInstallation {
    let mut installation = AppInstallation::default();
    let mut candidates: Vec<(InstallKind, PathBuf)> = Vec::new();

    if let Some(local) = env_path("LOCALAPPDATA") {
        candidates.push((InstallKind::PerUser, local.join("Programs").join("cursor")));
    }
    if let Some(pf) = env_path("ProgramFiles") {
        candidates.push((InstallKind::Machine, pf.join("Cursor")));
    }
    if let Some(pf86) = env_path("ProgramFiles(x86)") {
        candidates.push((InstallKind::Machine, pf86.join("Cursor")));
    }

    let processes = running_processes();
    // A running process is the most reliable evidence, including for portable
    // installs that live in an arbitrary folder.
    for process in &processes {
        if let Some(exe) = &process.exe_path {
            if let Some(dir) = exe.parent() {
                if !candidates.iter().any(|(_, p)| paths_equal(p, dir)) {
                    candidates.push((InstallKind::Portable, dir.to_path_buf()));
                }
            }
        }
    }

    let mut found: Vec<(InstallKind, PathBuf)> = Vec::new();
    for (kind, dir) in candidates {
        if dir.join("Cursor.exe").is_file() {
            found.push((kind, dir));
        }
    }

    if found.is_empty() {
        installation.processes = processes;
        installation.running = !installation.processes.is_empty();
        if installation.running {
            installation
                .notes
                .push("检测到 Cursor 进程，但未能定位安装目录".into());
        }
        return installation;
    }

    // Prefer whatever is actually running, then per-user, then the rest.
    found.sort_by_key(|(kind, dir)| {
        let running_here = processes.iter().any(|p| {
            p.exe_path
                .as_ref()
                .and_then(|e| e.parent())
                .map(|parent| paths_equal(parent, dir))
                .unwrap_or(false)
        });
        let kind_rank = match kind {
            InstallKind::PerUser => 1,
            InstallKind::Machine => 2,
            InstallKind::Portable => 3,
            _ => 4,
        };
        (if running_here { 0 } else { 1 }, kind_rank)
    });

    let (kind, dir) = found[0].clone();
    installation.found = true;
    installation.kind = kind;
    installation.exe_path = Some(dir.join("Cursor.exe"));
    installation.version = read_product_version(&dir);
    installation.install_dir = Some(dir);
    installation.conflicts = found.iter().skip(1).map(|(_, p)| p.clone()).collect();
    if !installation.conflicts.is_empty() {
        installation
            .notes
            .push("检测到多个 Cursor 安装位置，请确认要处理的是哪一个".into());
    }
    installation.running = !processes.is_empty();
    installation.processes = processes;
    installation
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

pub fn running_processes() -> Vec<RunningProcess> {
    let system = sysinfo::System::new_all();
    let mut result = Vec::new();
    for (pid, process) in system.processes() {
        let name = process.name().to_string_lossy().to_string();
        if !name.eq_ignore_ascii_case("Cursor.exe") {
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

// ---------------------------------------------------------------------------
// Candidate data roots
// ---------------------------------------------------------------------------

/// Reads `--user-data-dir` / `--extensions-dir` style overrides from the
/// command line of a running Cursor process, so we never migrate a directory
/// the app is no longer using.
fn launch_argument_overrides() -> Vec<(String, PathBuf)> {
    let system = sysinfo::System::new_all();
    let mut overrides = Vec::new();
    for process in system.processes().values() {
        let name = process.name().to_string_lossy().to_string();
        if !name.eq_ignore_ascii_case("Cursor.exe") {
            continue;
        }
        let args: Vec<String> = process
            .cmd()
            .iter()
            .map(|a| a.to_string_lossy().to_string())
            .collect();
        let mut index = 0;
        while index < args.len() {
            let arg = &args[index];
            for flag in ["--user-data-dir", "--extensions-dir"] {
                if let Some(rest) = arg.strip_prefix(&format!("{flag}=")) {
                    overrides.push((flag.to_string(), PathBuf::from(rest)));
                } else if arg == flag && index + 1 < args.len() {
                    overrides.push((flag.to_string(), PathBuf::from(&args[index + 1])));
                }
            }
            index += 1;
        }
    }
    overrides.sort();
    overrides.dedup();
    overrides
}

pub fn candidate_roots() -> Vec<CandidateRoot> {
    let mut roots = Vec::new();
    let appdata = env_path("APPDATA");
    let profile = env_path("USERPROFILE");
    let localappdata = env_path("LOCALAPPDATA");

    if let Some(appdata) = &appdata {
        let base = appdata.join("Cursor");
        roots.push(CandidateRoot::new(
            "roaming-cache",
            "网络与渲染缓存",
            "Chromium 网络请求与渲染缓存",
            base.join("Cache"),
            Recommendation::Recommended,
            "纯缓存数据，体积增长快，迁移风险最低",
        ));
        roots.push(CandidateRoot::new(
            "roaming-gpucache",
            "GPU 着色器缓存",
            "GPU 着色器编译缓存",
            base.join("GPUCache"),
            Recommendation::Recommended,
            "可随时重建的缓存",
        ));
        roots.push(CandidateRoot::new(
            "roaming-code-cache",
            "脚本编译缓存",
            "JavaScript 字节码缓存",
            base.join("Code Cache"),
            Recommendation::Recommended,
            "可随时重建的缓存",
        ));
        roots.push(CandidateRoot::new(
            "roaming-logs",
            "运行日志",
            "主进程与扩展宿主日志",
            base.join("logs"),
            Recommendation::Recommended,
            "只增不减的日志，迁移后不影响功能",
        ));
        roots.push(CandidateRoot::new(
            "roaming-workspace-storage",
            "工作区状态",
            "每个工作区的编辑器状态与索引",
            base.join("User").join("workspaceStorage"),
            Recommendation::Recommended,
            "通常是 Roaming 目录中最大的部分",
        ));
        roots.push(CandidateRoot::new(
            "roaming-global-storage",
            "全局存储",
            "扩展全局状态与聊天记录数据库",
            base.join("User").join("globalStorage"),
            Recommendation::Optional,
            "里面有 Cursor 正在使用的数据，搬家前必须完全退出 Cursor",
        ));
        roots.push(CandidateRoot::new(
            "roaming-service-worker",
            "Service Worker 缓存",
            "内置 Web 视图的离线缓存",
            base.join("Service Worker"),
            Recommendation::Optional,
            "缓存数据，体积中等",
        ));
        roots.push(CandidateRoot::new(
            "roaming-blob-storage",
            "Blob 存储",
            "渲染进程临时二进制数据",
            base.join("blob_storage"),
            Recommendation::Optional,
            "临时数据",
        ));
        roots.push(CandidateRoot::new(
            "roaming-history",
            "本地文件历史",
            "编辑器本地历史记录",
            base.join("User").join("History"),
            Recommendation::Optional,
            "文件数量多，但单个文件很小",
        ));
        roots.push(CandidateRoot::new(
            "roaming-cached-data",
            "扩展宿主编译缓存",
            "扩展宿主的代码缓存",
            base.join("CachedData"),
            Recommendation::Optional,
            "可重建的编译缓存，手工迁移时经常一起搬走",
        ));
        roots.push(CandidateRoot::new(
            "roaming-snapshots",
            "崩溃快照",
            "Chromium 崩溃与会话快照",
            base.join("snapshots"),
            Recommendation::Optional,
            "体积可能很大，不影响日常编辑",
        ));
        roots.push(CandidateRoot::new(
            "roaming-cached-vsix",
            "扩展安装包缓存",
            "已下载的 VSIX 安装包",
            base.join("CachedExtensionVSIXs"),
            Recommendation::Optional,
            "重新安装扩展时会再下载",
        ));
    }

    if let Some(profile) = &profile {
        let dot = profile.join(".cursor");
        roots.push(CandidateRoot::new(
            "dot-extensions",
            "已安装扩展",
            "扩展本体与依赖",
            dot.join("extensions"),
            Recommendation::Recommended,
            "扩展目录通常占用数 GB，迁移后 Cursor 会自动跟随链接",
        ));
        roots.push(CandidateRoot::new(
            "dot-ai-tracking",
            "AI 会话追踪",
            "AI 功能的本地追踪数据",
            dot.join("ai-tracking"),
            Recommendation::Optional,
            "本地分析数据",
        ));
        roots.push(CandidateRoot::new(
            "dot-projects",
            "项目索引",
            "项目级会话与终端快照",
            dot.join("projects"),
            Recommendation::Optional,
            "包含历史会话，迁移前建议退出 Cursor",
        ));
        roots.push(CandidateRoot::new(
            "dot-worktrees",
            "工作树",
            "Cursor 管理的 git worktree",
            dot.join("worktrees"),
            Recommendation::Optional,
            "与项目索引配套，手工迁移时经常一起搬走",
        ));
    }

    if let Some(local) = &localappdata {
        let install = local.join("Programs").join("cursor");
        roots.push(CandidateRoot::new(
            "install-dir",
            "安装目录",
            "Cursor 程序本体",
            install,
            Recommendation::NotRecommended,
            "请留在原处，搬走后可能没法自动更新或正常卸载",
        ));
        roots.push(CandidateRoot::new(
            "local-updater",
            "更新缓存",
            "安装包下载缓存",
            local.join("cursor-updater"),
            Recommendation::Optional,
            "更新时重新下载即可",
        ));
    }

    for (flag, path) in launch_argument_overrides() {
        let id = if flag == "--user-data-dir" {
            "arg-user-data-dir"
        } else {
            "arg-extensions-dir"
        };
        if roots.iter().any(|r| paths_equal(&r.path, &path)) {
            continue;
        }
        roots.push(
            CandidateRoot::new(
                id,
                if flag == "--user-data-dir" {
                    "自定义用户数据目录"
                } else {
                    "自定义扩展目录"
                },
                "由启动参数指定",
                path,
                Recommendation::Optional,
                "该目录由启动参数覆盖，请确认快捷方式也一并检查",
            )
            .from_launch_argument(),
        );
    }

    roots
}

pub fn default_target_directory(volume: &str) -> PathBuf {
    Path::new(volume).join("CursorData")
}

/// Directories that Cursor itself treats as data roots. Used to map a copy
/// on another drive back onto the original relative path.
pub fn cursor_data_bases() -> Vec<PathBuf> {
    let mut bases = Vec::new();
    if let Some(appdata) = env_path("APPDATA") {
        bases.push(appdata.join("Cursor"));
    }
    if let Some(profile) = env_path("USERPROFILE") {
        bases.push(profile.join(".cursor"));
    }
    if let Some(local) = env_path("LOCALAPPDATA") {
        bases.push(local.join("cursor-updater"));
    }
    bases
}

/// Places a previous manual migration commonly lands. Children of these
/// directories are inspected too, so `D:\cache\Cursor-Roaming` is found
/// even when no junction currently points at it.
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
    roots
}

// ---------------------------------------------------------------------------
// Restart Manager
// ---------------------------------------------------------------------------

fn sample_files_for_lock_check(root: &Path) -> Vec<PathBuf> {
    let mut priority = Vec::new();
    let mut regular = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .max_depth(6)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path().to_path_buf();
        let is_database = path
            .extension()
            .map(|e| {
                let ext = e.to_string_lossy().to_lowercase();
                ext == "db" || ext == "vscdb" || ext == "sqlite" || ext == "log" || ext == "lock"
            })
            .unwrap_or(false);
        if is_database {
            priority.push(path);
        } else if regular.len() < MAX_RM_FILES {
            regular.push(path);
        }
        if priority.len() >= MAX_RM_FILES {
            break;
        }
    }
    priority.extend(regular);
    priority.truncate(MAX_RM_FILES);
    priority
}

/// Reports which processes still hold files under `root` open.
///
/// Used to confirm Cursor really released everything; we never kill anything.
pub fn processes_locking(root: &Path) -> AppResult<Vec<RunningProcess>> {
    let files = sample_files_for_lock_check(root);
    if files.is_empty() {
        return Ok(Vec::new());
    }

    let mut session: u32 = 0;
    let mut key = [0u16; CCH_RM_SESSION_KEY + 1];
    let status = unsafe { RmStartSession(&mut session, 0, key.as_mut_ptr()) };
    if status != 0 {
        return Err(AppError::platform(format!(
            "无法启动重启管理器会话 (错误 {status})"
        )));
    }

    struct Session(u32);
    impl Drop for Session {
        fn drop(&mut self) {
            unsafe { RmEndSession(self.0) };
        }
    }
    let _guard = Session(session);

    let wide_files: Vec<Vec<u16>> = files.iter().map(|p| to_wide(p.as_os_str())).collect();
    let pointers: Vec<*const u16> = wide_files.iter().map(|w| w.as_ptr()).collect();

    let status = unsafe {
        RmRegisterResources(
            session,
            pointers.len() as u32,
            pointers.as_ptr(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
        )
    };
    if status != 0 {
        return Err(AppError::platform(format!(
            "无法注册待检测文件 (错误 {status})"
        )));
    }

    let mut needed: u32 = 0;
    let mut count: u32 = 16;
    let mut infos: Vec<RM_PROCESS_INFO> = vec![unsafe { std::mem::zeroed() }; count as usize];
    let mut reasons: u32 = 0;

    let mut status = unsafe {
        RmGetList(
            session,
            &mut needed,
            &mut count,
            infos.as_mut_ptr(),
            &mut reasons,
        )
    };
    // ERROR_MORE_DATA: retry once with the size the API asked for.
    if status == 234 {
        count = needed;
        infos = vec![unsafe { std::mem::zeroed() }; count.max(1) as usize];
        status = unsafe {
            RmGetList(
                session,
                &mut needed,
                &mut count,
                infos.as_mut_ptr(),
                &mut reasons,
            )
        };
    }
    if status != 0 {
        return Err(AppError::platform(format!(
            "无法读取占用进程列表 (错误 {status})"
        )));
    }

    let mut result = Vec::new();
    for info in infos.iter().take(count as usize) {
        result.push(RunningProcess {
            pid: info.Process.dwProcessId,
            name: wide_to_string(&info.strAppName),
            exe_path: None,
        });
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// Copying
// ---------------------------------------------------------------------------

/// Copies a directory tree using robocopy with a fixed argument vector.
///
/// The arguments are deliberately conservative: no `/MOVE`, `/MIR`, or `/PURGE`,
/// so a mistake can never delete the source. `/XJ` keeps us from following
/// junctions into an unrelated tree. Nothing here goes through a shell, so no
/// part of a path can be interpreted as a command.
pub fn copy_tree(
    source: &Path,
    destination: &Path,
    cancel: Arc<AtomicBool>,
    progress: &mut dyn FnMut(u64, u64),
) -> AppResult<CopyOutcome> {
    use std::process::{Command, Stdio};

    std::fs::create_dir_all(destination)?;

    let mut command = Command::new("robocopy");
    command
        .arg(source.as_os_str())
        .arg(destination.as_os_str())
        .arg("/E")
        .arg("/COPY:DAT")
        .arg("/DCOPY:DAT")
        .arg("/XJ")
        .arg("/R:1")
        .arg("/W:1")
        .arg("/MT:8")
        .arg("/NFL")
        .arg("/NDL")
        .arg("/NP")
        .arg("/NJH")
        .arg("/NJS")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command
        .spawn()
        .map_err(|e| AppError::platform(format!("无法启动 robocopy: {e}")))?;

    // robocopy's own progress output is hard to parse reliably, so we sample
    // the destination instead. It costs one stat walk per tick and works the
    // same for one huge file or a million small ones.
    loop {
        match child.try_wait()? {
            Some(status) => {
                let code = status.code().unwrap_or(-1);
                let stats = crate::core::scanner::measure_directory(destination, &cancel);
                progress(stats.logical_bytes, stats.file_count);
                // Anything below 8 means success, possibly with copied/extra files.
                if !(0..8).contains(&code) {
                    return Err(AppError::platform(format!(
                        "robocopy 复制失败，退出码 {code}"
                    )));
                }
                return Ok(CopyOutcome {
                    bytes: stats.logical_bytes,
                    files: stats.file_count,
                    raw_status: code,
                    log: Vec::new(),
                });
            }
            None => {
                if cancel.load(Ordering::Relaxed) {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AppError::Cancelled);
                }
                std::thread::sleep(std::time::Duration::from_millis(700));
                let stats = crate::core::scanner::measure_directory(destination, &cancel);
                progress(stats.logical_bytes, stats.file_count);
            }
        }
    }
}
