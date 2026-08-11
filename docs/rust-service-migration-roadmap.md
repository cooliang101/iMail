# Rust 服务重写与 Tauri 直连升级路线

## 状态与决策

本文档记录 iMail 从 Node.js 服务迁移到 Rust 服务核心，并在后续将 Windows 桌面本地模式切换为 Tauri 进程内直接调用的实施路线。

当前任务的完成范围仅为 Windows x64。Linux/WSL2、Docker 运行门禁、交叉编译及其他桌面平台不作为本路线的剩余阶段或完成条件，后续如需实施必须另建独立跨平台计划；本文中相关内容仅保留为既有架构、历史实现与证据记录。

当前决策如下：

- 先完成可独立运行、与现有协议和数据兼容的 Rust 服务端重写，再改变桌面调用方式。
- Windows 桌面本地模式最终把 Rust 服务核心直接嵌入 Tauri 进程，不再启动本地 Node 守护服务，也不常驻监听本地 HTTP 端口。
- 浏览器和 Docker 无法直接调用 Tauri，因此远程发布单元继续通过可选 Rust HTTP Adapter 提供 Web、REST、SSE、WebSocket、Gateway 与 MCP。
- 桌面远程模式继续通过 HTTPS 连接远程 Rust 服务；本地直连和远程 HTTP 必须复用同一领域服务、存储实现和协议模型。
- 桌面窗口关闭仍隐藏到托盘并继续同步；用户显式“退出 iMail”后，嵌入式服务和同步任务随 Tauri 进程结束。这是相对当前独立本地守护服务的有意行为变化，必须在切换阶段明确展示并测试。
- Google 与 Microsoft 桌面 OAuth 仍可在授权期间临时启动 localhost loopback callback listener；“本地无 HTTP”指不提供常驻业务 HTTP API，不禁止短生命周期 OAuth 回调监听。
- 迁移期间不得删除、重置或覆盖现有开发与测试数据。当前仓库 `.data`、Windows 用户数据目录中的数据库、主密钥、Logo、授权和邮件缓存都必须保留，以用于兼容验证和最终迁移。

## 实施进度

截至 2026-08-11：

- 已创建 `codex/rust-service-migration` 开发分支。
- R0 已提供非覆盖迁移基线命令、活动数据/输出目录重叠保护和自动化测试。首份当前测试数据快照保存在被 Git 忽略的 `output/rust-migration-tests/r0-initial-2026-08-10/`；schema v6、SQLite 完整性、外键、主密钥、实例身份和 Logo 清单均已验证，源数据库未被修改。R0 的完整 HTTP/MCP 契约快照和多场景进程内存基线仍待补齐。
- R1 已建立 `imail-protocol`、`imail-core` 和 `imail-storage-sqlite` workspace。Rust 只读实现以 read-only/query-only 打开数据，拒绝未来 schema 和无效主密钥，能够解析现有账户、邮件、草稿、联系人、公开 Token 视图及同步模型，且不序列化 `encrypted_secret` 或 `token_hash`。
- Rust 已在 R0 保留快照上与 Node 基线逐表、逐模型计数对照；18 张表及公开模型数量一致。账户、邮件、草稿、联系人和公开 Token 视图进一步执行跨语言规范化字段摘要对照，全部一致。旧 schema 只读、未来 schema 拒绝、损坏数据库、畸形 JSON/布尔字段和缺失主密钥矩阵均已覆盖，验证前后快照和当前 `.data` 数据库哈希没有变化。R1 后续继续补齐认证与同步事件的字段级兼容视图。
- R2 已完成，验收证据见 `docs/rust-migration-r2-report.md`。除安全、认证、Token、用户 metadata 和全部当前本地领域表的用户隔离写入外，Rust 已实现无版本旧库及 v1→v6 的单事务迁移执行器；未来 schema 被拒绝，并发打开会在持有写锁后重查版本，失败会回滚整轮升级。Node/Rust 从相同 v2 fixture 升级后的表列、索引、外键、数据及 metadata 语义签名一致。相同双向读写契约已在 R0 真实规模快照的临时副本上通过，并完成一个既有账户凭据的随机 IV 重加密和 Node 语义复核；工作副本完整性正常，保留快照与活动 `.data` 哈希不变。当前运行路径仍是 Node 唯一写入者，尚未发生运行时切换。
- R3 已完成，验收证据见 `docs/rust-migration-r3-report.md`。不访问真实邮箱的账户、草稿、偏好、主题、联系人、通知、隐私、授权导出与数据维护服务已迁入 Rust 核心/适配器边界；Node/Rust 共享领域 fixture、便携导出加密向量和 Rust 备份→Node 恢复预检均通过。当前运行路径仍保持 Node，所有写入与破坏性验证仅操作临时副本。
- R4 自动化实现与离线验收已完成，证据见 `docs/rust-migration-r4-report.md`。Rust 已具备 MIME、IMAP/SMTP、HTTP(S)/SOCKS5 代理、Google/Microsoft/Yahoo OAuth、PKCE、Token 刷新单航班与临时 loopback callback；Tauri/HTTP 无关的邮件应用门面可以从现有账户和缓存邮件直接调用这些端口。隔离的真实 TCP/TLS fixture 已让生产网络适配器完整执行 IMAP 登录/发现/FETCH literal/MIME 和 SMTP 认证/DATA 字节往返；该测试使用仅进程内信任的临时 CA，不代表公共服务商验收。专用真实邮箱的发送/附件/标记/移动往返尚未执行，因此 R4 暂不标记最终完成，也不提前切换运行路径。
- R5 离线实现、当前数据副本续跑和 60 秒资源烟雾门禁已完成：release 模式 2 个空闲 worker 的 RSS 峰值为 9,670,656 bytes，零任务、零遗留 worker/queue 且优雅关闭。专用真实邮箱、长时间网络稳定性及同负载 Node/Rust 资源对照仍待执行，详见 `docs/rust-migration-r5-report.md` 与 `docs/rust-migration-r5-resource-report.md`。
- R6 已开始可选 HTTP Adapter：默认启动模式保持无 HTTP 的 `Embedded`，只有显式 `--http` 才选择网络桥接。现有应用 HTTP 路由已全部具有 Rust 对应实现；除 Session、账户/OAuth、邮件、草稿、同步/SSE、Logo、Developer Token 外，本轮补齐服务商目录、安全审计、密码复核、两分钟一次性加密授权导出、按用户事务清理和桌面守护令牌停服。Node/Rust 双宿主契约已覆盖服务商目录与清理行为，Rust 隔离测试进一步解密验证导出只含当前用户、跨用户不可消费且下载一次后失效。MCP 默认不挂载，启用后逐请求校验 `mcp:full`、用户开关和归属；29 项 Node 工具均有 Rust 执行路径。正式 `imail-server` 无参数不监听，显式 `--http` 才装配网络桥接、同步运行时与优雅关闭；`--daemon-control-file` 已兼容当前 Tauri 守护协议。同步执行器新增可注入的 `SyncMailTransportFactory`，生产仍默认使用真实网络 adapter；隔离端到端测试已经从 HTTP 注册/账户创建贯穿真实 TLS IMAP/SMTP、正式 worker、SQLite 提交、HTTP 查询和发送，证明 HTTP 只是桥接而非第二套同步逻辑。Windows 同构 production HTTP 空闲对照中，Rust 仅宿主/完整拓扑峰值分别比 Node 低 83.85%/89.93%，并修复了 Node 打包 worker 重启和守护停服退出码问题。独立无 Node runtime 的 `Dockerfile.rust` 已在真实 WSL2 Docker/Buildx `linux/amd64` 门禁中通过：Node 先写入唯一命名卷，Rust 再从同一卷验证实例身份、密码登录、偏好与外部访问设置，并完成重启持久性、healthcheck、备份/非覆盖恢复/升级预检及两次优雅停机；候选镜像未推送且正式 Node Dockerfile 未切换。完整离线门禁为 Rust workspace 131 项、Node 66 个文件/332 项，另有 1 项显式真实 TLS 长稳验收独立通过；格式、严格 Clippy、Rust 1.77.2、typecheck、build 和 audit 均通过。真实公共邮箱浏览器流程和长稳网络资源对照仍未完成，因此当前不能替换 Node。证据见 `docs/rust-migration-r6-report.md` 与 `docs/rust-migration-r6-resource-comparison.md`。
- R7/R8 已完成真实四账户只读与受限互发验收、非覆盖切换快照、Node→Rust 一次性切换、真实 Windows Rust-only NSIS 覆盖安装和安装目录扫描。当前安装版保留原数据库、主密钥、实例身份、Logo 与历史迁移快照；本地模式没有 Node 进程和 8787 监听。
- R9 Windows 收尾已完成：桌面包已移除 Node SEA/manager/worker，前端已删除本地端口、暂停、移除与守护日志控制面，并在本地嵌入模式禁用 Gateway/MCP/Token 等 HTTP 外部访问操作；正式开发、远程启动与维护入口均使用 Rust。正式 `Dockerfile` 和发布工作流已切为 Rust HTTP Adapter，最终镜像无 Node runtime。
- R9 Windows 门禁已通过，证据见 `docs/rust-migration-r9-report.md`。Rust 常驻运行时在真实规模数据副本上的 60 秒峰值 RSS 为 9,666,560 bytes、首尾增长为 0；5 账户/1,000 邮件/2 MiB 详情的 300 批次完整拓扑长稳中，Rust 峰值 48,709,632 bytes、比 Node 低 97.08%，中位数低 97.72%；300 秒真实 TLS/IDLE 恢复与调度长稳峰值 21,585,920 bytes，18 个任务全部成功。三类测试均优雅停机且零遗留 worker/queue/PID。按用户决定，迁移验证完成后已删除旧 Node 服务源码、Express 路由、Worker、双实现测试、基准构建和专用依赖；删除后完整验证与新安装包证据见 `docs/rust-migration-r10-report.md`。历史实现由 Git 记录承担回溯，既有资源报告和数据快照继续保留。WSL2/Docker `linux/amd64` 真实构建与运行验收另立跨平台计划；已中止的交叉平台构建不计为通过。

