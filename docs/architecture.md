# 架构说明

## 后端同步控制面

邮箱同步是后端持久化任务，不以任何前端页面、用户会话、SSE/WebSocket 或 MCP 连接作为生命周期条件。默认启动器同时运行 API 与独立 Worker；外部进程管理模式也可以分别运行二者。

## 应用身份与数据边界

`app_users` 保存应用用户与 scrypt 密码派生值，`app_sessions` 只保存随机会话令牌的 SHA-256 哈希。浏览器使用 HttpOnly、SameSite=Lax Cookie；前端 `AuthGate` 在渲染邮件工作区前检查会话，并在任意数据 API 返回 401 时立即退回登录页。

HTTP 会话、API 网关 Token 与 MCP 授权码都会恢复同一个服务端用户上下文。存储层按该上下文过滤 `accounts.user_id`、`developer_tokens.user_id`、`contacts.user_id` 与 `logo_fetch_attempts.user_id`，邮件和草稿通过所属邮箱账户间接隔离。后台同步不依赖浏览器会话，而是按全局唯一邮箱账户 ID 工作；提交联系人快照时重新取得该账户的用户归属。

设置中心使用同一用户上下文，将 `app_preferences_v1` 保存为 `metadata` 中的用户命名空间键。HTTP `preferences` 路由和 MCP `settings_get` / `settings_update` 因此读取各自应用账号的启动、阅读、通知、邮件展示与快捷键设置，不共享全局设置。

旧数据库行在迁移时先标记为 `__legacy__`。第一个成功注册的应用用户在同一事务中接管这些行，并将旧的全局 `app_preferences_v1` 设置迁入其用户命名空间，避免升级后丢失本地数据与偏好；未完成归属的旧 MCP/API 授权码不会被外部入口接受。

```text
Frontend / MCP ── settings, status, sync-now ──► API
                                                   │
                                       policy / job / event
                                                   ▼
                                                SQLite
                                                   ▲
                                      lease / cursor / result
                                                   │
IMAP providers ◄──────────────────────────── Sync Worker
```

- `server/sync/store.ts`：同步策略、邮箱状态、任务租约、事件、Worker 心跳和积压指标。
- `server/sync/scheduler.ts`：扫描到期策略，合并重复任务并处理启动补同步和退避恢复。
- `server/sync/worker-runtime.ts`：领取任务、续租、执行 IMAP 同步、推进游标并记录安全错误。
- `server/sync/idle.ts`：为可用账户保持收件箱 IDLE 连接，只负责提前唤醒持久化任务；断线不影响周期轮询。
- `server/sync/worker.ts`：独立进程入口和信号关闭。
- `server/routes/sync.ts`：策略、状态、任务查询和前端 SSE 通知。客户端复用单个 SSE 连接；`sync.completed` 携带经过裁剪的邮件摘要增量，前端直接合并新增、标记变化与删除；`sync.status` 在连接、任务状态变化和 Worker 心跳时推送完整运行状态，设置页不再查询 `api/messages` 或 `api/sync-status`。默认策略仍按需单独读取，不参与轮询。浏览器场景是单向服务端推送，因此无需额外引入 MQTT broker。

同步游标按账户和真实邮箱文件夹保存，包括 `UIDVALIDITY`、最后 UID 与 `HIGHESTMODSEQ`。UIDVALIDITY 改变时只重建对应文件夹；支持 CONDSTORE 时按 modseq 获取标记变化，同时显式检查已缓存 UID 是否仍存在。API 的“立即同步”和 MCP `mailbox_sync` 都只创建持久化任务。

现有快照型写入已改为在 `BEGIN IMMEDIATE` 内读取和提交，使 API 与 Worker 的跨进程写入串行化；同步控制表使用细粒度 SQL 事务，不依赖进程内锁。

## MCP 控制面

MCP 是现有本地邮件能力上的受控适配层，不建立第二份邮件状态，也不绕过 IMAP/SMTP 服务边界。

