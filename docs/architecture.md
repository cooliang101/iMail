# 架构说明

## 服务与客户端边界

iMail 的 Rust 领域服务独立拥有邮箱凭据、SQLite、同步任务、Web API、Gateway 与 MCP 能力。通用能力统一位于根 workspace 的 `crates/`：Tauri 本地模式通过应用桥接直接调用，独立部署模式由 `http-service/` 的薄启动器挂载到 HTTP，两者不维护第二套业务实现。

桌面应用提供本地与远程两种模式。本地模式通过类型化 Tauri command/event 直调 Rust，不保存服务 URL、不使用 Cookie/SSE，也不开放常驻业务端口；窗口隐藏到托盘后同步继续，显式退出才停止。远程模式经受控 Rust 网络桥请求用户配置的 HTTPS 地址，WebView 不直接接触服务端 Cookie。模式切换只改变 adapter 和数据源，不复制、合并或迁移数据，远程连接失败也不自动回退。

当前支持的交付平台仅为 Windows x64 桌面端与服务端 Docker 镜像。Linux 只作为 Docker 运行环境，不维护原生安装或桌面包；macOS 桌面构建与发布不进入当前支持矩阵。

本地模式没有端口、暂停或移除守护程序的控制面。卸载默认保留应用数据；删除邮箱数据只能从“隐私与数据”执行。`127.0.0.1:8787` 只用于本机 HTTP 开发服务，不属于桌面本地模式。

桌面本地模式只在需要 Gateway 或 MCP 外部接入时启动回环 HTTP Adapter。首次由操作系统分配可用端口，成功端口记录在会随升级和默认卸载保留的本地服务数据目录。后续进程优先重新绑定该端口，只有端口已被占用时才回退到新的系统分配端口并更新记录。该端口同时服务 Gateway 与 MCP，不为两个控制面分别监听。

Windows 桌面 OAuth 使用系统浏览器、authorization code + PKCE 和单次临时 `localhost` callback listener；端口由操作系统动态分配，该 listener 不承载业务 API。远程服务通过同一 OAuth 引擎显式配置 HTTPS `OAUTH_CALLBACK_BASE_URL` 与 Web Client 凭据。

远程服务发布单元同时托管 Web 客户端，浏览器默认同源访问 API；需要跨源部署时才使用 `CORS_ORIGIN`。桌面 WebView 始终加载安装包内的前端资源。`http-service/` 只负责独立进程启动和部署，通用 Web API/MCP/Gateway 实现仍由 `crates/imail-http/` 提供。当前部署边界见[部署模式](deployment-modes.md)。

Web 生产构建注册独立 Service Worker：带内容哈希的 JS、CSS、字体和图片采用缓存优先，页面导航采用网络优先并回退到已缓存应用外壳。`/api`、`/gateway`、`/mcp` 与 `text/event-stream` 请求始终绕过缓存；Tauri 运行时不注册 Service Worker。

## 后端同步控制面

邮箱同步是 Rust 服务中的持久化任务，不以页面、用户会话、SSE/WebSocket 或 MCP 连接作为生命周期条件。本地嵌入式 worker/scheduler/IDLE watcher 随 Tauri 托盘进程运行；远程 Rust 服务在同一进程内装配同步运行时，并由容器或进程管理器控制生命周期。

## 应用身份与数据边界

`app_users` 保存应用用户与 scrypt 密码派生值，`app_sessions` 只保存随机会话令牌的 SHA-256 哈希。同源访问使用 HttpOnly、SameSite=Lax Cookie；跨源客户端使用 HttpOnly、SameSite=None、Secure Cookie。浏览器由浏览器 Cookie Store 管理会话；桌面宿主在 Rust 网络桥内按服务地址隔离 Cookie Jar，并将持久 Cookie 保存到当前用户的私有应用数据目录，以便桌面进程重启后恢复。Cookie 不进入 WebView 的 localStorage、前端状态或 IPC 响应。前端 `AuthGate` 在渲染邮件工作区前检查会话，并在任意数据 API 返回 401 时立即退回登录页。

