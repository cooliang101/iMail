# iMail 跨平台支持路线

更新日期：2026-08-12

## 当前基线

iMail 的 Node 服务迁移与 Windows Rust-only 桌面升级已经完成。当前代码以根目录 Cargo workspace 为主体：

- `crates/` 提供账户、存储、邮件、OAuth、同步、Web API、Gateway 与 MCP 等共享 Rust 能力。
- `src-tauri/` 是桌面适配层，本地模式通过类型化 command/event 进程内调用共享 Rust 服务，不开放常驻业务 HTTP 端口。
- `http-service/` 是独立 HTTP 部署入口，只负责装配监听器和容器部署，不复制领域实现。
- `frontend/` 是所有交付模式共用的 React/Vite 前端。

当前正式支持矩阵如下：

| 交付形态 | 架构 | 当前状态 |
| --- | --- | --- |
| Windows x64 桌面 | Tauri + 进程内 Rust 服务 | 已支持并完成本地验收 |
| 服务端 Docker `linux/amd64` | Rust HTTP 服务，Node 仅用于镜像构建前端 | 已发布到 GHCR；核心镜像生命周期门禁已通过，CP2 加固项进行中 |
| Linux 原生桌面 | Tauri + 进程内 Rust 服务 | 尚未支持 |
| macOS Apple Silicon / Intel 桌面 | Tauri + 进程内 Rust 服务 | 未来支持；当前不进入实施、CI 或发布门禁 |
| 服务端 Docker `linux/arm64` | Rust HTTP 服务 | 尚未支持 |

已经完成的服务迁移过程通过 Git 历史追溯，运行证据仍保留在 `output/rust-migration-tests/`；本文只维护后续跨平台开发计划。

## 目标

1. 保持一套 Rust 领域服务、一套前端和稳定的数据格式，在 Windows、Linux、macOS 与服务端容器中复用。
2. 为每个平台提供原生构建、安装、升级、卸载、通知、托盘、单实例、OAuth callback 和数据目录行为。
3. 不以交叉编译替代真实平台测试。桌面安装包默认在对应原生 CI runner 或受控实体机器上构建和验收。
4. 保持本地优先与单写入者边界；平台扩展不能清空数据库、重置同步游标或要求重新添加账户。
5. 分平台逐步开放支持。一个平台未完成签名、安装、升级和运行门禁前，不进入正式支持矩阵。

## 非目标

- 不重新引入 Node 服务、桌面 sidecar 或本地常驻 HTTP API。
- 不为不同平台复制业务实现、HTTP 路由、MCP 工具或数据库 schema。
- 不在本路线中引入移动端、浏览器扩展或多设备数据同步。
- 不把 SQLite 改为远程数据库，也不声明多个进程可同时写同一数据目录。
- 不要求 Windows 构建机直接交叉生成 Linux/macOS 正式桌面安装包。

## 必须保持的不变量

### 架构

- Tauri、HTTP 与 MCP 只能作为 adapter；共享业务继续位于 `crates/`。
- 桌面本地模式默认无常驻业务 listener；OAuth 只允许按授权流程临时绑定 loopback callback。
- `http-service/` 保持薄启动器，Docker 与桌面不得形成不同的领域行为。
- 前端 feature 只依赖领域客户端接口，不按操作系统复制页面或 API 逻辑。

### 数据与安全

- 每个平台使用操作系统认可的用户应用数据目录，数据库、主密钥、Logo、日志和迁移快照必须位于同一明确实例边界内。
- 升级和卸载默认保留用户数据；删除数据只能由明确的隐私操作触发。
- 主密钥、邮箱密码、OAuth Token、代理密码和会话 Token 不进入 WebView、日志、崩溃报告或安装脚本输出。
- SQLite schema、AES-256-GCM、scrypt、Token 哈希和同步游标格式跨平台一致。
- 自动测试只操作唯一临时目录或不可覆盖副本，不清理 `.data`、平台正式数据目录或 `output/rust-migration-tests/`。

### 发布

- Windows、Linux 桌面、macOS 和 Docker 是相互独立的构建目标；单个平台失败不能被其他平台产物掩盖。
- 普通分支推送和 pull request 只运行无发布副作用的验证，不推送镜像、不上传公开安装包。
- 未经明确授权，不创建版本 tag、不执行发布型 `workflow_dispatch`、不推送镜像。
- 所有发布产物记录版本、Git SHA、目标 triple、SHA-256 和签名/公证状态。

