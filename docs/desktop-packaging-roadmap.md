# Windows 与 macOS 桌面应用开发路线图

## 1. 目标

在不削弱现有 Web 部署能力、不复制邮件业务控制面的前提下，将 iMail 打包为 Windows 与 macOS 桌面应用。

桌面版继续复用现有 React 前端、Node/Express 服务端、SQLite、同步 Worker、HTTP API、Gateway 与 MCP。Tauri 负责桌面宿主、sidecar 生命周期和操作系统能力，不承载第二套邮件业务实现。

首期目标平台：

- Windows 10/11 x86_64，NSIS 安装包。
- macOS Apple Silicon 与 Intel，分别生成 DMG。
- Web 开发、构建和独立部署流程保持可用。

首期不包含：

- Android、iOS 与 Linux。
- Microsoft Store、Mac App Store。
- Windows ARM64 与 macOS Universal Binary。
- 将邮件、同步或存储逻辑迁移到 Rust。

### 1.1 当前实施状态（2026-07-30）

首期工程能力已经落地：Web 独立构建、Platform Adapter、Node API/Worker sidecar bundle、Tauri 生命周期与启动握手、Windows NSIS、macOS DMG/hardened runtime/entitlement 配置，以及桌面配置与运行时自动化测试。

Windows x64 安装包已在本地成功生成，并通过发布宿主进程级冒烟测试（sidecar 启动、受信健康检查、正常退出、端口释放）。macOS 因当前开发环境为 Windows，不在本机生成 DMG；其配置由单元测试覆盖，最终 DMG、签名和 notarization 必须在 macOS 构建机完成。

正式对外分发前仍属于发布运维工作的项目包括：Windows 代码签名证书、Apple Developer ID、macOS notarization 凭据，以及后续自动更新服务。它们不阻塞本地可安装包与双平台构建工程的完成。

## 2. 架构原则

### 2.1 HTTP 是共同业务协议

账户、邮件、草稿、偏好、同步、开发者 Token 等业务能力继续通过现有 HTTP API 和 SSE 暴露。Web 与桌面端复用同一套 API client、契约、错误处理和领域行为。

不得为现有邮件业务逐项增加同义的 Tauri `invoke` command。否则每项能力都需要同时维护 HTTP、MCP 和 Tauri IPC 三套协议，容易产生权限和行为漂移。

### 2.2 Bridge 只处理原生能力

Tauri Bridge/Adapter 仅处理浏览器无法稳定完成或桌面体验明显更好的能力：

- 使用系统浏览器打开 OAuth 和外部链接。
- 原生文件保存与打开。
- 系统通知。
- 单实例、窗口、托盘与自动启动。
- 应用数据目录、日志目录和应用生命周期。
- 更新检查与安装。

### 2.3 Feature 不感知运行平台

业务组件不得散落 `window.__TAURI__`、user-agent 或平台条件分支。运行环境只在应用入口识别一次，通过小型能力接口注入 feature。

建议的客户端依赖方向：

```text
features/
   │
   ├── api clients ─────────────── HTTP / SSE
   │
   └── platform ports
            ├── web adapters
            └── tauri adapters ── limited IPC
```

### 2.4 Web 是持续支持的一级目标

以下命令和行为必须始终独立于 Rust/Tauri 工具链：

- Web 本地开发。
- Web 类型检查与测试。
- Web 前端构建。
- Node 服务端独立启动和部署。

默认 `npm run build` 不应要求开发者安装 Rust、Xcode 或 Windows 构建工具。

## 3. 目标运行拓扑

### 3.1 Web

```text
Browser
  ├── React UI
  ├── same-origin /api
  └── /api/events SSE
          │
          ▼
Node/Express API ── SQLite / IMAP / SMTP / Sync Worker
```

### 3.2 Tauri 桌面版

```text
Tauri host
  ├── single-instance and window lifecycle
  ├── starts trusted Node sidecar
  ├── verifies sidecar startup handshake
  └── opens the main WebView after health succeeds
          │
          ▼
Node sidecar on 127.0.0.1:8787
  ├── serves bundled React assets
  ├── /api and /api/events
  ├── /gateway and /mcp
  ├── SQLite and encrypted credentials
  └── child Sync Worker
```

