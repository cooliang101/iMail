# Rust 服务迁移 R6 进展报告

日期：2026-08-10
状态：可选 HTTP Adapter 公开端点、安全边界与应用认证控制面完成；R6 未完成

## 本批范围

新增 `imail-http` crate，开始把远程 HTTP 作为 Rust 核心之外的可选桥接。这个 crate 不被 `imail-core`、存储或邮件协议层反向依赖，因此不会把 HTTP 重新变成领域服务的运行前提。

当前已开放以下 Node 兼容端点：

- `GET /api/system/info`
- `GET /api/health`
- `POST /api/system/shutdown`（仅桌面守护控制文件、回环来源和令牌验证通过时）
- `GET /api/providers`
- `GET /api/auth/status`
- `GET /api/auth/session`
- `POST /api/auth/register`
- `POST /api/auth/login`
- `POST /api/auth/logout`
- `GET /api/preferences`
- `PATCH /api/preferences`
- `GET /api/accounts`
- `POST /api/accounts`
- `PATCH /api/accounts/:id`
- `DELETE /api/accounts/:id`
- `POST /api/accounts/:id/connection-test`
- `PUT /api/accounts/:id/credential`
- `PUT /api/accounts/:id/proxy`
- `POST /api/accounts/:id/oauth/reconnect`
- `POST /api/oauth/start`
- `POST /api/oauth/status`
- `GET /api/oauth/:provider/callback`
- `POST /api/accounts/:id/sync`
- `POST /api/accounts/:id/mailboxes/:role/sync`
- `POST /api/accounts/:id/mailboxes/sync`
- `POST /api/mailboxes/:role/sync`
- `POST /api/sync`
- `GET /api/drafts`
- `POST /api/drafts`
- `PUT /api/drafts/:id`
- `DELETE /api/drafts/:id`
- `GET /api/developer-tokens`
- `POST /api/developer-tokens`
- `DELETE /api/developer-tokens/:id`
- `GET /api/external-access`
- `PATCH /api/external-access`
- `GET /api/sync-policy`
- `PATCH /api/sync-policy`
- `GET /api/accounts/:id/sync-policy`
- `PATCH /api/accounts/:id/sync-policy`
- `GET /api/sync-status`
- `GET /api/sync-jobs/:id`
- `GET /api/events`（SSE）
- `GET /api/messages`
- `GET /api/messages/:id`
- `PATCH /api/messages/:id`
- `POST /api/messages/:id/move`
- `GET /api/messages/:id/attachments/:index`
- `GET /api/messages/:id/sender-logo`
- `GET /api/message-stats`
- `GET /api/contacts`
- `GET /api/contacts/logo`
- `GET /api/labels`
- `GET /api/notifications`
- `POST /api/send`
- `GET /api/security/audit-events`
- `POST /api/security/mail-authorization-exports`
- `GET /api/security/mail-authorization-exports/:id`
- `POST /api/security/clear-user-data`

启用 `HttpAdapterConfig.gateway` 后还会挂载：

- `GET /gateway/openapi.json`
- `GET /gateway/docs`
- `GET /gateway/v1/health`
- `GET /gateway/v1/mailboxes`
- `GET /gateway/v1/messages`
- `GET /gateway/v1/mailboxes/:mailbox/messages`
- `GET /gateway/v1/messages/:messageId`
- `GET /gateway/v1/messages/:messageId/attachments/:index`
- `POST /gateway/v1/send`
- `GET /gateway/v1/events`（WebSocket upgrade）

MCP 已以显式 capability 挂到 Rust Router，默认仍不挂载。当前完成无状态 Streamable HTTP 的逐请求 `mcp:full`/归属/用户开关校验、JSON-RPC ping、29 项工具发现和全部现有工具执行路径。设置、主题、账户、OAuth、同步、邮件远端状态/移动/附件/发送、草稿、标签与通知均复用 Rust 领域服务；敏感账户操作复用主密钥、候选连接验证和成功后落库边界，账户 settings/proxy 使用响应白名单。工具发现不再维护第二份手写公开 Schema，而是直接嵌入由 Node 官方 SDK 从 Zod 注册表生成的 `contracts/mcp-tools.json`；Node 测试和 Rust `tools/list` 都逐值校验同一文件。HTTP 头部对齐官方 SDK 的 JSON+SSE Accept、JSON Content-Type 与协议版本拒绝语义，草稿参数在服务端再次执行 Base64/大小/地址边界，管理审计已验证不包含完整 Token。兼容层放行官方 SDK 的 2024-10-07、2024-11-05、2025-03-26、2025-06-18、2025-11-25 版本，保留 `initialize` 和 JSON-RPC 批处理（通知不生成响应），并实现 2026-07-28 `server/discover`、per-request `_meta`、`Mcp-Method`/`Mcp-Name` 头体一致性、禁止批处理、`resultType: complete`、server info，以及可缓存结果约束。官方 TypeScript SDK 已通过真实 HTTP 子进程自动协商 2026-07-28、读取 29 项共享工具并调用 Rust `imail_status`。Gateway 同样只有宿主显式开启时才报告并挂载。

静态 Web 只有配置有效且包含 `index.html` 的根目录后才启用 capability。Rust 直接提供静态文件、`/assets/` 一年 immutable 缓存、HTML `no-cache` 与 CSP，并仅对接受 HTML 的 GET/HEAD 非保留路径执行 SPA fallback；`/api`、`/gateway`、`/mcp` 不会被应用壳吞掉，静态文件 canonical path 必须留在已验证根目录内。正式 `imail-server` 无参数选择 Embedded 并且不触碰数据或监听端口；显式 `--http` 才读取生产 Host/CORS/Gateway/MCP/Web/注册/可信代理环境配置并启动。新数据目录必须为空才会生成随机主密钥和 schema v6；既有目录缺数据库或主密钥会拒绝，既有 v6 数据先验证再启动。宿主使用 Ctrl-C 优雅关闭，独立进程测试已验证新隔离目录、健康端点、capability 与静态 Web。

