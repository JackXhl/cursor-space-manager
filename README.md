# Cursor 空间迁移工具

[English](README.en.md)

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/JackXhl/cursor-space-manager?include_prereleases)](https://github.com/JackXhl/cursor-space-manager/releases)

<p align="center">
  <img src="assets/screenshot-scan.png" alt="扫描页：安装位置、占用和磁盘" width="720" />
</p>
<p align="center">
  <img src="assets/screenshot-settings.png" alt="设置页：外观、语言和扫描选项" width="720" />
</p>

## 它为什么不会弄丢数据

搬用户数据，出错的代价远大于省下的那几 GB。整个流程只有一条不变量：**任何时刻磁盘上都至少存在一份完整数据。**

- **Cursor 必须完全退出后才动手**（含托盘）。没有强制结束按钮。
- **只复制，不移动。** 源目录全程保留。不用 `robocopy /MOVE`、`/MIR`、`/PURGE`。
- **逐文件 SHA-256 校验通过后才切换**；SQLite 再做只读 `PRAGMA quick_check`。失败只删副本。
- **切换：同卷改名为备份 → 建链接 → 探针验证。** 任一步失败就回滚。
- **备份默认留着。** 确认 Cursor 正常后再清理，这时才真正腾出系统盘。
- **清理前再检查链接指向**，绝不顺着 Junction 递归删除。
- **每步先写 INTENT 再写 RESULT。** 恢复页只诊断，状态说不清时不自动修。

安装目录本身不搬，搬走后可能没法自动更新或正常卸载。

## 使用

1. 打开后自动只读扫描：安装位置、各数据目录占用、所在磁盘。
2. 「建议」里勾选要搬的目录，选一块本地固定的 NTFS 盘，并留够余量。
3. 「确认」里逐项看风险，并完全退出 Cursor。
4. 完成后先启动 Cursor 检查，再回来清理备份。

随时可以「撤销」：备份还在就改回去，备份没了就从目标盘校验后回迁。

## 故障恢复

**不要**手删看起来重复的目录。迁移过程中重复是刻意的，删错的那份可能是唯一完整的。

打开「恢复」页，对照日志和磁盘。它只说明现状。原路径空了的话，旁边的 `*.csm-backup-<时间戳>` 改回原名即可。删除 Junction 用 `rmdir`、不要加 `/s`。

日志和设置在 `%APPDATA%\com.cursorspacemanager.app\`，不在 Cursor 目录里。

## 从源码构建

需要 Node.js 20+、Rust stable，Windows 上还要 MSVC C++ 生成工具。

```bash
npm install
npm run tauri:dev     # 开发
npm run tauri:build   # 安装包
npm run typecheck
npm run test:rust
```

## 参与贡献

欢迎 Issue 和 Pull Request。用户可见文案请同时改 `frontend/src/i18n/locales/zh-CN.ts` 和 `en.ts`。

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
