# iMail

iMail 是一个本地优先的多邮箱集中管理 MVP。它把不同服务商的 IMAP/SMTP 邮箱聚合到一个轻量界面，并通过彼此隔离的 REST API Token 与 MCP 授权码，为本地项目和可信 Agent 提供外部接入能力。

## 当前能力

- 统一收件箱、账户切换、自定义工作空间分组、搜索、星标与邮件阅读
- Outlook、Gmail、QQ、Yahoo、Hotmail、iCloud 的内置 IMAP/SMTP 配置
- Gmail、Outlook、Hotmail 的 OAuth 2.0 授权码 + PKCE 登录和自动 Token 刷新
- OAuth PKCE 会话使用本地主密钥加密，授权窗口期间 API 热更新或重启不会丢失 state
- Yahoo OAuth 2.0 流程（需要 Yahoo 审核开放 `mail-r` / `mail-w`）
- Gmail、Outlook、Hotmail、QQ、Yahoo 与 iCloud 的交互式应用专用密码 / 授权码引导
- 设置页可直接复用现有授权重试连接，或验证并更新授权码 / 应用专用密码
- 邮箱管理提供每邮箱级 HTTP/HTTPS/SOCKS5 代理设置，服务重启、升级与备份恢复后继续保持
- 通用 IMAP/SMTP 接入
- 邮箱凭据本地 AES-256-GCM 加密
- SQLite 本地数据库、外键约束、事务写入与旧 JSON 自动迁移
- 应用账号注册、密码登录、30 天 HttpOnly 会话与登录页账号切换
- 邮箱、邮件、联系人、草稿和开发授权码按应用账号强制隔离；未登录 API 统一拒绝访问
- “隐私与数据”支持独立密码加密的邮箱授权导出，以及两阶段确认、当前密码复核的当前用户邮箱数据清除
- 设置中心的内置主题、启动、阅读、通知、邮件展示与快捷键偏好按应用账号同步保存；自定义主题令牌保存在当前设备
- 内置经典薄荷清新、琥珀终端科技风、深海蓝图商业风和 Soft Neubrutalism 柔和撞色四套即时切换主题；账户栏、邮件列表和阅读正文都随主题改变，并支持安全令牌式自定义主题、AI JSON 导入与规范复制
- Rust 后端采用 push-first 持久同步运行时；窗口隐藏后 worker/IDLE 继续运行并由服务商变化唤醒增量拉取
- 收件箱显式保持 IMAP IDLE 实时监听；启动和断线重连会立即校准，运行中到达的新事件保证补跑，固定低频后台校准兜底断线与非收件箱变化
- 设置页只展示邮件接收服务健康、账户最近状态与“立即校准”；文件夹游标、变化监听、重连恢复和校准计划均由系统维护
- 首次同步最近 80 封邮件，后续按 IMAP UID 增量更新收件箱、已发送和归档缓存
- 邮件摘要分页、正文懒加载与大邮箱虚拟列表
- 已读、星标、归档与移至垃圾箱会同步写回源 IMAP 邮箱；兼容 Gmail All Mail 归档体系
- SMTP 写信、回复、转发和发送
- SQLite 本地草稿，可继续编辑、删除并在发送成功后自动清理
- 邮件自定义标签、标签筛选和自定义工作空间
- 稍后处理，到期前从收件箱隐藏并可随时提前恢复
- 通知中心集中显示账户连接异常、稍后返回和最近未读邮件
- 附件元数据随正文缓存，文件内容点击时才从源 IMAP 按需下载
- 联系人档案与邮件发件人共用 SQLite 数据；网站 Logo 获取后作为联系人字段保存，并在邮件列表、阅读页和写信建议中复用
- 发件人 Logo 使用“子域优先、可注册主域兜底”的两级缓存；子域成功会补齐一级域，子域未命中会直接引用一级域并更新联系人，不触发额外采集
- 独立的短期 API Token 与 MCP 授权码，使用不同前缀、权限边界、创建流程和凭据列表
- 面向本地程序的账户、邮件读取和邮件发送 API
- MCP Streamable HTTP 接入，可信 Agent 可用短期授权码管理账户、邮件、附件、草稿、标签和同步
- 首次启动引导、加载态、空状态、错误提示和响应式布局

