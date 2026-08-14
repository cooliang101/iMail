# 工程交接

## 当前可用能力

- Streamable HTTP：`POST/GET/DELETE /mcp`，Bearer `mcp:full` 授权。
- 29 个 MCP 工具覆盖账户接入/授权、账户级 HTTP/HTTPS/SOCKS5 代理、自动同步设置与任务、邮件读写/移动、附件、草稿、标签、通知与自定义主题。
- MCP 授权码与普通网关 Token 共用哈希、过期和撤销基础设施，但 scope 严格隔离。
- UI 可在没有邮箱时签发 MCP 授权码，Agent 可以接入第一个授权码型邮箱。
- HTTP Host/Origin allowlist、参数大小限制、destructive annotations 和凭据裁剪已接入。
- 联系人已成为持久化档案；邮件发件人与写信联系人共用其中的 Logo 字段，采用子域优先、可注册主域兜底的两级缓存与引用。
- Logo 采集只信任同主域网站，过滤 HTML namespace、跟踪链接和访问验证页；每个 origin 的成功/失败均永久审计并阻止自动重试。
- 邮箱同步由 Rust 持久 worker pool、scheduler 与 IDLE watcher 执行，不依赖前端、SSE 或开发者网关连接；变化推送会唤醒增量拉取，固定低频校准负责最终一致性。
- 桌面端提供显式的本地/远程服务选择。本地模式由 Tauri 进程内直调 Rust，窗口隐藏后继续同步；远程模式只连接用户部署的 HTTPS Rust 实例，不做隐式迁移或故障回退。
- 当前交付范围是 Windows 桌面端与服务端 Docker。Windows NSIS 使用进程内 Rust 服务；Docker 使用独立 `http-service/` 启动器与维护 CLI。原生 Linux 与 macOS 桌面暂不在支持范围，后续实施计划见[跨平台支持路线](cross-platform-support-roadmap.md)。未经用户明确授权不得创建版本 tag 或手动触发工作流。
- 服务连接在身份或会话请求前完成实例与协议握手；非回环远程地址只接受 HTTPS，桌面 Rust 网络桥按服务地址隔离持久会话且不跟随重定向。
- “服务连接”不再提供数据删除；“隐私与数据”用两阶段确认、当前 iMail 密码和固定确认文字，仅清除当前登录用户的邮箱授权与邮箱数据，保留登录账号、服务和其他用户。相同页面可用独立密码导出当前用户全部邮箱的连接配置与授权凭据；文件排除邮件、附件、草稿、联系人和 iMail 登录密码，且导出只属于登录会话 HTTP UI，不加入 Gateway/MCP。
- 邮箱管理卡片提供独立“代理设置”；代理是每邮箱级配置。SQLite schema v5 使用 `accounts.proxy_json` 持久化非密码代理字段，代理密码仍在 `encryptedSecret` 中加密保存。
- 设置中心可即时切换薄荷清新、石墨琥珀科技、蓝色商业、柔和粗野主义和构成红内置主题，也可编辑颜色、圆角、阴影和字体令牌，导入 AI JSON，并复制仓库中的生成规范。内置主题与安全自定义主题都通过应用偏好同步，并在用户作用域保留本地缓存。MCP 的 `theme_custom_get` / `theme_custom_update` 与 HTTP 偏好接口共用同一份用户级主题令牌存储；两条控制面均拒绝任意 CSS、URL 和额外字段。

## 不变量

- 不在 MCP 响应、日志或错误中返回邮箱凭据、OAuth Token、主密钥或 `encryptedSecret`。
- 邮箱授权导出是登录后 HTTP UI 的敏感恢复能力；不得通过 MCP、API Gateway、日志或普通账户响应暴露导出内容或等价凭据。
- MCP 账户管理必须要求 `mcp:full`，不能用普通开发者网关 scope 代替。
- 新增邮件/账户管理能力时，同时评估 HTTP API、MCP 工具、README 与 `docs/mcp-integration.md` 是否需要同步。
- API 只创建同步任务，不直接承担长时间 IMAP 同步；同一账户/文件夹必须通过数据库租约互斥。
- 远程部署不能只放宽 `MCP_ALLOWED_HOSTS`；必须配套 HTTPS 和管理面安全控制。
- 桌面本地模式不得引入常驻 HTTP、sidecar、系统级服务或独立本地 Worker；OAuth 临时 loopback callback 不得承载业务 API。
- 本地与远程实例是独立数据源；模式切换不得复制、合并、静默回退或串用会话。
- 清除用户邮箱数据必须保持当前用户作用域并保留 `app_users`、服务文件、主密钥和其他用户数据；不得重新引入“服务连接”内的整实例删除按钮。
- 不得绕过 `logo_fetch_attempts` 对已记录域名自动重试，也不得为联系人建议和邮件发件人建立第二套头像缓存。
- 新增内置主题必须同时维护 `frontend/src/features/appearance/theme-model.ts`、`frontend/src/theme.ts` 和 `frontend/src/theme.css`；自定义主题字段必须同时维护客户端模型、`theme-runtime.ts`、`crates/imail-core/src/theme.rs`、`crates/imail-http/src/mcp.rs` 与 `docs/custom-theme.md`，并且不能接受任意 CSS。

## 验证基线

提交前运行：

```bash
npm --prefix frontend run typecheck
npm --prefix frontend test
npm --prefix frontend run build
npm --prefix frontend audit --omit=dev
```

当前测试包含 MCP 认证隔离、初始化、工具发现、账户读取和凭据不泄漏，以及联系人持久化、主域共享、升级回填、候选过滤和访问验证页识别。真实邮箱的网络操作仍按 README 的平台验收口径执行，不在自动化测试中连接生产邮箱。