正式 HTTP 宿主现已把 `PersistentSyncRuntime` 与 `EmbeddedSyncExecutor` 装入同一进程：默认启动 3 个 worker slot、scheduler 和 IDLE watcher，并兼容现有 Node 的 `IMAIL_SYNC_CONCURRENCY`、poll、lease、scheduler、reconcile 与 IDLE 调优变量。HTTP 账户操作和后台执行器共享同一个 `RefreshCoordinator`，不会并发刷新同一 OAuth 账户。只有运行时及主密钥成功初始化后才开始接受请求并报告 `syncWorker=true`；显式 `IMAIL_SYNC_WORKER=false` 才进入无执行器的诊断模式。收到关闭信号后先停止接收 HTTP，再协作取消 worker/scheduler/watcher，并要求 10 秒内清除 worker heartbeat，否则宿主以失败退出。Docker Rust 模式默认启用该运行时；保留快照回读和纯 HTTP 双实现测试则显式关闭，避免测试副本访问真实邮箱。

新增独立 `Dockerfile.rust`，不替换当前正式 Node `Dockerfile` 或正式发布目标。它在 Node build stage 生成 Web 资产，在固定 Rust 1.77.2 builder 中以 `--locked --release` 构建 `imail-server` 与 `imail-maintenance`，最终 Debian runtime 不包含 Node，以 UID/GID 10001 运行并配置 healthcheck、SIGTERM、数据/备份卷和显式 `--http`。维护 CLI 提供 JSON 输出的 `backup`、`restore` 和 `upgrade-preflight`：在线备份继续使用 SQLite backup API 和 v2 SHA-256 清单，恢复只写不存在的新目录，升级预检仅在恢复副本执行迁移、quick check 与外键检查。Rust 单元测试验证三条路径、现有目标拒绝和源数据非覆盖；对应发布机脚本现在先让当前 Node 镜像在唯一命名卷中创建实例身份、用户、偏好和外部访问设置，再停止 Node 并把完全相同的卷交给 Rust 候选。Rust 必须复用 instance identity、完成旧用户登录并逐值读回偏好和开关；随后再次用同一卷重启 Rust，执行三条维护命令，验证恢复目标非覆盖拒绝、真实 healthcheck、只读根文件系统、非 root 用户及两次 Rust SIGTERM。现有 `docker` 发布任务会在登录 GHCR 和发布正式 Node 镜像前执行该候选门禁；候选不会被推送，失败会阻止正式发布。真实 WSL2 Docker/Buildx `linux/amd64` 构建与容器运行已经通过；当前正式镜像入口仍未切换。

## 已实现边界