### 平台验收口径

Outlook / Hotmail、Gmail、QQ、Yahoo、iCloud 与通用 IMAP 均按 MVP 接入功能完成验收。Outlook、Gmail、QQ 已完成真实账户连接；Yahoo、iCloud 与通用 IMAP 依据实现、自动化测试和协议兼容设计验收，真实账户烟测不作为当前阶段的发布阻塞条件。

## 启动

前端开发需要 Node.js 22.5 或更新版本；服务核心和正式运行时使用 Rust。

```bash
npm install
npm run dev
```

浏览器访问 `http://localhost:5173`。API 默认只监听 `127.0.0.1:8787`，不会暴露给局域网。

API 启动器默认同时拉起独立同步 Worker。Worker 的任务、租约、邮箱 UID 游标、下次执行时间和失败状态均保存在 SQLite；浏览器、SSE 或开发者 WebSocket 断开不会停止同步。

开发服务使用 `npm start` 启动。远程生产构建会把 Web、API 和同步 Worker 作为同一发布单元，浏览器默认同源连接：

```bash
npm run build:remote
npm run start:remote
```

也可使用 Docker Compose：`compose.example.yml` 只向宿主机回环地址开放 8787，适合本机验证或接入已有代理；`compose.https.example.yml` 配合 `deploy/remote.env.example` 提供后端不暴露端口的 Caddy 自动 HTTPS 拓扑。远程部署必须持久化 `APP_MASTER_KEY` 或 `/data/master.key`，并把 `/data` 放在持久卷。完整步骤见[运维手册](docs/operator-runbook.md)。

生产服务的注册与登录限流写入 SQLite，服务重启不会清零。登录、注册、授权码签发/撤销、邮箱凭据/代理/删除，以及 MCP 管理工具调用会写入不含密码、Token、OAuth Code 或原始 IP 的安全审计事件；当前登录用户可通过 `GET /api/security/audit-events` 查询自己的最近事件。

生产环境不会隐式允许 Vite 的 `localhost:5173` CORS 来源。同源 Web 无需设置 `CORS_ORIGIN`；只有拆分 Web/API 域名时才显式列出 HTTPS Origin，并且不得带路径、查询、片段或凭据。

生产数据可用 `npm run backup -- <备份目录>` 在线取得一致快照；恢复前用 `npm run restore:prepare -- <备份目录> <新目录>` 校验数据库、密钥并生成不覆盖当前数据卷的恢复目录。远程镜像也内置 `imail-backup.mjs` 和 `imail-restore.mjs`，Compose 使用独立 `/backups` 卷。完整切换步骤见[运维手册](docs/operator-runbook.md)。

远程升级前运行 `npm run upgrade:preflight -- <新备份目录> <新预检目录>`。它使用将要发布的版本对一致性副本执行数据库迁移和完整性检查，不修改在线数据；远程镜像同时内置 `imail-upgrade-preflight.mjs`。

## 桌面应用

桌面版使用 Tauri v2 承载同一套 React 前端。本地模式由 Tauri 进程内直接调用 Rust 领域服务，不启动 Node 守护进程，也不开放常驻 HTTP 端口；远程模式连接显式启用 HTTP Adapter 的 Rust 服务。完整阶段、数据保留与回退门禁见 [Rust 服务重写与 Tauri 直连升级路线](docs/rust-service-migration-roadmap.md)。桌面 WebView 始终使用包内页面，浏览器访问远程服务时使用服务端托管的同版本 Web 页面。

本地嵌入模式没有可配置端口。`8787` 只用于 Docker/远程 Rust HTTP 模式；切换服务模式不会合并、删除或移动两端数据。

