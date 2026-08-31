# MCP 接入指南

iMail 默认关闭 MCP 调用。先在“外部接入 → MCP”中手动启用；开关即时生效、按应用账号保存，且不会同时启用独立的 API Gateway。

iMail 的 Rust 服务可选提供 Streamable HTTP MCP，使用“外部接入”页面签发的短期 `mcp:full` 授权码。Windows 桌面本地模式进入该页面后会启动仅监听 `127.0.0.1` 随机端口的进程内 HTTP Adapter，页面会显示本次应用运行期的实际地址；普通 API 网关 Token 不能调用 MCP。

每个 MCP 授权码都归属于创建它的应用账号。`mcp:full` 表示管理该应用账号当前及未来接入的全部邮箱，不会越过应用账号边界读取其他用户的数据。

设置、主题、自动同步设置和邮箱账户管理工具的调用会写入当前应用账号的安全审计。审计只保存工具名与授权码 ID，不保存工具参数、完整授权码或邮箱凭据。

## 1. 签发授权码

1. 启动 iMail，进入“外部接入”，切换到“MCP”标签页并启用 MCP 接入。
2. 点击“创建 MCP 授权码”。
3. 选择有效时间并创建，立即复制以 `imail_mcp_` 开头的授权码。
4. 用完后在同一页面撤销。

完整授权码只显示一次，SQLite 中仅保存 SHA-256 哈希。授权码最长有效 7 天，可以管理全部当前与未来邮箱。没有任何邮箱时也可签发，用于让 Agent 接入第一个账户。

## 2. Streamable HTTP

连接页面显示的本机地址，或显式启用 MCP 的远程 Rust HTTP 服务：

```text
URL: https://mail.example.com/mcp
Authorization: Bearer imail_mcp_xxx
```

开发时可以显式运行 `imail-server --http --host 127.0.0.1 --port 8787` 使用回环 HTTP；生产和跨设备访问必须使用 HTTPS。

通用配置：

```json
{
  "url": "https://mail.example.com/mcp",
  "headers": {
    "Authorization": "Bearer imail_mcp_xxx"
  }
}
```

将示例值替换为创建后只显示一次的完整 MCP 授权码。

服务兼容 2025-era 客户端，并支持 SDK v2 的 2026-07-28 协议。旧协议调用可能使用 SSE 格式返回单次结果；客户端应交给 MCP SDK 处理，不要自行假定响应一定是普通 JSON。

## 3. 工具速查

写信与会话的完整边界见[写信与会话](composition-and-conversations.md)。

- `message_send` / `draft_save` 新增可选 `bcc: string[]`、`inReplyTo: string[]`、`references: string[]`，每组最多 100 项。发送时 To/Cc/Bcc 合计至少一人；Bcc 不写入投递 MIME。
- `conversation_get` 输入 `{ "messageId": "记录 ID" }`，返回按时间排列的 `messages` 摘要，不含正文。使用每条摘要的 `id` 调用 `message_get`；不要把它与 RFC Message-ID 混淆。
- `settings_update.composition` 为当前用户整体替换写信配置，包含 `signatures` 与 `templates` 数组。签名字段为 `accountId`、`text`、`newMessages`、`replies`；模板字段为 `id`、`name`、`subject`、`text`。签名账户 ID 可从 `accounts_list` 取得，且必须属于当前用户。
- 签名和模板保存的是纯文本；调用发送/草稿工具时不会由服务端隐式追加签名或展开模板。客户端或 Agent 应先读取配置，生成明确正文，再保存或发送，避免重复追加。
- 会话查询仅向登录应用用户和 `mcp:full` 提供；Gateway 不增加跨邮箱会话聚合入口，保持 Token 的邮箱授权边界。

