# MCP 接入指南

iMail 为可信 Agent 提供 Streamable HTTP 与 stdio 两种 MCP 接入。两者暴露同一组工具，并使用开发者网关签发的短期 `mcp:full` 授权码。普通网关 Token 不能调用 MCP。

## 1. 签发授权码

1. 启动 iMail，进入“开发者网关”。
2. 点击“创建临时 Token”，勾选“MCP 完整控制”。
3. 选择有效时间并创建，立即复制以 `imail_mcp_` 开头的授权码。
4. 用完后在同一页面撤销。

完整授权码只显示一次，SQLite 中仅保存 SHA-256 哈希。授权码最长有效 7 天，可以管理全部当前与未来邮箱。没有任何邮箱时也可签发，用于让 Agent 接入第一个账户。

## 2. Streamable HTTP

推荐同一台设备上的 Agent 使用：

```text
URL: http://127.0.0.1:8787/mcp
Authorization: Bearer imail_mcp_xxx
```

通用配置：

```json
{
  "url": "http://127.0.0.1:8787/mcp",
  "headers": {
    "Authorization": "Bearer ${IMAIL_MCP_AUTH_CODE}"
  }
}
```

服务兼容 2025-era 客户端，并支持 SDK v2 的 2026-07-28 协议。旧协议调用可能使用 SSE 格式返回单次结果；客户端应交给 MCP SDK 处理，不要自行假定响应一定是普通 JSON。

## 3. stdio

本地进程型 Agent 使用：

```powershell
$env:IMAIL_MCP_AUTH_CODE='imail_mcp_xxx'
npm run mcp
```

通用配置：

```json
{
  "command": "npm",
  "args": ["run", "mcp"],
  "cwd": "C:/absolute/path/to/imail",
  "env": {
    "IMAIL_MCP_AUTH_CODE": "imail_mcp_xxx"
  }
}
```

stdio 进程只向 stdout 写 MCP 帧，诊断写 stderr。启动时会验证授权码；无效、过期、撤销或缺少 `mcp:full` 权限时立即失败。

## 4. 工具速查

| 领域 | 工具 | 说明 |
| --- | --- | --- |
| 状态 | `imail_status` | 账户、邮件、未读、草稿和最近同步概览 |
| 账户 | `accounts_list` | 非敏感账户元数据与文件夹 |
| 账户 | `account_add_with_code` | 用服务商授权码/应用专用密码添加 IMAP/SMTP 账户 |
| 账户 | `account_start_oauth` | 开始 Gmail、Outlook、Hotmail 或 Yahoo OAuth |
| 账户 | `account_reconnect_oauth` | 为已有 OAuth 账户生成重新授权网址 |
| 账户 | `account_update` | 更新名称、工作空间、图标和颜色 |
| 账户 | `account_update_authorization_code` | 验证并替换非 OAuth 账户凭据 |
| 账户 | `account_test_connection` | 验证已保存的 IMAP/SMTP 凭据 |
| 账户 | `account_remove` | 移除账户及其本地缓存和草稿 |
| 同步 | `mailbox_sync` | 为单个/全部账户的特殊或自定义文件夹创建持久化同步任务 |
| 同步 | `sync_policy_get` | 读取默认/账户级策略、邮箱状态和最近任务 |
| 同步 | `sync_policy_update` | 更新默认或账户级后端同步策略 |
| 邮件 | `messages_list` | 分页和多条件查询本地缓存 |
| 邮件 | `message_get` | 完整正文、HTML、标签与附件元数据 |
| 邮件 | `message_update` | 已读、星标、标签和稍后处理 |
| 邮件 | `message_move` | 归档或移至垃圾箱，并写回 IMAP |
| 邮件 | `message_send` | 文本/HTML 发信及 Base64 附件 |
| 附件 | `attachment_download` | 从 IMAP 下载并返回 Base64 内容 |
| 草稿 | `drafts_list` / `draft_get` | 查询草稿摘要或完整内容 |
| 草稿 | `draft_save` / `draft_delete` | 新建、覆盖或删除本地草稿 |
| 整理 | `labels_list` / `notifications_list` | 标签与连接/未读/稍后通知 |

## 5. 推荐工作流

- 操作账户前先调用 `accounts_list`，使用邮箱地址定位，不猜内部 ID。
- 操作邮件前先调用 `messages_list` 或 `message_get`，确认发件人、主题和目标邮箱。
- `mailbox_sync` 返回 `jobId` 和排队状态；任务由独立 Worker 执行，调用方可用 `sync_policy_get` 查看状态，不应依赖 MCP 连接存活。
- 发送邮件前确认 `accountEmail`、收件人、主题和正文；发送不是幂等操作。
- `account_remove`、`message_move` 和 `draft_delete` 带 destructive annotation，执行前应获得用户确认。
- 添加 QQ、iCloud 等账户时，把服务商生成的授权码传给 `account_add_with_code.authorizationCode`；不要把 iMail 的 `imail_mcp_` 授权码误当成邮箱凭据。

## 6. 安全约束

- MCP 响应不返回邮箱授权码、密码、OAuth Token、主密钥或 `encryptedSecret`。
- HTTP 默认限制 Host/Origin 为回环地址；远程部署必须配置 HTTPS 和 `MCP_ALLOWED_HOSTS`。
- 附件上传总大小限制 15 MB，工具参数和邮件正文继续受现有 Zod 限制。
- 授权码撤销或过期后，后续 HTTP 请求和新 stdio 进程都会拒绝认证。

更多运维和冒烟检查见 [operator-runbook.md](operator-runbook.md)，内部实现见 [architecture.md](architecture.md)。