- `BridgeMode` 默认是 `Embedded`；只有显式 `--http` 才选择 HTTP bridge，`--host` 与 `--port` 不能脱离 `--http` 使用。
- HTTP 默认监听配置为 `127.0.0.1:8787`；允许显式 `--host 0.0.0.0 --port <N>` 为后续 Docker 宿主装配提供参数模型。端口 0、无效 IP、缺值和未知参数均拒绝。
- 显式 `--http` 下，未提供命令行监听值时继续读取现有 `HOST`/`PORT`；命令行值优先。Rust 同时兼容现有 `CORS_ORIGIN`、`IMAIL_TRUST_PROXY`、`IMAIL_REGISTRATION_MODE`，迁移期别名 `IMAIL_CORS_ORIGINS`、`IMAIL_TRUST_PROXY_ONE_HOP`、`IMAIL_REGISTRATION_OPEN` 只作为显式覆盖，不要求现有部署改名。
- `HttpAdapterConfig::new` 是开发模式，包含现有前端开发来源；`HttpAdapterConfig::production` 不继承开发 CORS 来源，生产调用方必须显式配置。
- production Host 白名单忽略端口但要求规范化主机完全匹配；未知或缺失 Host 返回 421。
- CORS 只为严格规范化的精确来源返回凭据响应头；非 loopback HTTP、带路径、认证信息、query 或 fragment 的来源在配置阶段拒绝。
- 代理转发头默认完全不可信。只有宿主显式启用 `with_trusted_proxy_one_hop(true)` 时，Rust 才读取距离服务最近的一项 `X-Forwarded-For` 作为审计/限流来源，并读取第一项 `X-Forwarded-Proto` 判断代理外侧 HTTPS；畸形可信转发头返回 400，未启用时即使伪造这些头也不会改变身份或 Cookie。
- 可信代理报告 HTTPS 时，所有响应追加 `Strict-Transport-Security: max-age=31536000`；未明确信任代理时伪造 `X-Forwarded-Proto` 不会产生 HSTS 或改变 Cookie。
- 所有响应（包括 Host 拒绝）加入 `nosniff`、`DENY` frame、same-origin referrer 和禁用 camera/microphone/geolocation 的安全头。
- `instance-id` 复用现有 v4 UUID；首次创建使用 `create_new` 非覆盖语义并落盘同步。已有身份无效时启动失败且原文件不被替换。
- `/api/system/info` 保持 `Cache-Control: no-store`、camelCase 字段、服务/协议版本和能力对象；响应不包含凭据、数据库路径或内部错误。
- Axum 固定为 0.7.5；HTTP crate 已在项目 MSRV Rust 1.77.2 下编译。
- HTTP 认证直接复用 `SqliteAuthStore` 的 scrypt 密码、SHA-256 session token、30 天过期、持久限流和 HMAC 脱敏审计实现；响应只返回公开用户字段，原始 session 仅进入 HttpOnly Cookie。
- 服务商目录与 Node 保持同一七项账户类型和 Google/Microsoft/Yahoo OAuth 配置提示，只返回 `configured`、redirect URI 与 scope，不返回 Client ID、Client Secret 或其他环境凭据。
- 敏感授权导出和用户数据清理在执行前按来源与用户执行两层持久限流及当前密码复核。导出直接复用 Rust 便携 scrypt + AES-256-GCM 实现，只保留当前用户最新文件、绑定所有者、两分钟过期且只能消费一次；清理使用单事务删除当前用户邮件域数据，保留登录身份、Session、偏好和其他用户数据，并使待下载导出立即失效。
- `--daemon-control-file` 与现有 Tauri 本地守护协议兼容：未配置返回 404、控制文件不可读返回 503、非回环或令牌错误返回 403，授权后返回 202 并进入与 Ctrl-C/SIGTERM 相同的 HTTP、SSE/WebSocket 和同步运行时优雅关闭路径。
- 注册、登录、会话查询和退出的状态码、主要中文错误、`imail_session`、SameSite 与 Max-Age 保持 Node 契约；production Cookie 强制 `Secure`，跨来源 Cookie 使用 `SameSite=None; Secure`。
- production 默认只允许首次初始化注册，显式开放后才允许继续注册。首次用户判断与写入在 SQLite `IMMEDIATE` 事务内复核，并发初始化只会成功一个请求；旧 `__legacy__` 数据归属仍在同一事务中认领。
- 登录按真实 TCP 客户端地址和规范化登录名执行两层持久限流；Axum listener 显式注入 `ConnectInfo<SocketAddr>`，不会把所有远程请求错误合并到 `unknown`。
- 密码哈希、SQLite 与审计操作通过 `spawn_blocking` 离开 Tokio async worker；存储错误只返回固定 500 文案，不把数据库路径、密码、Cookie 或 Token 放入响应。
- 受保护路由统一经过 Session middleware；middleware 从 Cookie 恢复用户后，把可信 `userId` 和 TCP 来源 actor 放入仅存在于 Rust 请求扩展的 `AuthenticatedUser`。业务 DTO 中即使夹带 `userId` 也不会改变调用身份。
- 偏好 GET/PATCH 直接调用 `PreferencesService`，保持默认值、枚举、局部 notification/shortcut 合并、空更新与快捷键长度校验；metadata 继续按 `user:<id>:app_preferences_v1` 隔离。
- 账户 GET/PATCH 直接调用 `AccountService`，账户归属由 Session 用户决定；访问其他用户账户统一表现为 404，客户端请求体不能覆盖 owner。
- HTTP 账户 presenter 是独立白名单 DTO：完全不序列化 `ownerId`、`encryptedSecret`，并把 `settings` 与 `proxy` 再投影到协议允许字段。即使损坏或旧 JSON 中夹带 `password`、`proxyPassword`、Token 字段也不会进入响应。
- 账户删除复用 `AccountService` 与 SQLite 既有级联事务，只能删除当前 Session 用户的账户，并以可信 actor 写入 `account.removed` 安全事件；不会通过清空目录或重建数据库实现删除。
- 手动同步端点只做所有权校验和持久队列入队，不在 HTTP worker 中执行网络同步。账户、角色、自定义文件夹和当前用户全账户入口复用 `SyncRuntimeStore` 去重；标准文件夹会规范化为角色任务，自定义文件夹才保留原路径。
- 草稿 GET/POST/PUT/DELETE 直接复用 `DraftService`，保持 `X-Draft-Id` 幂等创建、创建时间不变、更新时间刷新、更新时间倒序、账户/草稿归属隔离及不存在删除仍返回 204 的 Node 契约。HTTP DTO 同步执行 UTF-16 字段长度、附件数量/单项/总大小校验和缺省字段填充；Router 的 JSON 上限显式对齐 Node 的 25 MB，而不是 Axum 默认 2 MB。
- 默认同步策略按 Session 用户写入 `sync_default_policy:<userId>`；账户策略先校验账户归属，`selectedMailboxes` 只接受账户已发现且可选择的文件夹。PATCH 保持 Node 的局部合并、UTF-16 长度和最多 100 个文件夹约束，停用策略复用存储事务取消排队任务并暂停文件夹状态。
- 同步状态为当前用户每个账户返回策略、文件夹状态和最近 10 个任务；底层使用单账户有界查询，不读取全库快照再过滤。任务详情在响应前再次验证任务所属账户，其他用户任务与不存在任务统一返回 404。
- SSE 保持 Node 的 `connected`、`sync.status`、`Last-Event-ID` 优先于 `after` query、每秒增量查询、事件变更后状态快照和 15 秒状态心跳语义。数据库游标会跨过无权事件但只序列化当前 Session 用户账户的事件；连接 Body 被客户端丢弃后 Rust stream future 一并取消，不遗留独立 interval 任务。
- 密码和代理更新读取数据目录主密钥，复用 `MasterKeyCredentialCodec` 与 `AccountService` 的候选记录验证；真实探针按同一候选配置依次验证 IMAP/SMTP，只有全部成功才写入账户。OAuth 代理更新会先通过共享 `RefreshCoordinator` 和现有 OAuth HTTP Adapter 刷新即将过期的 Token，再构造候选代理，保持 Token 刷新可持久化但失败代理不落库的 Node 行为。
- 连接测试同样通过刷新协调器解析现有授权；连接成功写回 `connected` 并清除旧错误，失败写回 `error` 和经过协议脱敏/UTF-16 截断的安全错误，同时仍以 200 返回公开账户视图。密码、Token、`encryptedSecret` 和代理密码不会进入响应或审计 detail；密码/代理成功更新分别记录 `account.credential-updated` 与 `account.proxy-updated`。
- 普通账户创建沿用 Node 的服务商设置、密码优先级、字段与代理约束；连接候选通过后才用 SQLite `IMMEDIATE` 事务执行 owner 范围的大小写不敏感邮箱唯一写入，并发同邮箱只能创建一份。成功后初始化同步策略并记录 `account.created`；验证失败不留下账户，响应不包含密码、Token 或代理密码。
- OAuth 开始与重连只允许当前 Session 用户调用，复用加密 state、PKCE、nonce 和 10 分钟有效期；state 固定 owner、服务商、重连账户和预期邮箱，客户端不能在 callback 时更换归属。公开 callback 通过可注入 Provider Adapter 换取 Token 和身份，再调用 `OAuthAccountService` 完成新增或原账户重连、连接验证、同步策略与审计。
- OAuth callback 对同一 state 设置进程内 in-flight 门闩和完成记录，重复/并发回调不会二次换 Token；完成状态只对 state 所属 Session 用户返回，并在 10 分钟后清理。回调 HTML 对文本和 `postMessage` JSON 分别转义，只向已校验的前端 Origin 发送，服务商错误与连接错误先脱敏，Token 与密文不进入 HTML、JSON 或审计。
- 邮件列表新增专用 `MessageRepository` 有界查询：用户归属、账户/组、搜索、未读/星标/附件、角色/路径/文件夹名、稍后处理、标签、limit/offset 和总数都在 SQLite 内过滤，正文不会进入摘要响应；详情、统计、联系人、标签和通知同样从可信 Session 用户范围读取，跨用户 ID 统一返回 404。
- 标记、移动、附件下载和发送在 blocking worker 内复用主密钥、`RefreshCoordinator`、OAuth HTTP Adapter 与可注入 IMAP/SMTP 端口。远程标记成功后才提交本地 flags/labels/snooze；远程移动确认后才写目标文件夹、角色和新 UID；失败响应不包含协议凭据且本地缓存保持不变。发送继续执行账户归属、地址、附件数量/单项/总量和 Base64 校验，成功后才删除当前用户对应草稿。
- 附件响应使用实际 MIME 解析结果、UTF-8 `Content-Disposition` 和安全 Content-Type；越权请求在连接邮箱前被拒绝。Logo 端点先验证联系人/邮件归属，可直接复用 Node `sender-logos` 缓存；未命中时通过可注入的 Rust 发现端口抓取同一可注册域的网站图标，并持久化联系人 Logo、采集审计和 24 小时/永久负缓存。
- Logo 抓取仅允许无认证信息的 HTTP/HTTPS 默认端口；每个初始 URL和最多 3 次重定向都重新解析全部地址，任一地址是 loopback、私网、链路本地、CGNAT、组播/保留地址时整次请求拒绝。实际连接固定到已验证的首个地址，同时保留原主机名用于 Host/TLS SNI，避免 DNS rebinding。HTML/图片分别限制为 512 KiB/1 MiB，只接受 PNG/JPEG/GIF/WebP/ICO 图片魔数，Cloudflare 明确挑战 403 记为永久失败；同一主域并发请求由 singleflight 锁合并且结束后清理。
- Developer Token 管理只在应用 Session 下开放。普通 Token 必须至少绑定一个当前用户邮箱并只能使用 `accounts:read`、`messages:read`、`messages:send`；包含 `mcp:full` 的请求会收敛为唯一 `mcp:full` scope 并绑定该用户当前全部账户。列表只返回邮箱地址，不返回原始 Token、SHA-256 哈希、owner 或内部 account ID；创建/撤销审计也不包含授权码。
- `external_access_v1` 继续按用户 metadata 隔离，缺失或损坏值安全回退为 Gateway/MCP 均关闭。有效 Token 在开关关闭时也不能进入 Gateway；开关变更立即影响现有 REST 和 WebSocket 连接。
- Gateway REST 统一使用 Bearer Token scope 和 account ID 白名单，外部 DTO 只暴露邮箱地址。邮件分页由 SQLite 在 owner、授权账户、时间、角色、未读、搜索和不透明 cursor 范围内有界查询；详情、附件和发送会再次验证 Token 账户范围，越权不会触发 IMAP/SMTP。
- Gateway 响应统一生成或复用安全的 `X-Request-Id` 并使用 `Cache-Control: no-store`；错误是 `{error:{code,message,requestId}}`。OpenAPI 3.1 与内置文档只在 Gateway capability 开启时挂载。
- Gateway WebSocket 在握手阶段执行 Host/Origin 边界，支持 Authorization header 或 5 秒内首条 `authenticate` 消息，要求 `messages:read`。连接建立及每次轮询都重新验证 Token 和 Gateway 开关；撤销、过期或关闭后以 policy close 终止。同步运行时只为非首次同步的新邮件写入脱敏 `message.created` 事件，连接仅收到 Token 授权账户的摘要，不含正文、内部账户 ID 或凭据。

