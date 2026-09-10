//! Fault injection against the real migration engine.
//!
//! Every test here asserts the invariant the whole tool is built around: at no
//! point may the only complete copy of the data disappear. Each scenario breaks
//! the migration at a different step and then checks what actually survived on
//! disk, not what the journal claims happened.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use cursor_space_manager_lib::core::executor::{Executor, ProcessGate};
use cursor_space_manager_lib::core::journal::Journal;
use cursor_space_manager_lib::core::model::*;
use cursor_space_manager_lib::core::planner::{BACKUP_SUFFIX, STAGING_DIR};
use cursor_space_manager_lib::core::{recovery, verifier};
use cursor_space_manager_lib::platforms;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// Reports whatever it was told to, so a machine with Cursor open can still run
/// the suite, and so the "app is running" branch can be exercised on demand.
struct StubGate(Vec<RunningProcess>);

impl ProcessGate for StubGate {
    fn running(&self) -> Vec<RunningProcess> {
        self.0.clone()
    }
}

fn quiet() -> Arc<dyn ProcessGate> {
    Arc::new(StubGate(Vec::new()))
}

fn busy() -> Arc<dyn ProcessGate> {
    Arc::new(StubGate(vec![RunningProcess {
        pid: 4242,
        name: "Cursor.exe".into(),
        exe_path: None,
    }]))
}

struct Fixture {
    _temp: tempfile::TempDir,
    source_parent: PathBuf,
    target_root: PathBuf,
    journal: Arc<Journal>,
}

/// A small but non-trivial tree: nested directories, an empty file, and a file
/// large enough that a truncated copy is detectable.
fn populate(root: &Path) {
    std::fs::create_dir_all(root.join("nested/deep")).unwrap();
    std::fs::write(root.join("top.txt"), b"top level contents").unwrap();
    std::fs::write(root.join("empty.bin"), b"").unwrap();
    std::fs::write(root.join("nested/mid.json"), br#"{"a":1}"#).unwrap();
    std::fs::write(root.join("nested/deep/blob.bin"), vec![7u8; 64 * 1024]).unwrap();
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let source_parent = temp.path().join("source-parent");
        let target_root = temp.path().join("target-root");
        std::fs::create_dir_all(&source_parent).unwrap();
        std::fs::create_dir_all(&target_root).unwrap();

        let journal = Arc::new(Journal::open(&temp.path().join("journal.sqlite")).unwrap());

        Self {
            _temp: temp,
            source_parent,
            target_root,
            journal,
        }
    }

    /// Builds a plan by hand. Volume eligibility is the planner's job and is
    /// tested there; here we want the executor exercised against scratch paths.
    fn plan(&self, roots: &[&str]) -> MigrationPlan {
        let operation_id = format!("op-{}", uuid::Uuid::new_v4());
        let mut planned = Vec::new();
        let mut total_bytes = 0;
        let mut total_files = 0;

        for name in roots {
            let source_path = self.source_parent.join(name);
            populate(&source_path);

            let cancel = AtomicBool::new(false);
            let size =
                cursor_space_manager_lib::core::scanner::measure_directory(&source_path, &cancel);
            total_bytes += size.logical_bytes;
            total_files += size.file_count;

            planned.push(PlannedRoot {
                root_id: (*name).to_string(),
                label: (*name).to_string(),
                source_path: source_path.clone(),
                target_path: self.target_root.join(name),
                backup_path: self
                    .source_parent
                    .join(format!("{name}.{BACKUP_SUFFIX}-test")),
                staging_path: self
                    .target_root
                    .join(STAGING_DIR)
                    .join(&operation_id)
                    .join(name),
                estimated_bytes: size.logical_bytes,
                estimated_files: size.file_count,
            });
        }

        let plan = MigrationPlan {
            operation_id,
            created_at: 0,
            target_root: self.target_root.clone(),
            target_volume: platforms::volume_root_for(&self.target_root).unwrap_or_default(),
            source_volume: platforms::volume_root_for(&self.source_parent).unwrap_or_default(),
            roots: planned,
            total_bytes,
            total_files,
            required_target_bytes: total_bytes,
            blockers: Vec::new(),
            warnings: Vec::new(),
        };
        self.journal.create_operation(&plan).unwrap();
        plan
    }

    fn executor(&self, gate: Arc<dyn ProcessGate>) -> Executor {
        Executor::with_gate(
            Arc::clone(&self.journal),
            Arc::new(AtomicBool::new(false)),
            gate,
        )
    }
}