## 目标

1. 用 Rust 重写账户、认证、存储、邮件、OAuth、同步、Gateway、MCP 和维护工具，降低常驻内存并移除桌面包中的 Node 运行时。
2. 将业务核心从 HTTP 路由中抽离，使 Tauri、HTTP 和 MCP 都只是适配器。
3. 保持现有 SQLite 数据、加密凭据、同步游标和用户作用域可原地升级，不要求用户重新添加邮箱或重新同步全部邮件。
4. 每个阶段都能独立构建、自动测试、在数据副本上集成验证，并能回到上一阶段的唯一写入实现。
5. 完成本任务内的 Windows x64 桌面交付；`linux/amd64` Docker、交叉编译及其他平台另立计划，不作为本任务完成条件。

## 非目标

- 本路线不更换 React/Vite 前端，不重设计现有 UI。
- 本路线不把 SQLite 改为 PostgreSQL，也不声明多副本水平扩展能力。
- 本路线不改变现有远程 HTTP、Gateway 或 MCP 的公开协议；协议演进必须另行版本化。
- 本路线不在本地与远程实例之间复制、合并或自动迁移数据。
- 本路线不以清空缓存、删除同步游标、重新添加邮箱或重新授权作为迁移手段。
- 本路线不允许 Node 与 Rust 同时写入同一个 SQLite 数据目录。

## 必须保持的不变量

### 数据不变量

- `.data` 和平台用户数据目录永远不是自动化测试的清理目标。
- 自动化测试只能创建并清理它自己生成的临时目录；迁移验收快照保存在带时间和版本标识的独立目录，不在测试结束时自动删除。
- 数据快照必须同时包含数据库、主密钥、Logo 文件、实例清单和 schema/version 信息。只复制数据库而遗漏主密钥不构成可恢复备份。
- 首次让 Rust 写入真实数据前，必须先生成一致性备份，在副本上完成迁移、`integrity_check`、`foreign_key_check`、解密抽样和回滚演练。
- schema 迁移在 Node/Rust 并行开发期只允许向前兼容的新增；需要破坏性重建时必须延后到 Node 已退出唯一写入路径之后，并提供非覆盖恢复方案。
- 任何实现都必须保持账户、邮件、草稿、联系人、Logo、开发者 Token、同步任务和事件的用户归属。

### 安全不变量

- Rust 必须逐字节兼容现有 scrypt、AES-256-GCM、Token SHA-256、审计 HMAC 与加密载荷格式，迁移不能以解密后重新明文落盘作为过渡。
- Tauri 前端不能传入一个可被直接信任的 `userId`。登录成功后的用户上下文由 Rust 宿主保存并注入调用，前端只能持有不含凭据的会话状态。
- 邮箱密码、OAuth Token、代理密码、主密钥、会话 Token 和 `encryptedSecret` 不进入 Tauri 事件、HTTP/MCP 响应或日志。
- 本地直连不能让 WebView 获得任意文件、任意网络或原始 SQLite 访问能力。
- Docker HTTP 模式继续要求 HTTPS、Host/Origin 防护、初始化控制、认证限流、安全审计和可信代理配置。

### 同步不变量

- 手动同步、IDLE 变化唤醒、启动校准、断线恢复和后台校准继续使用同一持久化任务模型。
- 必须保持 `UIDVALIDITY`、UID、`HIGHESTMODSEQ`、任务租约、`rerun_requested` 和持久化事件的现有语义。
- 测试迁移不得通过删除同步状态强迫全量重建来掩盖兼容问题。
- 同一数据目录任一时刻只能存在一个同步调度器和一个实现族的 Worker。

## 目标拓扑

### Windows 桌面本地模式

```text
React WebView
     │ typed Tauri invoke / event
     ▼
Tauri Rust Host
     │ direct Rust calls
     ├── application services
     ├── SQLite store
     ├── IMAP / SMTP / OAuth
     └── sync runtime

无常驻本地业务 HTTP listener
窗口隐藏后继续运行；显式退出后停止
```

### Windows 桌面远程模式

```text
React WebView
     │ typed client
     ▼
Tauri Remote HTTP Adapter
     │ HTTPS / Cookie / SSE
     ▼
Remote Rust Service
```

### Docker / Web 模式

```text
Browser / Agent / API Client
     │ HTTPS
     ▼
Rust HTTP Adapter
     ├── Web static + SPA fallback
     ├── App REST + session cookie
     ├── SSE
     ├── Gateway REST / WebSocket
     └── MCP Streamable HTTP
             │
             ▼
       shared Rust core
```

## 建议的 Rust workspace

```text
rust/
├── Cargo.toml
├── crates/
│   ├── imail-protocol/          共享 DTO、命令、事件、错误码和版本
│   ├── imail-core/              领域模型与应用服务
│   ├── imail-storage-sqlite/    schema、迁移、查询、事务和备份接口
│   ├── imail-security/          密码、加密、Token、会话和审计
│   ├── imail-mail/              IMAP、SMTP、MIME、附件和代理
│   ├── imail-oauth/             OAuth、PKCE、callback 和 Token 刷新
│   ├── imail-sync/              调度、IDLE、任务租约、游标和事件日志
│   ├── imail-gateway/           Gateway 共享应用服务
│   ├── imail-mcp/               MCP tools 和 transport adapter
│   ├── imail-http/              可选 HTTP、SSE、WebSocket 和 Web 静态托管
│   └── imail-tauri/             Tauri command/event adapter
└── bins/
    └── imail-server/            Docker、维护命令和 HTTP 参数入口
```

依赖只能指向内层：Adapter 可以依赖应用服务，应用服务不能依赖 Tauri、HTTP、MCP 或具体响应类型。SQLite、IMAP 和系统时间等外部能力通过明确接口注入，协议适配器不直接拼装跨领域业务事务。

## 前端调用边界

当前以 URL 为中心的 `api(path, options)` 在 Rust HTTP 等价服务完成前保持不变。进入 Tauri 直连阶段后，新增领域化客户端接口：

```ts
interface MailService {
  listAccounts(): Promise<Account[]>;
  listMessages(query: MessageQuery): Promise<MessagePage>;
  getMessage(id: string): Promise<Message>;
  updateMessage(id: string, changes: MessageChanges): Promise<Message>;
  enqueueSync(target: SyncTarget): Promise<SyncJob>;
  subscribe(listener: (event: ServiceEvent) => void): () => void;
}
```

- `TauriMailService` 使用 typed invoke 和 Tauri Event。
- `HttpMailService` 使用现有 HTTP、Cookie 和 SSE。
- feature 只依赖 `MailService`，不能判断运行平台或拼接服务 URL。
- 不引入以 `path/method/body` 为参数的通用 Tauri RPC；这种做法只是把 HTTP 语义搬进 IPC，无法形成稳定的内部接口。
- 附件、Logo 和授权导出等二进制数据使用 Tauri binary response、受控文件句柄或下载命令，不经 JSON 数组复制大块字节。

## 通用阶段门禁

每一阶段只有同时满足以下条件才能标记完成：

1. 阶段新增的 Rust crate 通过格式、静态检查、单元测试和文档测试。
2. 现有 TypeScript 类型检查和测试继续通过。
3. Windows 与 Docker 相关代码至少完成对应构建；影响交付拓扑的阶段完成现有发布冒烟。
4. 新旧实现使用相同 fixture 运行契约测试，结果经过规范化后等价。
5. 在当前数据的一致性副本上完成该阶段声明的读取、迁移或写入验证。
6. 测试前后记录真实数据目录的文件清单和校验摘要，确认没有删除、截断或意外改写。
7. 阶段报告包含测试命令、结果、数据快照位置、已知差异和回退步骤。

稳定后的统一门禁目标为：

```powershell
cargo fmt --all --check --manifest-path rust/Cargo.toml
cargo clippy --workspace --all-targets --all-features --manifest-path rust/Cargo.toml -- -D warnings
cargo test --workspace --all-features --manifest-path rust/Cargo.toml
npm run typecheck
npm test
npm run build
```

在 Rust workspace 尚未建立或某 feature 尚未存在的早期阶段，只运行当前阶段已有命令，但不得跳过现有 npm 门禁。

## 阶段 R0：冻结基线并建立数据保护工具

**目的**：在写 Rust 业务代码前固定当前行为、数据格式和资源基线。

交付物：

- 记录当前 Node HTTP、Gateway、MCP、同步事件和错误响应的契约快照。
- 为现有 SQLite schema、迁移、加密载荷和关键 JSON 字段建立跨实现 fixture。
- 增加只读的数据目录清单工具和非覆盖式迁移快照命令。
- 快照清单记录数据库 SHA-256、主密钥指纹、Logo 数量、schema 版本、账户/邮件/草稿/联系人/同步任务计数；不得输出凭据内容。
- 建立迁移测试目录约定，例如 `output/rust-migration-tests/<run-id>/`，已作为升级证据的目录不自动清理。
- 记录 Node API、Worker 和桌面进程在空闲、多个 IDLE 连接、批量同步时的 Working Set 与峰值，作为 Rust 验收基线。
- 为自动化命令增加保护：拒绝把仓库 `.data`、平台正式数据目录、安装目录或根目录作为临时测试输出和删除目标。

测试：

- 对当前真实测试数据执行只读清单和一致性备份。
- 从备份准备一个新目录并用当前 Node 服务只读/正常启动验证。
- 运行 SQLite `integrity_check`、`foreign_key_check` 和现有备份恢复测试。
- 验证故意传入 `.data` 作为测试清理目标时命令会失败且不改变文件。

完成标准：当前数据存在至少一份经验证、可由当前 Node 服务打开的非覆盖备份；契约和资源基线可以重复生成；没有修改真实数据。

回退：本阶段不改变运行实现，无运行时回退动作。

