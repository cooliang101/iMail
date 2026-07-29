# 架构说明

## MCP 控制面

MCP 是现有本地邮件能力上的受控适配层，不建立第二份邮件状态，也不绕过 IMAP/SMTP 服务边界。

```text
Agent
  ├─ Streamable HTTP /mcp ─ Bearer imail_mcp_* ─┐
  └─ stdio npm run mcp ─ IMAIL_MCP_AUTH_CODE ───┤
                                                ▼
                                     server/mcp/server.ts
                                                │
                  ┌─────────────────────────────┼──────────────────────────┐
                  ▼                             ▼                          ▼
             mail / oauth                  store.ts                  crypto.ts
             IMAP + SMTP             SQLite 本地缓存/草稿        AES-256-GCM 凭据
```

### 模块职责

- `server/mcp/http.ts`：Host/Origin 防护、Bearer 授权码认证、Express 与 Web Standard MCP 响应流转换。
- `server/mcp/stdio.ts`：从环境变量读取授权码，认证后启动 stdio MCP。
- `server/mcp/server.ts`：注册工具、Zod 参数模型、structured content 和 destructive/read-only annotations。
- `server/tokens.ts`：生成高熵授权码、SHA-256 哈希、常量时间比较、过期与撤销检查。

### 权限模型

`mcp:full` 是独立的管理权限。MCP 入口只接受包含该 scope 的 Token；`messages:read`、`messages:send` 和 `accounts:read` 仍只用于开发者网关。MCP 授权码以 `imail_mcp_` 开头，语义覆盖全部当前与未来账户，因此新增账户后无需重新签发。

授权码仍使用既有 `developer_tokens`、`developer_token_scopes` 和 `developer_token_accounts` 表，没有新增明文凭据列。`accountIds` 为兼容现有 Token 展示继续写入，但 MCP 管理权限不以创建时账户快照作为访问边界。

### 请求流程

1. HTTP 入口校验 Host，存在 Origin 时同时校验 Origin。
2. 从 `Authorization: Bearer` 提取授权码，经 `authenticateToken(..., 'mcp:full')` 验证。
3. 官方 MCP SDK 的 per-request factory 创建服务实例并完成协议分派。
4. 工具调用既有 `mail`、`oauth`、`store` 和加密能力。
5. 返回文本内容与 `structuredContent`；JSON 序列化会剔除 `undefined`，凭据字段从不进入返回对象。

stdio 在启动阶段完成同一项 Token 验证，之后由进程生命周期和 MCP 传输保护连接。授权码到期不会中断已经启动的 stdio 进程；需要严格即时撤销时应使用 HTTP，或在撤销后终止该 stdio 子进程。

### 安全取舍

- 服务默认监听 `127.0.0.1`，MCP 再增加 Host/Origin allowlist，降低 DNS rebinding 和浏览器跨站调用风险。
- 远程模式不是默认发布形态；仅配置 `MCP_ALLOWED_HOSTS` 不等于完成远程加固，还需要 HTTPS、管理端认证、审计和速率限制。
- 邮箱服务商授权码只在工具参数和加密流程中短暂存在，不写日志、不返回。
- 发送、远程状态更新、移动和同步复用现有实现，保持 API 与 MCP 的协议行为一致。