## 目标平台矩阵

建议按以下顺序开放，后一个目标不阻塞前一个目标稳定交付：

| 优先级 | 目标 | 建议产物 | 构建环境 |
| --- | --- | --- | --- |
| P0 | Docker `linux/amd64` | OCI 镜像 | 已发布；继续完成 CP2 的 HTTPS、完整协议和资源门禁 |
| P1 | Linux x64 桌面 | `.deb`，必要时追加 AppImage | Ubuntu x64 原生 runner |
| P2 | Docker `linux/arm64` | 多架构 OCI 镜像 | Buildx + arm64 原生运行门禁 |

macOS 桌面属于未来支持，不参与上述近期优先级。需要启动时另行确认 Apple Silicon/Intel 范围、原生 runner、Developer ID、签名、公证和长期维护资源。

Linux 桌面首期只承诺一个经过验证的发行版基线。其他发行版只有在 WebKit、托盘、通知和系统库兼容矩阵通过后才能列入支持范围。

## 阶段 CP0：冻结 Windows 基线并建立平台审计（已完成）

完成日期：2026-08-12。审计清单、Owner、目标阶段、CI 命名和 Windows 基线见 [CP0 平台审计](platform-audit.md)。

目的：保证跨平台修改不会回退当前 Windows 成果，并找出所有操作系统耦合点。

工作项：

- 将当前 Windows x64 测试、安装包结构、数据目录和资源基线记录为跨平台回归基线。
- 审计 `src-tauri/` 中的 Windows Registry、`LOCALAPPDATA`、Explorer、进程控制和安装器假设。
- 隔离或退役 `local_service.rs` 中只为旧 Node 守护迁移保留的注册、`launchctl` 和进程管理代码；历史升级读取与当前运行代码不得共享无约束的平台分支。
- 把日志目录、数据目录、打开文件夹、单实例、托盘、通知和 OAuth callback 整理为小型平台接口。
- 为平台能力建立显式 feature/cfg 边界，禁止在共享领域 crate 中读取桌面环境变量或调用系统命令。
- 建立平台矩阵文档和 CI 名称约定，但此阶段不发布新平台产物。

测试与完成标准：

- Windows 现有 Rust、前端、Tauri 和安装包门禁全部通过。
- `cargo check` 能明确区分共享 workspace 与桌面目标，平台专用代码有对应编译门禁。
- 活动测试数据库和迁移证据哈希不变。
- 完成平台耦合清单，每一项都有 owner、目标阶段和测试方式。

## 阶段 CP1：共享 Rust 核心的 Linux 可移植性

目的：先证明服务核心与 HTTP 部署在 Linux 上成立，再构建桌面 UI。

工作项：

- 在 Ubuntu 原生 runner 或受控 WSL2 环境编译和测试 `crates/*`、`http-service`；最终 Linux 门禁在 Ubuntu runner 复验。
- 清理路径分隔符、文件权限、原子替换、文件锁、SQLite busy/backup、信号与时钟差异。
- 验证 IPv4/IPv6 loopback、IMAP/SMTP TLS、HTTP/SOCKS5 代理、OAuth callback 和证书根存储。
- 为 Unix 文件权限增加断言：主密钥、会话和授权材料不得被其他用户读取。
- 让测试使用平台临时目录和动态端口，不依赖 PowerShell、盘符或 Windows 可执行文件后缀。
- 将真实邮件验收继续限制在用户指定的四个邮箱闭环内，不向其他地址发送。

测试与完成标准：

- Ubuntu、Windows 上共享 Rust 单元测试、文档测试、Clippy 和格式检查通过。
- 两个平台使用同一 fixture 得到一致的公开模型、安全向量和 SQLite 摘要。
- TLS/代理/OAuth loopback 的隔离网络测试通过，错误信息不泄露凭据。
- 本阶段不要求 Linux Tauri 安装包；macOS 不在当前阶段范围内。

## 阶段 CP2：Docker Linux 运行门禁与多架构准备（进行中）

当前进度（2026-08-12）：

