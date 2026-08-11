# Rust 服务迁移 R7 进展报告

日期：2026-08-11
状态：R7 完成；桌面领域调用、受保护二进制读取与事件通知均已直接接入 Rust host

## 本批目标

在不切换当前真实数据、不停止 Node 正式运行路径的前提下，建立前端 transport 边界和 Tauri 进程内 Rust host，证明桌面本地请求可以不经过 TCP listener。

## 已完成

- 前端新增 `MailService`、`HttpMailService`、`TauriMailService`。远程和 Web 默认行为不变；本地直连需要 Tauri runtime、本地选择与 `VITE_TAURI_EMBEDDED_SERVICE=true` 三项同时成立。
- 直连 WebView payload 只包含相对 `/api/` 路径、method 和 JSON 字符串，不包含服务 URL、数据库路径、凭据或 Cookie。
- Tauri 新增惰性 `EmbeddedMailServiceState`。首批通用 `desktop_mail_service_request` 已在领域映射补齐后删除；WebView 现在只能调用 tagged `desktop_mail_service_call`，host 对 Rust Router 执行 Tower 内存调用且不绑定端口。25 MiB 请求边界仍在进入核心前拒绝。
- 本地 Session 响应头在 Rust host 内截获，后续内存请求由 host 注入；Cookie 不返回 WebView。临时数据库已通过注册、持久 Session 状态读取，响应正文不含 Session 值。
- 运行时还要求 `IMAIL_TAURI_EMBEDDED_SERVICE=true`，并在旧服务 `enabled` 标记存在时拒绝打开数据目录。前端构建开关与宿主运行开关缺一不可，避免内部预览代码意外双写。
- Windows Tauri 单元测试从唯一临时目录初始化 Rust host，直接读取 `/api/system/info` 后清理临时数据；没有访问当前测试数据库。
- 本地事件入口直接消费内存 Router 的 SSE body，解析后沿用 `imail-sync-event` 发给 React；停止命令会中止先前任务。前端只有嵌入式本地模式选择该命令，远程和 Web 的 EventSource/HTTP bridge 不变。
- `imail-http` 新增不绑定 TCP 的 `EmbeddedServiceHost`，统一持有 application Router、持久同步 worker/scheduler/IDLE watcher 和连接关闭广播。Tauri 惰性初始化时启动该宿主，窗口隐藏不销毁运行时；显式退出会先取消事件流、关闭连接并在最多十秒内回收同步线程，Drop 仍提供兜底关闭。
- `/api/system/info` 的隔离 Tauri 测试现确认 `syncWorker=true`，并在删除临时目录前显式关闭宿主。该路径没有创建 listener，也没有打开当前测试数据。
- 本地附件下载和联系人 Logo 读取新增 Tauri binary command，直接读取进程内 Router 响应；payload 只含相对路径和用户选择的下载目标，不含 `baseUrl`。远程桌面继续使用 HTTPS/Cookie 隔离的原二进制桥。
- 首批只读领域调用已映射到 `desktop_mail_service_call` 的 tagged enum：认证状态、账户列表、邮件统计、邮件列表（结构化查询）和邮件详情（结构化 ID）。现有 React `api()` 调用无需改写；未迁移操作仍通过受限的进程内兼容命令。
- 嵌入式事件已移除 SSE body 与文本解析：启动订阅时用 host 内 Session 直接解析当前用户和账户白名单，以当前 `sync_events` 最新游标为起点；后台每秒读取 Rust 事件日志，只向 WebView 发布该用户账户的结构化事件。连接与 `sync.status` 初始事件立即发送，状态此后每 15 秒从同一进程内宿主刷新；停止订阅和应用退出均取消任务。
- 强类型 command 继续覆盖全局同步、单账户同步、账户文件夹同步、角色文件夹同步、邮件标记/标签/稍后处理更新和邮件移动。路径段及 JSON 由 Rust 从类型化字段重建，WebView 不再为这些操作传递 opaque route body。
- 当前 UI 使用的认证注册/登录/退出、账户与 OAuth、发信、草稿、偏好、Developer Token、外部访问、授权导出和用户数据清理均已加入 tagged command；未知本地操作直接失败，不再回落到通用路径命令。草稿创建的 `X-Draft-Id` 现在作为 `draftCreate.draftId` 跨 IPC，并只由 Rust host 重建幂等请求头。
- 启动认证门禁和服务选择 UI 已识别嵌入式本地模式：不再启动、探测、暂停或删除旧守护服务，也不再显示本地端口；本地身份检查通过 `systemInfo` 类型化命令完成。远程模式继续验证 HTTPS 服务并走原守护切换逻辑。
- 嵌入式 Session 现在以 raw token 形式写入应用私有 `local-service/embedded-session`，不写入数据库、日志或 WebView；Unix 文件模式为 `0600`，Windows 位于当前用户的应用数据目录。新 Tauri host 会恢复 token、从认证库解析显式用户 ID，再执行首次领域调用；退出登录会删除 Session 文件。隔离测试覆盖注册、同一 host 使用、宿主重启恢复、显式用户事件上下文和退出清除。
- tagged command 的内部 Router 转译继续按领域拆除。除 `authStatus`、`accountsList`、`draftsList` 外，邮件列表/详情/统计、标签、联系人、通知、偏好、Developer Token 列表和外部访问读取均直接使用 Rust repository 与领域服务；邮件 presenter 和草稿 payload 校验由 `imail-http` 与 Tauri facade 共用，避免两套契约漂移。
- 草稿创建/更新/删除、偏好更新和外部访问更新也已改为直接调用 Rust 领域服务。草稿创建保留类型化 `draftId` 的 UUID 校验和幂等语义；偏好与外部访问的无效 JSON、空更新、领域错误及服务错误保持原状态码和中文错误契约。
- Developer Token 的账户邮箱归属、scope 规范化、`mcp:full` 收敛、签发、公开视图和安全审计已从 HTTP handler 提取为共享 `DeveloperTokenService`。HTTP adapter 与 Tauri 直调现在调用同一应用服务；Tauri 使用固定的本地 actor 标识，仓储只保存其 HMAC，不暴露原值。
- 隔离测试分别经 Tauri 直调与 Rust HTTP adapter 创建真实随机 Token，确认混合 scope 收敛为唯一 `mcp:full`、明文只在创建响应返回、列表不含明文或 owner/account/token hash、创建与撤销审计均落盘，并分别撤销后返回 204。
- 注册、登录、状态、Session 解析和退出编排已提取为共享 `AuthenticationService`。服务保持原有安全顺序：注册先执行首用户/开放注册门禁和来源限流，再解析校验输入并用 SQLite `IMMEDIATE` 事务抢占首用户；登录先校验输入，再分别执行来源和账户持久限流。HTTP adapter 仅负责来源 actor、Cookie 属性与 `Retry-After` 协议呈现。
- Tauri 注册/登录/退出已直接调用共享认证服务。raw Session 只写入 Rust host 内存和应用私有 `embedded-session`，WebView 只收到公开用户 JSON；退出先撤销数据库 Session，再删除私有文件和用户上下文。隔离双宿主测试覆盖注册、状态、错误/成功登录、退出、审计事件以及直调 Session 的宿主重启恢复。
- 账户公开 presenter 已由 HTTP 与 Tauri 共用。此前空账户测试未暴露的风险已修正：直调账户列表不再直接序列化内部 `AccountReadModel`，有数据时也不会返回 `ownerId`，settings/proxy 只保留白名单字段，凭据、代理密码和加密字段不会进入 WebView。
- 非 OAuth 账户创建、元数据更新、密码替换、代理更新、连接测试和带审计删除均已从 Tauri 的 Router 转译移出。`EmbeddedServiceHost` 直接持有与可选 HTTP adapter 相同的应用状态和依赖；两端共享主密钥 codec、OAuth 刷新协调器、连接 probe、同步策略初始化与安全审计编排。
- 隔离 host 测试不经过 Router，实际创建账户、替换密码、配置 SOCKS5 代理并执行成功连接探测，确认数据库密文不含新密码/代理密码，`account.created`、`account.credential-updated`、`account.proxy-updated` 审计均落盘。Tauri 双路径测试另覆盖安全列表、元数据、删除审计、无效创建以及缺失账户的凭据/代理/连接错误，未访问公网。
- 隔离 Tauri 契约测试会在同一个临时数据库中逐项比较直调结果与 Rust HTTP adapter：读取覆盖账户、邮件、草稿、联系人、统计、标签、通知、设置和 Token；写入覆盖偏好、外部访问及草稿创建/更新/删除，并覆盖 400、404 和 204 分支。统一 `initialize` 仍先恢复 Session 并启动完整同步运行时，直调不会绕过宿主生命周期。