首期桌面版建议由 sidecar 同源提供 UI 与 API。这样可以保留现有相对 `/api` URL、HttpOnly Cookie、SSE 和附件读取行为，减少跨源 Cookie 与 WebView 差异。

## 4. 建议目录与接口

```text
src/
├── api/
│   ├── transport.ts
│   ├── event-transport.ts
│   ├── accounts.ts
│   ├── messages.ts
│   ├── drafts.ts
│   ├── preferences.ts
│   └── contracts.ts
├── platform/
│   ├── types.ts
│   ├── PlatformProvider.tsx
│   ├── web/
│   │   ├── web-platform.ts
│   │   ├── web-oauth.ts
│   │   └── web-download.ts
│   └── tauri/
│       ├── tauri-platform.ts
│       ├── tauri-oauth.ts
│       └── tauri-download.ts
server/
├── desktop/
│   ├── config.ts
│   ├── static-app.ts
│   ├── startup-handshake.ts
│   └── shutdown.ts
src-tauri/
├── capabilities/
├── src/
├── binaries/
├── resources/
└── tauri.conf.json
```

平台接口保持小而明确，例如：

```ts
export interface PlatformRuntime {
  kind: 'web' | 'tauri';
  openExternal(url: string): Promise<void>;
  saveDownload(input: DownloadRequest): Promise<void>;
  notify(input: NotificationRequest): Promise<void>;
  supportsAutostart: boolean;
}
```

API transport 与平台能力分开：

```ts
export interface ApiTransport {
  request<T>(path: string, options?: RequestInit): Promise<T>;
}

export interface EventTransport {
  subscribe(types: SyncEventType[], listener: SyncEventListener): () => void;
}
```

## 5. 交付阶段

### 阶段 0：建立桌面决策基线

任务：

- 确认正式 bundle identifier、产品名和发行者名称。
- 确认首期固定使用 `127.0.0.1:8787`，继续提供 Gateway 与 MCP。
- 确认 Windows 首发 NSIS，macOS 首发独立 arm64/x64 DMG。
- 确认 macOS 首期采用 Developer ID 分发而非 Mac App Store。
- 建立桌面威胁模型：回环端口劫持、恶意 Origin、XSS 后 IPC 权限、Token 和日志泄漏。

交付物：

- 记录上述决策的 ADR。
- 桌面版最小权限清单。
- Windows/macOS 构建矩阵。

验收：

- 不存在影响目录布局、认证模式或发行方式的未决关键问题。

### 阶段 1：通用 API 与 Platform Adapter

任务：

- 将 `src/api.ts` 扩展为可配置的 `ApiTransport`。
- 将散落在 `App.tsx` 和各 feature 中的请求整理为领域 API client。
- 集中前后端共享的请求/响应契约；保留服务端 presenter 对敏感字段的裁剪。
- 将 SSE 封装为 `EventTransport`。
- 增加 `PlatformProvider`、Web Adapter 和空的 Tauri Adapter。
- 抽象 OAuth launcher、附件保存、外链打开和系统通知。
- 禁止 feature 直接判断 Tauri 运行环境。

交付物：

- `src/api/` 与 `src/platform/`。
- Web Adapter 的单元测试。
- HTTP client 与主要 route 的契约测试。

验收：

- Web UI 行为无变化。
- `npm run typecheck`、`npm test`、`npm run build` 通过。
- 现有 Web 开发命令不依赖 Tauri。

### 阶段 2：服务端生产产物

任务：

- 增加服务端生产构建，生成 API 与 Worker 的 JavaScript bundle。
- 生产环境移除对 `tsx` 和 `.ts` Worker 入口的依赖。
- 将 Worker 入口从源码路径改为可配置的生产资源路径。
- 增加 API 主进程的 `SIGINT`、`SIGTERM` 和父进程断开处理。
- 增加 Express 静态资源托管和 SPA fallback；确保不会吞掉 `/api`、`/gateway`、`/mcp` 的 404。
- 选择并验证 Node Runtime 交付方案。

