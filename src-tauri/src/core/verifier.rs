//! Independent verification of a copied tree.
//!
//! A copy tool's exit code says the tool thought it succeeded. That is not the
//! same as the data being intact, so before anything is renamed we build a full
//! manifest of both sides and compare them entry by entry.

use crate::error::{AppError, AppResult};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    pub kind: EntryKind,
    pub size: u64,
    /// Hex SHA-256 for regular files; the link target for symlinks.
    pub digest: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    /// Relative path (forward-slash normalised) to entry.
    pub entries: BTreeMap<String, ManifestEntry>,
    pub total_bytes: u64,
    pub file_count: u64,
    pub dir_count: u64,
    pub unreadable: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationReport {
    pub ok: bool,
    pub checked_files: u64,
    pub checked_bytes: u64,
    pub missing: Vec<String>,
    pub extra: Vec<String>,
    pub mismatched: Vec<String>,
    pub unreadable: Vec<String>,
    /// SQLite files that failed an integrity check after the copy.
    pub corrupt_databases: Vec<String>,
    pub duration_ms: u64,
}

/// Comparisons must not be defeated by `\` vs `/` or by Windows case rules.
fn relative_key(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let text = relative.to_string_lossy().replace('\\', "/");
    if text.is_empty() {
        return None;
    }
    Some(if cfg!(windows) {
        text.to_lowercase()
    } else {
        text
    })
}

pub fn hash_file(path: &Path) -> AppResult<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 256];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Builds a manifest of `root`.
///
/// `hash_contents` is false for a fast structural pass and true for the real
/// pre-cutover check.
pub fn build_manifest(
    root: &Path,
    hash_contents: bool,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> AppResult<Manifest> {
    let mut manifest = Manifest::default();

    for entry in walkdir::WalkDir::new(root).follow_links(false).min_depth(1) {
        if cancel.load(Ordering::Relaxed) {
            return Err(AppError::Cancelled);
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                manifest.unreadable.push(
                    error
                        .path()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                );
                continue;
            }
        };
        let Some(key) = relative_key(root, entry.path()) else {
            continue;
        };
        let file_type = entry.file_type();

        if file_type.is_dir() {
            manifest.dir_count += 1;
            manifest.entries.insert(
                key,
                ManifestEntry {
                    kind: EntryKind::Directory,
                    size: 0,
                    digest: None,
                },
            );
            continue;
        }

        if file_type.is_symlink() {
            let target = std::fs::read_link(entry.path())
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();
            manifest.entries.insert(
                key,
                ManifestEntry {
                    kind: EntryKind::Symlink,
                    size: 0,
                    digest: Some(target),
                },
            );
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => {
                manifest.unreadable.push(key);
                continue;
            }
        };
        let size = metadata.len();
        let digest = if hash_contents {
            match hash_file(entry.path()) {
                Ok(digest) => Some(digest),
                Err(_) => {
                    manifest.unreadable.push(key.clone());
                    None
                }
            }
        } else {
            None
        };

        manifest.total_bytes += size;
        manifest.file_count += 1;
        manifest.entries.insert(
            key,
            ManifestEntry {
                kind: EntryKind::File,
                size,
                digest,
            },
        );

        if manifest.file_count % 64 == 0 {
            progress(manifest.total_bytes, manifest.file_count);
        }
    }

    progress(manifest.total_bytes, manifest.file_count);
    Ok(manifest)
}

/// SQLite header magic. Checking it avoids trying to open unrelated `.db` files.
const SQLITE_MAGIC: &[u8; 16] = b"SQLite format 3\0";

fn looks_like_sqlite(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut header = [0u8; 16];
    file.read_exact(&mut header).is_ok() && &header == SQLITE_MAGIC
}

/// Runs `PRAGMA quick_check` against copied databases.
///
/// A byte-identical copy is already proof of integrity, but Cursor's state
/// lives in SQLite and a corrupt database is the failure users would notice
/// last and hate most, so we confirm the copies open cleanly.
pub fn check_databases(root: &Path, cancel: &AtomicBool) -> Vec<String> {
    let mut corrupt = Vec::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false).min_depth(1) {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_file() {
            continue;
        }
        let extension = entry
            .path()
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !matches!(extension.as_str(), "db" | "sqlite" | "sqlite3" | "vscdb") {
            continue;
        }
        if !looks_like_sqlite(entry.path()) {
            continue;
        }

        let uri = format!(
            "file:{}?mode=ro&immutable=1",
            entry
                .path()
                .to_string_lossy()
                .replace('?', "%3f")
                .replace('#', "%23")
        );
        let outcome = rusqlite::Connection::open_with_flags(
            &uri,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
        )
        .and_then(|connection| {
            connection.query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
        });

        match outcome {
            Ok(result) if result.eq_ignore_ascii_case("ok") => {}
            Ok(result) => corrupt.push(format!("{}: {result}", entry.path().display())),
            Err(error) => corrupt.push(format!("{}: {error}", entry.path().display())),
        }
    }
    corrupt
}

