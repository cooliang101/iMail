# 运维手册

## 服务环境变量

| 变量 | 默认值 | 用途 |
| --- | --- | --- |
| `MCP_ALLOWED_HOSTS` | `localhost,127.0.0.1,::1` | HTTP MCP 允许的 Host 与 Origin 主机名，逗号分隔 |
| `HOST` | `127.0.0.1` | iMail API 监听地址；远程监听会扩大所有 API 的暴露面 |
| `PORT` | `8787` | API 与 `/mcp` 端口 |
| `IMAIL_SYNC_WORKER_MODE` | `child` | `child` 由 API 启动器监管；`external` 由外部管理器运行；`disabled` 仅用于诊断 |
| `IMAIL_SYNC_CONCURRENCY` | `3` | Worker 最大并发同步任务数，范围 1–10 |
| `IMAIL_SYNC_WORKER_POLL_MS` | `1000` | Worker 领取任务间隔，最小 250ms |
| `IMAIL_SYNC_SCHEDULER_INTERVAL_MS` | `5000` | 到期策略扫描间隔，最小 5 秒 |
| `IMAIL_SYNC_STARTUP_DELAY_MS` | `1000` | 服务启动后的首次补同步延迟 |
| `IMAIL_SYNC_JOB_LEASE_MS` | `120000` | 任务租约时间，最小 10 秒；执行中会自动续租 |
| `IMAIL_SYNC_IDLE_ENABLED` | `true` | 是否启用收件箱 IMAP IDLE 实时唤醒；关闭后仍按周期轮询 |
| `IMAIL_SYNC_IDLE_RECONCILE_MS` | `5000` | IDLE 连接期望状态检查与断线重建间隔，最小 5 秒 |
| `IMAIL_SYNC_IDLE_REFRESH_MS` | `60000` | IDLE 保活刷新周期；不支持 IDLE 的服务商以此间隔执行 STATUS 兜底，最小 15 秒 |

## 本地启动

Streamable HTTP：

```bash
npm run dev
```

默认模式会由 API 启动器拉起并监管 Worker。需要由 systemd、Docker Compose 等分别管理进程时：

```powershell
$env:IMAIL_SYNC_WORKER_MODE='external'
npm start
npm run worker
```

## 冒烟检查

1. 在“外部接入”的“MCP”标签页签发 `mcp:full` 授权码。
2. 用 MCP Inspector 或任意标准客户端连接 `http://127.0.0.1:8787/mcp`。
3. 确认 `tools/list` 包含 `accounts_list`、`messages_list`、`message_send`、`account_remove`、`theme_custom_get` 和 `theme_custom_update`。
4. 调用 `imail_status` 与 `accounts_list`，确认响应不含 `encryptedSecret`、密码或 OAuth Token。
5. 使用普通 `messages:read` Token 连接，预期得到 HTTP 401。
6. 在 UI 撤销授权码，再次请求，预期得到 HTTP 401。
7. 请求 `GET /api/sync-status`，确认 `worker.workers` 至少有一个十秒内更新的心跳。
8. 向测试邮箱发送一封新邮件，在不点击“立即同步”的情况下确认数秒内出现；服务日志不应持续出现 `[sync-idle]` 重连错误。
9. 关闭浏览器，等待一个同步周期后再次查询，确认 `lastSuccessAt` 和 `nextSyncAt` 继续推进。
10. 调用 `theme_custom_update` 写入测试主题，再用 `theme_custom_get` 读取并确认相等；`GET /api/preferences` 不应出现 `customTheme`。

仓库级自动验证：

```bash
npm run typecheck
npm test
npm run build
npm audit --omit=dev
```

`server/index.test.ts` 覆盖授权拒绝、MCP 初始化、工具清单、工具调用和凭据不泄漏；`server/tokens.test.ts` 覆盖 `imail_mcp_` 格式和 scope 隔离。

## 故障排查

### 邮件没有自动同步

- 请求 `GET /api/sync-status`；先检查账户策略的 `enabled`、邮箱状态的 `nextSyncAt`，以及 `worker.workers[].heartbeatAt`。
- `queuedJobs` 持续增加但没有新心跳，说明 Worker 未运行。默认 `child` 模式查看 API 控制台中的 `[sync-worker]` 日志；外部模式确认 `npm run worker` 或对应服务单元已启动。
- `connectionStatus=authRequired` 时自动重试会暂停，应在邮箱设置中重新授权或更新凭据；验证成功后调度器会恢复该账户。
- `syncState=backoff` 表示网络或服务商错误，按 1、5、15、30、60 分钟退避。不要通过频繁点击手动同步绕过服务商限流。
- 持续出现 `[sync-idle]` 表示长连接无法稳定建立或被服务商/网络设备关闭；Worker 会按 0.5–30 秒退避重连，同时保留一分钟周期同步兜底。不支持 IDLE 的服务器会按 `IMAIL_SYNC_IDLE_REFRESH_MS` 执行 `STATUS`。
- 前端 SSE 仅用于刷新界面；断开 SSE 不会影响 Worker。不要把网关订阅状态当成同步健康指标。

### 发件人 Logo 缺失或错误

- 查看服务端 `[sender-logo]` 日志；每行包含 `success` / `failed`、目标 origin、共享主域键和结果说明。访问验证页会记录为失败并回退到联系人首字母。
- 持久化审计在 `.data/imail.sqlite` 的 `logo_fetch_attempts` 表，联系人当前引用在 `contacts.logo_key` 等字段；图片文件位于 `.data/sender-logos/`。
- 同一 origin 有任何采集记录后都不会自动重试，失败主域的负缓存也不会过期。这是防止高频访问和错误图标的安全边界。
- 若管理员确认旧版本缓存了错误图标，需要在 iMail 停止后备份 `.data`，再同时删除该主域对应的 `contacts` Logo 字段、`logo_fetch_attempts` 记录以及 `.data/sender-logos/` 中对应缓存文件。三处必须一起处理；不要只删图片，否则仍会因审计记录而跳过采集。
- 不要通过定时任务批量清空采集记录。重新采集属于显式运维动作，执行前应确认目标站允许自动访问。

### HTTP 401

- 确认授权码以 `imail_mcp_` 开头且包含 `mcp:full`。
- 确认未超过签发时选择的有效期，且 Token 未在 UI 中撤销。
- 确认请求头是 `Authorization: Bearer <code>`，不要把邮箱服务商授权码放在这里。

### HTTP 403

- 检查请求 Host/Origin 是否在 `MCP_ALLOWED_HOSTS`。
- IPv6 回环可写 `::1` 或 `[::1]`。
- 远程主机名必须显式加入 allowlist；同时启用 HTTPS。

### 工具返回邮箱协议错误

- `IMAP 验证失败` / `SMTP 验证失败` 来自既有协议适配层。
- 对非 OAuth 账户使用 `account_update_authorization_code` 更新服务商授权码。
- 对 OAuth 账户使用 `account_reconnect_oauth` 获取新的官方登录网址。

## 撤销与轮换

MCP 授权码最长 7 天。建议每个 Agent 单独签发并使用可识别名称；任务结束立即撤销。Streamable HTTP 每次请求都会重新验证，撤销即时生效。