推荐首期采用“官方 Node Runtime + 服务端 JS bundle + 静态资源”，而不是立即依赖单文件 SEA/pkg。该方案更容易验证 `node:sqlite`、子 Worker、动态资源和双架构 macOS。

需要验证的运行依赖：

- `node:sqlite`。
- `imapflow`、`nodemailer`、`mailparser`。
- MCP SDK 与 WebSocket。
- 子进程 Worker 的创建和关闭。
- CA 证书、DNS 和 TLS 行为。

交付物：

```text
desktop-runtime/
├── server.cjs
├── worker.cjs
└── web/
```

验收：

- 在没有 `tsx` 的干净环境中可以启动服务端。
- API、SSE、MCP、Gateway 和同步 Worker 均可运行。
- 关闭父进程后不残留 API 或 Worker。

### 阶段 3：Tauri 宿主与 sidecar 生命周期

任务：

- 初始化 Tauri v2 项目与构建配置。
- 增加 Windows x64、macOS arm64/x64 的 Node sidecar。
- 由 Rust 主进程启动 sidecar，不向普通 Web 页面开放任意 shell 权限。
- Tauri 生成随机启动密钥并通过环境变量传给 sidecar。
- 增加桌面专用健康检查；只有验证启动密钥后才加载主窗口。
- 增加单实例插件；第二次启动时聚焦既有窗口。
- 启动失败、端口占用或握手失败时显示独立错误窗口。
- 主应用退出、更新或崩溃恢复时清理 sidecar 进程树。

交付物：

- 可运行的 Windows 与 macOS 开发构建。
- 最小 `capabilities` 配置。
- sidecar 启停集成测试。

验收：

- 不会加载占用 8787 的未知服务。
- 多次启动只保留一个 iMail 实例。
- 正常退出后没有残留进程和 SQLite 锁。
- sidecar 异常退出时应用能提示或受控重启，不能无限快速重启。

### 阶段 4：桌面数据与认证安全

任务：

- Tauri 将绝对应用数据目录通过 `IMAIL_DATA_DIR` 传给 sidecar。
- Windows 使用 AppData/LocalAppData，macOS 使用 Application Support。
- 日志写入独立日志目录并轮转；继续禁止记录邮箱凭据、OAuth Token 和加密字段。
- 引入明确的桌面模式配置，不依赖 `NODE_ENV` 推断 Cookie 行为。
- 桌面 loopback Cookie 使用 HttpOnly 与严格 SameSite；仅在实际 HTTPS 时增加 Secure。
- 对会话写操作增加 Origin/CSRF 防护。
- 限制 Host、Origin、CORS 和 MCP allowlist。
- 明确卸载、重装、备份、恢复和主密钥丢失行为。

后续增强：

- Windows Credential Manager。
- macOS Keychain。
- 现有文件主密钥向系统凭据存储的迁移方案。

验收：

- 安装目录只读时应用可以正常运行。
- 不同 OS 用户的数据互相隔离。
- 重启、升级和重装后数据库与主密钥行为符合文档。
- 非法 Host、Origin 和启动握手均被拒绝。

### 阶段 5：OAuth、附件与原生能力

任务：

- OAuth 业务仍通过 HTTP API 发起和完成。
- Web Adapter 保留浏览器弹窗流程或迁移到统一的完成事件。
- Tauri Adapter 使用系统默认浏览器打开授权 URL。
- OAuth 回调成功后由服务端写入持久化事件，前端通过 SSE 接收 `oauth.completed` 或 `oauth.failed`。
- 桌面流程不再依赖 `window.opener.postMessage`。
- 外部 HTTP/HTTPS 链接通过系统浏览器打开，不允许替换主 WebView。
- Tauri 附件流程使用带认证的请求、原生保存对话框和明确错误反馈。
- 增加原生通知 Adapter；Web 继续使用 Web 能力或应用内通知。
- 评估托盘与自动启动，默认不在用户未选择时启用。

验收：

- Gmail、Microsoft、Yahoo OAuth 在 Windows 与 macOS 上分别完成端到端测试。
- 用户取消、浏览器关闭、回调延迟和服务商错误均能恢复 UI。
- 中文名称、大附件、重复文件名和取消保存均处理正确。
- 邮件正文中的外链不能获得 Tauri IPC 权限。