## 阶段 R1：建立 Rust workspace、协议模型与只读 SQLite

**目的**：建立可编译骨架，并证明 Rust 能无损理解现有数据。

交付物：

- 创建 `imail-protocol`、`imail-core` 和 `imail-storage-sqlite`。
- 映射现有跨 feature 类型、服务端领域类型和稳定错误码。
- 实现 schema 版本读取、只读账户/邮件/草稿/联系人/Token/同步状态查询。
- Rust 打开未知高版本 schema 时必须拒绝写入；只读诊断也不能自动迁移。
- 对日期、布尔值、可空字段、JSON、邮箱地址大小写和排序建立黄金测试。

测试：

- Rust 对合成 fixture 和 R0 数据快照执行只读查询。
- Node 与 Rust 对同一副本产生规范化 JSON，逐字段比较。
- 验证 Rust 只读执行前后数据库及关联文件 SHA-256 不变。
- 验证损坏数据库、缺少主密钥、旧 schema 和未知新 schema 的安全失败。

完成标准：Rust 可以读取当前测试数据的所有公开模型，计数和关键字段与 Node 一致，且没有写入数据。

回退：删除或禁用尚未接入运行路径的 Rust 构建产物；当前 Node 服务不受影响。

## 阶段 R2：存储写入、迁移、安全与认证兼容

**目的**：让 Rust 在数据副本上成为可靠写入者，并保持现有凭据可用。

交付物：

- 实现 SQLite 事务、差异更新、用户作用域和后台全局读取边界。
- 兼容现有 schema 迁移和 metadata 命名空间。
- 实现 scrypt 密码验证、AES-256-GCM 加解密、Token 哈希、审计 HMAC、会话过期和限流。
- 实现应用用户注册、登录、会话、偏好、开发者 Token 和安全审计的应用服务。
- 所有测试日志使用脱敏摘要。

测试：

- Rust 在 R0 快照的工作副本上验证现有应用密码，并解密后重新加密抽样凭据；比较语义，不输出明文。
- Node 写入副本后由 Rust 读取，Rust 写入另一个副本后由 Node 读取。
- 覆盖事务中断、锁等待、并发用户、跨用户读取拒绝和迁移失败回滚。
- 每轮写入测试后运行完整性、外键、账户归属和加密可读性检查。

完成标准：Rust 写入的兼容数据仍能被当前 Node 版本读取；真实数据保持不变；所有破坏性试验只发生在独立工作副本。

回退：丢弃发生测试写入的副本，继续使用未改动的真实数据和 Node 服务。

## 阶段 R3：迁移低协议风险领域

**目的**：在引入 IMAP/SMTP 前完成大部分本地领域行为。

交付物：

- 账户元数据与代理配置领域服务，但暂不连接邮箱。
- 草稿、标签、通知、偏好、自定义主题、联系人和 Logo 引用模型。
- 开发者 Token、授权导出、用户数据清除和备份/恢复应用服务。
- Logo 获取审计和“已有成功或失败记录不自动重试”规则。
- HTTP/MCP/Tauri 共用的参数模型和安全响应视图。

测试：

- 将现有 Node 领域测试迁为语言无关 fixture，并在 Node/Rust 两侧运行。
- 重点验证账户删除级联、用户数据清除范围、授权导出兼容和 Logo 子域/主域回退。
- 数据清除测试只能操作测试副本；必须额外断言原始快照和真实 `.data` 未变化。
- Rust 生成的备份由 Rust 和当前 Node 恢复准备工具分别验证。

完成标准：不访问真实邮箱即可完成的服务能力达到行为等价，且所有敏感输出满足现有安全边界。

回退：运行路径仍使用 Node；Rust 结果只存在于测试副本。

### R3 完成记录（2026-08-10）

已完成并通过临时 SQLite 集成测试：

- HTTP/Tauri 无关的账户安全视图与元数据更新、草稿、偏好、标签和通知应用服务。
- 联系人重建、单封邮件参与者去重、本人地址排除、子域到可注册主域的 Logo 回退，以及成功/失败记录均不自动重试。
- 当前用户邮件数据清除事务；保留应用用户、登录能力和用户偏好，跨用户数据不受影响，事务失败完整回滚。
- `imail-protocol` 中对应的桥接 DTO 与稳定业务错误码；SQLite 联系人全量替换使用单事务。
- 代理配置/凭据候选在注入的连接验证器成功后才持久化；验证失败不修改密文，自定义主题只接受固定安全令牌。
- 授权导出与 Node 使用相同 scrypt/AES-256-GCM 参数、AAD 和白名单载荷；错误密码及篡改被拒绝。
- 在线 SQLite 备份、v1/v2 清单校验和非覆盖恢复准备；Rust 备份已由 Rust 与当前 Node 恢复工具分别验证。
- Node/Rust 共用 `r3-domain-v1.json`，覆盖联系人、中文标签排序和通知；账户删除级联矩阵覆盖内容、同步表与 Token 账户绑定。

## 阶段 R4：重写邮件、OAuth 与远程操作

**目的**：替换协议复杂度最高的外部能力，为同步引擎提供稳定端口。

交付物：

- IMAP 连接、能力发现、邮箱文件夹发现和代理支持。
- SMTP 发送、附件下载、远程标记更新和邮件移动。
- MIME 解析、HTML/文本正文、地址、附件元数据和大小限制。
- Google/Microsoft OAuth、PKCE、loopback callback、Token 刷新和账户重连。
- 所有网络连接的超时、取消、TLS、代理和安全日志策略。

测试：

- 协议替身覆盖成功、超时、断线、畸形响应、超大邮件、代理和授权失效。
- 使用专用验收邮箱执行连接、发送、下载、移动和标记往返；不得删除当前测试数据或清空现有邮箱。
- 远程有副作用的测试使用新建的唯一主题邮件，结束后允许将其移动到测试文件夹，但不把删除作为成功条件。
- Node/Rust 对相同 RFC822 fixture 的解析结果做规范化比较。
- OAuth 使用测试配置和可控 callback；日志、事件和错误中断言不存在 Token。

完成标准：Rust 可以完成账户接入、收取单封邮件、发送、附件和远程操作，现有测试邮箱与本地缓存均保留。

回退：专用邮箱不切换后台同步；生产/当前本地数据仍由 Node 唯一写入。

### R4 自动化完成记录（2026-08-10，真实邮箱验收待执行）

已完成并通过离线测试：

- Node/Rust 共用 RFC822 fixture，覆盖 RFC2047 中文主题和姓名、multipart alternative、HTML/文本、RFC2231 中文附件名、日期时区、Message-ID、附件内容与 UTF-16 预览截断。
- 独立 `imail-mail` 核心端口和无 HTTP 邮件应用门面；账户验证、发送、按需附件、已读/星标、归档/垃圾箱移动均可由 Tauri 或未来 HTTP Adapter 调用同一服务。
- `imail-mail-network` 真实 IMAP/SMTP 适配器；密码、XOAUTH2、Yahoo OAUTHBEARER、隐式 TLS、STARTTLS、HTTP/HTTPS CONNECT 与 SOCKS5 代理均已接入。
- loopback TCP/TLS fixture 使用仅测试进程信任的临时 CA，已让同一个生产适配器完成 IMAP LOGIN/CAPABILITY/LIST/STATUS/EXAMINE/FETCH literal/LOGOUT 和 SMTP EHLO/AUTH/MAIL/RCPT/DATA/QUIT；入站与出站 RFC822 均再次执行 MIME 解析。TLS connector 在适配器实例内加载并复用，生产信任边界不接受测试 CA。
- 30 秒连接和 120 秒操作超时；Windows 系统信任根、Docker/公共 WebPKI 根合并使用；协议和 OAuth 错误统一折叠、限长并脱敏。
- Google、Microsoft、Yahoo provider 目录、PKCE S256、AES-GCM state、10 分钟 TTL、owner/provider/nonce/email 校验、Microsoft RS256 JWKS 验证、90 秒刷新偏移和账户级单航班刷新。
- 短生命周期 loopback callback 只允许回环地址和预期 path/state；动态端口、错误回调、`Cache-Control: no-store` 和错误请求继续等待均有测试。
- OAuth 新增/重连领域写入保留代理密码，公开账户视图不含密码或 Token；连接失败以安全错误进入账户状态。

尚未完成：

- 需要用户提供专用验收邮箱和对应 OAuth 测试配置后，执行真实连接、唯一主题发送、下载附件、标记和移动往返。测试不删除邮件，也不清空现有邮箱或本地数据。
- `async-imap` 当前确认 MOVE 成功但不暴露 UIDPLUS 的目标 UID 映射，因此返回模型中的新 UID 可能为空；R5 同步时必须通过目标文件夹增量重新确认。
- SMTP 适配器在整封投递成功时把全部 envelope recipients 记为 accepted；服务商部分接受、部分拒绝的细粒度结果仍需在真实验收中确认。

自动化门禁：Rust workspace 63 项、Node 57 个测试文件/318 项、严格 Clippy、Rust 1.77.2 全目标编译、TypeScript typecheck 和生产构建全部通过。R0 快照与当前 `.data` 校验仍为 `snapshotUnchanged=true`、`activeDataUnchanged=true`。

## 阶段 R5：重写同步运行时

**目的**：达到当前持久化同步模型的完整等价。

交付物：

- 持久化策略、任务创建、领取、续租、完成、失败、退避和 Worker 心跳。
- IMAP IDLE、STATUS fallback、启动校准、连接恢复和低频一致性校准。
- `UIDVALIDITY`、UID、`HIGHESTMODSEQ`、CONDSTORE、删除检测和文件夹级重建。
- `rerun_requested` 与 recovery 任务，保持运行中变化通知不丢失。
- 持久化安全事件和裁剪后的邮件增量事件。
- Rust runtime 内 API/adapter task 与同步 task 的取消、故障隔离和优雅关闭。

测试：

