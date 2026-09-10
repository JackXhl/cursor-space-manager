# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-09-10

### Added

- Initial public release: scan Cursor data usage, copy selected directories to
  another local disk, cut over with NTFS junctions (symlinks on macOS/Linux),
  keep source backups until the user confirms, and recover interrupted
  operations from the journal.
- English UI via `vue-i18n`, with language selection in Settings.
- In-app download and install for signed GitHub Releases.
- Release workflow verifies updater assets and publishes the GitHub draft
  automatically after all platforms succeed.

[Unreleased]: https://github.com/JackXhl/cursor-space-manager/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/JackXhl/cursor-space-manager/releases/tag/v0.1.0