## 当前限制

- 所有生产 tagged command 已直接调用 Rust host；Router 转译只保留在 `cfg(test)` 契约夹具中，用于长期对照可选 HTTP adapter，不参与桌面生产路径。
- 附件、联系人 Logo 和一次性授权导出下载均由 Rust host 直接解析类型化目标并返回字节；任意 `/api/` 二进制路径不再进入 Router。
- Tauri Event 使用同步运行时的进程内条件信号主动唤醒；15 秒只承担状态刷新兜底，不再每秒轮询数据库。停止订阅和宿主退出会主动唤醒等待者并取消任务。
- 当前 Node 本地服务仍是唯一真实数据写入者；R8 数据原地切换尚未开始。

## 下一批

1. 进入 R8：实现旧守护状态、数据目录、实例身份、schema、密钥与备份空间的只读升级预检。
2. 在当前数据的一致性副本上执行 Rust 首启、登录、账户/邮件/草稿/联系人/Token/Logo/同步摘要核对。
3. 只有副本验收通过后，才停止旧 Node 单写入者并创建不可覆盖的切换前快照；失败不得删除旧运行文件或数据。

## R7 第八批：OAuth 进程内直连

- OAuth start、reconnect 和 status 已从 Tauri 内部 Router 转译移出，由 `EmbeddedServiceHost` 直接调用与可选 HTTP adapter 共用的 OAuth 应用函数。PKCE、加密 state、账户归属、刷新协调器、主密钥 codec、连接探测和安全审计仍使用同一实现。
- 桌面授权只为单次流程绑定严格回环地址；支持端口 `0` 由操作系统分配实际端口，授权 URL 使用实际 redirect URI。该 listener 仅接受预期 path、state 和 GET callback，不是业务 HTTP API，也不会暴露账户或服务路由。
- 回调在原生 Rust 侧完成 code/token 交换、身份读取、密文持久化和公开账户视图生成。WebView 只接收授权 URL、opaque state、完成状态和安全账户字段，access token、refresh token、client secret 与 PKCE verifier 不进入 IPC 响应。
- 临时 listener 订阅宿主关闭广播；显式退出或 host 回收时会取消等待，不会占用端口直至 OAuth state 超时。授权失败以清理后的安全消息记录，状态记录继续按 owner 隔离并按 TTL 清理。
- 新增隔离端到端测试：不启动业务 listener，完成动态回环 callback、一次 token exchange、账户落库和 status 轮询，并验证响应与持久密文均不含明文 token。另增加 listener 主动取消测试，以及 Tauri OAuth 无效 start、缺失 reconnect、未知 state 与 HTTP adapter 状态码契约对照。
- 本批测试全部使用唯一临时目录，没有打开当前测试数据库，也没有连接真实 OAuth 服务或发送邮件。
- 第八批完整回归：Node 68 个文件/346 项、Rust workspace 137 项通过（另 1 项显式长稳按设计忽略）、Windows Tauri 20 项通过；typecheck、production build、Rust workspace/Tauri 严格 Clippy 与双 workspace rustfmt 均通过。仓库测试数据库 SHA-256 保持 `074B3D437ADDDEBE3BD8020C3DAA18B825DE440EDA34AD01838979574BD72000`。

