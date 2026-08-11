# Rust 服务迁移 R3 验收报告

日期：2026-08-10
分支：`codex/rust-service-migration`

## 结论

R3“低协议风险领域”已完成。所有能力均位于 HTTP/Tauri 无关的 Rust 应用服务、协议 DTO 或可替换端口之后；当前 Node 服务仍是唯一运行时和真实数据写入者，未发生切换。

## 已交付能力

- 账户安全视图、元数据更新、账户删除级联、代理配置复制/显式更新/禁用及应用密码替换。
- 候选账户先由注入的连接验证器验证，成功后才写入；验证失败不会修改数据库或密文。
- 草稿、偏好、严格自定义主题、标签、通知、联系人重建与 Logo 主域回退。
- Logo 成功或失败尝试一经记录均不自动重试；后续重试只能由显式操作触发。
- 用户邮件数据清除单事务，保留应用身份、登录能力和用户 metadata，并隔离其他用户。
- Node 兼容的邮件授权导出：scrypt `N=32768,r=8,p=1,keyLength=32`、AES-256-GCM、12 字节 IV，以及 `imail-mail-authorizations:v1` AAD。
- 在线 SQLite 一致性备份、主密钥/实例 ID/Logo 同批复制、v2 SHA-256 清单、v1/v2 恢复校验、未来 schema 拒绝和非覆盖恢复准备。
- `DataMaintenancePort`、凭据编解码端口、连接验证端口及桥接安全 DTO，可供后续 Tauri/HTTP/MCP 适配器共用。

## 契约证据

- `rust/fixtures/r3-domain-v1.json` 同时由 Node Vitest 与 Rust 测试执行，覆盖联系人去重/本人地址排除/主域 Logo、`zh-CN` 标签排序及通知顺序。
- Rust 便携导出固定向量由 Node `crypto.scrypt` 与 AES-GCM 独立生成；Rust 加密结果的 salt、IV、tag、ciphertext 完全一致，并覆盖错误密码和篡改。
- Rust 生成的备份同时由 Rust 恢复服务和当前 Node `scripts/prepare-restore.mjs` 验证；Logo 篡改被双方完整性清单拒绝。
- 账户删除测试确认邮件、草稿、同步策略、同步状态、同步任务及开发者 Token 的账户绑定被清除，Token 与其他账户保留。

## 数据保护结果

- 所有清除、迁移、凭据更新、备份恢复和失败回滚测试只操作系统临时目录或 R0 独立工作副本。
- R0 快照 SHA-256：`dff6a56c17b3c4f15cf5745d52e8da871db528165425a28336240850c6de91d9`。
- 活动 `.data/imail.sqlite` SHA-256：`074b3d437adddebe3bd8020c3daa18b825de440eda34ad01838979574bd72000`。
- 快照验证报告的 `snapshotUnchanged` 与 `activeDataUnchanged` 均为 `true`。

## 回退状态

无需运行时回退：Rust R3 尚未接入当前应用或服务启动路径。删除临时测试副本即可撤销测试写入，Node 服务与原始数据保持原状。