/// Content of a tree as (relative path, bytes), so two locations can be
/// compared regardless of where they live.
fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut entries = Vec::new();
    for entry in walkdir::WalkDir::new(root).follow_links(false).min_depth(1) {
        let entry = entry.unwrap();
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        entries.push((relative, std::fs::read(entry.path()).unwrap()));
    }
    entries.sort();
    entries
}

/// The invariant: a complete, readable copy exists somewhere.
fn assert_data_survives(expected: &[(String, Vec<u8>)], candidates: &[&Path]) {
    let found = candidates
        .iter()
        .filter(|path| path.exists())
        .any(|path| snapshot(path) == *expected);
    assert!(
        found,
        "no location holds a complete copy; checked {:?}",
        candidates
    );
}

// ---------------------------------------------------------------------------
// Happy path, so the fault cases have a baseline
// ---------------------------------------------------------------------------

#[test]
fn successful_migration_links_source_and_retains_backup() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&["cache"]);
    let root = &plan.roots[0];
    let expected = snapshot(&root.source_path);

    let result = fixture
        .executor(quiet())
        .run(&plan, &mut |_| {})
        .expect("migration should succeed");

    assert_eq!(result.state, MigrationState::ActiveBackupRetained);
    assert!(result.backups_retained);

    // The original path now resolves to the target.
    let state = platforms::link_state(&root.source_path);
    assert!(state.is_link(), "source should be a link, got {state:?}");
    assert_eq!(
        state.target().map(|t| t.to_path_buf()),
        Some(root.target_path.clone())
    );

    // Data is readable through the link, present at the target, and still in
    // the retained backup. Nothing has been given up yet.
    assert_eq!(snapshot(&root.source_path), expected);
    assert_eq!(snapshot(&root.target_path), expected);
    assert_eq!(snapshot(&root.backup_path), expected);

    // Staging is not left behind.
    assert!(!fixture
        .target_root
        .join(STAGING_DIR)
        .join(&plan.operation_id)
        .exists());
}

#[test]
fn cleanup_frees_the_backup_only_after_the_link_is_verified() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&["cache"]);
    let root = &plan.roots[0];
    let expected = snapshot(&root.source_path);

    let executor = fixture.executor(quiet());
    executor.run(&plan, &mut |_| {}).unwrap();
    let freed = executor.cleanup_backups(&plan).unwrap();

    assert!(freed > 0, "cleanup should report the reclaimed bytes");
    assert!(!root.backup_path.exists(), "backup should be gone");
    // The data itself is untouched, still reachable under the original path.
    assert_eq!(snapshot(&root.source_path), expected);
    assert_eq!(
        fixture.journal.operation_state(&plan.operation_id).unwrap(),
        MigrationState::Completed
    );
}

// ---------------------------------------------------------------------------
// Fault: the application is still running
// ---------------------------------------------------------------------------

#[test]
fn a_running_cursor_stops_the_migration_before_anything_is_written() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&["cache"]);
    let root = &plan.roots[0];
    let expected = snapshot(&root.source_path);

    let error = fixture
        .executor(busy())
        .run(&plan, &mut |_| {})
        .expect_err("a running Cursor must block the run");
    assert!(error.to_string().contains("Cursor 仍在运行"));

    // Untouched: still a plain directory, no target, no backup.
    assert!(!platforms::link_state(&root.source_path).is_link());
    assert_eq!(snapshot(&root.source_path), expected);
    assert!(!root.target_path.exists());
    assert!(!root.backup_path.exists());
    assert_eq!(
        fixture.journal.operation_state(&plan.operation_id).unwrap(),
        MigrationState::FailedSafe
    );
}