远程服务地址必须使用 HTTPS；仅 `localhost`、`127.0.0.0/8` 和 `::1` 这类本机回环开发地址可使用 HTTP。前端在身份握手之前拒绝不安全地址，Rust 网络桥会再次校验并禁止自动跟随 HTTP 重定向，避免登录请求被降级传输。

```bash
npm run dev:web
npm run build:web
```

Windows 开发和 NSIS 安装包构建需要 Node.js 22.5+、Rust stable、Microsoft C++ Build Tools 与 WebView2。当前桌面交付只支持 Windows x64，统一入口生成内部测试 NSIS：

```bash
npm run dev:desktop
npm run build:desktop:internal
```

当前交付范围只有 Windows 桌面端和服务端 Docker 镜像；不提供原生 Linux 或 macOS 桌面安装包。Linux 侧只需运行 Docker 镜像，不维护额外的原生部署脚本。普通分支推送与 pull request 不触发 GitHub Actions；手动运行时必须选择 `docker` 或 `windows`，两条任务不会互相连带执行。三段式版本标签只发布 Docker，Windows 日常仍优先本机构建。

桌面版默认保持后台运行：点击主窗口关闭按钮会隐藏到系统托盘，左键托盘图标或选择“打开 iMail”可恢复窗口；托盘右键菜单的“写邮件”会恢复窗口并直接打开新邮件编辑器，选择“退出 iMail”才会结束进程。应用采用单实例模式；再次启动 iMail 会恢复并聚焦已有窗口，不会创建第二个进程实例。

“设置 → 服务连接”只负责选择本地/远程服务、端口恢复和用户级守护程序生命周期，不提供数据删除。要清理数据，登录后进入“设置 → 隐私与数据 → 清除我的邮箱数据”：界面先展示清除范围，再要求当前 iMail 密码和指定确认文字。该操作只清除当前登录用户的邮箱授权、邮件缓存、草稿、联系人、开发者令牌与同步状态；保留 iMail 登录账号、服务程序、主密钥、其他用户及其数据。“移除运行文件”和卸载桌面应用也仍默认保留数据。

“隐私与数据”还可导出当前登录用户全部邮箱的连接配置与授权凭据。导出文件使用独立的至少 12 位密码加密，可能包含应用专用密码、OAuth Token 和代理密码，因此文件与密码必须分开保管；导出内容不包含邮件、附件、草稿、联系人或 iMail 登录密码。该能力只通过登录后的应用 HTTP 界面提供，不加入 API Gateway 或 MCP。

安装包输出到 `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/`。Windows 桌面包固定使用 Tauri 官方支持的 MSVC 目标，避免把 GNU 运行时隐式依赖带到测试机器。交付本地产物前可用 `Get-FileHash <安装包路径> -Algorithm SHA256` 生成并记录校验值。内测前验证真实桌面宿主：

```bash
npm run test:desktop-release
```

服务端镜像由 GitHub Actions 推送到 `ghcr.io/cooliang101/imail`。一次手动发布生成 `edge` 与完整 `sha-<提交>` 标签；三段式版本标签生成完整版本号与提交 SHA，不生成含义模糊的 `latest`。Compose 默认使用 `edge`，固定部署应把 `IMAIL_IMAGE` 改为版本标签或 digest。首次发布后的 GHCR 可见性由包设置决定，工作流不会自动将其公开。

```bash
docker pull ghcr.io/cooliang101/imail:edge
docker compose --env-file .env.remote -f compose.https.example.yml up -d --pull always
```

仓库根目录的 `Dockerfile` 构建 Rust 正式镜像，最终 runtime 不包含 Node；执行 `npm run test:container-release` 验证 `linux/amd64`、非 root、只读根文件系统、健康检查、Web/API 同源访问、持久卷重启及备份恢复。内部验收口径见[内部测试构建说明](docs/internal-testing.md)。