- 将现有同步测试逐项迁移，覆盖重复通知、租约过期、进程/任务中断、UID 重置和多账户公平性。
- 在数据库副本上从现有游标继续同步，禁止删除游标或先执行全量重建。
- 同一验收账户不得同时由 Node 和 Rust 保持 IDLE；切换前停旧实现并确认租约与连接释放。
- 对专用测试邮箱制造新增、已读、星标、移动和删除变化，比较 Node/Rust 最终快照。
- 运行长时间空闲、断网恢复和内存/连接泄漏测试。

完成标准：Rust 能从当前数据副本的既有同步游标继续运行，增量结果与 Node 等价，失败恢复不依赖前端或 HTTP 连接。

回退：停止 Rust runtime，保留失败现场副本；Node 继续使用从未交给 Rust 写入的原始数据。不得把两个实现同时指向原始目录。

### R5 持久化控制面进展（2026-08-10）

已完成第一批离线实现，尚未达到 R5 完成标准：

- 新增 Rust `SyncRuntimeStore`，直接复用 schema v6 的同步表，不提升 schema 版本，也不改变 Node 的现有读写格式。
- 策略默认值、账户策略初始化、暂停/恢复与重新授权恢复已迁移；停用自动同步会取消排队任务并暂停邮箱状态。
- 任务创建、活跃目标去重、优先级/执行时间合并、领取、续租、过期租约回收、成功、失败、取消和运行中 `rerun_requested` 已迁移为 SQLite 事务。
- 邮箱游标、`UIDVALIDITY`、`HIGHESTMODSEQ`、下一次校准时间、连续失败、授权暂停和 recovery 任务均按现有 Node 数据模型持久化。
- `sync.started`、`sync.completed`、`sync.failed` 事件以及 worker 心跳、过期心跳过滤和队列健康视图已实现；事件查询继续限制为最多 500 项。
- 新增 Rust 增量同步核心与真实 IMAP Adapter：初始最近 80 封窗口、UID 增量、500 UID 分批删除检测、CONDSTORE `CHANGEDSINCE` 标记刷新、UIDVALIDITY 文件夹重建计划、Gmail All Mail 标签过滤、邮箱角色解析和文件夹状态采集。
- 新增邮箱缓存单事务提交：保留本地标签与稍后处理时间、Message-ID 去重、标记更新、账户同步状态/文件夹列表更新、每账户 5000 封上限和不含正文的邮件变更摘要。
- 邮箱缓存提交现在在同一事务内重建用户联系人；内置 Mozilla PSL 与 Node `tldts` 一样处理多级及私有后缀，继续支持子域键到可注册主域 Logo 的回退。故障注入确认联系人写入失败时邮件、账户同步时间和联系人会整体回滚。
- 新增 `MailboxSyncApplicationService`，在无 HTTP 情况下完成用户作用域账户读取、凭据解密、差异规划和 SQLite 提交；协议替身集成测试已证明该入口可以被 Tauri command 或后台 worker 直接调用。
- OAuth 90 秒刷新窗口和账户级 singleflight 已接入连接配置解析；刷新成功后先加密持久化旋转后的 access token，并保留旧 refresh token 与代理密码，再把配置交给同步/邮件端口。
- 新增 IDLE/STATUS wake primitive；IDLE 或低频 STATUS 只返回 `Changed/Reconcile` 信号，实际变更仍必须进入持久化 recovery 任务，避免长连接直接修改缓存。
- 新增无 HTTP `imail-runtime` supervisor：共享取消令牌、任务名称去重、panic 故障隔离、可中断周期等待、有界优雅关闭和不合作任务超时报告；后续同步 worker、watcher、Tauri 与 HTTP Adapter 统一挂到该生命周期。
- 持久同步 worker pool 已挂到 supervisor：1–16 个固定 slot 分别打开 SQLite 连接，领取持久任务、上报心跳、标记执行并提交成功/失败；独立租约线程在长任务期间自动续租，协作式 shutdown 会取消在途任务且不把退出误记为 IMAP 故障。真实邮件执行器仍通过无 HTTP 的进程内 trait 注入，尚未切换正式路径。
- 新增 `EmbeddedSyncExecutor` 生产装配器：任务执行时从数据目录加载主密钥和账户 owner，读取现有 mailbox/role 游标，按需刷新并加密持久化 OAuth Token，再直接调用真实 IMAP Adapter、`MailboxSyncApplicationService` 和 SQLite 原子提交；整个调用链不经过 HTTP。排队后被删除的账户归类为不可重试的 `ACCOUNT_NOT_FOUND`。
- 新增动态账户 watcher manager：定期读取启用策略并增删每账户 watcher，策略停用和账户删除会取消对应 watcher；真实 watcher 使用同一 OAuth 刷新和 IMAP IDLE/STATUS Adapter。唤醒只进入持久队列，Changed 映射为高优先级 `recovery`，Reconcile 映射为 `scheduled`，保持 Node 既有 reason 契约。
- 新增无前端策略校准器：启动延迟后扫描 `startup`，再按间隔扫描 `scheduled`；初始化缺失策略、展开 inbox/standard/selected、跳过授权暂停与未到期退避，到期失败转 `recovery`，并清理七天前事件。
- 真实 IMAP/SMTP Adapter 新增 50ms 轮询的取消探针，worker shutdown、租约丢失和 watcher 停用可以打断 pending 网络 future。能力协商会识别 QRESYNC 并选择当前 async-imap 可公开调用的 CONDSTORE/CHANGEDSINCE 路径；因 0.11.3 不公开 QRESYNC SELECT/VANISHED API，删除仍用 500 UID 分批核对，保证正确性而不依赖不可访问接口。
- runtime 新增进程内 health snapshot，记录 worker 开始/成功/失败/取消、scheduler 扫描/错误、活跃 watcher 与断线/重连次数，可由未来 Tauri command 和可选 HTTP Adapter 读取同一状态。最终 complete/fail/cancel 写入在租约续期期间重试，避免 SQLite 瞬时忙留下 running 空壳。
- 临时 SQLite 与协议替身测试现覆盖去重、租约失效/回收、游标提交、rerun、授权暂停、策略停用、worker 健康、缓存/联系人原子提交与回滚、首次同步通知抑制、UIDVALIDITY 重建计数、策略目标展开、重试曲线、错误脱敏、OAuth 刷新持久化、worker 成功/失败持久化、在途取消、watcher 动态启停/可靠入队、断线/重连观测、无前端三目标校准、退避到期 recovery、网络 future 取消、五账户三 worker 同优先级进度以及远程写入双 guard。完整 Rust workspace 现为 93 项测试，Node 仍为 57 个测试文件/318 项。

本批次门禁已通过：完整 Rust workspace 测试、严格 Clippy、Rust 1.77.2 全目标编译、TypeScript typecheck、Node 测试与生产构建。所有 Rust 写入测试只使用系统临时目录；R0 快照与当前 `.data` SHA-256 保持不变。

现有数据副本预检已完成：新 R5 snapshot 的 Node/Rust 字段与凭据摘要一致；最终写入副本完成 4 个历史 due job 和 1 个 rerun recovery，5 次执行全部成功、最终 queued 为 0，16 个既有 UIDVALIDITY/UID/MODSEQ 及领域数据计数全部保持。证据见 `docs/rust-migration-r5-data-copy-preflight.md`。

专用邮箱验收驱动已实现为 `npm run rust:mail-acceptance`：默认拒绝运行，要求专用账户/远程写入双 guard，验证连接、自投递、MIME、附件、flags、真实取消、重连和归档，不删除邮件且只输出脱敏报告。配置见 `docs/rust-mail-acceptance.md`。

离线资源门禁已实现为 `npm run rust:runtime-soak`：只接受带备份清单、无 queued job 的数据副本，拒绝 `.data` 与覆盖报告；60 秒 release 基线的 RSS 峰值为 9,670,656 bytes，零任务且优雅关闭。证据及限制见 `docs/rust-migration-r5-resource-report.md`。

下一批：准备专用邮箱的真实断网重连、取消、Node/Rust 网络增量快照和数小时以上资源验收。当前正式运行路径仍为 Node。

## 阶段 R6：完成 Rust HTTP、Gateway、MCP 与 Docker 等价服务

**目的**：先得到完整可替换 Node 服务端，再开始桌面直连改造。

交付物：

- 可选 HTTP Adapter：认证 API、应用 API、SSE、附件、Logo 和 OAuth callback。
- Gateway REST、OpenAPI、WebSocket 和 Token scope。
- MCP Streamable HTTP 及全部现有工具。
- Web 静态托管、SPA fallback、CORS、Host/Origin、可信代理和安全响应头。
- Docker `linux/amd64` 构建、健康检查、非 root 运行、备份、恢复和升级预检。
- Rust 服务启动参数明确区分默认核心运行与 `--http --host --port` 远程模式。

测试：

- 现有前端不修改，通过 Rust HTTP 服务运行全部功能和浏览器冒烟。
- 现有 HTTP、SSE、WebSocket、Gateway 和 MCP 契约测试对 Node/Rust 运行相同用例。
- 在迁移数据副本上执行 Docker 持久卷升级、重启、备份恢复和非覆盖回滚。
- 比较 Node 与 Rust 的空闲、多账户 IDLE、批量同步和大邮件内存基线。
- 完成真实反向代理下的 Cookie、OAuth、SSE、WebSocket、MCP Host/Origin 验收。

完成标准：不修改现有 Web 客户端即可把测试环境的 Node 服务替换为 Rust HTTP 服务；Docker 发布能力等价；所有真实数据仍保留。

回退：停止 Rust 服务并重新启动 Node 服务。首次在真实目录切换前必须确认 Node 能读取 Rust 当前 schema；否则只允许恢复切换前快照到新目录，禁止覆盖原目录。

### R6 首批完成记录（2026-08-10）