// ---------------------------------------------------------------------------
// Fault: the copy cannot complete
// ---------------------------------------------------------------------------

#[test]
fn an_unwritable_target_leaves_the_source_intact() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&["cache"]);
    let root = &plan.roots[0];
    let expected = snapshot(&root.source_path);

    // Occupy the staging path with a file, so creating it as a directory fails.
    std::fs::create_dir_all(root.staging_path.parent().unwrap()).unwrap();
    std::fs::write(&root.staging_path, b"in the way").unwrap();

    let error = fixture
        .executor(quiet())
        .run(&plan, &mut |_| {})
        .expect_err("staging must fail");
    assert!(!error.to_string().is_empty());

    assert_eq!(
        snapshot(&root.source_path),
        expected,
        "source must be untouched when the copy fails"
    );
    assert!(!platforms::link_state(&root.source_path).is_link());
    assert!(!root.backup_path.exists());
}

// ---------------------------------------------------------------------------
// Fault: the copy is complete but wrong
// ---------------------------------------------------------------------------

#[test]
fn verification_catches_a_corrupted_copy() {
    // Verification is what stands between a bad copy and a deleted original,
    // so it is asserted directly rather than through an injected failure.
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let target = temp.path().join("target");
    populate(&source);
    populate(&target);

    let cancel = AtomicBool::new(false);
    let build =
        |path: &Path| verifier::build_manifest(path, true, &cancel, &mut |_, _| {}).unwrap();

    assert!(
        verifier::compare(&build(&source), &build(&target)).ok,
        "identical trees must verify"
    );

    // Same length, different bytes: only hashing catches this.
    std::fs::write(target.join("nested/deep/blob.bin"), vec![9u8; 64 * 1024]).unwrap();
    let report = verifier::compare(&build(&source), &build(&target));
    assert!(!report.ok);
    assert_eq!(report.mismatched.len(), 1);

    // A missing file is caught too.
    std::fs::remove_file(target.join("top.txt")).unwrap();
    let report = verifier::compare(&build(&source), &build(&target));
    assert!(!report.ok);
    assert_eq!(report.missing.len(), 1);
}

#[test]
fn an_occupied_target_is_parked_instead_of_overwritten() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&["cache"]);
    let root = &plan.roots[0];
    let expected = snapshot(&root.source_path);

    std::fs::create_dir_all(&root.target_path).unwrap();
    std::fs::write(root.target_path.join("squatter.txt"), b"not ours").unwrap();

    let result = fixture
        .executor(quiet())
        .run(&plan, &mut |_| {})
        .expect("an occupied slot is parked, then the verified copy takes its place");
    assert_eq!(result.state, MigrationState::ActiveBackupRetained);
    assert_eq!(snapshot(&root.source_path), expected);
    assert!(!root.target_path.join("squatter.txt").exists());

    let parent = root.target_path.parent().unwrap();
    let parked = std::fs::read_dir(parent)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .find(|entry| entry.file_name().to_string_lossy().contains(".csm-prior"))
        .expect("the previous occupant must still exist under a parked name");
    assert!(parked.path().join("squatter.txt").exists());
}

// ---------------------------------------------------------------------------
// Fault: the process dies mid-migration
// ---------------------------------------------------------------------------

