//! Platform-specific filesystem, process, and path knowledge.
//!
//! Everything that differs between Windows, macOS, and Linux lives behind this
//! module so the migration state machine in `core` stays identical everywhere.

use crate::core::model::Recommendation;
use std::path::PathBuf;

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