- GitHub Actions 已从提交 `58a0c4ddae908ac6a4c79dc3c72d91dfd798212b` 成功发布 `ghcr.io/cooliang101/imail:edge` 与对应完整 `sha-*` 标签；两者指向 manifest digest `sha256:30854d249022ec9bf32525760056994e1d51274f43b45e165f1d05b7c259a8c9`。
- 发布工作流只运行 Docker job，Windows job 保持跳过；候选镜像必须先通过完整 lifecycle smoke，成功后才能使用 `packages: write` 的 `GITHUB_TOKEN` 推送 GHCR。
- `linux/amd64` Buildx 构建、非 root UID 10001、只读根文件系统、healthcheck、持久卷重启、实例身份与用户偏好保留、备份、非覆盖恢复、升级预检和 SIGTERM 优雅退出均已通过。
- 最终 Debian runtime 已自动验证不包含 Node、npm、Cargo 或 rustc；发布 manifest 附带 SBOM 与 provenance。
- 本次发布证据见 [GitHub Actions run 31561301344](https://github.com/cooliang101/iMail/actions/runs/31561301344)。GHCR 包当前需要认证拉取；是否公开由包设置管理，不由发布工作流自动修改。

尚未完成：

- 在候选容器内跑完 REST、SSE、WebSocket、Gateway 与 MCP 的完整协议验收，而不只检查能力装配和持久设置。
- 使用 Caddy 示例完成真实 HTTPS、Host/Origin、可信代理、SSE 禁用缓冲和 WebSocket Upgrade 验收。
- 记录镜像大小、空闲 RSS、同步峰值与漏洞扫描结果，并在升级数据副本上保存可复现摘要。
- `linux/arm64` 构建和原生或受控仿真运行门禁。

目的：在当前根 Rust workspace 与 `http-service/` 布局下重新确认服务端容器交付。

工作项：

- 在干净的 `linux/amd64` Buildx 环境构建 `http-service/Dockerfile`。
- 验证最终镜像没有 Node runtime、编译工具、前端源码或测试凭据。
- 使用持久卷完成首次初始化、登录、账户公开视图、偏好、Web、REST、SSE、WebSocket、Gateway 和 MCP 验收。
- 验证重启持久性、healthcheck、SIGTERM 优雅关闭、备份、非覆盖恢复和升级预检。
- 使用 Caddy 示例验证 HTTPS、安全 Cookie、Host/Origin、可信代理、SSE 禁用缓冲和 WebSocket Upgrade。
- 记录镜像大小、空闲 RSS、同步峰值和漏洞扫描结果。
- `linux/amd64` 稳定后再增加 `linux/arm64` 构建；arm64 必须在原生或受控仿真运行环境执行同一门禁，不能只证明镜像可构建。

测试与完成标准：

- 候选镜像在全新卷和升级副本上都通过，且测试前后数据摘要符合预期。
- 容器停止后没有遗留进程或未提交同步任务。
- amd64/arm64 对外协议和数据库格式一致。
- 未经用户授权只生成本地/CI 候选产物，不推送正式镜像。

## 阶段 CP3：Linux 原生桌面

目的：提供首个 Linux Tauri 本地桌面版本，同时保持远程服务模式。

工作项：

- 选择并记录首个支持的发行版、最低 glibc/WebKitGTK 与桌面环境范围。
- 增加 Linux Tauri bundle 配置、图标、桌面文件、MIME/URL handler 和包依赖。
- 验证 WebKitGTK 下 CSP、编辑器、附件选择、下载、剪贴板和高 DPI/缩放。
- 验证托盘与 AppIndicator、关闭隐藏、显式退出、单实例和通知权限。
- 使用 XDG data/config/cache 目录；主密钥优先接入系统 Secret Service，无法使用时必须有明确且安全的降级策略。
- 实现登录启动时使用桌面标准机制，不复用旧 Node 守护逻辑。
- 在 X11 和 Wayland 的支持范围内验证窗口恢复、系统浏览器 OAuth 和 loopback callback。
- 打包首选 `.deb`；AppImage 只有在更新、WebKit 和系统集成门禁明确后再加入。

测试与完成标准：

- 全新安装、覆盖升级、卸载保留数据、重装恢复和损坏数据拒绝路径通过。
- 本地模式无 Node、无业务 HTTP listener，托盘后台同步和显式退出行为与 Windows 一致。
- 远程模式 Cookie 隔离、SSE 恢复、附件下载和失败不回退通过。
- 至少一台实体或稳定虚拟桌面环境完成真实 UI、通知、OAuth 和四邮箱闭环验收。

## 未来阶段：macOS 桌面

状态：未来支持，不进入当前 CP0–CP3 的实施、CI 或发布门禁；启动前必须重新确认范围与资源。

目的：提供符合 macOS 安全与分发要求的原生桌面版本。

工作项：

- 先支持 Apple Silicon，再评估 Intel；不得用未运行的交叉产物声明支持。
- 增加 `.icns`、bundle metadata、最低 macOS 版本、权限说明和平台 Tauri 配置。
- 主密钥和长期会话材料接入 Keychain；验证应用升级、重签名和迁移时的访问连续性。
- 验证菜单栏/托盘、Dock、窗口关闭与应用退出语义、单实例、通知和深色模式。
- 验证系统浏览器 OAuth、临时 loopback callback、防火墙提示和休眠/唤醒后的 IDLE 恢复。
- 配置 Developer ID 签名、Hardened Runtime、entitlements、公证和 stapling；密钥只存在于受保护发布环境。
- 全面检查应用沙箱取舍。若启用沙箱，必须验证网络、文件选择、下载和 Keychain entitlement；若不启用，文档明确分发边界。

测试与完成标准：

- 签名和公证验证通过，Gatekeeper 在干净机器上允许启动。
- Apple Silicon 完成安装、覆盖升级、卸载保留数据、重装恢复和真实 UI/网络验收。
- Intel 只有在同等门禁通过后才加入支持矩阵；Universal Binary 必须验证两种架构内容和签名。
- 本地与远程模式的安全、数据和协议行为与 Windows/Linux 一致。

## 阶段 CP5：统一发布、升级与长期维护

目的：把已通过的平台变成可持续维护的发布矩阵。

工作项：

- 将 CI 分为无副作用验证和显式发布两层；当前支持平台和 Docker job 可独立选择、独立失败、独立重跑。
- 为当前支持平台与 Docker 分别生成 SBOM、SHA-256、签名状态和构建元数据；未来 macOS 支持启动后再加入对应产物。
- 制定版本兼容策略：同一版本使用同一数据库 schema、MCP 工具清单和 Web API 契约。
- 增加跨平台升级矩阵，验证同一数据副本在支持的平台间复制后可读取；这只是便携性验证，不实现自动跨设备同步。
- 建立崩溃、日志、资源和长期 IDLE 预算，平台差异必须有明确阈值。
- 更新 README、安装说明、故障排查、支持矩阵和发布回滚手册。

完成标准：

- 每个标记为支持的平台都有可重复的原生构建、签名/校验、安装、升级、卸载与运行证据。
- 发布失败不会影响已支持平台的已有产物。
- 数据、协议与安全测试在所有支持目标上持续运行。

## 每阶段统一门禁

提交前继续从仓库根目录运行：

```powershell
npm --prefix frontend run typecheck
npm --prefix frontend test
npm --prefix frontend run build
```

涉及 Rust 或桌面平台时还必须在对应原生环境运行：

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --target <target> -- -D warnings
cargo test --workspace --all-features --target <target>
```

涉及安装包或容器时，再追加该平台的真实安装/启动/升级/停止门禁。仅有 `cargo check`、仅生成 bundle 或仅成功构建镜像都不能将平台标记为支持。

## 数据迁移与回滚

1. 所有新平台首次接触现有数据前创建不可覆盖完整副本，包含数据库、主密钥、Logo、实例清单和 schema 信息。
2. 在副本上执行完整性、外键、凭据解密、公开模型和同步游标摘要。
3. 新平台首次启动失败时保留失败现场，不自动删除或覆盖数据。
4. 同一数据目录任一时刻只有一个 iMail 进程族作为写入者。
5. 回滚以完整应用版本和完整数据副本为单位，不降级 schema、不清空缓存、不重置同步状态。
6. `.data`、平台用户数据和 `output/rust-migration-tests` 永远不属于自动清理目标。

## 推荐执行顺序

CP0 已完成。近期并行推进 CP1 的共享 Linux 核心门禁与 CP2 的 Docker 加固项；CP3 必须在 CP1 完成后进入原生 Linux 桌面实施，随后再进入 CP5。macOS 不在当前关键路径中。

Docker `linux/amd64` 已具备可追溯的 GHCR 候选发布和核心生命周期证据，但在上述 CP2 剩余项完成前不把整个阶段标记为完成。Docker arm64 属于后续扩展；macOS 作为未来支持单独启动，不阻塞已经验收的平台发布。