已新增独立 `imail-http` crate。`BridgeMode` 默认无 HTTP，仅显式 `--http` 选择网络桥接；公开健康/服务信息、v4 instance identity 非覆盖创建与复用、production Host 白名单、严格 CORS 来源及基础安全响应头已通过测试。注册、登录、会话状态/查询与退出已复用 Rust 持久认证库；production 初始化注册具备并发事务门禁，实际 listener 注入客户端地址用于双层登录限流，Cookie 不暴露到 JSON。统一 Session middleware 已向偏好、账户、同步和草稿路由注入可信用户身份与来源 actor；账户 presenter 额外白名单化 settings/proxy，owner、密文和故意夹带的明文凭据均不会响应。密码/代理更新与连接测试已接入主密钥、OAuth 刷新协调和真实 IMAP/SMTP 候选验证，失败候选不写入且连接错误先脱敏。手动同步入口、默认/账户同步策略、状态快照、任务详情和 SSE 增量流已接入有界租户视图；账户删除复用既有级联事务并记录安全事件。草稿 CRUD 已复用领域服务并保持 `X-Draft-Id` 幂等和附件边界，JSON 请求上限对齐 Node 的 25 MB。正式 HTTP/Docker 宿主现在默认装配 Rust worker pool、scheduler 与 IDLE watcher，复用现有 `IMAIL_SYNC_*` 调优参数，并与 HTTP 邮件操作共享 OAuth 刷新协调器；只有运行时成功启动才报告 `syncWorker=true`，关闭后必须清除 heartbeat。隔离契约与迁移副本测试显式关闭 worker，避免连接真实邮箱。完整范围与未完成项见 `docs/rust-migration-r6-report.md`。

后续增量已完成普通账户创建的连接候选验证、owner 范围原子唯一写入和同步策略初始化；OAuth PKCE 开始、原账户重连、公开 callback 与 owner 隔离的完成查询也已闭环。同一 state 的重复或并发 callback 由 in-flight 门闩和限时完成记录保护，不会重复换 Token，回调 HTML、错误响应和审计均不包含邮箱凭据。

邮件 HTTP 增量现已覆盖有界 SQL 查询、详情、统计、联系人、标签、通知、远程 flags、移动、发送与 MIME 附件流；所有入口先执行 Session owner 约束，远程失败不提交本地标记，OAuth 账户在连接前使用共享协调器刷新。Logo 缓存兼容 Node 的哈希文件；未命中时 Rust 发现端口执行同域候选、逐跳 DNS 全地址公网校验、固定地址连接、Host/TLS SNI 保留、重定向重验、内容上限、图片魔数、失败记录、负缓存和主域 singleflight。代理头默认无效，只有宿主显式启用一跳可信代理时才影响审计来源和 HTTPS Cookie，畸形可信转发链直接拒绝。

开发者控制面与 Gateway 增量已接入：应用 Session 可管理按用户隔离的外部访问开关和短期 Token，公开列表不包含授权码、哈希、owner 或内部账户 ID。Gateway capability 开启后提供 REST、OpenAPI/docs 与 WebSocket；所有入口复用 scope、账户范围和即时开关校验，邮件分页在 SQLite 内有界执行，附件/发送在网络前再次授权。WebSocket 校验 Host/Origin，支持 header/首帧认证，并在每次事件轮询重新验证撤销与过期；Rust 同步执行器只把非首次同步的新邮件写成无正文的 `message.created` 摘要事件。

MCP 已接入显式 capability：默认不挂载，启用后每次请求重新验证 `mcp:full`、授权码归属和用户开关，支持初始化、ping、29 项工具发现与全部现有工具执行。设置、主题、账户、OAuth、同步、邮件远端状态/移动/附件/发送、草稿、标签和通知均复用 Rust 领域服务；敏感更新执行主密钥解密、候选连接验证和成功后持久化，公开账户字段采用白名单。工具契约最初由 Node 官方 SDK 生成，迁移完成后改由 Rust 实现直接嵌入并在 Rust 测试中校验；HTTP Accept/Content-Type/协议版本拒绝、2025 批处理、2026-07-28 `server/discover`/per-request envelope、草稿服务端边界和管理审计不含 Token 已通过测试，官方 TypeScript SDK 已真实协商并调用 Rust。迁移阶段的基础 REST 双实现契约已完成使命并随旧 Node 实现删除，长期门禁以固定协议清单和 Rust HTTP/SDK 互操作测试为准。

数据回读门禁已新增：R5 保留快照先以只读方式生成摘要并复制到系统临时目录，Rust 正式宿主只打开该副本；Rust 停止后 Node 摘要和 Node 正式宿主均能重新读取同一副本，账户、邮件、草稿、联系人和 Developer Token 摘要保持一致。测试复核保留快照源数据库哈希与领域摘要未变化，不删除现有测试数据，也没有让 Rust 打开当前 `.data`。

真实浏览器基础门禁已完成：当前生产 Web 构建由 Rust 同源托管后，可完成首次注册、应用启动、Session 重启恢复、偏好修改、MCP 开关和一次性授权码创建，常规流程控制台无错误；独立 Origin 页面验证带凭据 CORS 精确放行、未列 Origin 拒绝和未知 Host 421。浏览器传输增量还覆盖了 EventSource 在 Rust 重启后的自动恢复、Gateway WebSocket 首帧认证和 Token 撤销后 1008 关闭。验收发现并修复了活跃 SSE 阻塞 Axum 优雅退出的问题，宿主现在会在 shutdown 时广播取消 SSE/WebSocket，并以保持真实 SSE 连接的自动化用例做回归。本机 HTTPS 流式反向代理进一步验证了现有 Node 部署变量兼容、安全 Cookie、HSTS、未缓冲 SSE、`wss://` Upgrade 与 OpenAPI 文档；关键 OpenAPI 结构已加入自动化断言。该门禁只使用隔离合成目录，详见 `docs/rust-migration-r6-browser-acceptance.md`。真实邮箱 OAuth 与发布拓扑有效证书/Caddy 仍属于 R6 后续项。

Rust Docker 候选现已补齐无 Node runtime 的 `imail-maintenance`，提供在线 backup、非覆盖 restore 与只在新副本迁移的 upgrade-preflight。Rust 自动化验证 v2 完整性清单、现有目标拒绝、主密钥/实例身份/Logo 保留和源数据不覆盖；容器冒烟脚本会先由当前 Node 镜像在唯一命名卷中创建身份、用户、偏好与外部访问设置，再把同一 `/data` 卷切换给 Rust，验证 identity、密码登录和设置逐值保留。Rust 随后再次从同一卷启动，并在只读根文件系统、非 root 用户下执行 backup、restore、重复 restore 非覆盖拒绝和 upgrade-preflight，再验证 healthcheck 与两次优雅停机。该脚本已作为现有 `docker` 工作流登录和发布前的失败门禁，但不替换正式 Node Dockerfile，也不推送 Rust 候选。该门禁现已通过 WSL2 Docker/Buildx 在真实 `linux/amd64` 镜像内执行，不可覆盖报告与哈希记录在 `docs/rust-migration-r6-report.md`。

应用路由清单复核已补齐此前缺少的服务商目录、安全审计、授权导出、用户邮件数据清理和桌面守护停服。`contracts/application-http-routes.json` 现固定 59 个应用方法/路径，自动化会分别从 Express 与 Axum 生产路由抽取并拒绝任一侧漂移。Rust 测试实际解密 `.imailauth` 证明仅导出当前用户白名单凭据，并覆盖跨用户 404、一次性下载、两分钟待处理状态、持久密码复核限流、清理事务范围和待下载失效；双宿主契约也分别使用 Node/Rust 实例自己的主密钥种入凭据、下载并实际解密，再比较授权白名单和清理后失效。真实 Rust 子进程还使用当前 Tauri 的控制文件参数完成错误令牌拒绝与 202 优雅停服。至此应用 HTTP 路由已有完整 Rust 对应实现；Linux 容器外部门禁已经完成，但真实公共邮箱门禁完成前仍不进入 R7。

真实协议 fixture 已进一步穿透 HTTP/worker/SQLite：自定义账户创建先通过真实 TLS IMAP/SMTP 验证，手动同步由持久 worker 领取并用 IMAP literal 写入缓存，再从 HTTP 列表与详情读回；发送端点通过真实 SMTP DATA 投递并重新解析服务端收到的 RFC822。同步 adapter 的创建由 `SyncMailTransportFactory` 注入，生产默认实现不变，Tauri 后续可直接复用相同 `EmbeddedSyncExecutor`。该 fixture 不访问公共邮箱，仍不能替代专用账户和服务商扩展验收。

该正式链路现增加可控 TLS 故障：首个 worker IMAP 登录后连接被强制关闭，任务失败安全落库且缓存保持为空；第二次 HTTP 手动同步重新建连并成功提交。恢复邮件的 2 MiB 附件通过真实 IMAP 再次下载、逐字节核验，并由真实 SMTP DATA 发送后从 RFC822 中再次提取核验。由此本地断线恢复和大附件协议路径已有自动化证据，但公共邮箱长时间 IDLE/限流/网络抖动仍不能由 loopback fixture 替代。

真实 TLS fixture 现继续覆盖正式 IDLE watcher：首连登录后断开，生产 watcher 按与 Node 一致的 500ms 起步、30 秒封顶指数曲线重连；第二条连接执行 `CAPABILITY IDLE`、`EXAMINE`、continuation 和 `EXISTS`，随后生成唯一 recovery 持久任务，由 worker 在并发 IMAP 连接上 FETCH 并提交 SQLite。health 断线/重连/成功/失败计数和停机取消活跃 IDLE 均被断言。短时 loopback 恢复门禁已完成，公共邮箱数小时 IDLE、NAT/代理超时和服务商限流仍属于外部门禁。

真实 TLS fixture 已增加 30 秒至 24 小时的显式长稳入口：默认拒绝，报告只允许写入保留输出目录且绝不覆盖，运行时逐秒采样 RSS、队列、watcher 与 heartbeat。首份 60 秒 release 证据包含一次断线/重连、一个 recovery、scheduler 在第 60 秒生成的三个 scheduled 任务和 59 个资源样本；全部任务成功、最大 queued 为 0、停机无 heartbeat，峰值 RSS 20,897,792 bytes、首尾增长 24,576 bytes。该证据补齐了可重复的本地真实网络长稳机制，但仍不把一分钟 loopback 扩大解释为公共邮箱数小时门禁通过。