桌面本地模式通过类型化 Tauri command/event 直连 Rust；远程模式才使用 Rust 网络桥并按服务地址隔离 Cookie、事件流与附件下载。Web 客户端同源连接远程服务。只有拆分 Web 与 API 域名时才需要在 `CORS_ORIGIN` 中列出实际 Web 来源。

## 添加邮箱

点击左侧账户栏的 `+` 并选择服务商。Gmail、Outlook、Hotmail 和审核通过的 Yahoo 应用会打开服务商官方登录窗口；iMail 使用 OAuth 2.0 Authorization Code + PKCE 获取授权并加密保存 Refresh Token。OAuth 授权会先安全保存，再验证 IMAP 与 SMTP；即使邮件协议暂时不可用，已取得的 Refresh Token 也不会丢失。进入“邮箱设置”点击“重试连接”会直接复用已保存授权，只有 Token 被服务商撤销或失效时才需要“重新授权”。这些服务商也可在添加页切换到应用专用密码；QQ、iCloud、未获审核的 Yahoo 和通用 IMAP 默认使用应用专用密码或授权码。所有专用凭据都会先验证连接，再加密保存。

不同服务商的准备工作：

- Gmail：优先使用 Google OAuth；也可为已开启两步验证的账户创建 16 位 Google 应用专用密码。`https://mail.google.com/` 是受限 scope，应用对外发布前必须完成 Google OAuth 验证。
- Outlook / Microsoft 365：优先使用 Microsoft OAuth 的多租户入口；也可选择 Microsoft 应用专用密码，但组织必须允许该登录方式，并在租户与邮箱级别允许 IMAP 和 SMTP AUTH。
- Hotmail / Outlook.com：优先使用 Microsoft OAuth 的 `consumers` 个人账户入口；已开启两步验证且仍允许密码验证的个人账户也可使用 Microsoft 应用专用密码。
- QQ 邮箱：QQ 没有公开第三方邮件 OAuth；在邮箱设置中开启 IMAP/SMTP，并使用生成的授权码。添加页内置完整的三步引导。
- Yahoo：OAuth 邮件权限需要先向 Yahoo Developer Access 申请，未审核时可使用第三方应用密码。
- iCloud：Apple 已为“受支持应用”提供账户授权，但公开开发文档尚未提供普通跨平台邮件客户端可申请的 iCloud Mail scope；iMail 当前使用 Apple 官方应用专用密码流程，并内置三步引导。
- 自定义邮箱：准备 IMAP/SMTP 主机、端口、TLS 设置和授权凭据。

### 邮件代理

添加邮箱时展开“网络代理”，或在“设置 → 邮箱管理”中点击对应邮箱卡片上的“代理设置”，为单个账户配置代理。当前支持 `http`、`https` 和 `socks5`，配置会同时用于 IMAP、SMTP、连接测试、后台同步和发信；关闭代理后该账户恢复直连。HTTP/HTTPS 代理使用 CONNECT 隧道，SOCKS5 的目标域名由代理端解析。

代理主机、端口、协议和可选用户名从 SQLite schema v5 起持久化在账户记录中；代理密码仍与邮箱凭据一起使用 AES-256-GCM 加密，HTTP API 与 MCP 响应均不返回代理密码。更新已有代理时密码留空会保留原密码；关闭代理会删除已保存的代理密码。OAuth 服务商的网页授权仍由浏览器完成，代理仅作用于 iMail 服务端发起的 IMAP/SMTP 连接。

### OAuth 应用配置

复制 `.env.example` 为 `.env`，然后按需要配置服务商。OAuth Client Secret 只能保存在本地 `.env`，不得提交到 Git。

Google Cloud Console（Windows 桌面端）：

1. 创建 OAuth 2.0 Desktop app Client。
2. 配置 OAuth consent screen 与测试用户，并申请 `https://mail.google.com/`。
3. 将 Desktop app 凭据中的两个字段分别填入 `GOOGLE_OAUTH_DESKTOP_CLIENT_ID` 与 `GOOGLE_OAUTH_DESKTOP_CLIENT_SECRET`。
4. Windows 安装包使用 authorization code + PKCE；Google 的 Desktop Client Secret 会作为 Token 端点参数打包，但在公共客户端中不视为可保密凭据。

