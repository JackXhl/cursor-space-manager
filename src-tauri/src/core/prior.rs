//! Discovery of migrations that happened before this tool existed.
//!
//! The original path of a Cursor data directory is the source of truth for
//! "is this already a link". That is not enough: a manual copy to `D:\cache`
//! (or any similar layout) can leave a complete tree that is not yet linked,
//! or extra folders this tool's candidate list never knew about. This module
//! finds those locations by walking link targets and a short list of
//! well-known directories, then matching them back to known roots.

use crate::core::model::*;
use crate::core::scanner::measure_directory;
use crate::platforms;
use crate::util::{is_inside, paths_equal};

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// Walks up `original` and `target` while their trailing names match, and
/// returns the remaining prefix of `target`. That prefix is the mirror root:
/// `...\Cursor\User\workspaceStorage` pointing at
/// `D:\cache\Cursor-Roaming\User\workspaceStorage` yields
/// `D:\cache\Cursor-Roaming`.
pub fn mirror_root_from_link(original: &Path, target: &Path) -> Option<PathBuf> {
    let mut original = original.to_path_buf();
    let mut target = target.to_path_buf();
    let mut matched = 0usize;
    while original.file_name().is_some() && target.file_name().is_some() {
        let orig_name = original.file_name().unwrap().to_string_lossy().into_owned();
        let tgt_name = target.file_name().unwrap().to_string_lossy().into_owned();
        if !names_equal(&orig_name, &tgt_name) {
            break;
        }
        matched += 1;
        original.pop();
        target.pop();
    }
    if matched == 0 {
        None
    } else {
        Some(target)
    }
}

fn strip_prefix_compat(path: &Path, base: &Path) -> Option<PathBuf> {
    if !cfg!(windows) {
        return path
            .strip_prefix(base)
            .ok()
            .filter(|relative| !relative.as_os_str().is_empty())
            .map(|relative| relative.to_path_buf());
    }
    let path_lower = path.to_string_lossy().to_lowercase();
    let base_lower = base.to_string_lossy().to_lowercase();
    let prefix = if base_lower.ends_with('\\') || base_lower.ends_with('/') {
        base_lower
    } else {
        format!("{base_lower}\\")
    };
    path_lower
        .strip_prefix(&prefix)
        .filter(|relative| !relative.is_empty())
        .map(PathBuf::from)
}

fn names_equal(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

fn looks_like_roaming_mirror(path: &Path) -> bool {
    path.join("Cache").is_dir()
        || path.join("GPUCache").is_dir()
        || path.join("User").join("workspaceStorage").is_dir()
        || path.join("logs").is_dir()
        || path.join("CachedData").is_dir()
        || path.join("snapshots").is_dir()
}

fn looks_like_dot_mirror(path: &Path) -> bool {
    path.join("extensions").is_dir()
        || path.join("ai-tracking").is_dir()
        || path.join("argv.json").is_file()
        || path.join("worktrees").is_dir()
}

fn looks_like_tool_default(path: &Path) -> bool {
    path.file_name()
        .map(|name| names_equal(&name.to_string_lossy(), "CursorData"))
        .unwrap_or(false)
}

fn classify_site(path: &Path) -> Option<PriorSiteKind> {
    let roaming = looks_like_roaming_mirror(path);
    let dot = looks_like_dot_mirror(path);
    let tool = looks_like_tool_default(path);
    match (roaming, dot, tool) {
        (true, true, _) => Some(PriorSiteKind::Mixed),
        (true, false, _) => Some(PriorSiteKind::RoamingMirror),
        (false, true, _) => Some(PriorSiteKind::DotCursor),
        (false, false, true) => Some(PriorSiteKind::ToolDefault),
        _ if path.join("finish-cursor-junction.bat").is_file() => Some(PriorSiteKind::Mixed),
        _ => None,
    }
}

fn site_kind_label(kind: PriorSiteKind) -> &'static str {
    match kind {
        PriorSiteKind::RoamingMirror => "Cursor 用户数据副本",
        PriorSiteKind::DotCursor => ".cursor 目录副本",
        PriorSiteKind::ToolDefault => "本工具默认目标",
        PriorSiteKind::Mixed => "混合的 Cursor 数据副本",
    }
}