OAuth fixture 也已贯穿同一正式链路：宿主注入的 `OAuthProviderPortFactory` 和 `OAuthConfigResolver` 会统一进入 HTTP callback、账户操作、worker 与 watcher，生产默认仍解析到既有 Google/Microsoft/Yahoo 端点。隔离测试实际执行本地 HTTP Token/UserInfo、PKCE、TLS IMAP/SMTP XOAUTH2、过期 Token 单次刷新、加密持久化、同步提交与发送，并断言响应不泄漏 Token。完整离线门禁现为 Rust workspace 131 项、Node 66 个文件/332 项，另有 1 项显式真实 TLS 长稳验收独立通过，Linux Docker 运行证据也已取得；公共服务商真实账户仍是进入 R7 前的外部门禁。

专用邮箱验收驱动已扩展为完整交互式 OAuth 入口和现有四账户闭环入口。闭环入口只读打开测试数据库、凭据只保留在子进程环境和内存，双层强制恰好四地址集合、跨账户单收件人且无 CC/BCC；Node supervisor 在验收期间优雅暂停，结束后恢复。2026-08-11 当前桌面测试库凭据预检通过且数据库哈希未变，但 Gmail→iCloud、Gmail→Outlook 两次投递均在 SMTP 接受后 180 秒内无法由接收方 IMAP 确认，后续边被立即停止；未向集合外发送、未删除邮件。失败报告 v1/v2 保留，查明公共投递可见性前不重复发送，也不进入 R7。

该增量完成后的本地门禁为 Rust 133 项、Node 67 个文件/334 项，typecheck、production build、严格 Clippy 与 rustfmt 全部通过。

只读复核随后确认两封测试邮件实际分别进入 iCloud Junk 和 Outlook Junk，发送方 Gmail Sent/All Mail 也存在对应唯一主题；先前 180 秒失败是验收驱动仅查询 Inbox 的误判，并非 SMTP 未投递。驱动已扩展为 Inbox + Junk 双落点查询并记录脱敏角色，本轮没有再次发送或修改邮件。四边闭环仍待重新执行，因此 R7 门禁状态不变。

四账户闭环 v4 已于 2026-08-11 通过：Gmail→Outlook 复用既有 Junk 邮件完成状态与归档，Outlook→QQ、QQ→iCloud、iCloud→Gmail 各新发一封；四边均验证 IMAP/SMTP、MIME、附件、已读/星标往返、真实取消、重连和归档，落点为两次 Inbox、两次 Junk，删除始终为 false。闭集严格保持四个地址且无 CC/BCC，测试数据库执行前后 SHA-256 一致；报告 `output/rust-migration-tests/r6-public-mail-closed-ring-v4.json` SHA-256 为 `1AFC918F4E1E64D98182A7B3C88EBCA2BA8666F3182DA0FA798BDB4FA4D6382D`。旧 Node supervisor 在验收后恢复，R6 公共邮箱门禁完成，可以进入 R7。

Windows production 资源对照已从空闲扩展到缓存大邮件查询：合成 5 个账户、500 封双 16 KiB 正文和单封 2 MiB 详情，每批 8 个并发请求且四组各传输约 41 MiB；Node/Rust 在采样前分别通过列表正文裁剪、总数和详情长度契约。Rust 仅宿主和 3-worker 峰值分别比 Node 低 89.50% 与 95.51%，四组均优雅退出且无遗留进程。该短时合成结果不替代真实 IMAP/MIME/附件和长稳测试，细节见 `docs/rust-migration-r6-resource-comparison.md`。

合成持续读取门禁现进一步覆盖完整拓扑各 60 个负载后采样点、61 批 8 路并发请求和约 492 MiB 返回数据，并在结束时复验数据契约。Rust 峰值/中位 Working Set 比 Node 低 93.47%/92.49%，双方均优雅停机且无残留进程。Rust 首尾 10 点窗口中位增加约 5.64 MiB，短窗口不足以证明或否定泄漏；因此五分钟可重复入口已经建立，但数小时真实 IMAP IDLE、断网恢复、MIME/附件峰值仍是进入 R7 前的外部门禁。

## 阶段 R7：引入前端领域客户端与 Tauri 直连

**目的**：在服务端已经等价后，将桌面本地调用从 HTTP 切换为进程内 Rust。

交付物：

- `MailService` 等领域客户端接口和 React provider。
- `HttpMailService` 保持 Web及桌面远程模式行为。
- `TauriMailService` 映射到 Rust command，并使用 Tauri Event 接收同步状态和邮件增量。
- Rust 宿主管理登录用户上下文、服务生命周期、取消和二进制读取。
- 本地模式不再使用服务 URL、Cookie Jar、SSE 或回环业务端口。
- 远程模式继续使用当前 HTTPS、Cookie 隔离、下载和事件桥。
- 本地/远程切换只改变 adapter 和数据源，不复制数据或复用会话。

测试：

- 对同一领域客户端契约分别运行内存 Tauri adapter 和 HTTP adapter 测试。
- 覆盖初始化、登录、加载邮件、同步、写信、附件、设置、Token、MCP 展示和隐私操作。
- 覆盖本地/远程反复切换、迟到响应、事件取消和远程失败不回退。
- 验证本地模式没有常驻业务 HTTP listener，且 WebView 不能直接取得凭据或数据库路径。
- Windows 实机验证窗口隐藏后持续同步、托盘恢复、显式退出后停止以及再次启动后的任务恢复。

完成标准：桌面本地所有功能经 Tauri 直连运行，远程和 Web 仍经同一 Rust HTTP 服务运行，用户数据无需重新创建或重新授权。

回退：保留一个受控版本开关将桌面本地 adapter 切回 Rust HTTP 服务；回退只切换 adapter，不回滚或删除数据。

### R7 首批增量（2026-08-11）

- 新增 `MailService` 客户端边界、`HttpMailService` 和 `TauriMailService`。Web 与远程桌面继续使用原 HTTP adapter；只有 Tauri、本地模式、构建期开关同时满足时才选择直连命令。直连请求不携带 base URL，非字符串请求体在 WebView 边界前拒绝。
- Tauri 新增 `desktop_mail_service_request` 与惰性进程内 Rust host。首个门禁直接对 Rust application Router 执行内存调用，不创建 listener；测试已从临时数据目录读取 `/api/system/info`。旧本地服务 `enabled` 标记存在或运行时环境开关未显式启用时，命令拒绝初始化，防止两个实现打开同一数据目录。
- 该增量仍是内部关闭状态，尚未迁移二进制读取，也没有对当前真实数据启用；现有 Node 回退路径与远程 Cookie/SSE 桥保持不变。Tauri host 已截获并持有本地 Session，WebView 请求/响应不接触 Cookie，临时数据库中的注册→认证状态读取通过。下一批需要装配同步生命周期后，才允许扩大本地 API 覆盖面。
- 进程内事件增量已接通：Tauri host 直接消费 Rust Router 的 SSE body 并发射既有 `imail-sync-event`，不创建 EventSource 或网络连接；最后一个前端订阅释放时调用独立 stop command 中止 Rust task。远程与 Web 仍使用原 SSE 桥。同步 worker 目前仍关闭，因此下一批必须装配窗口隐藏后的运行时生命周期并加入类型化领域 command。
- 本批完整回归：Node 68 个文件/338 项、Rust workspace 134 项通过（另 1 项显式长稳按设计忽略）、Windows Tauri 17 项通过；typecheck、production build、Rust workspace/Tauri 严格 Clippy 与双 workspace rustfmt 均通过。

### R7 第二批增量（2026-08-11）

- `imail-http` 新增 `EmbeddedServiceHost`，不绑定 TCP，直接持有 Router、持久同步 worker/scheduler/IDLE watcher 与连接关闭广播。Tauri host 初始化时启用完整同步运行时，窗口隐藏后继续工作；显式退出先取消事件流并优雅关闭运行时，Drop 提供兜底。隔离测试通过 `/api/system/info` 确认 `syncWorker=true`，关闭完成后才清理唯一临时目录。
- 本地附件与 Logo 使用新的 Tauri binary command 从进程内 Router 读取，下载命令直接写入用户选择的目标；命令参数不含服务 URL，远程桌面仍走原 HTTPS/Cookie bridge。
- 首批类型化 command enum 已覆盖认证状态、账户列表、邮件统计、结构化邮件查询与邮件详情。前端 `TauriMailService` 自动将这些 GET 路径转为领域调用；未迁移操作暂留进程内兼容层，远程/Web adapter 不变。
- 本批仍不启用真实数据开关：当前 Node supervisor/service/worker 是唯一真实数据写入者，仓库测试数据库未由嵌入式宿主打开。R7 后续仍需显式用户上下文、其余领域命令、直接领域事件和 Windows 实机无 listener 冒烟。
- 第二批完整回归：Node 68 个文件/340 项、Rust workspace 129 项通过（另 1 项显式长稳按设计忽略）、Windows Tauri 17 项通过；typecheck、production build、Rust workspace/Tauri 严格 Clippy 与双 workspace rustfmt 均通过。

### R7 第三批增量（2026-08-11）