| 领域 | 工具 | 说明 |
| --- | --- | --- |
| 状态 | `imail_status` | 账户、邮件、未读、草稿和最近同步概览 |
| 设置 | `settings_get` / `settings_update` | 读取或更新主题、启动、阅读、通知、邮件展示、写信签名/模板与快捷键偏好 |
| 主题 | `theme_custom_get` / `theme_custom_update` | 读取或保存经过校验的自定义主题令牌，不接受任意 CSS |
| 账户 | `accounts_list` | 非敏感账户元数据与文件夹 |
| 账户 | `account_add_with_code` | 用服务商授权码/应用专用密码添加 IMAP/SMTP 账户 |
| 账户 | `account_start_oauth` | 开始 Gmail、Outlook、Hotmail 或 Yahoo OAuth |
| 账户 | `account_reconnect_oauth` | 为已有 OAuth 账户生成重新授权网址 |
| 账户 | `account_update` | 更新名称、工作空间、图标和颜色 |
| 账户 | `account_update_authorization_code` | 验证并替换非 OAuth 账户凭据 |
| 账户 | `account_test_connection` | 验证已保存的 IMAP/SMTP 凭据 |
| 账户 | `account_proxy_update` | 启用、修改、关闭或从另一邮箱复用账户级 HTTP/HTTPS/SOCKS5 代理 |
| 账户 | `account_remove` | 移除账户及其本地缓存和草稿 |
| Apple HME | `apple_hme_status` | 查看 Apple Account 与 iCloud Web 授权状态 |
| Apple HME | `apple_hme_start_login` | 使用 Apple 密码开始 SRP 网页授权 |
| Apple HME | `apple_hme_submit_two_factor` | 提交 6 位双重认证验证码 |
| Apple HME | `apple_hme_list` | 读取 iMail 本地持久化的 Hide My Email 地址 |
| Apple HME | `apple_hme_sync` | 从 Apple 手动同步地址并保存到本地 |
| Apple HME | `apple_hme_create` | 创建 Hide My Email 地址 |
| Apple HME | `apple_hme_deactivate` | 停用 Hide My Email 地址 |
| Apple HME | `apple_hme_delete` | 永久删除已经停用的地址 |
| Apple HME | `apple_hme_disconnect` | 删除 iMail 本地加密会话 |
| 同步 | `mailbox_sync` | 为单个/全部账户的特殊或自定义文件夹创建持久化同步任务 |
| 同步 | `sync_policy_get` | 读取默认/账户级自动同步设置、邮箱状态和最近任务 |
| 同步 | `sync_policy_update` | 更新自动同步开关、文件夹范围与失败通知 |
| 邮件 | `messages_list` | 分页和多条件查询本地缓存 |
| 搜索 | `smart_folders_list` | 列出当前用户保存的查询定义 |
| 搜索 | `smart_folder_save` | 用 `name` 和 `filters` 新建；提供 `folderId` 时更新单个查询 |
| 搜索 | `smart_folder_delete` | 删除保存的查询定义，不删除邮件 |
| 规则 | `mail_rules_list`、`mail_rule_save`、`mail_rule_delete` | 列出、创建/更新、删除当前用户的邮件规则 |
| 规则 | `mail_rule_preview`、`mail_rule_apply` | 只读预览；使用一次性令牌和 `confirmed: true` 单独确认处理历史邮件 |
| 规则 | `mail_rule_runs`、`mail_rule_retry` | 最近 200 条执行记录；重试失败的未完成动作，不重放结果不确定的归档；详见[邮件规则引擎](mail-rules.md) |
| 邮件 | `message_get` | 完整正文、HTML、标签与附件元数据 |
| 邮件 | `conversation_get` | 按明确回复头读取本地会话摘要；保留账户/文件夹副本，不修改已读状态 |
| 邮件 | `message_update` | 已读、星标、标签和稍后处理 |
| 邮件 | `message_move` | 归档或移至垃圾箱，并写回 IMAP |
| 邮件 | `message_send` | 文本/HTML 发信、Bcc、回复关联头及 Base64 附件 |
| 翻译 | `translation_profiles_list` | 列出当前用户可用翻译 Profile 与状态，不返回凭据原文 |
| 翻译 | `message_translate` | 使用已配置的服务端 Profile 翻译可见正文；不支持 WebView 本地 Profile |
| 附件 | `attachment_download` | 从用户隔离的本地缓存读取；未命中时从 IMAP 下载、写入缓存并返回 Base64 内容 |
| 草稿 | `drafts_list` / `draft_get` | 查询草稿摘要或完整内容 |
| 草稿 | `draft_save` / `draft_delete` | 新建、覆盖或删除本地草稿；Bcc 与回复关联头随草稿保存 |
| 整理 | `labels_list` / `notifications_list` | 标签与连接/未读/稍后通知 |