/// Unique, existing directories that might hold a previous migration.
pub fn candidate_site_paths(roots: &[DataRoot]) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();

    for root in roots {
        if root.id == "install-dir" {
            continue;
        }
        if let Some(target) = root.link_state.target() {
            if let Some(mirror) = mirror_root_from_link(&root.path, target) {
                push_unique(&mut found, mirror);
            }
            if let Some(parent) = target.parent() {
                push_unique(&mut found, parent.to_path_buf());
            }
        }
    }

    for search in platforms::well_known_prior_search_roots() {
        if !search.is_dir() {
            continue;
        }
        push_unique(&mut found, search.clone());
        if let Ok(entries) = std::fs::read_dir(&search) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    push_unique(&mut found, path);
                }
            }
        }
    }

    found
}

fn push_unique(found: &mut Vec<PathBuf>, path: PathBuf) {
    if found.iter().any(|existing| paths_equal(existing, &path)) {
        return;
    }
    found.push(path);
}

fn relatives_for_kind(
    kind: PriorSiteKind,
    roots: &[DataRoot],
) -> Vec<(String, String, PathBuf, PathBuf)> {
    // (root_id, label, original_path, relative)
    let mut out = Vec::new();
    for root in roots {
        if root.id == "install-dir" {
            continue;
        }
        let Some(relative) = relative_inside_known_base(&root.path) else {
            continue;
        };
        let roaming = is_roaming_relative(&relative);
        let matches = match kind {
            PriorSiteKind::RoamingMirror => roaming,
            PriorSiteKind::DotCursor => !roaming && relative_looks_like_dot(&relative),
            PriorSiteKind::ToolDefault | PriorSiteKind::Mixed => true,
        };
        if matches {
            out.push((
                root.id.clone(),
                root.label.clone(),
                root.path.clone(),
                relative,
            ));
        }
    }
    out
}

fn is_roaming_relative(relative: &Path) -> bool {
    let first = relative
        .components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .unwrap_or_default();
    !matches!(
        first.to_ascii_lowercase().as_str(),
        "extensions" | "ai-tracking" | "projects" | "worktrees" | "argv.json"
    ) && first != "cursor-updater"
}

fn relative_looks_like_dot(relative: &Path) -> bool {
    let text = relative.to_string_lossy().to_ascii_lowercase();
    text.starts_with("extensions")
        || text.starts_with("ai-tracking")
        || text.starts_with("projects")
        || text.starts_with("worktrees")
}

/// Strips the well-known Cursor base (`%APPDATA%\Cursor`, `~/.cursor`, …)
/// so a copy at another drive can be matched by relative path.
pub fn relative_inside_known_base(path: &Path) -> Option<PathBuf> {
    for base in known_bases() {
        if let Ok(relative) = path.strip_prefix(&base) {
            if !relative.as_os_str().is_empty() {
                return Some(relative.to_path_buf());
            }
        }
        if let Some(relative) = strip_prefix_compat(path, &base) {
            return Some(relative);
        }
    }
    path.file_name().map(PathBuf::from)
}

fn known_bases() -> Vec<PathBuf> {
    platforms::cursor_data_bases()
}

fn classify_entry(source: &Path, prior: &Path) -> PriorDisposition {
    let state = platforms::link_state(source);
    match state.target() {
        Some(target) if paths_equal(target, prior) => PriorDisposition::Linked,
        Some(target) if paths_equal(&target_resolved(target), prior) => PriorDisposition::Linked,
        _ if matches!(state, LinkState::Missing) => PriorDisposition::Leftover,
        _ => PriorDisposition::OrphanCopy,
    }
}