- 嵌入式 Tauri Event 已移除 Router SSE body 与 SSE 文本解析。host 内 Session 直接解析用户，账户服务生成 owner 白名单，事件任务从当前最新游标轮询 `sync_events` 并只发布允许账户的结构化事件；初始与每 15 秒 `sync.status` 仍通过进程内 Router 获取，不产生网络连接。取消订阅、重新订阅和应用退出继续中止旧任务。
- 类型化 command 扩展到全局同步、单账户同步、账户文件夹同步、角色文件夹同步、邮件更新和邮件移动；Rust 从结构化字段重建安全路径及 JSON，请求不携带 base URL 或 Cookie。远程与 Web 继续使用 Rust HTTP adapter。
- 事件初始化测试现同时验证 host 内 Session 可解析为事件数据目录、空账户范围与初始游标；临时宿主随后显式关闭。下一步继续迁移认证写操作、账户/OAuth、发信、草稿、设置和控制面，并为事件增加 channel 唤醒和 Windows 实机门禁。
- 第三批完整回归：Node 68 个文件/341 项、Rust workspace 129 项通过（另 1 项显式长稳按设计忽略）、Windows Tauri 17 项通过；typecheck、production build、Rust workspace/Tauri 严格 Clippy 与双 workspace rustfmt 均通过。

### R7 第四批增量（2026-08-11）

- 当前 React UI 的全部 API 调用均已映射为 tagged Tauri command：认证、账户/OAuth、邮件读写与同步、发信、草稿、偏好、Token/MCP 外部访问、授权导出及用户数据清理。未知本地操作立即失败；`desktop_mail_service_request` 已从 Rust command handler 删除，WebView 不再拥有传递任意 method/path/body 的入口。
- 草稿创建幂等头不再在嵌入模式丢失：前端抽取 `X-Draft-Id` 为 `draftCreate.draftId`，Rust 只在该 variant 上重建内部请求头。类型化路径段由 Rust 编码，远程/Web 协议不变。
- 认证启动和服务设置已区分嵌入式本地与旧守护模式。嵌入式构建不会启动或探测旧 Node 守护、不会显示端口冲突/暂停/移除入口，本地身份通过 `systemInfo` tagged command 验证；远程切换仍先验证 HTTPS 身份。
- 嵌入式 Session 由 Rust host 在应用私有目录持久化，WebView 只收到认证结果而不接触 token。重启后的 host 从 token 解析显式用户 ID；事件账户范围直接使用该上下文。隔离测试覆盖重启恢复及 logout 删除 Session 文件。
- 第四批完整回归：Node 68 个文件/346 项、Rust workspace 129 项通过（另 1 项显式长稳按设计忽略）、Windows Tauri 18 项通过；typecheck、production build、Rust workspace/Tauri 严格 Clippy 与双 workspace rustfmt 均通过。

### R7 第五批起步（2026-08-11）

- 内部 Router 转译开始按领域移除。`authStatus` 直接使用 `SqliteAuthStore`，`accountsList` 与 `draftsList` 直接使用 `AccountService`/`DraftService`；三者共享 host 持有的显式用户 ID，未登录仍返回原 401 JSON 契约。
- 直接调用前统一执行 host 初始化，因此 Session 恢复、同步 worker/scheduler/IDLE watcher 启动和单数据目录约束不会被绕过。隔离 Tauri 测试已同时比较直接认证状态、空账户和空草稿读取。
- 后续按同一模式下沉邮件查询、偏好、同步控制和写操作；所有 tagged command 都直接领域调用后，才删除进程内 Router 兼容门面。

### R7 第五批扩展（2026-08-11）

- 直调读取现覆盖邮件列表/详情/统计、标签、联系人、通知、偏好、Developer Token 列表和外部访问设置。邮件查询继续使用 `MessageQueryService`，联系人 Logo presenter 与 HTTP adapter 共用，因此列表裁剪、分页字段、Logo 回退和 404 错误正文保持一致。
- 草稿创建/更新/删除、偏好更新、外部访问更新已从内部 Router 转译移出。草稿 payload 校验提取为 Tauri/HTTP 共用的纯 Rust helper，`draftId` UUID 与幂等语义不变；设置写入直接使用 `PreferencesService`、`ExternalAccessService` 的现有领域校验。
- 隔离 Windows Tauri 测试逐项对照直调与 HTTP adapter 的 JSON/状态码，覆盖空读取、缺失邮件、有效与空设置更新、草稿缺失账户和幂等删除。测试只使用唯一临时目录，不打开或修改当前测试数据库。
- Developer Token 创建/撤销已完成直接下沉。新增共享 `DeveloperTokenService` 统一账户邮箱归属、scope 规范化、`mcp:full` 收敛、签发公开视图及创建/撤销审计；HTTP handler 已收敛为协议适配，Tauri 不复制安全规则。
- 隔离契约测试分别从直调和 HTTP adapter 签发真实随机 MCP Token，确认明文只返回一次且不出现在列表，公开 detail 不含 owner/account/token hash，混合 scope 收敛为 `mcp:full`，两条创建与两条撤销审计落盘，最终两端撤销均为 204。
- 下一步按相同原则提取认证应用服务；注册/登录的持久限流、Session 和安全审计不得直接复制进 Tauri command。

### R7 第六批增量（2026-08-11）

- 新增共享 `AuthenticationService`，统一认证状态、Session 解析、注册、登录和退出。原 HTTP handler 中的输入校验、首用户/开放注册门禁、持久来源/账户限流、登录审计与 Session 生命周期已下沉；HTTP adapter 只保留来源 actor 解析、Cookie 安全属性和 `Retry-After` 响应头。
- 注册流程继续先执行门禁和来源限流、再处理无效输入，并通过 SQLite `IMMEDIATE` 事务保证只有一个并发首用户成功；登录继续先校验输入，再消耗来源和账户限流，成功后清除账户限流并写审计。登录名仍使用与 Node/Rust 原实现一致的 Unicode `L/N` 正则和 UTF-16 长度边界。
- Tauri 的 `authRegister`、`authLogin`、`authLogout` 与 `authStatus` 已直接调用共享服务。raw Session 只存在 Rust host 内存和应用私有文件中，WebView 响应不含 Cookie 或 token；退出撤销数据库 Session 后清理文件，重启恢复测试通过。
- 双隔离宿主契约测试分别经直调与 HTTP 完成注册、状态、错误登录、成功登录和退出，核对状态码/公开字段、确认 Session 不泄漏，并验证 `registration.succeeded`、`login.succeeded`、`logout` 审计各自落盘。当前测试数据库未被打开。
- 下一批迁移账户元数据、凭据、代理和删除；连接测试、OAuth 与网络写操作仍需继续共用现有端口和安全验证，不能在 Tauri command 内缩减校验。
- 第六批完整回归：Node 68 个文件/346 项、Rust workspace 129 项通过（另 1 项显式长稳按设计忽略）、Windows Tauri 19 项通过；typecheck、production build、Rust workspace/Tauri 严格 Clippy 与双 workspace rustfmt 均通过。仓库测试数据库 SHA-256 保持 `074B3D437ADDDEBE3BD8020C3DAA18B825DE440EDA34AD01838979574BD72000`，旧 Node manager 与两个 service 继续运行。

### R7 第七批增量（2026-08-11）

- 账户公开视图改为 HTTP/Tauri 共用 presenter，修复非空账户直调可能额外序列化 `ownerId` 的边界；settings 与 proxy 继续只输出协议白名单字段，凭据、代理密码和密文均被排除。含敏感占位字段的隔离账户已完成直调/HTTP JSON 全等测试。
- 非 OAuth 账户创建、元数据更新、密码替换、代理更新、连接测试和删除已接入直接应用入口。`EmbeddedServiceHost` 现持有构建 Router 时的同一 `AppState`，原生入口直接复用主密钥、连接 probe、OAuth refresh 依赖、同步策略初始化和审计编排；HTTP handlers 也调用这些函数，不存在第二套敏感规则。
- 进程内宿主测试在注入的无网络 probe 上实际完成账户创建、换密、显式 SOCKS5 代理和连接测试，确认持久密文不含明文，并验证创建、凭据更新、代理更新审计。Tauri 契约测试覆盖安全列表、元数据规范化、无效输入、404、204 与删除审计；缺失账户路径在进入网络探测前返回。
- OAuth start/reconnect/status 尚保留内部 Router 兼容门面，下一批需复用现有 PKCE state、刷新协调器和 callback 生命周期直接下沉，不能把 OAuth token 或 client secret 暴露给 WebView。
- 第七批完整回归：Node 68 个文件/346 项、Rust workspace 130 项通过（另 1 项显式长稳按设计忽略）、Windows Tauri 20 项通过；typecheck、production build、Rust workspace/Tauri 严格 Clippy 与双 workspace rustfmt 均通过。测试数据库哈希保持不变，旧 Node manager 与两个 service 继续作为真实数据唯一写入者。

### R7 第八批增量（2026-08-11）

- OAuth start/reconnect/status 已接入 `EmbeddedServiceHost` 直接应用入口。HTTP adapter 与 Tauri 复用相同的输入规范化、PKCE/state、callback 完成、账户持久化、连接探测、刷新协调和公开 presenter；桌面不再为 OAuth 业务调用内部 Router。
- 桌面授权按单次流程创建严格 loopback listener，端口可由系统动态分配。listener 只识别预期 callback path 与 state，不承载任何业务 API；宿主关闭广播可主动取消等待并释放端口。
- token exchange、身份校验和加密写入完全留在 Rust 侧；WebView 只取得授权 URL、opaque state 和公开完成结果。隔离端到端测试验证动态回环、一次交换、owner 状态隔离、安全账户响应和密文不含 token 明文。
- 下一批继续迁移邮件写操作、同步控制、发信、授权导出和隐私操作，并将事件日志轮询替换为进程内唤醒 channel。真实数据与正式 Node 写入路径保持不变。
- 第八批完整回归：Node 68 个文件/346 项、Rust workspace 137 项通过（另 1 项显式长稳按设计忽略）、Windows Tauri 20 项通过；typecheck、production build、Rust/Tauri 严格 Clippy 与格式检查通过。当前测试数据库哈希保持不变。

### R7 第九批增量（2026-08-11）

