# Security Policy

[简体中文](#中文)

This tool copies and relinks user data directories. Treat security reports as
high priority: a defect can destroy files, follow a junction into the wrong
tree, or leak paths in diagnostics.

## Supported versions

| Version | Supported |
| --- | --- |
| 0.1.x | Yes |
| &lt; 0.1 | No |

## Reporting a vulnerability

**Do not open a public issue** for a vulnerability.

1. Use GitHub’s private advisory form:
   [Report a vulnerability](https://github.com/JackXhl/cursor-space-manager/security/advisories/new)
2. If that form is unavailable, contact the repository owner
   [@JackXhl](https://github.com/JackXhl) and ask for a private channel.

Please include:

- Affected version / commit
- OS and filesystem (NTFS, APFS, ext4, …)
- What you expected vs what happened
- A **minimal** reproduction that does **not** include real user paths, tokens,
  or file contents
- Whether data was modified, deleted, or only readable

You should hear back within **7 days**. If we confirm the report, we will
coordinate a fix and credit you unless you ask otherwise.

## What we do not want in public issues or PRs

- Exploit write-ups, proof-of-concept attacks, or step-by-step abuse recipes
- Real usernames, absolute home paths, or journal dumps that still contain them
- Signing keys, `.pfx` / `.p12` files, or updater private keys

Use **Copy problem notes** in the app when you need logs: it strips the
username before the text leaves the machine.

## Scope that is in

- Accidental recursive delete through a junction / symlink
- Path confusion (migrating or deleting the wrong directory)
- Command IPC that accepts a path the latest scan did not authorize
- Unsigned or mismatched updater payloads being accepted
- Secrets committed to the repository

## Scope that is out

- Cursor itself, including its update channel and data format
- Social engineering against a user who already has local admin rights
- Issues that require a compromised webview **and** a missing capability
  (report those anyway if you can bypass the allowlist)

---

## 中文

这是一个会复制、改名并重新接上用户数据目录的工具。安全问题按高优先级处理：缺陷可能导致删错文件、顺着链接进到错误目录，或在诊断信息里泄漏路径。

**不要用公开 Issue 报告漏洞。** 请使用
[GitHub 私密安全公告](https://github.com/JackXhl/cursor-space-manager/security/advisories/new)，
或联系仓库所有者 [@JackXhl](https://github.com/JackXhl)。

请说明受影响版本、操作系统与文件系统，以及**不含真实用户路径和文件内容**的最小复现步骤。我们会在 7 天内回复。

应用内的「复制问题说明」会去掉用户名，适合附在反馈里。