fn target_resolved(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn extra_children(site: &Path, known_relatives: &[PathBuf]) -> Vec<PathBuf> {
    let mut extras = Vec::new();
    let Ok(entries) = std::fs::read_dir(site) else {
        return extras;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name() else {
            continue;
        };
        let name = name.to_string_lossy();
        if name.starts_with('.') || name.eq_ignore_ascii_case("finish-cursor-junction.bat") {
            continue;
        }
        if !path.is_dir() {
            continue;
        }
        let covered = known_relatives.iter().any(|relative| {
            relative
                .components()
                .next()
                .map(|c| names_equal(&c.as_os_str().to_string_lossy(), &name))
                .unwrap_or(false)
        });
        if !covered {
            extras.push(path);
        }
    }
    extras
}

/// Inspects candidate directories and returns the ones that actually look
/// like a previous Cursor migration, with each known folder classified.
pub fn discover(roots: &[DataRoot], cancel: &AtomicBool) -> Vec<PriorSite> {
    let mut sites = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for path in candidate_site_paths(roots) {
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
        let Some(kind) = classify_site(&path) else {
            continue;
        };
        let key = if cfg!(windows) {
            path.to_string_lossy().to_lowercase()
        } else {
            path.to_string_lossy().to_string()
        };
        if !seen.insert(key) {
            continue;
        }

        let relatives = relatives_for_kind(kind, roots);
        let known_rel_paths: Vec<PathBuf> = relatives.iter().map(|r| r.3.clone()).collect();
        let mut entries = Vec::new();
        let mut linked_count = 0u32;
        let mut orphan_count = 0u32;
        let mut leftover_count = 0u32;
        let mut extra_count = 0u32;
        let mut total_bytes = 0u64;
        let mut evidence = Vec::new();

        for (root_id, label, source, relative) in &relatives {
            let prior_path = path.join(relative);
            if !prior_path.exists() {
                continue;
            }
            let disposition = classify_entry(source, &prior_path);
            match disposition {
                PriorDisposition::Linked => linked_count += 1,
                PriorDisposition::OrphanCopy => orphan_count += 1,
                PriorDisposition::Leftover => leftover_count += 1,
            }
            let size = roots
                .iter()
                .find(|r| r.id == *root_id)
                .and_then(|r| r.size)
                .or_else(|| Some(measure_directory(&prior_path, cancel)));
            if let Some(size) = &size {
                total_bytes = total_bytes.saturating_add(size.logical_bytes);
            }
            entries.push(PriorEntry {
                path: prior_path,
                label: label.clone(),
                root_id: Some(root_id.clone()),
                disposition,
                size,
                source_path: Some(source.clone()),
            });
        }

        for extra in extra_children(&path, &known_rel_paths) {
            extra_count += 1;
            let size = measure_directory(&extra, cancel);
            total_bytes = total_bytes.saturating_add(size.logical_bytes);
            entries.push(PriorEntry {
                label: extra
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| extra.display().to_string()),
                path: extra,
                root_id: None,
                disposition: PriorDisposition::Leftover,
                size: Some(size),
                source_path: None,
            });
        }

        if entries.is_empty() && !path.join("finish-cursor-junction.bat").is_file() {
            continue;
        }

        if linked_count > 0 {
            evidence.push(format!("{linked_count} 个目录已经通过链接指向这里"));
        }
        if orphan_count > 0 {
            evidence.push(format!(
                "{orphan_count} 个目录有副本但原位置还不是链接，迁移未完成"
            ));
        }
        if leftover_count > 0 {
            evidence.push(format!(
                "{leftover_count} 个目录只在这里找到，原位置已不存在"
            ));
        }
        if path.join("finish-cursor-junction.bat").is_file() {
            evidence.push("发现手工迁移脚本 finish-cursor-junction.bat".into());
        } else if path
            .parent()
            .map(|parent| parent.join("finish-cursor-junction.bat").is_file())
            .unwrap_or(false)
        {
            evidence.push("上级目录有手工迁移脚本 finish-cursor-junction.bat".into());
        }
        if evidence.is_empty() {
            evidence.push(format!("目录布局符合{}", site_kind_label(kind)));
        }

        sites.push(PriorSite {
            path: path.clone(),
            kind,
            volume: platforms::volume_root_for(&path),
            linked_count,
            orphan_count,
            leftover_count,
            extra_count,
            total_bytes,
            entries,
            evidence,
        });
    }

    // Nested sites: D:\cache\Cursor-Roaming inside D:\cache. Keep the more
    // specific ones and drop the container so we do not measure the same tree
    // twice. The shared parent is recovered later as the suggested destination.
    let nested_parents: Vec<PathBuf> = sites
        .iter()
        .filter(|site| {
            sites.iter().any(|other| {
                !paths_equal(&other.path, &site.path) && is_inside(&other.path, &site.path)
            })
        })
        .map(|site| site.path.clone())
        .collect();
    sites.retain(|site| {
        !nested_parents
            .iter()
            .any(|parent| paths_equal(parent, &site.path))
    });

    sites.sort_by_key(|site| std::cmp::Reverse(site.total_bytes));
    sites
}

