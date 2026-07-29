# 运维手册

## MCP 环境变量

| 变量 | 默认值 | 用途 |
| --- | --- | --- |
| `MCP_ALLOWED_HOSTS` | `localhost,127.0.0.1,::1` | HTTP MCP 允许的 Host 与 Origin 主机名，逗号分隔 |
| `IMAIL_MCP_AUTH_CODE` | 无 | 仅 stdio 启动使用的 `mcp:full` 授权码 |
| `HOST` | `127.0.0.1` | iMail API 监听地址；远程监听会扩大所有 API 的暴露面 |
| `PORT` | `8787` | API 与 `/mcp` 端口 |

## 本地启动

Streamable HTTP：

```bash
npm run dev
```

stdio：

```powershell
$env:IMAIL_MCP_AUTH_CODE='imail_mcp_xxx'
npm run mcp
```

## 冒烟检查

1. 在开发者网关签发 `mcp:full` 授权码。
2. 用 MCP Inspector 或任意标准客户端连接 `http://127.0.0.1:8787/mcp`。
3. 确认 `tools/list` 包含 `accounts_list`、`messages_list`、`message_send` 和 `account_remove`。
4. 调用 `imail_status` 与 `accounts_list`，确认响应不含 `encryptedSecret`、密码或 OAuth Token。
5. 使用普通 `messages:read` Token 连接，预期得到 HTTP 401。
6. 在 UI 撤销授权码，再次请求，预期得到 HTTP 401。

仓库级自动验证：

```bash
npm run typecheck
npm test
npm run build
npm audit --omit=dev
```

`server/index.test.ts` 覆盖授权拒绝、MCP 初始化、工具清单、工具调用和凭据不泄漏；`server/tokens.test.ts` 覆盖 `imail_mcp_` 格式和 scope 隔离。

## 故障排查

### HTTP 401

- 确认授权码以 `imail_mcp_` 开头且包含 `mcp:full`。
- 确认未超过签发时选择的有效期，且 Token 未在 UI 中撤销。
- 确认请求头是 `Authorization: Bearer <code>`，不要把邮箱服务商授权码放在这里。

### HTTP 403

- 检查请求 Host/Origin 是否在 `MCP_ALLOWED_HOSTS`。
- IPv6 回环可写 `::1` 或 `[::1]`。
- 远程主机名必须显式加入 allowlist；同时启用 HTTPS。

### stdio 启动即退出

- 确认环境变量对启动 `npm` 的同一进程可见。
- 授权码必须来自当前 `IMAIL_DATA_DIR` 对应的数据库。
- 诊断只写 stderr；不要把应用日志写入 stdout，否则会破坏 MCP 帧。

### 工具返回邮箱协议错误

- `IMAP 验证失败` / `SMTP 验证失败` 来自既有协议适配层。
- 对非 OAuth 账户使用 `account_update_authorization_code` 更新服务商授权码。
- 对 OAuth 账户使用 `account_reconnect_oauth` 获取新的官方登录网址。

## 撤销与轮换

MCP 授权码最长 7 天。建议每个 Agent 单独签发并使用可识别名称；任务结束立即撤销。HTTP 每次请求都会重新验证，撤销即时生效。stdio 在启动时验证一次，撤销后还需终止已运行的 stdio 进程。
