# Cursor Space Manager

[简体中文](README.zh-CN.md)

[![CI](https://github.com/JackXhl/cursor-space-manager/actions/workflows/ci.yml/badge.svg)](https://github.com/JackXhl/cursor-space-manager/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/JackXhl/cursor-space-manager?include_prereleases)](https://github.com/JackXhl/cursor-space-manager/releases)

Move Cursor’s data directories off a full system drive without reinstalling.

Cursor caches, extensions, and workspace state grow over time, and those
folders stay on the system disk. This app measures real usage, copies what is
safe to move onto another local disk, then points the original paths there with
an NTFS junction (a symlink on macOS/Linux). Cursor keeps working with no
settings change.

> Status: v0.1.0, **Windows first**. macOS/Linux platform code is in the tree
> but has not been fully verified on those systems.

This project is unofficial and is **not** affiliated with Anysphere. Cursor is
a trademark of its respective owner.

## Why it does not throw your data away

A tool that moves user data fails more expensively than it saves gigabytes, so
the whole flow is built around one invariant: **a complete copy exists on disk
at every moment.**

- **It only starts after Cursor has fully quit.** There is no force-kill
  button. Killing a process mid-write is how you get half-written SQLite files.
- **Copy, never move.** Files go to a staging folder on the target disk. The
  source is left alone. `robocopy` is used without `/MOVE`, `/MIR`, or `/PURGE`.
- **Per-file SHA-256, then cut over.** Copied SQLite files also get a read-only
  `PRAGMA quick_check`. A failed check deletes the copy; the source is
  untouched.
- **Cutover is atomic.** Rename the source to a backup on the same volume →
  create the link → probe through the original path. Any step failure rolls
  back.
- **Source backups stay by default.** Right after a successful switch, the
  system disk is not smaller yet. Start Cursor, confirm settings, extensions,
  and workspaces, then remove backups yourself.
- **Cleanup re-checks the link.** It inspects the reparse tag and target and
  will not recursively delete through a junction.
- **Everything is journaled.** Each action writes `INTENT` then `RESULT` to
  SQLite. After a crash, the app reconciles the journal with the disk and
  suggests a recovery — it does not guess.

The install directory itself is not moved. Relocating the app binary breaks
auto-update and uninstall metadata. The scanner lists it as “better left as
is”.

## Usage

1. Open the app. A read-only scan lists the Cursor install, each data folder’s
   size, and the disks they sit on.
2. On **Choose**, tick folders to move and pick a target. The target must be a
   local, always-connected disk (NTFS on Windows) with headroom after the copy.
3. On **Confirm**, acknowledge each risk and fully quit Cursor (including the
   tray icon).
4. After the move, start Cursor. If everything looks right, come back and
   remove backups. That is when the system disk actually frees space.

You can **Undo** at any time: restore from the backup if it is still there, or
copy back from the target after verification if it is not.

## Recovery

Do **not** delete folders that look duplicated. During a move that is
deliberate; the copy you delete might be the only complete one.

Open the app again. The **Recover** page reconciles the journal with the disk
and tells you what it found. It only diagnoses — it will not auto-repair when
the state is ambiguous.

If Cursor will not start because the original path is gone, a backup named
`*.csm-backup-<timestamp>` is next to it. Rename that backup back to the
original folder name (or use Recover). To remove a junction later, use `rmdir`
without `/s` — never recurse through the link.

Journal and settings live in `%APPDATA%\com.cursorspacemanager.app\`, not
inside any Cursor folder.

## Build from source

You need Node.js 20+, Rust stable, and on Windows the MSVC C++ build tools.

```bash
npm install
npm run tauri:dev     # development
npm run tauri:build   # installers
```

Checks:

```bash
npm run typecheck
npm run test:rust
```

Rust includes unit tests and a fault-injection suite
(`src-tauri/tests/fault_injection.rs`) that creates real links, moves real
files, and asserts the data is still intact after copy failure, verify
failure, a busy process, and a crash during cutover.

## Layout

| Path | What it is |
| --- | --- |
| `frontend/` | Vue 3 + TypeScript + Pinia; wizard, disk view, history, settings |
| `src-tauri/src/core/` | `scanner`, `planner`, `executor`, `verifier`, `journal`, `recovery` |
| `src-tauri/src/platforms/` | Windows: Restart Manager + NTFS junctions; Unix: symlinks |
| `src-tauri/capabilities/` | Least-privilege Tauri permissions; the UI can only call allowlisted commands |
| `assets/` | Static images used in the README (donation QR codes) |

The UI has no filesystem or shell access. Commands check incoming paths
against the latest scan.

## Release and signing

The version comes from build metadata; a settings file cannot change it.
Pushing a `v*` tag runs `.github/workflows/release.yml`. Each platform uploads
into a **draft** GitHub Release (so the updater cannot see a half-built
`latest.json`). After every OS succeeds, the workflow checks that `latest.json`,
`.sig` files, and Windows / macOS / Linux updater entries exist, then
**publishes the draft automatically**. There is no extra click on GitHub.

Configure these secrets:

| Secret | Purpose |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | Updater signing key; without it the updater rejects the artifact |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Password for that key |
| `TAURI_WINDOWS_SIGN_COMMAND` | Authenticode command |
| `APPLE_*` | macOS signing and notarization |

The updater only accepts a payload that verifies and matches the current
platform. A failed download can be retried; it will not break the installed
build.

## Contributing

Issues and pull requests are welcome. Keep the diff focused, and put
user-visible copy in both `frontend/src/i18n/locales/zh-CN.ts` and `en.ts`.

Security reports: [SECURITY.md](SECURITY.md). Changelog: [CHANGELOG.md](CHANGELOG.md).

## Support

If this tool saved you some disk space, you can buy the maintainer a coffee.

<p align="center">
  <img src="assets/donate-wechat.png" alt="WeChat Pay" width="240" />
  &nbsp;
  <img src="assets/donate-alipay.png" alt="Alipay" width="240" />
</p>

<p align="center"><sub>WeChat Pay (left) · Alipay (right)</sub></p>

## License

[MIT](LICENSE)