/// Picks a destination that continues an existing migration rather than
/// starting a new tree at `CursorData`.
pub fn preferred_directory(sites: &[PriorSite]) -> Option<PathBuf> {
    if sites.is_empty() {
        return None;
    }
    if sites.len() == 1 {
        return Some(container_for(&sites[0]));
    }

    let mut parent = sites[0]
        .path
        .parent()
        .unwrap_or(&sites[0].path)
        .to_path_buf();
    for site in &sites[1..] {
        while !is_inside(&site.path, &parent) && parent.pop() {}
        if parent.as_os_str().is_empty() || looks_like_volume_root(&parent) {
            return Some(container_for(&sites[0]));
        }
    }
    if looks_like_volume_root(&parent) {
        return Some(container_for(&sites[0]));
    }
    Some(parent)
}

fn container_for(site: &PriorSite) -> PathBuf {
    if matches!(site.kind, PriorSiteKind::ToolDefault) {
        return site.path.clone();
    }
    let Some(parent) = site.path.parent() else {
        return site.path.clone();
    };
    if looks_like_volume_root(parent) {
        site.path.clone()
    } else {
        parent.to_path_buf()
    }
}

fn looks_like_volume_root(path: &Path) -> bool {
    let text = path.to_string_lossy();
    let trimmed = text.trim_end_matches(['\\', '/']);
    path.parent().is_none()
        || trimmed == "/"
        || (trimmed.len() == 2 && trimmed.as_bytes().get(1) == Some(&b':'))
}

