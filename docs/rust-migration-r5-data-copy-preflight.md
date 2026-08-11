# Rust R5 现有数据副本预检报告

日期：2026-08-10
状态：通过；仅验证离线副本，不代表真实邮箱网络验收完成

## 安全边界

- 源数据为当前 `.data` 的 SQLite 一致性备份，保存在 `output/rust-migration-tests/r5-preflight-2026-08-10/snapshot/`。
- 原 `.data`、R0 快照和新 R5 snapshot 均未交给 Rust 写入。
- runtime 写入只发生在 `runtime-write-copy`、`runtime-execution-copy` 和最终证据副本 `runtime-final-copy`。
- 三个写入副本均保留，未删除测试数据。
- `imail-runtime-preflight` 明确拒绝名为 `.data` 的目录，并要求目标含 `backup-manifest.json` 和显式 `--write-copy`。

## 快照兼容性

新 R5 snapshot 的 Node/Rust 对照全部通过：

- schema、SQLite quick check、foreign key check 和 18 张表计数一致；
- Rust/Node 模型计数和字段摘要一致；
- 加密凭据兼容摘要一致；
- snapshot 与 active database 在验证前后均未变化。

保留模型计数：4 个账户、1363 封邮件、261 个联系人、16 个 mailbox sync state、25378 条历史 sync job、4 条同步策略和 1 个 developer token。

## 既有游标续跑

最终证据副本运行命令：

```powershell
npm run rust:runtime-preflight -- `
  D:\code\imail\output\rust-migration-tests\r5-preflight-2026-08-10\runtime-final-copy `
  --write-copy
```

结果：

- schema 从 v6 到 v6，没有应用迁移；执行前后 quick check 与 foreign key check 均通过；
- scheduler 从历史状态识别 1 个 recovery 和 3 个 startup inbox 目标；
- 历史 running job 的 rerun 请求在完成后生成额外 recovery，因此 worker 共 started 5、succeeded 5、failed 0、cancelled 0；
- 四个原始 due job 最终状态全部为 succeeded，最终 queued job 为 0；
- runtime 有界优雅关闭成功；
- 账户、邮件、草稿、联系人、developer token 和 mailbox state 数量全部保持；
- 16 个既有 mailbox state 的 UIDVALIDITY、last seen UID 和 highest MODSEQ 全部保持，没有通过清空游标或全量重建伪造成功。

该预检执行器只回写原游标和零变化计数，用于验证持久队列、租约回收、rerun、worker 完成事务和既有游标保持。它没有连接真实 IMAP，也不能替代专用邮箱上的增量结果对照。