`settings_update.theme` 接受 `mint-fresh`、`tech`、`business-blue`、`soft-neubrutalism` 或 `constructivist-red`；未提供该字段时保持当前主题。

应用界面的图片、PDF、视频和 ZIP 安全预览使用短期登录会话 API，不属于 MCP 控制面。MCP 调用方继续使用 `attachment_download` 获取原始 Base64 内容，并自行决定后续展示方式。

`theme_custom_update` 接受 9 个 `#RRGGBB` 颜色字段以及受限的圆角、阴影和字体枚举。它与 `/api/preferences` 的 `customTheme` 共用用户级安全主题存储；返回的 `theme` JSON 可直接粘贴到“设置 → 主题 → 自定义主题”。两条控制面都拒绝任意 CSS、URL、透明色与额外字段。完整生成约束见 [`custom-theme.md`](custom-theme.md)。

## 4. 推荐工作流

高级搜索：`messages_list.filters` 接受账户、主题、正文、To/Cc、日期和状态等组合条件对象。先用 `smart_folders_list` 读取定义，再把所选文件夹的 `filters` 传入 `messages_list` 执行动态查询。条件与既有参数及授权范围取交集；正文仅覆盖本实例缓存，不搜索附件。完整语义、限制和维护方式见[高级搜索与智能文件夹](advanced-search.md)。

- 操作账户前先调用 `accounts_list`，使用邮箱地址定位，不猜内部 ID。
- 操作邮件前先调用 `messages_list` 或 `message_get`，确认发件人、主题和目标邮箱。
- `mailbox_sync` 返回 `jobId` 和排队状态；任务由 Rust 持久 worker pool 执行，调用方可用 `sync_policy_get` 查看状态，不应依赖 MCP 连接存活。
- `sync_policy_update` 只接受 `enabled`、`folderMode`、`selectedMailboxes` 和 `notifyOnError`。IMAP 推送负责变化唤醒，启动、重连与低频一致性校准由服务自动执行，不提供按账户分钟频率或恢复重试开关。
- `mailbox_sync` 和 `messages_list` 的 `mailboxRole` 支持 `inbox`、`sent`、`archive`、`drafts`、`trash`、`junk` 与 `custom`；常见的 Drafts、Deleted Items/Message(s)、Junk/Spam 等无 Special-Use 标记文件夹也会归入对应标准角色。
- 发送邮件前确认 `accountEmail`、收件人、主题和正文；发送不是幂等操作。
- 调用 `message_translate` 前先用 `translation_profiles_list` 选择状态可用、执行位置为本机或远程服务的 Profile。云端与 Bing Web Profile 只有在应用内完成对应隐私披露同意后才能执行；Edge 本地模型只存在于 WebView，MCP 不会静默换用其他服务。
- `account_remove`、`message_move` 和 `draft_delete` 带 destructive annotation，执行前应获得用户确认。
- 使用 Gmail、Outlook、Hotmail、QQ、Yahoo 或 iCloud 的应用专用密码/授权码时，把服务商生成的凭据传给 `account_add_with_code.authorizationCode`；Microsoft 账户还必须允许 IMAP/SMTP 密码验证。不要把 iMail 的 `imail_mcp_` 授权码误当成邮箱凭据。
- `account_add_with_code.proxy` 可在添加时设置代理；已有账户使用 `account_proxy_update`。`protocol` 仅接受 `http`、`https`、`socks5`，关闭时只需传 `enabled: false`。修改代理且省略 `password` 会保留已加密的现有代理密码；传入 `enabled: true` 与 `sourceEmail` 可复制另一邮箱的代理和加密密码，复制后两边可独立修改。

