# iMail

iMail 是一个本地优先的多邮箱集中管理 MVP。它把不同服务商的 IMAP/SMTP 邮箱聚合到一个轻量界面，同时提供带范围和过期时间的开发 Token，让本地项目通过统一 API 读取或发送邮件。

## 当前能力

- 统一收件箱、账户切换、自定义工作空间分组、搜索、星标与邮件阅读
- Outlook、Gmail、QQ、Yahoo、Hotmail、iCloud 的内置 IMAP/SMTP 配置
- Gmail、Outlook、Hotmail 的 OAuth 2.0 授权码 + PKCE 登录和自动 Token 刷新
- OAuth PKCE 会话使用本地主密钥加密，授权窗口期间 API 热更新或重启不会丢失 state
- Yahoo OAuth 2.0 流程（需要 Yahoo 审核开放 `mail-r` / `mail-w`）
- QQ 与 iCloud 的交互式应用专用密码 / 授权码引导
- 设置页可直接复用现有授权重试连接，或验证并更新授权码 / 应用专用密码
- 通用 IMAP/SMTP 接入
- 邮箱凭据本地 AES-256-GCM 加密
- SQLite 本地数据库、外键约束、事务写入与旧 JSON 自动迁移
- 首次同步最近 80 封邮件，后续按 IMAP UID 增量更新收件箱、已发送和归档缓存
- 邮件摘要分页、正文懒加载与大邮箱虚拟列表
- 已读、星标、归档与移至垃圾箱会同步写回源 IMAP 邮箱；兼容 Gmail All Mail 归档体系
- SMTP 写信、回复、转发和发送
- SQLite 本地草稿，可继续编辑、删除并在发送成功后自动清理
- 邮件自定义标签、标签筛选和自定义工作空间
- 稍后处理，到期前从收件箱隐藏并可随时提前恢复
- 通知中心集中显示账户连接异常、稍后返回和最近未读邮件
- 附件元数据随正文缓存，文件内容点击时才从源 IMAP 按需下载
- 短期开发 Token，支持指定邮箱、最小权限、自动过期和即时撤销
- 面向本地程序的账户、邮件读取和邮件发送 API
- 首次启动引导、加载态、空状态、错误提示和响应式布局

### 平台验收口径

Outlook / Hotmail、Gmail、QQ、Yahoo、iCloud 与通用 IMAP 均按 MVP 接入功能完成验收。Outlook、Gmail、QQ 已完成真实账户连接；Yahoo、iCloud 与通用 IMAP 依据实现、自动化测试和协议兼容设计验收，真实账户烟测不作为当前阶段的发布阻塞条件。

## 启动

需要 Node.js 22.5 或更新版本（使用 Node 内置 `node:sqlite`）。

```bash
npm install
npm run dev
```

浏览器访问 `http://localhost:5173`。API 默认只监听 `127.0.0.1:8787`，不会暴露给局域网。

生产构建：

```bash
npm run build
npm start
```

## 添加邮箱

点击左侧账户栏的 `+` 并选择服务商。Gmail、Outlook、Hotmail 和审核通过的 Yahoo 应用会打开服务商官方登录窗口；iMail 使用 OAuth 2.0 Authorization Code + PKCE 获取授权并加密保存 Refresh Token。OAuth 授权会先安全保存，再验证 IMAP 与 SMTP；即使邮件协议暂时不可用，已取得的 Refresh Token 也不会丢失。进入“邮箱设置”点击“重试连接”会直接复用已保存授权，只有 Token 被服务商撤销或失效时才需要“重新授权”。QQ、iCloud、未获审核的 Yahoo 和通用 IMAP 使用应用专用密码或授权码，并在保存前完成连接验证。

不同服务商的准备工作：

- Gmail：使用 Google OAuth；`https://mail.google.com/` 是受限 scope，应用对外发布前必须完成 Google OAuth 验证。
- Outlook / Microsoft 365：使用 Microsoft OAuth 的多租户入口；组织仍需在租户与邮箱级别允许 IMAP 和 SMTP AUTH。
- Hotmail / Outlook.com：使用 Microsoft OAuth 的 `consumers` 个人账户入口，避免个人账户被错误路由到组织租户。
- QQ 邮箱：QQ 没有公开第三方邮件 OAuth；在邮箱设置中开启 IMAP/SMTP，并使用生成的授权码。添加页内置完整的三步引导。
- Yahoo：OAuth 邮件权限需要先向 Yahoo Developer Access 申请，未审核时可使用第三方应用密码。
- iCloud：Apple 已为“受支持应用”提供账户授权，但公开开发文档尚未提供普通跨平台邮件客户端可申请的 iCloud Mail scope；iMail 当前使用 Apple 官方应用专用密码流程，并内置三步引导。
- 自定义邮箱：准备 IMAP/SMTP 主机、端口、TLS 设置和授权凭据。

### OAuth 应用配置

复制 `.env.example` 为 `.env`，然后按需要配置服务商。OAuth Client Secret 只能保存在本地 `.env`，不得提交到 Git。

Google Cloud Console：

1. 创建 OAuth 2.0 Web application Client。
2. 登记回调 `http://localhost:8787/api/oauth/google/callback`。
3. 配置 OAuth consent screen 与测试用户，并申请 `https://mail.google.com/`。
4. 填写 `GOOGLE_OAUTH_CLIENT_ID`、`GOOGLE_OAUTH_CLIENT_SECRET`。

Microsoft Entra：