### 阶段 6：Windows 发布

首发配置：

- Windows 10/11 x86_64。
- NSIS per-user 安装包。
- WebView2 bootstrapper 或明确的离线策略。

任务：

- 生成 Windows 图标、版本资源、安装包信息和卸载信息。
- 签名 Tauri 主程序、Node sidecar 与安装包。
- 安装、升级和卸载前确保 sidecar 已退出。
- 检查仅绑定回环地址时的 Windows 防火墙行为。
- 验证 SmartScreen、Defender 和常见企业杀软。
- 验证睡眠/唤醒、网络切换、注销和系统关机。
- 验证包含中文和空格的用户目录。

验收：

- 干净的 Windows 10/11 虚拟机无需预装 Node 即可安装运行。
- 安装、覆盖升级、降级阻止和卸载行为符合发布策略。
- 主程序、sidecar 和安装包签名可验证。
- WebView2 缺失时能安装或给出明确提示。

### 阶段 7：macOS 发布

首发配置：

- Apple Silicon DMG。
- Intel DMG。
- Developer ID 签名与 notarization。

任务：

- 在对应架构的 macOS 构建环境中生成 Node sidecar。
- 签名主程序、Node sidecar 和所有嵌套可执行资源。
- 启用 Hardened Runtime，验证 Node/V8 所需 entitlement；只授予实际需要的能力。
- 分别生成 arm64/x64 `.app` 与 DMG。
- 完成 notarization 和 stapling。
- 验证 App Translocation、quarantine、Application Support 与 Keychain 行为。
- 验证 `Cmd+Q`、关闭最后窗口、睡眠/唤醒和网络切换。
- 在真实 Intel Mac 或可靠的 Intel CI/测试环境中验证 x64 包。

验收：

- 两种架构的干净 macOS 环境无需预装 Node 即可运行。
- `codesign`、`spctl`、notary 与 stapling 检查通过。
- `.app` bundle 内所有嵌套可执行文件签名一致。
- 从浏览器下载 DMG 后不出现“应用已损坏”或未公证警告。

### 阶段 8：发布工程与持续回归

任务：

- 建立 Windows 与 macOS 独立 CI runner。
- 将签名证书和公证凭据放入 CI Secret，不进入仓库或构建日志。
- 统一应用版本、Node sidecar 版本和数据库 schema 兼容策略。
- 增加签名更新清单和自动更新机制。
- 更新前通知 sidecar 停止，更新失败时保持旧版本可运行。
- 产出 SBOM、第三方许可证和发布校验和。
- 每个桌面版本同时执行 Web 回归流水线。

验收：

- 同一 Git tag 可稳定产出 Web、Windows x64、macOS arm64 和 macOS x64 制品。
- 桌面改动不会让 Web 构建依赖 Rust 或平台工具链。
- 发布制品可追溯到 commit、版本和 sidecar hash。

## 6. 建议构建命令

目标脚本语义：

```json
{
  "scripts": {
    "dev": "npm run dev:web",
    "dev:web": "...",
    "dev:desktop": "...",
    "build": "npm run build:web",
    "build:web": "...",
    "build:server": "...",
    "build:desktop": "...",
    "build:desktop:windows": "...",
    "build:desktop:macos": "..."
  }
}
```

原则：

- `build:web` 不启动 Cargo。
- `build:server` 可以在 Windows 与 macOS 分别验证，但不要求 Tauri。
- `build:desktop` 先构建 Web 与服务端资源，再执行 Tauri build。
- 不从 Windows 生成正式 macOS 制品；macOS 签名、公证和 DMG 在 Mac runner 上完成。

## 7. 测试矩阵

### 7.1 每次提交

- `npm run typecheck`
- `npm test`
- `npm run build`
- 服务端生产 bundle smoke test
- API/SSE/Worker 集成测试

### 7.2 桌面主干构建

- Tauri Rust tests。
- sidecar 启动握手。
- 单实例。
- 父子进程退出。
- 端口冲突。
- 数据目录权限。
- OAuth callback。
- 附件保存。
- 安装后首次启动。