## 自动化证据

当前 `imail-http` 有 33 项测试，Rust workspace 共 130 项，覆盖：

1. 默认嵌入与显式 HTTP 启动参数。
2. 公开健康与服务身份响应。
3. 重启复用 instance identity。
4. 无效 identity 非覆盖拒绝。
5. production Host 拒绝及安全响应头。
6. 精确 CORS 与危险配置拒绝。
7. 注册、公开用户响应、持久 Cookie 会话与退出撤销。
8. production 并发首次注册只成功一个请求，以及 Secure/SameSite Cookie。
9. 持久登录限流在密码验证前返回 429 与 `Retry-After`。
10. 未登录偏好拒绝、可信用户注入、伪造 `userId` 无效、跨用户隔离、无效枚举拒绝和更新持久化。
11. 账户列表/元数据更新、跨用户空列表、越权 404、空更新校验，以及 owner、密文和 settings/proxy 明文诱饵不泄漏；同时覆盖跨用户同步/删除拒绝、INBOX 任务去重、全账户结果隔离和删除后的账户/同步任务级联清理。
12. 草稿缺省字段、地址 trim、指定 ID 幂等覆盖且保留创建时间、跨用户列表/更新/删除隔离、附件大小拒绝与所有者删除。
13. 默认策略用户隔离、账户策略继承与局部更新、不可选择文件夹拒绝、跨用户策略/状态/任务隔离、最近任务视图及停用策略取消排队任务；同时覆盖 SSE 初始帧、历史游标回放、跨用户事件过滤、连接后增量事件、状态快照、header 游标优先和可终止 Body stream。
14. 敏感账户变更越权拒绝、OAuth 密码替换冲突、密码/代理候选验证失败整条记录不变、代理密码保留/复制、成功结果解密确认、连接失败状态持久化与错误脱敏，以及恢复后错误清除。
15. 普通账户创建字段/连接校验、密文与同步策略、直接 access token 兼容、并发同邮箱原子唯一；OAuth PKCE 开始、公开 callback、重复回调单次换 Token、owner 状态隔离、响应凭据不泄漏，以及保持账户 ID 的重连闭环。
16. 邮件 SQL 搜索/分页/摘要正文裁剪、详情/统计/联系人/标签租户隔离、缓存 Logo 安全读取、远程 flags 失败不落本地、越权附件不触发 IMAP、MIME 附件流、移动确认与 UID 更新、无效 Base64 不发送且保留草稿，以及成功发送后删除本用户草稿。
17. Logo 公网/私网/保留地址分类与 IPv4-mapped IPv6 拒绝。
18. 同域站点候选、跟踪链接排除及 HTML icon/fallback 发现。
19. 支持图片魔数拒绝 SVG 等非白名单内容，以及 24 小时失败 TTL 边界。
20. 可注入 Logo 发现端口的成功持久化、采集记录、联系人回写及第二次请求只读缓存。
21. 默认忽略伪造转发头，显式一跳代理模式拒绝畸形链并从 HTTPS 转发协议生成 Secure Cookie。
22. 外部访问默认关闭/局部更新/跨用户隔离，普通与 MCP Token 创建、邮箱范围、公开 DTO、使用时间、撤销及 Gateway REST 的开关、scope、分页 cursor、详情、附件和发送闭环。
23. 真实本地 WebSocket 握手的恶意 Origin 拒绝、Bearer 认证、脱敏 `message.created` 推送与在线 Token 撤销关闭。
24. Rust MCP 拒绝恶意 Host 和关闭的用户开关；启用后完成 2025 初始化/批处理、2026 per-request envelope、29 项工具发现和设置/账户/OAuth/同步/邮件工具调用，验证现代头体不一致拒绝、未来新增邮箱的 `mcp:full` 语义、账户字段白名单、凭据不泄漏、所选文件夹约束、持久同步任务、附件/移动/发送及远端失败不提交本地状态。
25. 静态文件 Content-Type、assets immutable 缓存、SPA fallback、HTML CSP/no-cache、保留 API 不被吞掉及编码路径逃逸拒绝。
26. 独立 HTTP listener 收到关闭信号后在有界时间内优雅退出。
27. 正式宿主启动真实 worker/scheduler，执行隔离队列任务、准确报告 capability，并在关闭后移除全部 heartbeat；HTTP 与 worker 共享 OAuth 刷新协调器。
28. 服务商目录字段等价，以及授权导出的当前密码复核、所有者绑定、一次性消费、实际解密内容白名单、用户清理范围、待下载失效和安全审计不泄漏。
29. 真实 Rust 宿主拒绝错误桌面守护令牌，接受正确回环令牌后返回 202，并停止 HTTP 与同步运行时、清除 worker heartbeat。
30. HTTP 注册与自定义账户创建通过真实 TLS IMAP/SMTP 探针后持久化账户，HTTP 手动同步由正式 worker 领取并经真实 IMAP FETCH 写入 SQLite，列表/详情重新读出邮件，`/api/send` 再经真实 SMTP DATA 投递；同步任务成功、公开响应不泄漏凭据/owner、停机后 heartbeat 清空。
31. OAuth 端到端隔离夹具通过真实 HTTP Token/UserInfo、PKCE code verifier、TLS IMAP XOAUTH2 和 TLS SMTP XOAUTH2 完成 callback、连接探测、过期 Token 刷新、worker 同步与发送；刷新后的 access token 加密写回 SQLite，旧 refresh token 被保留，HTTP 状态与 callback HTML 均不泄漏 Token 或密文。
32. 正式持久 worker 首次 TLS IMAP 同步在登录后被 fixture 强制断开，失败状态与安全错误落库且没有部分邮件提交；再次从 HTTP 手动入队后恢复同步，并通过真实 IMAP FETCH 下载 2 MiB MIME 附件、再由真实 SMTP DATA 发送同尺寸附件，双方内容逐字节一致且运行时成功/失败计数准确。
33. 正式 watcher manager 通过真实 TLS IMAP IDLE 首连断开、500ms 有界指数退避重连、`EXISTS` 变化通知、唯一 `recovery` 持久任务、worker FETCH 和 SQLite 提交闭环；运行时 health 精确记录一次断线、一次重连、一个成功任务和零失败任务，停机取消活跃 IDLE 连接时 fixture 接受正常 TLS EOF。