pub fn compare(source: &Manifest, target: &Manifest) -> VerificationReport {
    let mut report = VerificationReport::default();

    for (key, expected) in &source.entries {
        match target.entries.get(key) {
            None => report.missing.push(key.clone()),
            Some(actual) => {
                if actual.kind != expected.kind {
                    report.mismatched.push(format!("{key} (类型不同)"));
                } else if expected.kind == EntryKind::File {
                    if actual.size != expected.size {
                        report.mismatched.push(format!("{key} (大小不同)"));
                    } else if expected.digest.is_some() && actual.digest != expected.digest {
                        report.mismatched.push(format!("{key} (内容不同)"));
                    }
                } else if expected.kind == EntryKind::Symlink && actual.digest != expected.digest {
                    report.mismatched.push(format!("{key} (链接目标不同)"));
                }
            }
        }
    }

    for key in target.entries.keys() {
        if !source.entries.contains_key(key) {
            report.extra.push(key.clone());
        }
    }

    report.checked_files = source.file_count;
    report.checked_bytes = source.total_bytes;
    report.unreadable = source
        .unreadable
        .iter()
        .chain(target.unreadable.iter())
        .cloned()
        .collect();

    // Extra files in the destination are reported but are not a failure: a
    // partially resumed copy can legitimately leave them, and they do not put
    // source data at risk.
    report.ok =
        report.missing.is_empty() && report.mismatched.is_empty() && report.unreadable.is_empty();
    report
}

/// Truncates verification lists before they are persisted or shown.
///
/// A mismatch on 50,000 files is not more actionable than a mismatch on 50, and
/// storing every path would bloat the journal.
pub fn truncate_lists(report: &mut VerificationReport, limit: usize) {
    for list in [
        &mut report.missing,
        &mut report.extra,
        &mut report.mismatched,
        &mut report.unreadable,
        &mut report.corrupt_databases,
    ] {
        if list.len() > limit {
            let hidden = list.len() - limit;
            list.truncate(limit);
            list.push(format!("... 另有 {hidden} 项未显示"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_of(root: &Path) -> Manifest {
        let cancel = AtomicBool::new(false);
        build_manifest(root, true, &cancel, &mut |_, _| {}).unwrap()
    }

    #[test]
    fn identical_trees_verify() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        for base in [source.path(), target.path()] {
            std::fs::create_dir_all(base.join("nested")).unwrap();
            std::fs::write(base.join("a.txt"), b"hello").unwrap();
            std::fs::write(base.join("nested/b.bin"), vec![7u8; 4096]).unwrap();
        }

        let report = compare(&manifest_of(source.path()), &manifest_of(target.path()));
        assert!(report.ok, "{report:?}");
        assert_eq!(report.checked_files, 2);
    }

    #[test]
    fn a_missing_file_fails_verification() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("a.txt"), b"hello").unwrap();
        std::fs::write(source.path().join("b.txt"), b"world").unwrap();
        std::fs::write(target.path().join("a.txt"), b"hello").unwrap();

        let report = compare(&manifest_of(source.path()), &manifest_of(target.path()));
        assert!(!report.ok);
        assert_eq!(report.missing, vec!["b.txt".to_string()]);
    }

    #[test]
    fn same_size_different_content_is_caught_by_hashing() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("a.txt"), b"aaaaa").unwrap();
        std::fs::write(target.path().join("a.txt"), b"bbbbb").unwrap();

        let report = compare(&manifest_of(source.path()), &manifest_of(target.path()));
        assert!(!report.ok);
        assert_eq!(report.mismatched.len(), 1);
        assert!(report.mismatched[0].contains("内容不同"));
    }

    #[test]
    fn extra_destination_files_are_reported_but_not_fatal() {
        let source = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        std::fs::write(source.path().join("a.txt"), b"hello").unwrap();
        std::fs::write(target.path().join("a.txt"), b"hello").unwrap();
        std::fs::write(target.path().join("leftover.tmp"), b"x").unwrap();

        let report = compare(&manifest_of(source.path()), &manifest_of(target.path()));
        assert!(report.ok);
        assert_eq!(report.extra, vec!["leftover.tmp".to_string()]);
    }

    #[test]
    fn a_healthy_sqlite_copy_passes_quick_check() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.vscdb");
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute(
                "CREATE TABLE items (id INTEGER PRIMARY KEY, value TEXT)",
                [],
            )
            .unwrap();
        connection
            .execute("INSERT INTO items (value) VALUES ('x')", [])
            .unwrap();
        drop(connection);

        let cancel = AtomicBool::new(false);
        assert!(check_databases(dir.path(), &cancel).is_empty());
    }

    #[test]
    fn truncation_keeps_the_list_bounded_and_says_so() {
        let mut report = VerificationReport {
            missing: (0..30).map(|i| format!("file-{i}")).collect(),
            ..Default::default()
        };
        truncate_lists(&mut report, 10);
        assert_eq!(report.missing.len(), 11);
        assert!(report.missing.last().unwrap().contains("另有 20 项"));
    }
}
