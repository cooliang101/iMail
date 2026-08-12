# iMail Rust workspace

根目录 Cargo workspace 是 iMail 当前唯一的服务端实现。`crates/` 中的通用能力同时服务于 Windows Tauri 进程内直连与 `http-service/` 独立部署入口；旧 Node 服务已在等价迁移和 Windows 验收完成后删除，历史实现通过 Git 追溯。

## 当前 crate

- `imail-protocol`：稳定错误、数据库清单和共享协议模型。
- `imail-core`：不依赖 HTTP/Tauri/MCP 的应用端口。
- `imail-security`：与 Node 兼容的 AES-256-GCM、scrypt、Token SHA-256 和审计 HMAC。
- `imail-storage-sqlite`：SQLite schema v6、迁移、备份、认证、内容与同步事务；拒绝未来 schema，不向公开模型返回加密凭据或 Token 哈希。

## 验证

```powershell
npm --prefix frontend run rust:fmt
npm --prefix frontend run rust:clippy
npm --prefix frontend run rust:test
```

迁移检查器必须指向由 `npm --prefix frontend run migration:baseline` 创建的快照，不要在开发验证中直接改写当前 `.data`：

```powershell
cargo run -p imail-storage-sqlite --bin imail-db-inspect -- `
  output/rust-migration-tests/<run-id>/snapshot
```

追加 `--models` 会解析账户、邮件、草稿、联系人、公开 Token 视图和同步模型，但只输出各模型数量，不输出邮件正文、邮箱地址、凭据或 Token 哈希。

追加 `--credentials` 会使用快照中的主密钥验证全部账户加密载荷，只输出成功数量和字段名计数，不输出账户标识或任何凭据值。

迁移期 Node/Rust 对照入口已经随旧 Node 服务删除；历史结果保留在 `docs/rust-migration-*-report.md` 与 `output/rust-migration-tests/`。当前持续门禁只运行 Rust 实现，并继续保护活动 `.data`。