#[test]
fn a_crash_between_rename_and_link_is_diagnosed_not_silently_repaired() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&["cache"]);
    let root = &plan.roots[0];
    let expected = snapshot(&root.source_path);

    // Reproduce the worst crash window by hand: the source has been renamed to
    // the backup and the process died before the link existed.
    std::fs::create_dir_all(&root.target_path).unwrap();
    for (relative, bytes) in &expected {
        let path = root.target_path.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    std::fs::rename(&root.source_path, &root.backup_path).unwrap();
    fixture
        .journal
        .set_operation_state(&plan.operation_id, MigrationState::CutoverIntentRecorded)
        .unwrap();
    fixture
        .journal
        .set_root_state(
            &plan.operation_id,
            &root.root_id,
            MigrationState::SourceRenamed,
        )
        .unwrap();

    let report = recovery::diagnose(&fixture.journal, &plan.operation_id).unwrap();

    // Diagnosis must not touch the disk.
    assert!(
        !root.source_path.exists(),
        "recovery must not restore on its own"
    );
    assert!(root.backup_path.exists());
    assert_data_survives(&expected, &[&root.backup_path, &root.target_path]);
    assert!(
        report.requires_user_decision,
        "an interrupted cutover must be flagged: {report:?}"
    );
}

#[test]
fn an_unfinished_operation_is_found_at_startup() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&["cache"]);
    fixture
        .journal
        .set_operation_state(&plan.operation_id, MigrationState::Copying)
        .unwrap();

    let reports = recovery::diagnose_all(&fixture.journal).unwrap();
    assert!(
        reports.iter().any(|r| r.operation_id == plan.operation_id),
        "startup reconciliation must surface the interrupted operation"
    );
}

// ---------------------------------------------------------------------------
// Fault: a later root fails after an earlier one already switched over
// ---------------------------------------------------------------------------

#[test]
fn undo_returns_every_root_to_its_original_location() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&["cache", "extensions"]);
    let expected: Vec<_> = plan
        .roots
        .iter()
        .map(|root| snapshot(&root.source_path))
        .collect();

    let executor = fixture.executor(quiet());
    let result = executor.run(&plan, &mut |_| {}).unwrap();
    assert_eq!(result.state, MigrationState::ActiveBackupRetained);

    let undone = executor.undo(&plan).unwrap();
    assert_eq!(undone.state, MigrationState::RolledBack);

    for (root, before) in plan.roots.iter().zip(expected) {
        assert!(
            !platforms::link_state(&root.source_path).is_link(),
            "{} should be a real directory again",
            root.label
        );
        assert_eq!(snapshot(&root.source_path), before);
        assert!(!root.target_path.exists(), "migrated copy should be gone");
    }
}

#[test]
fn undo_works_after_the_backups_have_been_cleaned_up() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&["cache"]);
    let root = &plan.roots[0];
    let expected = snapshot(&root.source_path);

    let executor = fixture.executor(quiet());
    executor.run(&plan, &mut |_| {}).unwrap();
    executor.cleanup_backups(&plan).unwrap();
    assert!(!root.backup_path.exists());

    // With no backup left, undo has to copy back from the target.
    let undone = executor.undo(&plan).unwrap();
    assert_eq!(undone.state, MigrationState::RolledBack);
    assert!(!platforms::link_state(&root.source_path).is_link());
    assert_eq!(snapshot(&root.source_path), expected);
}

// ---------------------------------------------------------------------------
// The deletion guard
// ---------------------------------------------------------------------------

#[test]
fn cleanup_refuses_when_the_link_points_somewhere_unexpected() {
    let fixture = Fixture::new();
    let plan = fixture.plan(&["cache"]);
    let root = &plan.roots[0];
    let expected = snapshot(&root.source_path);

    let executor = fixture.executor(quiet());
    executor.run(&plan, &mut |_| {}).unwrap();

    // Repoint the link at an unrelated directory, as a competing tool might.
    let elsewhere = fixture.target_root.join("somewhere-else");
    std::fs::create_dir_all(&elsewhere).unwrap();
    platforms::remove_directory_link(&root.source_path).unwrap();
    platforms::create_directory_link(&root.source_path, &elsewhere).unwrap();

    let error = executor
        .cleanup_backups(&plan)
        .expect_err("a redirected link must stop the delete");
    assert!(error.to_string().contains("不会删除备份"));
    assert!(root.backup_path.exists(), "backup must be kept");
    assert_eq!(snapshot(&root.backup_path), expected);
}
