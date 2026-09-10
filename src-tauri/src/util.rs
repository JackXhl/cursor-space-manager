use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Timestamp component safe to embed in a directory name on every platform.
pub fn timestamp_slug() -> String {
    let millis = now_millis().max(0) as u64;
    let secs = millis / 1000;

    // Civil-from-days, so we do not need a date library for a folder name.
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        year,
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Same filesystem location, not just the same string.
///
/// Windows runners (and some TEMP values) mix 8.3 names (`RUNNER~1`) with the
/// long form (`runneradmin`). Junctions are written via `canonicalize`, so a
/// string compare would treat a finished move as an unexpected link.
pub fn paths_equal(a: &Path, b: &Path) -> bool {
    a == b || comparable_path(a) == comparable_path(b)
}

pub fn is_inside(child: &Path, parent: &Path) -> bool {
    let child = comparable_path(child);
    let parent = comparable_path(parent);
    let sep = std::path::MAIN_SEPARATOR;
    child == parent || child.starts_with(&format!("{parent}{sep}"))
}

fn comparable_path(path: &Path) -> String {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut text = resolved.to_string_lossy().replace('/', "\\");
    if cfg!(windows) {
        const UNC: &str = r"\\?\UNC\";
        const VERBATIM: &str = r"\\?\";
        if let Some(rest) = text.strip_prefix(UNC) {
            text = format!(r"\\{rest}");
        } else if let Some(rest) = text.strip_prefix(VERBATIM) {
            text = rest.to_string();
        }
        text.trim_end_matches('\\').to_lowercase()
    } else {
        resolved.to_string_lossy().trim_end_matches('/').to_string()
    }
}

/// Strips user names and other identifying path segments before anything is
/// written to an exportable diagnostics report.
pub fn redact_path(path: &Path) -> String {
    let text = path.to_string_lossy().to_string();
    let mut redacted = text;
    for key in ["USERPROFILE", "HOME"] {
        if let Some(home) = std::env::var_os(key) {
            let home = home.to_string_lossy().to_string();
            if home.is_empty() {
                continue;
            }
            if cfg!(windows) {
                if redacted.to_lowercase().starts_with(&home.to_lowercase()) {
                    redacted = format!("<HOME>{}", &redacted[home.len()..]);
                }
            } else if let Some(rest) = redacted.strip_prefix(&home) {
                redacted = format!("<HOME>{rest}");
            }
        }
    }
    redacted
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn formats_byte_magnitudes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1024 * 1024 * 3 / 2), "1.50 MB");
        assert_eq!(format_bytes(5 * 1024 * 1024 * 1024), "5.00 GB");
    }

    #[test]
    fn detects_containment_without_prefix_false_positives() {
        let parent = PathBuf::from(if cfg!(windows) {
            r"D:\CursorData"
        } else {
            "/data/cursor"
        });
        let inside = parent.join("cache");
        let sibling = PathBuf::from(if cfg!(windows) {
            r"D:\CursorDataBackup"
        } else {
            "/data/cursor-backup"
        });
        assert!(is_inside(&inside, &parent));
        assert!(is_inside(&parent, &parent));
        assert!(!is_inside(&sibling, &parent));
    }

    #[test]
    fn existing_directory_equals_its_canonical_path() {
        let temp = tempfile::tempdir().unwrap();
        let inner = temp.path().join("folder");
        std::fs::create_dir_all(&inner).unwrap();
        let canonical = std::fs::canonicalize(&inner).unwrap();
        assert!(
            paths_equal(&inner, &canonical),
            "temp {:?}\ncanonical {:?}",
            inner,
            canonical
        );
        assert!(is_inside(&inner, temp.path()));
        assert!(is_inside(&canonical, temp.path()));
    }

    #[test]
    fn timestamp_slug_has_expected_shape() {
        let slug = timestamp_slug();
        assert_eq!(slug.len(), 15);
        assert_eq!(slug.as_bytes()[8], b'-');
        assert!(slug.chars().filter(|c| c.is_ascii_digit()).count() == 14);
    }
}