- 邮件更新、移动、发送以及全局/账户/指定邮箱/邮箱角色同步已从 Tauri 内部 Router 转译下沉到 `EmbeddedServiceHost`。邮件路径复用相同的校验、OAuth refresh、IMAP/SMTP、密钥 codec、持久化和公开 presenter；同步路径复用账户归属、canonical mailbox target 与持久任务队列。
- Tauri/HTTP 契约覆盖邮件无效输入、缺失消息、无效/缺失同步目标和空账户全局同步。测试均在唯一临时目录内完成，未连接公网、未发送邮件，也未打开当前测试数据库。
- 第九批回归：Rust workspace 137 项通过（另 1 项显式长稳按设计忽略）、Windows Tauri 20 项通过，严格 Clippy 与双 workspace 格式检查通过；同一工作树本轮 Node 68 文件/346 项、typecheck 和 production build 已通过。
- R7 剩余直接下沉项为授权导出、隐私清理及事件主动唤醒；完成后再进入 R8 真实数据的备份预检与单写入者切换。

### R7 第十批增量（2026-08-11）

- 授权导出准备/一次性下载和隐私清理已直接接入 Rust host，并共用持久限流、重新认证、逐用户锁、TTL、单次消费、内存清零和安全审计。隔离成功/失败测试通过。
- 附件、联系人 Logo 与授权导出二进制读取已移除 Router 路径；未知二进制 API 直接拒绝。生产 `desktop_mail_service_call` 已删除内部 Router fallback，所有 tagged command 均为直接 Rust 分支。
- 同步 worker 的事件提交通过 `SyncEventSignal` 主动唤醒 Tauri；事件批次延迟不再受一秒轮询影响，15 秒定时器仅保留状态刷新。停止订阅会唤醒并取消等待任务。
- 至此 R7 代码目标完成。下一步进入 R8 的只读升级预检、数据副本验收、不可覆盖快照与单写入者切换；在副本门禁通过前，当前 Node 服务仍是唯一真实数据写入者。
- R7 最终回归：Node 68 文件/346 项、Rust workspace 138 项通过（另 1 项显式长稳按设计忽略）、Windows Tauri 20 项通过，typecheck、production build、严格 Clippy 与格式检查通过。

## 阶段 R8：桌面数据原地切换与移除本地 Node 运行时

**目的**：完成 Windows 交付形态切换，并安全停止旧守护架构。

进展（2026-08-11）：已对当前 Windows 内测安装数据完成在线只读源预检，创建不可覆盖的一致性 backup 和离线 preflight 副本；schema、完整性、外键、表计数、五类模型 digest 与四账户凭据兼容摘要一致。专用 `imail-embedded-preflight` 随后在副本上以无 HTTP、无同步 worker 模式完成 Rust Host 首启，且数据库、主密钥、实例身份哈希均未变化。桌面切换事务也已接入默认 Tauri 本地模式：单次门闩停止并确认旧 API/supervisor 退出，停机后创建不可覆盖快照，执行完整性、全部凭据解密和无网络 Rust Host 首启；哈希未变的失败才自动恢复旧守护，成功前不删除任何旧运行文件。旧 HTTP Session 可在严格目录与回环身份匹配后导入 Rust 私有 Session。详见 `docs/rust-migration-r8-preflight-report.md`。真实写入者尚未切换。

交付物：

- 桌面升级预检识别旧守护服务状态、数据目录、实例身份和 schema。
- 在旧 Node 服务停止并确认退出后，对原数据创建一致性备份，再由嵌入式 Rust 打开同一数据目录。
- 旧守护注册和 Node 运行文件只在 Rust 成功启动、登录、读取账户并完成健康校验后注销/移除。
- 移除运行文件不删除数据库、主密钥、Logo、日志和迁移快照。
- 桌面包不再携带 Node SEA、Node Worker 或本地 HTTP 端口恢复 UI。
- 设置页将“本地服务暂停/移除”改为适合嵌入模式的“后台运行/退出行为/数据位置”说明；数据删除仍只属于“隐私与数据”。
- 更新架构、打包、部署、运维、MCP、交接和上下文术语文档。

测试：

- 从当前已安装 Windows 内测版执行真实覆盖升级，验证旧守护停止、数据保留、Rust 直连启动和账户无需重建。
- 人为制造备份失败、旧服务停不下、Rust 解密失败、schema 不兼容和首次启动崩溃，确认不会删除旧运行文件或数据。
- 验证卸载仍默认保留数据，重新安装后可由 Rust 读取。
- 检查安装包和运行进程，确认不存在 Node SEA 和本地 Node Worker。
- 记录迁移前后账户、邮件、草稿、联系人、Token、Logo、同步状态计数和安全摘要。

完成标准：Windows 桌面只包含 React/Tauri/Rust 运行时；现有测试数据原地可用；升级失败存在可验证的非覆盖恢复路径。

回退：在一个过渡发布周期内保留经过签名的旧 Node 服务安装来源和切换前快照。若 Rust 首次启用失败，在没有 Rust 写入或 schema 保持 Node 兼容时恢复旧守护；否则从切换前快照准备新的恢复目录，不能覆盖失败现场。

## 阶段 R9：收敛、性能优化与后续升级

**目的**：在功能迁移完成后清理双实现并兑现资源收益。

交付物：

- 删除不再使用的 Node 服务、Express 路由、Node Worker 构建和双实现对照入口；旧实现由 Git 历史追溯。（已完成）
- 保留 Web/远程协议测试作为 Rust HTTP Adapter 的长期兼容门禁。
- 优化 Rust task 数量、IMAP 连接、MIME 缓冲、SQLite statement cache 和事件批处理。
- 建立空闲、同步峰值、长时间 IDLE、附件和多账户资源预算。
- 根据实际需求评估本地 stdio MCP；默认不为本地桌面重新开启常驻 HTTP。
- 重新评估是否需要把嵌入式核心拆为 Rust Named Pipe 守护进程。该项属于未来独立决策，不影响本路线先采用直接嵌入 Tauri。

测试：

- 运行完整 Rust、前端、Windows 安装包、备份恢复和真实邮箱验收矩阵；Docker 与跨平台验收另立计划。
- 与 R0 基线比较 Working Set、峰值、启动时间、安装包大小和同步吞吐。
- 连续运行多账户 IDLE 和周期校准，检查内存增长、任务堆积、数据库锁和连接恢复。

完成标准：Node 不再属于任何交付或运行路径；资源数据证明 Rust 迁移达到预设预算；所有兼容与迁移证据可追溯。

回退：本阶段开始前 Rust 已是唯一受支持实现；回退以发布版本和完整数据备份为单位，不重新引入运行时双写。

## 数据迁移与保留流程

任何开始写真实数据的阶段都必须执行以下流程：

1. 解析并记录真实数据目录的绝对路径，拒绝根目录、仓库根目录和安装目录。
2. 停止当前唯一写入者并确认 API、Worker、IDLE 和 SQLite 写连接已退出。
3. 创建带版本和时间标识的一致性快照，不覆盖既有快照。
4. 在快照副本上运行 Rust 升级预检和完整测试。
5. 比较迁移前后的安全清单：schema、计数、归属、游标、事件、Logo、解密抽样和完整性。
6. 只有副本验证通过后，才允许 Rust 打开原数据目录；开启前再次确认 Node 未运行。
7. 首次启动失败时保留失败现场和日志，不自动删除数据库、同步状态或旧快照。
8. 用户或明确的恢复流程决定是否回到旧实现；自动回退不能让旧实现打开它不理解的高版本 schema。

至少保留：

- 最近一次切换前快照；
- 当前版本首次成功启动后的验证快照；
- 当前开发测试数据；
- 最近一次失败迁移现场，直到原因解决并人工确认可以归档。

自动化测试生成的普通临时目录可由创建它的测试清理，但不得匹配或递归清理上述保留目录。

## 发布与回滚原则

- `R0–R5` 不改变正式运行路径，Node 始终是原数据的唯一写入者。
- `R6` 首次允许在明确选择的测试/迁移副本上把 Rust 作为完整服务运行。
- `R7` 先通过开发/内部开关启用 Tauri 直连，不立即删除 Rust HTTP 本地回退能力。
- `R8` 才从 Windows 包中移除 Node 本地运行时；移除动作晚于 Rust 对真实数据的成功校验。
- 任一时刻只允许一种实现写一个数据目录。所谓灰度只能按完整数据实例切换，不能让 Node/Rust 按接口或账户分流写同一个库。
- 不通过降级 schema、删除新表、清空缓存或重置同步游标完成回滚。
- 未经用户明确授权，不创建版本 tag，不触发 GitHub Actions `workflow_dispatch`。

## 总体验收标准

- 当前开发和测试数据在整个迁移周期内保留，最终无需重新添加邮箱、重新授权或重新全量同步。
- Rust 能从现有 SQLite 和同步游标继续工作，账户与用户归属不变。
- 桌面本地模式不运行 Node，不开放常驻业务 HTTP 端口，React 通过 Tauri 类型化接口调用 Rust。
- 桌面窗口隐藏后同步继续；显式退出后同步停止；重新启动后持久化任务安全恢复。
- Docker Rust 服务继续提供现有 Web、REST、SSE、WebSocket、Gateway、MCP、OAuth、备份恢复和升级预检。
- 本地直连、远程桌面和浏览器三种入口复用同一业务实现，响应差异只来自 transport 能力。
- 安全边界、隐私操作、Logo 规则和多用户隔离不因去除 HTTP 而弱化。
- Rust 的实际内存、启动和长期运行数据优于 R0 Node 基线，并形成持续回归预算。

## 推荐执行顺序

严格按 `R0 → R1 → R2 → R3 → R4 → R5 → R6 → R7 → R8 → R9` 推进。

`R6` 是开始改变桌面调用模型的前置门禁：只有完整 Rust HTTP 服务已经能承接现有前端和 Docker，才进入 Tauri 直连。`R8` 是数据和安装包切换点，在此之前不删除 Node 运行文件；在此之后仍保留切换前数据快照和一个过渡发布周期的恢复证据。任何阶段若只能通过删除当前数据或重建同步状态才能通过测试，该阶段不得完成。