认证失败计数保存在 `auth_rate_limits`，因此 API 重启不会清空登录与注册限流；`security_audit_events` 记录注册、登录、授权码和敏感管理操作等安全事件。来源地址只保存使用实例随机盐生成的 HMAC，审计载荷不写入密码、会话、OAuth Token 或邮箱凭据。事件保留 90 天且全实例最多 10,000 条，登录用户只能读取自己的事件。生产服务完成首个用户初始化后默认关闭继续注册。

HTTP 会话、API 网关 Token 与 MCP 授权码都会恢复同一个服务端用户上下文。存储层按该上下文过滤 `accounts.user_id`、`developer_tokens.user_id`、`contacts.user_id` 与 `logo_fetch_attempts.user_id`，邮件和草稿通过所属邮箱账户间接隔离。后台同步不依赖浏览器会话，而是按全局唯一邮箱账户 ID 工作；提交联系人快照时重新取得该账户的用户归属。

用户作用域存储门面缺少上下文时直接失败，不再回退到全量数据。Worker、调度器和 IDLE 监听必须通过名称明确的 `readAllStore` 进入全局读取作用域；后台 OAuth 刷新只允许按全局唯一账户 ID 定向更新状态或密钥，不提供通用全局快照写入，避免普通请求因上下文遗漏跨租户读取。

设置中心使用同一用户上下文，将 `app_preferences_v1` 保存为 `metadata` 中的用户命名空间键。HTTP `preferences` 路由和 MCP `settings_get` / `settings_update` 负责主题、启动、阅读、通知、邮件展示、写信签名/模板与快捷键设置。`customTheme` 只接受九个 `#RRGGBB` 安全颜色和受限的圆角、阴影、字体枚举；客户端保留用户作用域本地缓存，同时通过偏好接口同步。MCP 的 `theme_custom_get` / `theme_custom_update` 与偏好接口共用用户命名空间键 `mcp_custom_theme_v1`，两条控制面读取同一份安全主题令牌。

“设置 → 隐私与数据”的敏感操作只走登录会话保护的应用入口：本地模式使用类型化 Tauri command，远程/Web 模式使用应用 HTTP API。授权导出必须先用当前 iMail 密码重新验证身份，再以用户单独提供的导出密码通过 scrypt 派生密钥，并使用 AES-256-GCM 加密当前用户的邮箱连接配置、应用专用密码/OAuth Token 和代理凭据。服务只在内存中保留与该用户绑定的单次下载两分钟；文件不包含邮件、附件、草稿、联系人或 iMail 登录密码。该导出能力不出现在 API Gateway 或 MCP 中。

附件预览由共享的 `imail-attachment` Rust crate 提供类型识别、文本编码规范化、安全策略与 ZIP 按项读取，`imail-http` 为当前登录用户维护短期预览会话。首次请求在本地磁盘缓存未命中时从 IMAP 获取附件；完整原文读取使用标准括号 fetch-items，兼容 `BODY.PEEK[]`、`RFC822` 与 `BODY[]`，并在同步 UID 失效时用 Message-ID 重新定位。服务器端 Message-ID 搜索未命中时，Rust 会从新到旧分批读取实际存在邮件的最小 Message-ID 头部并在本地匹配；本地文件夹位置过期时再跨可选文件夹定位，以兼容 iCloud 等 IMAP 实现，定位阶段不读取无关邮件的正文或附件。成功取得的原始附件进入按用户隔离、带摘要校验、三十天过期且每用户最多 512 MB 的本地缓存，供应用下载、预览、Gateway 与 MCP 复用；缓存不进入邮件数据库、备份或授权导出，并随“清除我的邮箱数据”删除。图片、PDF、视频与纯文本的最终绘制由客户端查看器或 WebView 媒体能力完成，ZIP 解压内容只存在短期预览会话。预览属于应用交互 API，本地模式通过类型化 Tauri command 和二进制 IPC 复用，远程/Web 模式使用登录会话 HTTP API；Gateway 与 MCP 继续只提供原有附件下载能力。

清除用户邮箱数据同样要求当前密码重新验证，并只接受固定确认文字。前端先展示清除范围，再进入密码和文字确认，因此构成两阶段确认；服务端在当前用户上下文中删除邮箱账户、授权、邮件缓存、草稿、联系人、开发者令牌及随账户级联的同步状态，同时保留 `app_users` 登录账号、应用偏好、服务运行文件、主密钥和其他用户的数据。通用页中的服务连接只选择本地/远程数据源，不承载该数据动作。