Microsoft Entra：

1. 创建 App Registration，账户类型选择同时支持组织账户与个人 Microsoft 账户。
2. 远程服务登记由 `OAUTH_CALLBACK_BASE_URL` 派生的 HTTPS 回调；原生桌面公共客户端登记 `http://localhost/api/oauth/microsoft/callback`，授权时使用单次动态 loopback listener（Entra 的 `localhost` 匹配会忽略端口）。
3. 添加 Office 365 Exchange Online Delegated Permissions：`IMAP.AccessAsUser.All` 与 `SMTP.Send`。
4. 桌面 Client ID 填入 `MICROSOFT_OAUTH_DESKTOP_CLIENT_ID`；Windows 安装包与 Google 共用 authorization code + PKCE、动态 loopback 回调和无 Client Secret 的公共客户端流程。Web 机密客户端继续填写 `MICROSOFT_OAUTH_CLIENT_ID` 与 `MICROSOFT_OAUTH_CLIENT_SECRET`。

Yahoo Developer Network：

1. 先在 [Yahoo Developer Access](https://senders.yahooinc.com/developer/developer-access/) 申请 IMAP/SMTP 商业接入。
2. 审核通过并获得 `mail-r`、`mail-w` 后，登记由 `OAUTH_CALLBACK_BASE_URL` 派生的 HTTPS 回调。
3. 填写 `YAHOO_OAUTH_CLIENT_ID`、`YAHOO_OAUTH_CLIENT_SECRET`，并设置 `YAHOO_MAIL_OAUTH_APPROVED=true`。

可用的公开回调地址、scope 与当前配置状态可通过 `GET /api/providers` 查看。Windows 桌面端的 Google 与 Microsoft Desktop app 统一使用系统浏览器、PKCE 与单次动态 `localhost` loopback callback，不注册自定义 URI scheme，也不开放业务 HTTP。Microsoft 不打包 Client Secret；Google Desktop Client Secret 在公共客户端中不具备保密性。生产环境必须设置 HTTPS 的 `OAUTH_CALLBACK_BASE_URL` 和 `FRONTEND_URL`。

应用用户、会话、邮箱账户、邮件缓存、联系人档案、Logo 采集记录、草稿、标签、稍后处理状态和开发 Token 保存在 `.data/imail.sqlite`。首次启动必须创建应用账号；升级已有数据库时，第一个注册用户会接管升级前的本地邮件数据。不同应用用户的数据彼此隔离，并可分别添加相同邮箱地址。密码使用带随机盐的 scrypt 派生值保存，会话使用 HttpOnly、SameSite=Lax Cookie，数据库只保存会话令牌的 SHA-256 哈希。登录按 IP 与账号双重限速，注册按 IP 限速；登录页只在浏览器本地记住曾登录账号的显示名称和登录名，不保存密码。

其中 `contacts` 保存按应用用户隔离的统一联系人资料，`logo_fetch_attempts` 保存不可自动重试的采集审计。Logo 图片内容保存在 `.data/sender-logos/`，联系人记录保存其共享资源键、来源和获取时间；附件文件不长期写入数据库，下载时按需从源 IMAP 获取。邮箱凭据字段仍使用 AES-256-GCM 加密，加密主密钥默认生成在 `.data/master.key`。也可在 `.env` 中配置数据目录和 32 字节密钥的 64 位十六进制值：

```env
IMAIL_DATA_DIR=.data
APP_MASTER_KEY=请替换为64位十六进制值
```

从旧版本升级时，首次启动会在一个事务中把 `.data/store.json` 导入 SQLite；成功后原文件会保留为 `.data/store.json.migrated`，不会重复导入。

不要提交 `.data`、`.env` 或任何 Token，项目已在 `.gitignore` 中排除这些文件。

## 外部接入

Gateway 与 MCP 默认关闭，只在显式启用 HTTP Adapter 的 Rust 服务中可访问。桌面本地嵌入模式不开放这些网络端点；连接远程 Rust 服务后，可在“外部接入”分别启用 Gateway 或 MCP。

进入界面底部的“外部接入”。“API 网关”标签页用于选择邮箱、API 权限和有效时间；“MCP”标签页用于为可信 Agent 创建独立授权码、复制 Streamable HTTP 配置，并查看或复制仓库中的原始 MCP 接入文档。API Token 以 `imail_` 开头，MCP 授权码以 `imail_mcp_` 开头；完整凭据只在创建成功时显示一次，服务端只保存 SHA-256 哈希。

基础地址：

```text
https://mail.example.com/gateway/v1
```

以下示例使用远程 HTTPS 地址；本机开发可显式启动 Rust `imail-server --http` 后改用 `http://127.0.0.1:8787`。

轻量交互文档：

```text
https://mail.example.com/gateway/docs
```

该页面无第三方 UI 运行时依赖，可直接填入 Token、参数和 JSON 正文测试接口。OpenAPI 3.1 契约位于 `/gateway/openapi.json`。

订阅新邮件：

```js
const socket = new WebSocket('wss://mail.example.com/gateway/v1/events');

socket.addEventListener('open', () => {
  socket.send(JSON.stringify({ type: 'authenticate', token: 'imail_your_token' }));
});

socket.addEventListener('message', ({ data }) => {
  const event = JSON.parse(data);
  if (event.type === 'message.created') console.log(event.data.message);
});
```

连接要求 `messages:read` 权限。网关仅推送 Token 授权邮箱的新邮件摘要，不包含正文或内部账户 ID；Token 被撤销或过期后连接会以 `1008` 关闭。新邮件由独立同步 Worker 按持久化策略采集，网关只转发已经写入数据库的事件，不触发也不维持同步。首次同步用于建立本地基线，不会把历史邮件当作新邮件推送。服务端客户端也可以在 WebSocket 握手中使用 `Authorization: Bearer ...`。

读取邮件：

```bash
curl "https://mail.example.com/gateway/v1/messages?limit=10" \
  -H "Authorization: Bearer imail_your_token"
```

列表只返回摘要，不加载邮件正文。使用响应中的 `page.nextCursor` 获取下一页：

```bash
curl "https://mail.example.com/gateway/v1/messages?limit=10&cursor=上一页游标" \
  -H "Authorization: Bearer imail_your_token"
```

读取单封邮件正文：

```bash
curl "https://mail.example.com/gateway/v1/messages/邮件ID" \
  -H "Authorization: Bearer imail_your_token"
```

指定邮箱可使用邮箱级路由，或在聚合路由上传入 `mailbox`：

```bash
curl "https://mail.example.com/gateway/v1/mailboxes/user@example.com/messages?limit=10" \
  -H "Authorization: Bearer imail_your_token"

curl "https://mail.example.com/gateway/v1/messages?mailbox=user@example.com" \
  -H "Authorization: Bearer imail_your_token"
```

读取可用账户：

```bash
curl "https://mail.example.com/gateway/v1/mailboxes" \
  -H "Authorization: Bearer imail_your_token"
```

发送邮件：

```bash
curl -X POST "https://mail.example.com/gateway/v1/send" \
  -H "Authorization: Bearer imail_your_token" \
  -H "Content-Type: application/json" \
  -d '{
    "mailbox": "sender@example.com",
    "to": ["recipient@example.com"],
    "subject": "iMail test",
    "text": "Hello from a local app"
  }'
```

权限范围：

| Scope | 能力 |
| --- | --- |
| `messages:read` | 读取指定邮箱的本地邮件缓存 |
| `messages:send` | 通过指定邮箱发送邮件 |
| `accounts:read` | 读取指定账户的非敏感元数据 |

Token 有效期范围为 5 分钟至 7 天，且只能访问创建时选中的邮箱。撤销立即生效。

网关错误统一返回 `error.code`、`error.message` 与 `error.requestId`，响应头同时包含 `X-Request-Id`，便于日志关联。

## MCP Agent 接入

iMail 内置基于官方 TypeScript SDK v2 的 Streamable HTTP MCP 服务。MCP 使用单独的 `mcp:full` 短期授权码；普通 `messages:*` / `accounts:read` Token 无法调用 MCP，避免已有只读 Token 意外获得账户删除、授权码更新或发信能力。

在“外部接入”的“MCP”标签页点击“创建 MCP 授权码”。生成的授权码以 `imail_mcp_` 开头，只显示一次，服务端仍只保存 SHA-256 哈希。它最长有效 7 天，可以在同一标签页即时撤销。即使尚未接入邮箱，也可以先签发 MCP 授权码，让可信 Agent 通过 `account_add_with_code` 接入第一个邮箱。页面同时提供 Streamable HTTP 配置、工具速查、安全调用顺序，以及 [`docs/mcp-integration.md`](docs/mcp-integration.md) 原文的展开与复制功能。

### Streamable HTTP

在“外部接入 → MCP”中启用后，MCP 地址为：

```text
https://mail.example.com/mcp
```

桌面本地嵌入模式没有 MCP HTTP 地址；该地址来自当前远程 Rust 服务。

客户端应把授权码放入 Bearer 请求头：

```text
Authorization: Bearer imail_mcp_xxx
```

通用远程 MCP 客户端配置示例：

```json
{
  "url": "https://mail.example.com/mcp",
  "headers": {
    "Authorization": "Bearer imail_mcp_xxx"
  }
}
```

HTTP MCP 默认只接受 `localhost`、`127.0.0.1` 和 `::1` 的 Host/Origin，并且 API 默认只监听回环地址。确需远程部署时，必须使用 HTTPS，并通过 `MCP_ALLOWED_HOSTS` 显式添加实际主机名：

```env
MCP_ALLOWED_HOSTS=mail.example.com
```

### MCP 工具

| 领域 | 工具 |
| --- | --- |
| 状态 | `imail_status` |
| 设置 | `settings_get`、`settings_update`、`theme_custom_get`、`theme_custom_update` |
| 账户 | `accounts_list`、`account_add_with_code`、`account_start_oauth`、`account_reconnect_oauth`、`account_update`、`account_update_authorization_code`、`account_test_connection`、`account_proxy_update`、`account_remove` |
| 同步 | `mailbox_sync`、`sync_policy_get`、`sync_policy_update` |
| 邮件与附件 | `messages_list`、`message_get`、`message_update`、`message_move`、`message_send`、`attachment_download` |
| 草稿 | `drafts_list`、`draft_get`、`draft_save`、`draft_delete` |
| 整理 | `labels_list`、`notifications_list` |

带副作用的工具提供 MCP annotations：读取工具标记为只读，账户移除、邮件移动和草稿删除标记为 destructive。邮箱授权码、应用专用密码、OAuth Token 和加密字段永远不会出现在 MCP 响应中。

完整接入流程、工具参数、安全模型和排障见 [`docs/mcp-integration.md`](docs/mcp-integration.md)、[`docs/architecture.md`](docs/architecture.md) 与 [`docs/operator-runbook.md`](docs/operator-runbook.md)。AI 自定义主题生成格式见 [`docs/custom-theme.md`](docs/custom-theme.md)。

## 数据与安全边界

- 除健康检查、登录注册和带加密 state 的 OAuth 回调外，应用 HTTP API 必须持有有效应用会话；登录失败按 IP 与登录名限速。
- 邮箱账户、邮件、联系人、草稿、Logo 采集记录和开发授权码均绑定应用用户；API、网关与 MCP 会在服务端恢复该用户上下文，不能靠客户端参数跨账号读取。
- 服务默认仅监听回环地址，适合作为本地开发工具。
- 邮箱密码和授权码不会由 API 返回，落盘前使用 AES-256-GCM 加密。
- 临时 Token 不以明文落盘。
- MCP 完整控制需要独立的 `mcp:full` 授权码；普通网关 Token 不能升级为 MCP 管理权限。
- 邮件正文和元数据保存在 `.data/imail.sqlite`，因此磁盘权限和设备加密仍然重要。
- SQLite 启用外键、WAL、繁忙等待和事务替换；账户删除会级联清理邮件及 Token 账户授权关系。
- HTML 邮件在带 CSP 的 sandbox iframe 中渲染，禁用脚本、对象和表单提交；纯文本邮件保持文本展示。
- 应用账号解决同一 iMail 实例内的数据访问隔离；远程发布包已提供 Host allowlist、持久认证限流、安全审计和一致性备份工具，公网部署仍必须配置 TLS、反向代理连接级限流和专业密钥托管。

## 工程结构

```text
src/App.tsx          客户端顶层状态与页面编排，不放业务组件实现
src/components/      跨业务复用的基础 UI、服务商标识与展示工具
src/features/mail/   邮件列表虚拟化与阅读器
src/features/accounts/ 邮箱接入、授权与账户设置
src/features/compose/  写信与草稿工作区
src/features/organize/ 标签、稍后处理、通知和工作空间
src/features/developer/ 外部接入、API Token 与 MCP 授权码 UI
src/features/appearance/ 主题元数据、根主题 Provider 与本地回退
src/features/settings/ 设置窗口与各偏好面板
src/app-model.ts     跨 feature 的客户端类型
rust/crates/imail-http/            可选 HTTP、Gateway、MCP 与 Web 适配器
rust/crates/imail-core/            Tauri、HTTP 与 MCP 共用的应用服务
rust/crates/imail-mail-network/    IMAP/SMTP、代理与 MIME 网络实现
rust/crates/imail-runtime/         持久化 worker、scheduler、IDLE 与生命周期
rust/crates/imail-storage-sqlite/  SQLite、迁移、备份与同步事务
src-tauri/src/embedded_service.rs  Windows 桌面类型化直调适配器
.data/               本地数据与密钥，不进入 Git
```

服务端模块边界见 [`rust/README.md`](rust/README.md)。旧 Node 服务实现已在迁移验收完成后删除，需要追溯时使用 Git 历史。界面主题、排版、布局、响应式与新增样式的维护规则见 [`docs/style-system.md`](docs/style-system.md)。

## 品牌素材

- `public/brand/imail-logo.png`：1024×1024 透明 Logo 母版
- `public/brand/imail-app-icon.png`：1024×1024 应用图标
- `public/favicon.svg`：跟随系统明暗模式的现代浏览器图标
- `public/favicon.ico`：包含 16 至 256 像素的 Windows / 浏览器图标
- `public/favicon-16.png`、`favicon-32.png`、`favicon-48.png`：浏览器图标
- `public/apple-touch-icon.png`：180×180 Apple Touch Icon
- `public/pwa-192.png`、`public/pwa-512.png`：PWA 安装图标
- `public/manifest.webmanifest`：iMail Web App Manifest
- `public/sw.js`：仅 Web 生产环境注册的 Service Worker；缓存版本化静态资源与离线页面外壳，明确绕过 API、SSE、MCP 和 Gateway

## 验证

```bash
npm run typecheck
npm test
npm run build
# Windows 内部测试完整冒烟
npm run test:internal-release
```

Docker 测试机额外运行 `npm run test:container-release`。平台人工验收和证据要求见 [`docs/deployment-verification.md`](docs/deployment-verification.md)。

## 后续增强方向

1. 可配置的更深历史同步与邮箱会话视图
2. 会话视图、联系人分组和模板化写信
3. SQLite FTS 全文索引和可选 PostgreSQL 远程模式
4. 设备会话管理、密码重置与安全审计查询界面