1. 创建 App Registration，账户类型选择同时支持组织账户与个人 Microsoft 账户。
2. 登记回调 `http://localhost:8787/api/oauth/microsoft/callback`。
3. 添加 Office 365 Exchange Online Delegated Permissions：`IMAP.AccessAsUser.All` 与 `SMTP.Send`。
4. 填写 `MICROSOFT_OAUTH_CLIENT_ID`；Web 机密客户端同时填写 `MICROSOFT_OAUTH_CLIENT_SECRET`。

Yahoo Developer Network：

1. 先在 [Yahoo Developer Access](https://senders.yahooinc.com/developer/developer-access/) 申请 IMAP/SMTP 商业接入。
2. 审核通过并获得 `mail-r`、`mail-w` 后登记回调 `http://localhost:8787/api/oauth/yahoo/callback`。
3. 填写 `YAHOO_OAUTH_CLIENT_ID`、`YAHOO_OAUTH_CLIENT_SECRET`，并设置 `YAHOO_MAIL_OAUTH_APPROVED=true`。

可用的公开回调地址、scope 与当前配置状态可通过 `GET /api/providers` 查看。生产环境必须设置 HTTPS 的 `OAUTH_CALLBACK_BASE_URL` 和 `FRONTEND_URL`。

账户、邮件缓存、草稿、标签、稍后处理状态和开发 Token 保存在 `.data/imail.sqlite`。附件文件不长期写入数据库，下载时按需从源 IMAP 获取。凭据字段仍使用 AES-256-GCM 加密，加密主密钥默认生成在 `.data/master.key`。也可在 `.env` 中配置数据目录和 32 字节密钥的 64 位十六进制值：

```env
IMAIL_DATA_DIR=.data
APP_MASTER_KEY=请替换为64位十六进制值
```

从旧版本升级时，首次启动会在一个事务中把 `.data/store.json` 导入 SQLite；成功后原文件会保留为 `.data/store.json.migrated`，不会重复导入。

不要提交 `.data`、`.env` 或任何 Token，项目已在 `.gitignore` 中排除这些文件。

## 开发者邮件网关

进入界面底部的“开发者网关”，选择邮箱、权限和有效时间。完整 Token 只在创建成功时显示一次，服务端只保存 SHA-256 哈希。

基础地址：

```text
http://127.0.0.1:8787/api/dev/v1
```

读取邮件：

```bash
curl "http://127.0.0.1:8787/api/dev/v1/messages?limit=10" \
  -H "Authorization: Bearer imail_your_token"
```

指定邮箱可使用账户级路由，或在聚合路由上传入 `accountId`、`accountEmail`、`account`（UUID 或邮箱地址）或 `provider`：

```bash
curl "http://127.0.0.1:8787/api/dev/v1/accounts/账户UUID/messages?limit=10" \
  -H "Authorization: Bearer imail_your_token"

curl "http://127.0.0.1:8787/api/dev/v1/messages?accountEmail=user@example.com" \
  -H "Authorization: Bearer imail_your_token"
```

读取可用账户：

```bash
curl "http://127.0.0.1:8787/api/dev/v1/accounts" \
  -H "Authorization: Bearer imail_your_token"
```

发送邮件：

```bash
curl -X POST "http://127.0.0.1:8787/api/dev/v1/send" \
  -H "Authorization: Bearer imail_your_token" \
  -H "Content-Type: application/json" \
  -d '{
    "accountEmail": "sender@example.com",
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

## 数据与安全边界

- 服务默认仅监听回环地址，适合作为本地开发工具。
- 邮箱密码和授权码不会由 API 返回，落盘前使用 AES-256-GCM 加密。
- 临时 Token 不以明文落盘。
- 邮件正文和元数据保存在 `.data/imail.sqlite`，因此磁盘权限和设备加密仍然重要。
- SQLite 启用外键、WAL、繁忙等待和事务替换；账户删除会级联清理邮件及 Token 账户授权关系。
- HTML 邮件当前以纯文本正文展示，避免直接渲染不可信 HTML。
- 这是本地单用户 MVP。若需要远程部署，必须先增加管理端身份验证、TLS、数据库权限隔离、审计日志、速率限制和密钥托管。

## 工程结构

```text
src/                 React + TypeScript 客户端
server/index.ts      管理 API 与开发者 API
server/mail.ts       IMAP 同步和 SMTP 发送
server/oauth.ts      OAuth PKCE、回调、身份校验与 Token 刷新
server/crypto.ts     本地凭据加密
server/store.ts      SQLite schema、事务存储与 JSON 迁移
server/providers.ts  服务商预设
.data/               本地数据与密钥，不进入 Git
```

## 品牌素材

- `public/brand/imail-logo.png`：1024×1024 透明 Logo 母版
- `public/brand/imail-app-icon.png`：1024×1024 应用图标
- `public/favicon.ico`：包含 16 至 256 像素的 Windows / 浏览器图标
- `public/favicon-16.png`、`favicon-32.png`、`favicon-48.png`：浏览器图标
- `public/apple-touch-icon.png`：180×180 Apple Touch Icon
- `public/pwa-192.png`、`public/pwa-512.png`：PWA 安装图标
- `public/manifest.webmanifest`：iMail Web App Manifest

## 验证

```bash
npm run typecheck
npm test
npm run build
```

## 后续增强方向

1. IMAP IDLE 实时收信、任意自定义文件夹和更深历史同步
2. 富文本写信、会话视图和发送附件
3. SQLite FTS 全文索引和可选 PostgreSQL 远程模式
4. 管理端登录、设备会话、审计日志和远程安全部署模式