客户端切换主题时，`AppThemeProvider` 同步更新 Fluent 品牌色、根 `data-theme` 与运行时 CSS token。四套内置主题继续来自静态色阶；`custom` 只接受名称、九个六位十六进制颜色、圆角/阴影/字体枚举，由 `theme-runtime.ts` 派生完整中性色阶、品牌色阶和语义 token，不执行任意 CSS、URL 或脚本。

旧数据库行在迁移时先标记为 `__legacy__`。第一个成功注册的应用用户在同一事务中接管这些行，并将旧的全局 `app_preferences_v1` 设置迁入其用户命名空间，避免升级后丢失本地数据与偏好；未完成归属的旧 MCP/API 授权码不会被外部入口接受。

```text
Tauri / HTTP / MCP ── settings, status, sync-now ──► Rust application services
                                                   │
                                       policy / job / event
                                                   ▼
                                                SQLite
                                                   ▲
                                      lease / cursor / result
                                                   │
IMAP providers ◄──────────────────────────── Sync Worker
```

- `crates/imail-storage-sqlite/src/sync_runtime.rs`：同步策略、任务租约、游标、事件、Worker 心跳和原子提交。
- `crates/imail-runtime/src/sync_workers.rs`：worker pool、scheduler、退避、续租和协作式关闭。
- `crates/imail-runtime/src/account_watchers.rs`：IMAP IDLE/STATUS watcher、断线恢复与任务唤醒。
- `crates/imail-core/src/sync_runtime.rs`：与 transport 无关的同步计划和安全错误分类。
- Tauri Event 直接读取持久事件并主动唤醒；Rust HTTP Adapter 将相同事件映射为 SSE/WebSocket。两种入口不维护第二份同步状态。

同步执行只把安全裁剪后的领域事件写入 SQLite。SSE 和开发者 WebSocket 从同一持久化事件日志读取，避免独立 Worker 无法触达进程内事件总线，也避免同一封新邮件被内存总线和数据库重复投递。

`sync_jobs.rerun_requested` 保存任务运行期间到达的后续唤醒。若 IDLE 通知发生在当前 IMAP 快照已读取之后，入队事务会标记当前任务；完成事务随即创建一个互斥的 recovery 任务，避免唯一索引去重造成永久漏信。多个运行中通知仍合并为一次补跑，不产生并发同步。

同步游标按账户和真实邮箱文件夹保存，包括 `UIDVALIDITY`、最后 UID 与 `HIGHESTMODSEQ`。UIDVALIDITY 改变时只重建对应文件夹；支持 CONDSTORE 时按 modseq 获取标记变化，同时显式检查已缓存 UID 是否仍存在。API 的“立即同步”和 MCP `mailbox_sync` 都只创建持久化任务。

领域更新在 `BEGIN IMMEDIATE` 内读取当前用户快照并计算差异，只对新增、变化或删除的记录执行 SQL；不会因更新一个标签或授权码使用时间重写整份邮件缓存。Token 鉴权使用独立哈希查询和轻量 `last_used_at` 更新。API 与 Worker 的跨进程写入由 SQLite 串行化，同步控制表继续使用细粒度 SQL 事务。

## MCP 控制面

MCP 是 Rust HTTP 服务的可选受控适配层，不建立第二份邮件状态，也不绕过 IMAP/SMTP 服务边界。桌面本地嵌入模式默认不开放 MCP HTTP 地址。

```text
Agent
  │
  └─ Streamable HTTP /mcp ─ Bearer imail_mcp_* ─┐
                                                ▼
                                     imail-http MCP adapter
                                                │
                  ┌─────────────────────────────┼──────────────────────────┐
                  ▼                             ▼                          ▼
             imail-mail             imail-storage-sqlite          imail-security
             IMAP + SMTP             SQLite 本地缓存/草稿        AES-256-GCM 凭据
```

### 模块职责

- `crates/imail-http/src/mcp.rs`：通用 MCP 能力、Host/Origin 防护、Bearer 授权和协议分派。
- `contracts/mcp-tools.json`：由 Rust MCP 实现嵌入并在 Rust 测试中校验的稳定工具契约。
- `imail-core`、`imail-mail` 与 `imail-storage-sqlite`：HTTP、MCP 与 Tauri 共用的领域行为。