## 5. 安全约束

- MCP 响应不返回邮箱授权码、邮箱/代理密码、OAuth Token、主密钥或 `encryptedSecret`。
- 翻译 API Key 与 Google Service Account 只在 Rust 服务内从 `master.key` 加密存储解密。`message_translate` 只返回译文分段、语言、Profile 与时间，不返回原文、用户 ID、正文哈希、凭据或供应商原始错误正文；调用审计只记录工具名与授权码 ID。
- 邮件翻译默认排除引用历史；云端 Profile 会把剩余可见正文发送给所选供应商。Bing Web 是非官方实验协议，默认关闭且不会作为其他 Provider 失败后的自动降级。清除当前用户邮箱数据会同时删除翻译 Profile、加密凭据与译文缓存。
- Apple HME 工具只允许操作属于当前应用用户的 `icloud` 邮箱。Apple 主密码只用于当前 SRP 请求；会话 Cookie、`scnt`、Session Token 和 API Key 使用 `master.key` 加密保存在 `apple_hme_sessions`，任何 HTTP/MCP 响应都只返回连接状态。完整管理通常需要分别调用 `apple_hme_start_login` 完成 `appleAccount` 与 `icloudWeb` 两类授权。`apple_hme_list` 只读本地缓存；显式调用 `apple_hme_sync` 才会访问 Apple 并以事务替换本地地址快照。
- `apple_hme_delete` 只接受已停用地址；活动地址必须先调用 `apple_hme_deactivate`。`apple_hme_disconnect` 只删除本地会话，不影响 Apple 端已有地址。
- 本地 Gateway 的 `GET /gateway/v1/mailboxes/{mailbox}/messages` 接受已持久化的 iCloud Hide My Email 地址。服务只在 Token 已授权其所属 iCloud 账户时解析该地址，并按邮件的实际收件人过滤；响应中的 `accountEmail` 使用请求的隐私邮箱，不暴露主 iCloud 地址。停用地址仍可查询历史邮件，HME 地址不作为 SMTP 发件身份。
- 完整 EML / RFC 822 原始字节只通过已登录应用的 `GET /api/messages/:id/source` 按需读取，不进入邮件列表、Gateway 或 MCP 响应，避免完整邮件头与附件内容被 Agent 授权面意外扩大。
- “设置 → 隐私与数据”的邮箱授权导出只属于登录会话保护的应用 HTTP UI。即使授权码具有 `mcp:full`，MCP 也不能创建或下载该文件；API Gateway 同样不提供该能力。
- 清除当前用户邮箱数据也不作为 MCP 或 Gateway 工具提供。需要执行时，用户必须在应用 UI 中完成两阶段确认、当前 iMail 密码复核和固定确认文字。
- HTTP 默认限制 Host/Origin 为回环地址；远程部署必须配置 HTTPS 和 `MCP_ALLOWED_HOSTS`。
- 附件上传总大小限制 15 MB，工具参数和邮件正文继续受现有 Zod 限制。
- 授权码撤销或过期后，后续 HTTP 请求会立即拒绝认证。
- 旧版本升级前创建、尚未绑定应用账号的授权码不会被 MCP 接受；请登录后重新签发。

更多运维和冒烟检查见 [operator-runbook.md](operator-runbook.md)，内部实现见 [architecture.md](architecture.md)。
