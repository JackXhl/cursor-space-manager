# Cursor 空间迁移工具

[English](README.md)

[![CI](https://github.com/JackXhl/cursor-space-manager/actions/workflows/ci.yml/badge.svg)](https://github.com/JackXhl/cursor-space-manager/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/JackXhl/cursor-space-manager?include_prereleases)](https://github.com/JackXhl/cursor-space-manager/releases)

把 Cursor 占用的数据目录安全搬到另一块磁盘，系统盘快满了也不用重装。

Cursor 的缓存、扩展和工作区状态会随使用不断变大，而这些目录都固定在系统盘。本工具扫描出它们的真实占用，把可以搬走的部分复制到目标盘，再用 NTFS Junction（macOS/Linux 上是符号链接）把原路径指过去，Cursor 无需任何配置改动即可继续使用。

> 状态：v0.1.0，Windows 优先。macOS/Linux 的平台适配代码已就位，但尚未在这两个系统上完成验证。

本项目为非官方工具，与 Anysphere 无关。Cursor 为其权利人的商标。

## 它为什么不会弄丢数据

搬移用户数据这类工具，出错的代价远大于省下的那几 GB，所以整个流程围绕一条不变量设计：**任何时刻磁盘上都至少存在一份完整数据。**

- **只有确认 Cursor 完全退出后才动手。** 工具不提供强制结束进程的按钮——强杀正是产生半截 SQLite 文件的原因。
- **先复制，不移动。** 复制到目标盘的独立暂存目录，全程不碰源目录。使用 `robocopy`，且绝不使用 `/MOVE`、`/MIR`、`/PURGE`。
- **逐文件 SHA-256 校验后才切换。** 同时对复制出来的 SQLite 文件执行只读 `PRAGMA quick_check`。校验不通过就清掉副本，源目录原封不动。
- **切换是原子的。** 源目录同卷改名为备份 → 创建链接 → 写入探针验证数据确实落到目标盘。任何一步失败都自动回滚。
- **源备份默认保留。** 所以切换成功的那一刻系统盘还没真正释放空间。等你启动 Cursor 确认设置、扩展、工作区都正常，再手动清理备份。
- **删除前重新验证。** 每次清理都会重新检查链接的 reparse tag 和指向，绝不递归删除 Junction 指向的数据。
- **全程事务日志。** 每个动作先写 `INTENT` 再写 `RESULT` 到 SQLite。程序被强杀或断电后重启，会把日志和磁盘实际状态对账，给出明确的恢复建议，但不会自作主张地修复。

安装目录本身不搬——移动程序本体会让自动更新和卸载信息失效，工具只会把它列出来并标记「不建议迁移」。

## 使用

1. 打开程序，它会自动开始只读扫描，列出 Cursor 的安装位置、各数据目录的占用和所在磁盘。
2. 在「建议」步骤勾选要迁移的目录并选择目标盘。目标盘必须是本地固定的 NTFS 磁盘，且复制后仍有足够余量。
3. 「确认」步骤逐项确认风险，并完全退出 Cursor。
4. 执行完成后启动 Cursor 验证一切正常，再回到工具清理备份，这时才真正释放系统盘空间。

出问题可以随时「撤销」：备份还在就直接还原，备份已清理则从目标盘校验后回迁。

## 故障恢复

**不要**手动删除看起来重复的目录。迁移过程中「重复」是刻意的，删错的那一份可能正是唯一完整的。

重新打开工具，在「恢复」页对照事务日志和磁盘状态。它只诊断，状态说不清时不会自动修复。

如果原路径消失、Cursor 打不开，旁边会有名为 `*.csm-backup-<时间戳>` 的备份目录，把它改回原来的名字即可（或走恢复页）。以后删除 Junction 请用 `rmdir`、不要加 `/s`，切勿顺着链接递归删除。

日志和配置在 `%APPDATA%\com.cursorspacemanager.app\`，不在任何 Cursor 目录里。

## 从源码构建

需要 Node.js 20+、Rust stable，以及 Windows 上的 MSVC C++ 生成工具。

```bash
npm install
npm run tauri:dev     # 开发模式
npm run tauri:build   # 生成安装包
```

测试：

```bash
npm run typecheck
npm run test:rust
```

Rust 侧包含单元测试和一套故障注入集成测试（`src-tauri/tests/fault_injection.rs`），后者会真实创建链接、移动文件，并在复制失败、校验失败、进程占用、切换中途崩溃等场景下断言数据仍然完整。

## 结构

| 目录 | 内容 |
| --- | --- |
| `frontend/` | Vue 3 + TypeScript + Pinia，五步向导、磁盘可视化、历史与设置 |
| `src-tauri/src/core/` | 迁移核心：`scanner`、`planner`、`executor`、`verifier`、`journal`、`recovery` |
| `src-tauri/src/platforms/` | 平台适配：Windows 走 Restart Manager 与 NTFS Junction，Unix 走符号链接 |
| `src-tauri/capabilities/` | Tauri 最小权限清单，前端只能调用白名单命令 |
| `assets/` | README 用到的静态图（打赏收款码） |

前端没有任何文件系统或 Shell 权限，所有能力都通过受限的 IPC 命令暴露，且命令会对照最近一次扫描校验传入的路径。

## 发布与签名

版本号来自构建元数据，不能通过配置文件篡改。推送 `v*` tag 会跑 `.github/workflows/release.yml`：各平台先把安装包写进 **草稿** Release（避免更新器读到半成品 `latest.json`）。三端都成功后，工作流会校验 `latest.json`、`.sig` 以及 Windows / macOS / Linux 更新条目，再 **自动把草稿改成正式版**，不用再去 GitHub 点 Publish。

需要配置以下 secrets：

| Secret | 用途 |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | 更新包签名私钥，缺失时更新器会拒绝产物 |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 上述私钥的密码 |
| `TAURI_WINDOWS_SIGN_COMMAND` | Authenticode 签名命令 |
| `APPLE_*` | macOS 签名与公证 |

更新器只接受签名校验通过、且与当前平台匹配的更新包；下载失败可以重试，不会破坏已安装的版本。

## 参与贡献

欢迎开 Issue 和 Pull Request。请保持 diff 聚焦，用户能看见的文案必须同时改
`frontend/src/i18n/locales/zh-CN.ts` 和 `en.ts`。

安全披露见 [SECURITY.md](SECURITY.md)。变更记录见 [CHANGELOG.md](CHANGELOG.md)。

## 打赏

如果这个工具帮你腾出了系统盘空间，欢迎请作者喝杯咖啡。

<p align="center">
  <img src="assets/donate-wechat.png" alt="微信支付" width="240" />
  &nbsp;
  <img src="assets/donate-alipay.png" alt="支付宝" width="240" />
</p>

<p align="center"><sub>微信（左）· 支付宝（右）</sub></p>

## 许可证

[MIT](LICENSE)
