//! Platform-specific filesystem, process, and path knowledge.
//!
//! Everything that differs between Windows, macOS, and Linux lives behind this
//! module so the migration state machine in `core` stays identical everywhere.

use crate::core::model::Recommendation;
use std::path::{Path, PathBuf};

/// True for Cursor itself and its Electron helpers, never for this tool.
///
/// Matching only `Cursor.exe` / `cursor` misses tray and renderer helpers that
/// still hold SQLite files open after the main window is closed.
pub fn is_cursor_process(name: &str, exe: Option<&Path>) -> bool {
    fn stem_looks_like_cursor(raw: &str) -> Option<bool> {
        let stem = raw
            .strip_suffix(".exe")
            .unwrap_or(raw)
            .trim()
            .to_ascii_lowercase();
        if stem.is_empty() {
            return None;
        }
        if stem.contains("space-manager") || stem.contains("cursorspacemanager") {
            return Some(false);
        }
        if stem == "cursor"
            || stem.starts_with("cursor helper")
            || stem.starts_with("cursor-helper")
            || stem.starts_with("cursor_helper")
        {
            return Some(true);
        }
        None
    }

    if let Some(matched) = stem_looks_like_cursor(name) {
        return matched;
    }
    if let Some(file_name) = exe.and_then(|path| path.file_name()) {
        if let Some(matched) = stem_looks_like_cursor(&file_name.to_string_lossy()) {
            return matched;
        }
    }
    false
}

#[cfg(test)]
mod process_tests {
    use super::is_cursor_process;
    use std::path::Path;

    #[test]
    fn recognises_cursor_and_helpers() {
        assert!(is_cursor_process("Cursor.exe", None));
        assert!(is_cursor_process("cursor", None));
        assert!(is_cursor_process("Cursor Helper.exe", None));
        assert!(is_cursor_process("Cursor Helper (Renderer).exe", None));
        assert!(is_cursor_process("Cursor Helper (GPU).exe", None));
        assert!(is_cursor_process(
            "ignored",
            Some(Path::new(
                r"C:\Users\me\AppData\Local\Programs\cursor\Cursor.exe"
            )),
        ));
    }

    #[test]
    fn ignores_this_tool_and_unrelated_names() {
        assert!(!is_cursor_process("cursor-space-manager.exe", None));
        assert!(!is_cursor_process("Cursor Space Manager", None));
        assert!(!is_cursor_process("code.exe", None));
        assert!(!is_cursor_process("explorer.exe", None));
    }
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::*;

/// A known location worth inspecting, before we check whether it exists.
#[derive(Debug, Clone)]
pub struct CandidateRoot {
    pub id: String,
    pub label: String,
    pub purpose: String,
    pub path: PathBuf,
    pub recommendation: Recommendation,
    pub reason: String,
    pub from_launch_argument: bool,
}

impl CandidateRoot {
    pub fn new(
        id: &str,
        label: &str,
        purpose: &str,
        path: PathBuf,
        recommendation: Recommendation,
        reason: &str,
    ) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            purpose: purpose.to_string(),
            path,
            recommendation,
            reason: reason.to_string(),
            from_launch_argument: false,
        }
    }

    pub fn from_launch_argument(mut self) -> Self {
        self.from_launch_argument = true;
        self
    }
}

/// Outcome of a directory copy, before independent verification runs.
#[derive(Debug, Clone, Default)]
pub struct CopyOutcome {
    pub bytes: u64,
    pub files: u64,
    /// Native tool exit code, kept for the diagnostics report.
    pub raw_status: i32,
    pub log: Vec<String>,
}