完整门禁结果：Rust workspace 131 项测试通过，另有 1 项显式真实 TLS 长稳验收独立通过；格式检查、全 workspace/all-targets/all-features 严格 Clippy 及 Rust 1.77.2 全 workspace/all-targets/all-features 编译通过。Node 66 个测试文件/332 项通过，TypeScript typecheck、Vite production build 与 `npm audit --omit=dev` 均通过。新增独立 Node 契约/互操作测试覆盖：Node SDK 注册表与共享工具文件逐值一致；官方 TypeScript SDK 对 Rust MCP 的现代协商和真实调用；正式 Rust 服务从空隔离目录启动并使用既有 HOST/PORT/CORS/可信代理/注册变量提供健康、capability、同步运行时、HSTS 与 Web 页面；同一真实宿主还通过 `--daemon-control-file` 完成 403/202 与优雅停服。Node/Rust 应用契约现额外逐值比较服务商目录、按用户清理、清理后账户视图和隐私审计；Rust 容器定义和发布机冒烟脚本继续约束固定 MSRV、无 Node runtime、非 root、只读根文件系统、Node→Rust 同卷迁移、真实 healthy 状态、命名卷重启持久性、纯 Rust 备份/恢复/升级预检、非覆盖恢复和后台同步 capability。该 Linux 容器门禁现已在真实 WSL2 Docker 上执行通过，证据如下。