## R7 第九批：邮件写操作与同步控制直连

- 邮件更新、移动和发送已接入 `EmbeddedServiceHost` 直接应用入口。Tauri 与 HTTP adapter 共用发送 payload 校验、附件边界、邮件 patch 规范化、OAuth 刷新协调、IMAP/SMTP transport、凭据 codec、草稿发送后删除和安全错误映射。
- 全局同步、单账户同步、指定邮箱同步和邮箱角色同步已改为直接调用同步队列应用入口。账户归属、角色白名单、自定义邮箱规范化、canonical target、优先级和持久任务写入保持原语义。
- Tauri 契约测试新增邮件 400/404/422 与同步 200/400/404 对照；同步空账户列表的公开结果与 HTTP adapter 全等。缺失消息/账户在进入真实网络前结束，因此本批测试没有发送邮件或访问公网。
- 第九批 Rust workspace 137 项通过（另 1 项显式长稳按设计忽略），Windows Tauri 20 项通过，Rust/Tauri 严格 Clippy 与格式检查通过。Node 代码未在本批变更，沿用同一工作树本轮已通过的 68 文件/346 项、typecheck 与 production build 门禁。

## R7 第十批：隐私控制、二进制读取与事件主动唤醒

- 授权导出准备、一次性下载和用户邮件数据清理已接入共享 Rust 应用入口。密码长度、确认文本、持久限流、逐用户互斥、重新认证、导出 TTL/单次消费、内存清零与三类安全审计保持 HTTP 契约。
- Tauri 隔离成功路径完成零账户加密导出、一次下载、重复下载拒绝和清理，核对 prepared/downloaded/cleared 审计；无效输入继续与 HTTP adapter 对照。测试没有打开当前数据库。
- 附件、联系人 Logo 和授权导出二进制读取不再调用进程内 Router；只接受明确类型化路径，未知 `/api/` 路径直接拒绝。所有 tagged command 均已有直接分支，生产 IPC 已删除 Router fallback。
- 同步 runtime 新增可合并的进程内 `SyncEventSignal`。worker 在 `sync.started` 与最终成功/失败事件提交后通知，Tauri 立即读取事件批次；无事件时只在 15 秒状态截止点读取，不再一秒轮询。测试覆盖即时唤醒、超时和真实 worker 两次提交通知。
- R7 最终门禁：Node 68 个文件/346 项、Rust workspace 138 项通过（另 1 项显式长稳按设计忽略）、Windows Tauri 20 项通过；typecheck、production build、双 workspace 严格 Clippy 与 rustfmt 全部通过。

## 本批门禁

- Node：68 个测试文件、346 项通过；typecheck 与 production build 通过。
- Rust workspace：138 项通过，1 项显式长稳测试按设计忽略；严格 Clippy 与 rustfmt 通过。
- Windows Tauri：20 项通过；`x86_64-pc-windows-msvc` 严格 Clippy 与 rustfmt 通过。
- 当前桌面 Node service、worker、supervisor 保持恢复运行；嵌入式构建/运行双开关均未对真实数据启用。