账户可选的 `http`、`https`、`socks5` 代理由 `imail-mail-network` 统一注入 IMAP 与 SMTP 连接，因此连接测试、同步、远程邮件操作、附件下载和发信遵循同一邮箱配置。SQLite schema v5 为 `accounts` 增加可空的 `proxy_json`，持久化协议、主机、端口和可选用户名；v4 升级只添加该列，不改写已有账户。代理密码继续合并进加密凭据载荷，公开账户视图和 MCP 输出不返回密码。关闭代理时同时清除 `proxy_json` 和加密载荷中的代理密码，备份/恢复保留两部分。

SQLite schema v7 增加 `apple_hme_sessions`，schema v8 增加 `apple_hme_addresses` 与 `apple_hme_sync_state`，schema v9 增加按应用用户隔离的 `translation_provider_profiles`，schema v10 增加按用户、邮件正文哈希、语言、Profile、Provider 修订与分段版本隔离的 `message_translation_cache`。翻译服务 Profile 只公开提供商、执行位置和凭据是否已配置，真实 API Key 或服务账号使用 `master.key` 加密保存。Apple 会话记录以邮箱账户 ID 为主键并通过外键随账户级联删除，只保存 `master.key` 加密后的 `AppleSession` 与更新时间；Apple ID、Cookie、`scnt`、Session Token、API Key 和数据访问 Token 均不进入公开账户模型。地址表保存最近一次手动同步的 HME 管理快照，按应用用户和邮箱账户隔离，创建、停用和删除成功后同步更新本地记录。HME pending 2FA 状态仅在进程内保留十分钟，并额外绑定应用用户与邮箱账户。Apple Account 会话用于新建地址，iCloud Web 会话用于手动同步、创建、停用和删除；IMAP/SMTP 仍使用原邮箱应用专用密码。 SQLite schema v11 新增与 `messages` 级联绑定的 `message_sources` BLOB，只保存同步时收到的完整 RFC 822 原始字节；解析正文与原始源分开读取，历史邮件没有原始存档时由客户端明确回退为未清洗正文。

Edge 本地翻译通过当前 WebView 的官方 `Translator` 与 `LanguageDetector` Web API 执行，每个应用用户只允许一个 Edge 本地 Profile，前端只在两个 API 都存在时声明运行时可用。模型已就绪时一次用户操作完成语言识别、翻译与结果回写；首次模型下载若要求新的用户手势，则明确显示“继续翻译”并保留下载进度。每次翻译都按服务端生成的稳定分段执行，客户端结果回传后由 Rust 重新读取邮件、Profile 与语言参数并校验全部分段 ID，只有 WebView 目标的 Edge 本地 Profile 可以提交结果。正文与译文不会因本地 Provider 进入第三方网络请求，译文是否持久化继续服从当前用户的缓存开关。
- `crates/imail-core/src/theme.rs`、`crates/imail-core/src/preferences.rs` 与 `crates/imail-http/src/mcp.rs`：校验并按应用用户保存自定义主题令牌，供 HTTP preferences 与 MCP 控制面共同使用。
- `crates/imail-core/src/developer_tokens.rs`、`crates/imail-storage-sqlite/src/developer_tokens.rs` 与 `imail-security`：生成和验证高熵授权码、SHA-256 哈希、过期、撤销与用户作用域。

### 权限模型

`mcp:full` 是独立的管理权限。MCP 入口只接受包含该 scope 的 Token；`messages:read`、`messages:send` 和 `accounts:read` 仍只用于开发者网关。MCP 授权码以 `imail_mcp_` 开头，语义覆盖所属应用用户的全部当前与未来邮箱账户，因此新增邮箱后无需重新签发，也不能访问其他应用用户的数据。

授权码仍使用既有 `developer_tokens`、`developer_token_scopes` 和 `developer_token_accounts` 表，没有新增明文凭据列。`accountIds` 为兼容现有 Token 展示继续写入，但 MCP 管理权限不以创建时账户快照作为访问边界。

### 请求流程