邮件网络层新增了不经过 trait mock 的 loopback TCP/TLS 验收。测试用仅进程内信任的临时 CA 启动 IMAP 与 SMTP 服务，Rust 生产协议适配器实际完成密码登录、能力/文件夹/状态发现、EXAMINE、带 literal 的增量 FETCH、RFC822/MIME 解析，以及 SMTP EHLO、认证、信封、DATA 和 QUIT；服务端收到的出站字节再次由 Rust MIME 解析器核验。为支持该验收，TLS connector 改为在 `NetworkMailAdapter` 构造时加载并复用，生产信任根没有扩展，测试 CA 不进入运行二进制。该测试证明本地真实 socket/TLS 和第三方协议库互操作，但不替代 Google/Microsoft/Yahoo 等真实服务商的 OAuth、限流和扩展行为门禁。

同一 fixture 现已上移到正式应用链路。`EmbeddedSyncExecutor` 不再硬编码构造网络 adapter，而是持有可注入的 `SyncMailTransportFactory`；生产默认 factory 行为不变，仍为每次任务创建带租约/停机取消探针的 `NetworkMailAdapter`。隔离验收将仅测试进程信任的 connector 同时交给账户连接探针、HTTP 邮件操作和同步 worker，然后实际执行 HTTP 注册、账户创建、手动同步、SQLite 提交、HTTP 查询和发送。由此证明 HTTP bridge 与默认无 HTTP 的同步核心之间没有第二份同步实现；未来 Tauri 宿主可以注入同一 factory 和 executor，而不依赖 Axum handler。

真实协议链路现额外执行确定性故障恢复：账户探针成功后，fixture 在首个 worker IMAP 连接收到登录命令时直接关闭 TLS；任务必须进入 failed、缓存仍为空，随后第二次 HTTP 手动同步通过新连接恢复并提交。恢复邮件带 2 MiB 二进制附件，下载端点会再次经 IMAP FETCH 解码并逐字节核验；同一附件再经 SMTP 发送并从服务端收到的 RFC822 中重新提取核验。该测试覆盖短时断线后的显式恢复和大附件峰值路径，但不把单次 loopback 恢复扩大解释为数小时公共邮箱稳定性。

IDLE watcher 也已由 trait mock 提升到真实 socket/TLS。生产 watcher 重连曲线现在与 Node 对齐为 500ms 起步、指数增长并在 30 秒封顶，成功唤醒后清零连续失败计数。并发 fixture 让首次 watcher 登录连接直接断开，第二次连接完成 `CAPABILITY IDLE`、`EXAMINE`、IDLE continuation 和 `EXISTS`；watcher 将变化写成唯一 recovery 任务，worker 在另一条真实连接上 FETCH 并提交。测试还保持下一条 IDLE 连接活跃并从 runtime shutdown 取消，验证 watcher、worker 和测试服务器都能收敛退出。它证明短时断线窗口内不会依赖前端在线，但仍不替代公共服务商数小时连接和 NAT/代理超时。

同一正式链路现提供 `npm run rust:tls-soak -- <秒> <最大增长MiB> <新报告>` 长稳入口。它默认不运行，内部 test 还要求精确 `isolated-loopback` guard；只接受 30–86400 秒、`output/rust-migration-tests` 下的新 JSON 报告，路径含 `.data`、目标已存在或父目录不存在都会拒绝。验收在系统临时目录创建主密钥与 SQLite，实际启动账户探针、worker、scheduler 和 watcher，首次 IDLE 登录断开后恢复、接收 `EXISTS`、入队 recovery、经另一条 TLS IMAP 连接 FETCH，并在持续 IDLE 期间逐秒记录 RSS、CPU、watcher、queue 和 heartbeat。结束时要求全部任务成功、零失败/取消、零队列/heartbeat 残留与优雅停机。

