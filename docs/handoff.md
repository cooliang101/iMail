# 工程交接

## 当前可用能力

- Streamable HTTP：`POST/GET/DELETE /mcp`，Bearer `mcp:full` 授权。
- stdio：`npm run mcp`，通过 `IMAIL_MCP_AUTH_CODE` 启动认证。
- 22 个 MCP 工具覆盖账户接入/授权、同步、邮件读写/移动、附件、草稿、标签与通知。
- MCP 授权码与普通网关 Token 共用哈希、过期和撤销基础设施，但 scope 严格隔离。
- UI 可在没有邮箱时签发 MCP 授权码，Agent 可以接入第一个授权码型邮箱。
- HTTP Host/Origin allowlist、参数大小限制、destructive annotations 和凭据裁剪已接入。

## 不变量

- 不在 MCP 响应、日志或错误中返回邮箱凭据、OAuth Token、主密钥或 `encryptedSecret`。
- MCP 账户管理必须要求 `mcp:full`，不能用普通开发者网关 scope 代替。
- 新增邮件/账户管理能力时，同时评估 HTTP API、MCP 工具、README 与 `docs/mcp-integration.md` 是否需要同步。
- 远程部署不能只放宽 `MCP_ALLOWED_HOSTS`；必须配套 HTTPS 和管理面安全控制。

## 验证基线

提交前运行：

```bash
npm run typecheck
npm test
npm run build
npm audit --omit=dev
```

当前测试包含 MCP 认证隔离、初始化、工具发现、账户读取和凭据不泄漏。真实邮箱的网络操作仍按 README 的平台验收口径执行，不在自动化测试中连接生产邮箱。