### 7.3 发布前人工检查

| 场景 | Windows | macOS arm64 | macOS x64 |
|---|---:|---:|---:|
| 首次安装/启动 | 必测 | 必测 | 必测 |
| 注册、登录、退出 | 必测 | 必测 | 必测 |
| Gmail/Microsoft OAuth | 必测 | 必测 | 必测 |
| IMAP 应用密码账户 | 必测 | 必测 | 必测 |
| 同步、发送、移动、附件 | 必测 | 必测 | 必测 |
| 睡眠/唤醒与网络恢复 | 必测 | 必测 | 必测 |
| Gateway/MCP 外部连接 | 必测 | 必测 | 必测 |
| 覆盖升级 | 必测 | 必测 | 必测 |
| 卸载/保留数据 | 必测 | 不适用 | 不适用 |
| 签名与公证验证 | 必测 | 必测 | 必测 |

## 8. 主要风险与缓解措施

| 风险 | 影响 | 缓解措施 |
|---|---|---|
| `node:sqlite` 与单文件打包器不兼容 | sidecar 无法启动或数据损坏 | 首期携带官方 Node Runtime；建立真实制品 smoke test |
| 8787 被未知程序占用 | WebView 加载错误或恶意页面 | 单实例、随机启动密钥、健康证明、失败时不打开页面 |
| sidecar 升级时仍在运行 | 安装失败或新旧版本混用 | 更新前优雅退出，安装器兜底终止，校验 sidecar 版本/hash |
| macOS 嵌套二进制签名失败 | notarization 失败 | 构建后递归验签，在 CI 中运行 `codesign`、`spctl` 与 notary 检查 |
| localhost 页面获得过宽 IPC | XSS 扩大为本机权限 | capability 最小化；邮件正文隔离；不开放任意 shell/fs；外链系统打开 |
| Web 与桌面行为漂移 | 两套产品长期难维护 | HTTP/SSE 共同协议；Adapter 契约测试；每个桌面 PR 同时运行 Web 回归 |
| 主密钥与数据库分离 | 邮箱凭据无法解密 | 同目录迁移、备份提示、系统凭据存储迁移设计 |
| Worker 残留或快速重启 | SQLite 锁、CPU 占用 | 统一生命周期、退避上限、父进程断开处理和进程树测试 |

## 9. 桌面首发完成定义

只有同时满足以下条件，Windows/macOS 桌面首发才视为完成：

- Web 本地开发、生产构建和独立部署能力未回归。
- Windows x64、macOS arm64、macOS x64 均无需用户安装 Node。
- API、SSE、同步 Worker、Gateway 和 MCP 均在安装后可用。
- OAuth 使用系统浏览器并能可靠回到应用状态。
- 数据只写入平台应用数据目录，升级不丢失数据库或密钥。
- 应用退出后无残留 sidecar/Worker，无 SQLite 锁。
- Tauri capability 不包含任意 shell 或无限制文件系统权限。
- Windows 制品完成代码签名；macOS 制品完成签名、公证和 stapling。
- 安装、首次启动、升级、恢复与卸载策略均有测试和文档。
- `npm run typecheck`、`npm test`、`npm run build` 全部通过。

## 10. 官方参考

- Tauri prerequisites: <https://v2.tauri.app/start/prerequisites/>
- Tauri sidecar: <https://v2.tauri.app/develop/sidecar/>
- Tauri Node.js sidecar: <https://v2.tauri.app/learn/sidecar-nodejs/>
- Tauri capabilities: <https://v2.tauri.app/security/capabilities/>
- Tauri CSP: <https://v2.tauri.app/security/csp/>
- Tauri opener: <https://v2.tauri.app/plugin/opener/>
- Tauri single instance: <https://v2.tauri.app/plugin/single-instance/>
- Windows installer: <https://v2.tauri.app/distribute/windows-installer/>
- macOS application bundle: <https://v2.tauri.app/distribute/macos-application-bundle/>
- Node.js single executable applications: <https://nodejs.org/api/single-executable-applications.html>
