# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] - 2026-09-16

### Added

- First public release: scan Cursor data usage, copy selected directories to
  another local disk, cut over with NTFS junctions (symlinks on macOS/Linux),
  keep source backups until the user confirms, and recover interrupted
  operations from the journal.
- Recovery can remove a leftover target copy when the original folder is still
  intact, or restore a single folder from backup or from the target according
  to what is actually on disk.
- English UI via `vue-i18n`, with language selection in Settings.
- In-app download and install for signed GitHub Releases.

### Changed

- Windows installers are named `Cursor_Space_Manager_*` so GitHub asset names
  stay ASCII. The window title and Chinese UI are unchanged.
- Cursor detection includes Electron helper processes, not only `Cursor.exe`.
- File lock checks that fail now block the move instead of continuing.
- Execute re-checks the latest scan, source paths, and that the target is still
  writable. An unexpected link is never undone automatically.

[Unreleased]: https://github.com/JackXhl/cursor-space-manager/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/JackXhl/cursor-space-manager/releases/tag/v0.1.1
