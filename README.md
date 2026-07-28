# iMail

iMail 是一个本地优先的多邮箱集中管理 MVP。它把不同服务商的 IMAP/SMTP 邮箱聚合到一个轻量界面，同时提供带范围和过期时间的开发 Token，让本地项目通过统一 API 读取或发送邮件。

## 当前能力

- 统一收件箱、账户切换、工作空间分组、搜索、星标与邮件阅读
- Outlook、Gmail、QQ、Yahoo、Hotmail、iCloud 的内置 IMAP/SMTP 配置
- 通用 IMAP/SMTP 接入
- 邮箱凭据本地 AES-256-GCM 加密
- 同步最近 80 封收件箱邮件并保存在本地缓存
- SMTP 写信、回复和发送
- 短期开发 Token，支持指定邮箱、最小权限、自动过期和即时撤销
- 面向本地程序的账户、邮件读取和邮件发送 API
- 首次启动预览数据、加载态、空状态、错误提示和响应式布局

## 启动

需要 Node.js 22 或更新版本。

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

点击左侧账户栏的 `+`，选择服务商并填写邮箱、显示名称、分组和应用专用密码。系统会先测试 IMAP 连接，连接成功才会保存。

不同服务商的准备工作：

- Gmail：开启两步验证后创建应用专用密码。
- Outlook / Hotmail：组织策略需要允许 IMAP；账户启用现代身份验证时可使用 OAuth Access Token，当前 UI 的交互式 OAuth 流程留待下一阶段。
- QQ 邮箱：在邮箱设置中开启 IMAP/SMTP，并使用生成的授权码。
- Yahoo：创建第三方应用密码。
- iCloud：在 Apple Account 中创建应用专用密码。
- 自定义邮箱：准备 IMAP/SMTP 主机、端口、TLS 设置和授权凭据。

凭据保存在 `.data/store.json`，其中敏感字段为密文。加密主密钥默认生成在 `.data/master.key`。也可在 `.env` 中配置 32 字节密钥的 64 位十六进制值：

```env
APP_MASTER_KEY=请替换为64位十六进制值
```

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
    "accountId": "账户 UUID",
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
- 邮件正文和元数据保存在 `.data/store.json`，因此磁盘权限和设备加密仍然重要。
- HTML 邮件当前以纯文本正文展示，避免直接渲染不可信 HTML。
- 这是本地单用户 MVP。若需要远程部署，必须先增加管理端身份验证、TLS、数据库权限隔离、审计日志、速率限制和密钥托管。

## 工程结构

```text
src/                 React + TypeScript 客户端
server/index.ts      管理 API 与开发者 API
server/mail.ts       IMAP 同步和 SMTP 发送
server/crypto.ts     本地凭据加密
server/store.ts      原子 JSON 数据存储
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

## MVP 后续优先级

1. Gmail 与 Microsoft 的完整 OAuth 2.0 授权、刷新与撤销流程
2. IMAP IDLE 实时收信、分页同步、自定义文件夹和服务端已读/星标回写
3. 附件下载、富文本写信、草稿与会话视图
4. SQLite/PostgreSQL 存储、全文索引和大邮箱增量同步
5. 管理端登录、设备会话、审计日志和远程安全部署模式