首份 60 秒 release 运行在第 60 秒触发 scheduler 启动校准，揭示初版门禁把成功任务数错误固定为 1；诊断报告 `r6-real-tls-soak-v1.json` 按非覆盖原则保留。修正后的 `output/rust-migration-tests/r6-real-tls-soak-v2.json` 通过：59 个样本、峰值 RSS 20,897,792 bytes、首尾增长 24,576 bytes（32 MiB 预算内），1 次断线、1 次重连、1 个 recovery 与 3 个 scheduled 任务全部成功，最大 queued 为 0，停机后 worker heartbeat 为 0。v2 SHA-256 为 `A3A363E85F41DA05F935D30DB7CFC86939590679E912F408F769C831BBFBE984`。该入口可用于数小时本地协议栈回归，但本次 60 秒 loopback 证据仍不替代公共服务商、NAT/代理和真实限流的长稳验收。

OAuth 依赖也已从桥接层贯穿到核心执行器。`OAuthProviderPortFactory` 与 `OAuthConfigResolver` 定义在不依赖具体 HTTP 实现的 `imail-oauth` 中，生产默认 resolver 仍只生成 Google、Microsoft 和 Yahoo 的既有批准端点；HTTP callback、账户连接测试、消息操作、worker 与 watcher 使用宿主传入的同一 factory/resolver，不再各自硬编码创建 OAuth HTTP adapter。隔离 fixture 只在测试配置中把端点指向 loopback，生产信任边界和公开配置面没有扩展。这一边界可由后续 Tauri 宿主直接复用。

真实公共服务商门禁现在也有可执行入口，而不再要求操作者手工复制 access token。除交互式 OAuth 单账户入口外，新增 `npm run rust:mail-matrix`：只读解密现有测试数据，在内存刷新 OAuth，要求恰好四个唯一账户，并由 Node 编排层和 Rust 写入层双重强制发送方/收件方都位于同一闭集、禁止自发自收及 CC/BCC。当前桌面数据中的 Gmail、Outlook、QQ、iCloud 均通过配置和凭据预检；执行期间先优雅暂停使用同一邮箱的 Node supervisor，结束后恢复，数据库哈希保持不变。

2026-08-11 的公共投递尝试尚未通过门禁。仓库 `.data` 的 OAuth refresh token 已失效，因此没有发送；改用桌面当前测试数据库后，Gmail→iCloud 与 Gmail→Outlook 各有一封闭集邮件被 SMTP 接受，但接收方在 180 秒内均未由 Rust IMAP 观察到，脚本在第一条失败边停止，未继续其他边。没有闭集外收件人，也没有删除邮件。Modified UTF-7 中文归档目录识别已补齐；失败证据保留为 `r6-public-mail-closed-ring-v1.json`（归档识别诊断）和 `r6-public-mail-closed-ring-v2.json`（投递确认失败）。在查明服务商投递/收件可见性之前不重复发送，也不进入 R7。

本次增量后的完整本地回归为 Rust 133 项通过、1 项显式长稳测试按设计忽略，Node 67 个文件/334 项通过；TypeScript typecheck、production build、严格 Clippy 与 rustfmt 均通过。

后续只读 IMAP 诊断纠正了上述“未投递”判断：两封邮件均存在 Gmail Sent/All Mail，且分别存在 iCloud Junk 与 Outlook Junk，时间和唯一主题一一对应。SMTP 投递成功，失败来自验收驱动只轮询 Inbox。驱动现同时轮询 Inbox 与 Junk，并把实际落点角色写入脱敏报告；此次诊断没有发送、移动、改旗标或删除邮件。v2 继续作为旧门禁误判证据保留，完成四边闭环前 R6 公共邮箱门禁仍未通过。

最终 v4 闭环通过。Gmail→Outlook 使用已存在的唯一主题恢复执行，避免重复发送；Outlook→QQ、QQ→iCloud、iCloud→Gmail 新发三封。四边全部完成附件逐字节往返、flags、取消、重连和归档，接收落点分别为 Junk、Inbox、Junk、Inbox，删除均为 false；报告确认数据库 SHA-256 前后一致。脱敏证据为 `output/rust-migration-tests/r6-public-mail-closed-ring-v4.json`，SHA-256 `1AFC918F4E1E64D98182A7B3C88EBCA2BA8666F3182DA0FA798BDB4FA4D6382D`。旧 Node 服务、worker 与 supervisor 已恢复，R6 公共邮箱门禁完成。

`npm run rust:http-contract` 还会从 Node Router 与 Rust Axum 源码抽取方法/路径，并要求双方逐项等于 `contracts/application-http-routes.json` 中的 59 个应用端点；Gateway 与 MCP 由各自版本化契约单独约束。随后并行启动隔离的 Node/Rust 服务，执行同一套健康、服务信息、认证、服务商目录、偏好、外部开关、空数据读取、账户读取、账户级同步策略、草稿和隐私清理契约。授权导出不只比较随机加密信封结构：测试分别以每个宿主自己的主密钥种入兼容凭据，下载后使用导出密码实际解密，并逐值比较账户配置、密码/代理密码白名单、任意敏感字段排除、错误密码、一次性消费和清理后失效。动态 UUID、随机密文与时间戳只做类型归一化，其余状态码、头部和 JSON 逐值相等。保留的 R5 snapshot 只作为只读复制源：测试先复制到系统临时目录，在副本上启动 Rust，停止后由 Node 读取并启动，再比较账户、邮件、草稿、联系人和 Developer Token 领域摘要；源数据库哈希和源领域摘要前后相同。