/// Annotates each scanned root with the matching prior copy, if any.
pub fn attach_to_roots(roots: &mut [DataRoot], sites: &[PriorSite]) {
    for root in roots.iter_mut() {
        if root.prior_copy.is_some() {
            continue;
        }
        if let Some(target) = root.link_state.target() {
            root.prior_copy = Some(target.clone());
            continue;
        }
        for site in sites {
            if let Some(entry) = site
                .entries
                .iter()
                .find(|entry| entry.root_id.as_deref() == Some(root.id.as_str()))
            {
                root.prior_copy = Some(entry.path.clone());
                if !root.link_state.is_link()
                    && matches!(entry.disposition, PriorDisposition::OrphanCopy)
                {
                    root.reason =
                        format!("{} 已有先前副本，但原目录尚未联接", entry.path.display());
                }
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_root_strips_the_shared_suffix() {
        let original = PathBuf::from(r"C:\Users\a\AppData\Roaming\Cursor\User\workspaceStorage");
        let target = PathBuf::from(r"D:\cache\Cursor-Roaming\User\workspaceStorage");
        let mirror = mirror_root_from_link(&original, &target).expect("suffix matches");
        assert_eq!(mirror, PathBuf::from(r"D:\cache\Cursor-Roaming"));
    }

    #[test]
    fn a_single_matching_name_still_yields_the_parent() {
        let original = PathBuf::from(r"C:\Users\a\AppData\Roaming\Cursor\Cache");
        let target = PathBuf::from(r"D:\cache\Cursor-Roaming\Cache");
        assert_eq!(
            mirror_root_from_link(&original, &target),
            Some(PathBuf::from(r"D:\cache\Cursor-Roaming"))
        );
    }

    #[test]
    fn unrelated_names_are_not_a_mirror() {
        let original = PathBuf::from(r"C:\Users\a\AppData\Roaming\Cursor\Cache");
        let target = PathBuf::from(r"D:\other\completely-different");
        assert!(mirror_root_from_link(&original, &target).is_none());
    }

    #[test]
    fn preferred_directory_is_the_shared_parent() {
        let sites = vec![
            PriorSite {
                path: PathBuf::from(r"D:\cache\Cursor-Roaming"),
                kind: PriorSiteKind::RoamingMirror,
                volume: Some("D:\\".into()),
                linked_count: 1,
                orphan_count: 0,
                leftover_count: 0,
                extra_count: 0,
                total_bytes: 10,
                entries: Vec::new(),
                evidence: Vec::new(),
            },
            PriorSite {
                path: PathBuf::from(r"D:\cache\dot-cursor"),
                kind: PriorSiteKind::DotCursor,
                volume: Some("D:\\".into()),
                linked_count: 1,
                orphan_count: 0,
                leftover_count: 0,
                extra_count: 0,
                total_bytes: 5,
                entries: Vec::new(),
                evidence: Vec::new(),
            },
        ];
        assert_eq!(
            preferred_directory(&sites),
            Some(PathBuf::from(r"D:\cache"))
        );
    }

    #[test]
    fn a_tool_default_site_is_not_collapsed_to_the_drive_root() {
        let sites = vec![PriorSite {
            path: PathBuf::from(r"D:\CursorData"),
            kind: PriorSiteKind::ToolDefault,
            volume: Some("D:\\".into()),
            linked_count: 1,
            orphan_count: 0,
            leftover_count: 0,
            extra_count: 0,
            total_bytes: 10,
            entries: Vec::new(),
            evidence: Vec::new(),
        }];
        assert_eq!(
            preferred_directory(&sites),
            Some(PathBuf::from(r"D:\CursorData"))
        );
    }

    #[test]
    fn roaming_and_dot_mirrors_are_recognised() {
        let temp = tempfile::tempdir().unwrap();
        let roaming = temp.path().join("Cursor-Roaming");
        std::fs::create_dir_all(roaming.join("Cache")).unwrap();
        let dot = temp.path().join("dot-cursor");
        std::fs::create_dir_all(dot.join("extensions")).unwrap();
        assert_eq!(classify_site(&roaming), Some(PriorSiteKind::RoamingMirror));
        assert_eq!(classify_site(&dot), Some(PriorSiteKind::DotCursor));
        assert!(classify_site(temp.path()).is_none());
    }

    fn stub_root(id: &str, label: &str, path: PathBuf) -> DataRoot {
        let state = platforms::link_state(&path);
        let resolved = state.target().cloned().unwrap_or_else(|| path.clone());
        DataRoot {
            id: id.into(),
            label: label.into(),
            purpose: String::new(),
            resolved_path: resolved,
            path,
            link_state: state,
            exists: true,
            recommendation: Recommendation::Recommended,
            reason: String::new(),
            size: None,
            volume: None,
            scan_errors: Vec::new(),
            from_launch_argument: false,
            prior_copy: None,
        }
    }

    #[test]
    fn discover_follows_a_link_to_the_manual_mirror() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("Cursor").join("Cache");
        let mirror_root = temp.path().join("cache").join("Cursor-Roaming");
        let mirror = mirror_root.join("Cache");
        std::fs::create_dir_all(&mirror).unwrap();
        std::fs::write(mirror.join("a.bin"), b"copy").unwrap();
        std::fs::create_dir_all(original.parent().unwrap()).unwrap();
        platforms::create_directory_link(&original, &mirror).unwrap();

        let root = stub_root("roaming-cache", "网络与渲染缓存", original);
        let sites = discover(&[root], &AtomicBool::new(false));
        assert!(
            sites
                .iter()
                .any(|site| paths_equal(&site.path, &mirror_root)),
            "expected {:?}, got {:?}",
            mirror_root,
            sites.iter().map(|s| s.path.clone()).collect::<Vec<_>>()
        );
        assert!(sites.iter().any(|site| site.linked_count >= 1));
    }

    #[cfg(windows)]
    #[test]
    fn live_d_cache_layout_is_found() {
        let roaming = PathBuf::from(r"D:\cache\Cursor-Roaming");
        let dot = PathBuf::from(r"D:\cache\dot-cursor");
        if !roaming.join("Cache").is_dir() {
            return;
        }
        let Some(appdata) = std::env::var_os("APPDATA") else {
            return;
        };
        let Some(profile) = std::env::var_os("USERPROFILE") else {
            return;
        };
        let roots = vec![
            stub_root(
                "roaming-cache",
                "网络与渲染缓存",
                PathBuf::from(appdata).join("Cursor").join("Cache"),
            ),
            stub_root(
                "dot-extensions",
                "已安装扩展",
                PathBuf::from(profile).join(".cursor").join("extensions"),
            ),
        ];
        let sites = discover(&roots, &AtomicBool::new(false));
        assert!(
            sites.iter().any(|site| paths_equal(&site.path, &roaming)),
            "expected D:\\cache\\Cursor-Roaming, got {:?}",
            sites.iter().map(|s| s.path.clone()).collect::<Vec<_>>()
        );
        if dot.join("extensions").is_dir() {
            assert!(
                sites.iter().any(|site| paths_equal(&site.path, &dot)),
                "expected D:\\cache\\dot-cursor, got {:?}",
                sites.iter().map(|s| s.path.clone()).collect::<Vec<_>>()
            );
        }
    }
}