1. HTTP 入口校验 Host，存在 Origin 时同时校验 Origin。
2. 从 `Authorization: Bearer` 提取授权码，由 Rust 存储与授权服务验证 `mcp:full`。
3. Rust MCP adapter 完成协议协商、请求分派和逐请求用户上下文恢复。
4. 工具调用 `imail-core`、`imail-mail`、`imail-storage-sqlite` 和 `imail-security` 能力。
5. 返回文本内容与 `structuredContent`；JSON 序列化会剔除 `undefined`，凭据字段从不进入返回对象。

### 安全取舍

- 服务默认监听 `127.0.0.1`，MCP 再增加 Host/Origin allowlist，降低 DNS rebinding 和浏览器跨站调用风险。
- 远程模式不是默认发布形态；仅配置 `MCP_ALLOWED_HOSTS` 不等于完成远程加固，还需要 HTTPS、管理端认证和审计。登录按 IP 与账号双重限速，注册按 IP 限速，但生产入口仍应配置反向代理级限流。
- 邮箱服务商授权码只在工具参数和加密流程中短暂存在，不写日志、不返回。
- 发送、远程状态更新、移动和同步复用现有实现，保持 API 与 MCP 的协议行为一致。

## 邮件参与者筛选

阅读页中的发件人和每一个去重后的收件人都使用同一参与者信息浮窗。浮窗只提供复制地址、写信，以及“来自此地址”或“发往此地址”的精确筛选入口；筛选不会隐式替换关键词、邮箱范围、文件夹、标签、未读或附件条件。当前最多同时保留一个发件人条件和一个收件人条件，两者组合时使用 AND。条件条允许逐项清除；组合后没有结果时，可以只清除其他搜索条件，或清除全部参与者条件。

`GET /api/messages` 与本地 Tauri 查询接受可选的 `sender`、`recipient` 参数。地址经过去空白、控制字符、长度和基本邮箱格式校验后，在 SQLite 中按完整地址、不区分大小写匹配：`sender` 读取 `from_json`，`recipient` 只遍历 `to_json`。普通文本搜索仍可匹配主题、摘要和参与者 JSON，但不会替代这里的精确语义。前端同步事件也使用相同的完整地址规则判断增量消息是否属于当前列表。

“发往此地址”严格表示邮件详情当前展示的 `To` 收件人，不代表 SMTP 信封收件人，也不推断 `Delivered-To`、`X-Original-To`、转发链或 Bcc。当前缓存模型没有保存这些投递头，因此 UI 不宣称能够按真实投递目标筛选。若以后需要支持 Bcc 或别名实际投递查询，应先扩充同步数据模型并明确隐私边界，不能复用 `recipient` 参数改变现有含义。

参与者条件只存在于当前客户端内存中：在邮件范围、邮箱、文件夹和标签间导航时保留，进入联系人或外部接入页时暂停生效，应用重启后不恢复。通过系统通知直接打开邮件会清空关键词、快捷筛选和参与者条件，避免旧上下文隐藏目标邮件。窄屏从参与者浮窗应用条件后返回邮件列表；桌面端若当前邮件仍匹配则继续保留选择。

该能力属于登录会话保护的应用查询，不扩展 Gateway 或 MCP 契约，也不新增数据库 schema。SQLite 会先利用既有账户、邮箱角色、未读和日期索引缩小候选范围，再对候选行解析参与者 JSON；现阶段避免为 UI 筛选引入重复参与者表。若真实数据量和查询剖析表明 JSON 扫描成为瓶颈，再通过独立迁移增加规范化参与者索引表，并同时验证写入、删除、用户隔离和升级回滚行为。

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
4. 页面必须是有限大小的 HTML 且不是访问验证页；每一跳先解析并验证全部地址，再把实际连接固定到已验证的公网 IP，同时保留原 Host 与 TLS SNI，阻断 DNS 重绑定。图标响应继续受协议、端口、重定向、大小和图片 magic bytes 校验。
5. 若从非一级域成功获取图标，先写入对应子域缓存；一级域尚无有效图标时，再用相同图片补齐一级域缓存。并发任务仍按可注册主域合并，避免同一组织的多个子域同时外连。
6. 联系人优先保存精确子域引用；当精确子域没有图标而一级域已有图标时，保存一级域引用。一级域引用可被同主域联系人共享，后续列表和阅读页不会重复拉取。