真实 Chromium 浏览器验收也已完成：Rust 同源托管当前生产 Web 构建，首次注册、主界面、偏好 PATCH、MCP 开关与一次性授权码创建均成功，重启 Rust 后 Session 可恢复，正常应用流程无控制台错误。独立 Origin 探针验证精确 CORS 允许带凭据登录/会话、未列 Origin 被浏览器拒绝，以及解析到本机但不在白名单的 Host 返回 421。传输增量进一步验证 EventSource 在 Rust 进程停止后断线、重启后自动恢复，以及 Gateway WebSocket 首帧认证和 Token 撤销后 1008 关闭。该验收发现并修复了活跃 SSE 阻塞 Axum 优雅退出的问题：宿主 shutdown 现在广播取消 SSE/WebSocket，自动化测试保持 SSE 不释放并要求服务在 5 秒内退出。真实本地 HTTPS 流式代理还验证了安全 Session Cookie、HSTS、未缓冲 SSE、`wss://` Upgrade、撤销关闭，以及浏览器可访问的 OpenAPI 3.1 文档；关键 OpenAPI 结构已固化为 Rust 自动化断言。证据与限制见 `docs/rust-migration-r6-browser-acceptance.md`。

正式 `imail-maintenance` 二进制也已在隔离 HTTPS 合成数据上逐命令执行，而不只调用内部库。保留证据位于 `output/rust-migration-tests/r6-maintenance-2026-08-10/`，包含 backup、restore、preflight-backup 与 preflight-copy 四个互不覆盖目录；恢复完整性清单、schema v6、quick check 和外键检查均通过。再次写入已有 backup 被拒绝，源合成数据库前后 SHA-256 均为 `6317cd3b7d1efe348f20224bc5a025c073df29a823874eb3e7ab9ce669426ec2`。

Windows production 构建的同构空闲资源对照也已完成。两边启用相同 Web/Gateway/MCP、预热 50 个 HTTP 请求并分别测试仅宿主和 3-worker 完整拓扑；Node 按父子进程树统计。15 秒报告中，Node/Rust 仅宿主峰值分别为 129.46/20.91 MiB，Rust 低 83.85%；完整拓扑峰值分别为 216.11/21.76 MiB，Rust 低 89.93%。四组均退出码 0、无 worker 重启和遗留进程。采样同时发现并修复 Node 打包 worker 入口失效与 Windows 守护停服绕过 graceful handler 两个现存问题。证据、命令和限制见 `docs/rust-migration-r6-resource-comparison.md`。

同一 production 采样器现新增有界 `large-mail` 负载：5 个账户、500 封各含 16 KiB 文本与 HTML 的缓存邮件、单封 2 MiB 详情，每批 8 个并发查询，四组各传输约 41 MiB。采样前分别验证 Node/Rust 列表总数、摘要正文裁剪和详情长度；两边都关闭 IDLE，不触发真实网络。5 秒增量报告中，Rust 仅宿主/3-worker 峰值分别为 25.46/27.40 MiB，Node 为 242.43/610.54 MiB，对应低 89.50%/95.51%。该结果证明大缓存读取下仍保持数量级优势，但短窗口和合成数据不能替代真实附件、同步解析或长稳门禁。

完整拓扑持续负载也已形成独立非覆盖报告：Node/Rust 各执行 60 个负载后采样点、61 批 8 路并发请求，分别返回约 492 MiB，并在结束时重新验证邮件总数、摘要裁剪和详情长度。Rust 峰值/中位 Working Set 为 36.31/28.87 MiB，Node 为 555.77/384.64 MiB，对应低 93.47%/92.49%；两组均退出码 0、无残留进程。Rust 末 10 点中位比首 10 点高 5.64 MiB（20.65%），绝对值仍在低位但窗口不足以判断泄漏，因此只把本轮记作合成持续读取门禁通过，真实网络长稳仍保留在 R6。

真实 Linux 容器门禁已在 `Ubuntu-22.04` WSL2、Docker Server 29.1.3、Docker Buildx/BuildKit 和 `linux/amd64` 上执行通过。脚本先由当前 Node 镜像在唯一命名卷创建实例身份、用户、密码登录、偏好和外部访问设置，再停掉 Node 并让无 Node runtime 的 Rust 候选读取同一 `/data` 卷；随后验证 Rust 重启持久性、Web/Gateway/MCP/sync worker capability、固定非 root 用户、只读根文件系统、healthy、纯 Rust backup/restore、重复 restore 拒绝、upgrade-preflight 及两次 SIGTERM 正常退出。成功后所有临时容器、卷与候选镜像标签均已清理。不可覆盖报告保存在 `output/rust-migration-tests/r6-rust-container-wsl-v1.json`，SHA-256 为 `A46932D90EAC1D535F9D64F5D383AF8C2378A6E341538642CCE5B005B227FFCF`；报告不包含账户、凭据或邮件内容。

所有可能写入的测试仍只使用系统临时目录。当前 `.data` 未被 Rust Router 打开；保留快照仅被只读摘要工具访问和复制，Rust/Node 宿主只打开临时副本。Node 仍是正式数据的唯一写入者。

## R6 剩余工作

- 浏览器级公共服务商真实邮箱 OAuth、发布拓扑有效证书/Caddy 复验与更完整的 Node/Rust 网络契约对照；本地真实 HTTP/TLS OAuth+邮件链路已经通过，但不能替代 Google/Microsoft/Yahoo 的限流、授权策略和服务商扩展验收。
- 真实邮箱浏览器冒烟、公共服务商数据副本的升级/备份恢复，以及公共 IMAP/SMTP 数小时长稳资源与吞吐对照；隔离的 Node→Rust 容器卷迁移、备份恢复、升级预检和 60 秒真实 TLS 运行时长稳已经通过，并已有最长 24 小时的可执行入口。

这些项目完成前不得把 Rust HTTP 健康端点当作可替换 Node 服务，也不得切换桌面或 Docker 正式运行路径。