```text
Agent
  │
  └─ Streamable HTTP /mcp ─ Bearer imail_mcp_* ─┐
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
- `server/mcp/server.ts`：注册工具、Zod 参数模型、structured content 和 destructive/read-only annotations。
- `server/tokens.ts`：生成高熵授权码、SHA-256 哈希、常量时间比较、过期与撤销检查。

### 权限模型

`mcp:full` 是独立的管理权限。MCP 入口只接受包含该 scope 的 Token；`messages:read`、`messages:send` 和 `accounts:read` 仍只用于开发者网关。MCP 授权码以 `imail_mcp_` 开头，语义覆盖所属应用用户的全部当前与未来邮箱账户，因此新增邮箱后无需重新签发，也不能访问其他应用用户的数据。

授权码仍使用既有 `developer_tokens`、`developer_token_scopes` 和 `developer_token_accounts` 表，没有新增明文凭据列。`accountIds` 为兼容现有 Token 展示继续写入，但 MCP 管理权限不以创建时账户快照作为访问边界。

### 请求流程

1. HTTP 入口校验 Host，存在 Origin 时同时校验 Origin。
2. 从 `Authorization: Bearer` 提取授权码，经 `authenticateToken(..., 'mcp:full')` 验证。
3. 官方 MCP SDK 的 per-request factory 创建服务实例并完成协议分派。
4. 工具调用既有 `mail`、`oauth`、`store` 和加密能力。
5. 返回文本内容与 `structuredContent`；JSON 序列化会剔除 `undefined`，凭据字段从不进入返回对象。

### 安全取舍

- 服务默认监听 `127.0.0.1`，MCP 再增加 Host/Origin allowlist，降低 DNS rebinding 和浏览器跨站调用风险。
- 远程模式不是默认发布形态；仅配置 `MCP_ALLOWED_HOSTS` 不等于完成远程加固，还需要 HTTPS、管理端认证、审计和速率限制。
- 邮箱服务商授权码只在工具参数和加密流程中短暂存在，不写日志、不返回。
- 发送、远程状态更新、移动和同步复用现有实现，保持 API 与 MCP 的协议行为一致。

## 联系人与发件人 Logo

联系人建议与邮件发件人不维护两套头像状态。每次事务写入会从缓存邮件参与者重建 `contacts`，保留已有 Logo 元数据；`GET /api/contacts` 和邮件列表/详情响应再从同一联系人记录生成 `from.logo`。客户端的邮件列表、阅读页和写信建议统一渲染该字段。`GET /api/contacts/logo?address=...` 是统一的图片读取与首次采集入口，旧的按邮件 ID 路由仅为兼容保留。

### 数据模型

- `contacts`：以不区分大小写的邮箱地址为主键，保存名称、往来次数、最后往来时间，以及可选的 `logo_key`、内容类型、来源网址和获取时间。
- `logo_fetch_attempts`：以网站 origin 为主键，保存所属可注册主域键、成功/失败状态、结果说明和采集时间。记录写入后不会自动删除或重试。
- `.data/sender-logos/`：按 `logo_key` 的 SHA-256 文件名保存图片和缓存元数据。缓存同时支持 `domain:<完整域名>` 子域键和 `domain:<可注册主域>` 兜底键。

### 采集流程与约束

1. 使用公共后缀列表为 `no-reply@accounts.google.com` 生成子域键 `domain:accounts.google.com` 和一级域键 `domain:google.com`。读取时先查子域，未命中再查一级域；一级域命中后直接返回并把该引用写回子域联系人，不发起网络请求。`noreply-accounts@google.com` 直接使用一级域键。
2. 只从邮件 HTML 的真实 `href`、纯文本网址中收集与发件邮箱同一可注册主域的 origin；不会把 HTML namespace、跟踪/退订链接或第三方站点作为候选。
3. 候选依次为邮件内同主域网址、发件邮箱完整域名、可注册主域。每个候选最多请求一次；无论成功、失败、超时或遇到 Cloudflare 等访问验证页，都写入 `logo_fetch_attempts` 并永久跳过后续采集。
4. 页面必须是有限大小的 HTML 且不是访问验证页；图标响应受 SSRF、重定向、端口、大小和图片 magic bytes 校验。全部失败时写入负缓存并显示联系人首字母，不使用未经验证的图标。
5. 若从非一级域成功获取图标，先写入对应子域缓存；一级域尚无有效图标时，再用相同图片补齐一级域缓存。并发任务仍按可注册主域合并，避免同一组织的多个子域同时外连。
6. 联系人优先保存精确子域引用；当精确子域没有图标而一级域已有图标时，保存一级域引用。一级域引用可被同主域联系人共享，后续列表和阅读页不会重复拉取。
