# Rust 迁移 R2 阶段报告

日期：2026-08-10
分支：`codex/rust-service-migration`

## 结论

R2 的 Rust SQLite 写入、安全、认证和 schema v0→v6 兼容实现已完成。所有会改变数据的自动化验证均在系统临时目录中的合成库或 R0 保留快照副本上运行；仓库 `.data` 和 `output/rust-migration-tests/r0-initial-2026-08-10/snapshot` 未作为写入目标。

## 已验证能力

- Node/Rust 逐字节兼容 AES-256-GCM、scrypt、Token/会话 SHA-256 和审计 HMAC。
- 用户、会话、限流、安全审计、开发者 Token 与首用户旧数据接管。
- 用户 metadata、账户、邮件、草稿、联系人和 Logo 采集记录的用户隔离写入。
- 邮件和草稿通过账户关系阻止跨用户 ID 覆盖。
- 普通开发者 Token 只绑定当前用户账户；`mcp:full` 独占 scope 并覆盖当前用户全部账户。
- Rust schema 迁移器支持无版本旧库和 v1→v6 逐级升级，持有 `BEGIN IMMEDIATE` 锁后重新读取版本。
- 迁移中任一步失败会回滚整轮升级；未来版本不提交任何 schema 变化；并发打开者只执行一次迁移。
- Node 与 Rust 从相同 v2 fixture 迁移后，全部表列、索引、外键、数据和 metadata 语义签名一致。
- R0 快照副本上的既有账户凭据经 Rust 解密并以随机 IV 重加密后，Node 解密语义不变。

## 自动化门禁

```powershell
npm run rust:fmt
npm run rust:clippy
cargo check --workspace --all-targets --all-features --manifest-path rust/Cargo.toml --target x86_64-pc-windows-msvc
npm run rust:test
npm run rust:verify-migrations
npm run rust:verify-auth-store
npm run rust:verify-write-copy
npm run rust:verify-snapshot -- r0-initial-2026-08-10
npm run typecheck
npm test
npm run build
```

验收时还必须确认：

- 临时工作副本 `PRAGMA quick_check` 返回 `ok`；
- `PRAGMA foreign_key_check` 无记录；
- R0 保留快照数据库 SHA-256 前后相同；
- 活动 `.data/imail.sqlite` SHA-256 保持为 `074b3d437adddebe3bd8020c3daa18b825de440eda34ad01838979574bd72000`。

## 数据位置和回退

- 保留基线：`output/rust-migration-tests/r0-initial-2026-08-10/`
- 写入测试：操作系统临时目录中的自动生成副本，测试结束后仅删除该副本。
- 当前运行路径仍由 Node 服务唯一写入，Rust 尚未接入桌面或 Docker 运行入口。
- 回退方式：停止使用尚未接入的 Rust 二进制并继续运行 Node；不需要恢复或改写现有数据。

## 已知边界

- 本阶段验证存储与安全兼容，不包含 IMAP、SMTP、OAuth 网络行为和同步运行时等价；这些属于后续阶段。
- Rust 迁移器当前目标版本固定为 schema v6；未来 schema 必须新增显式迁移步骤和新的 Node/Rust 契约 fixture。
- 桌面仍通过现有 API 调用 Node 服务，尚未切换 Tauri 进程内调用。
