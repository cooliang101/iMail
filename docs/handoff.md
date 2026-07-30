# 工程交接

## 当前可用能力

- Streamable HTTP：`POST/GET/DELETE /mcp`，Bearer `mcp:full` 授权。
- 28 个 MCP 工具覆盖账户接入/授权、同步策略与任务、邮件读写/移动、附件、草稿、标签、通知与自定义主题。
- MCP 授权码与普通网关 Token 共用哈希、过期和撤销基础设施，但 scope 严格隔离。
- UI 可在没有邮箱时签发 MCP 授权码，Agent 可以接入第一个授权码型邮箱。
- HTTP Host/Origin allowlist、参数大小限制、destructive annotations 和凭据裁剪已接入。
- 联系人已成为持久化档案；邮件发件人与写信联系人共用其中的 Logo 字段，采用子域优先、可注册主域兜底的两级缓存与引用。
- Logo 采集只信任同主域网站，过滤 HTML namespace、跟踪链接和访问验证页；每个 origin 的成功/失败均永久审计并阻止自动重试。
- 邮箱同步由独立 Worker 根据 SQLite 中的持久化策略和任务执行，不依赖前端、SSE 或开发者网关连接。
- 设置中心可即时切换四套内置主题，也可编辑颜色、圆角、阴影和字体令牌，导入 AI JSON，并复制仓库中的生成规范。内置主题继续通过应用偏好同步；自定义主题保存在用户作用域的本地缓存。MCP 通过独立的 `theme_custom_get` / `theme_custom_update` 用户存储提供同一 JSON 协议，不扩展 HTTP 网关。

## 不变量

- 不在 MCP 响应、日志或错误中返回邮箱凭据、OAuth Token、主密钥或 `encryptedSecret`。
- MCP 账户管理必须要求 `mcp:full`，不能用普通开发者网关 scope 代替。
- 新增邮件/账户管理能力时，同时评估 HTTP API、MCP 工具、README 与 `docs/mcp-integration.md` 是否需要同步。
- API 只创建同步任务，不直接承担长时间 IMAP 同步；同一账户/文件夹必须通过数据库租约互斥。
- 远程部署不能只放宽 `MCP_ALLOWED_HOSTS`；必须配套 HTTPS 和管理面安全控制。
- 不得绕过 `logo_fetch_attempts` 对已记录域名自动重试，也不得为联系人建议和邮件发件人建立第二套头像缓存。
- 新增内置主题必须同时维护 `src/features/appearance/theme-model.ts`、`src/theme.ts` 和 `src/theme.css`；自定义主题字段必须同时维护客户端模型、`theme-runtime.ts`、`server/mcp/custom-theme.ts` 与 `docs/custom-theme.md`，并且不能接受任意 CSS。

## 验证基线

提交前运行：

```bash
npm run typecheck
npm test
npm run build
npm audit --omit=dev
```

当前测试包含 MCP 认证隔离、初始化、工具发现、账户读取和凭据不泄漏，以及联系人持久化、主域共享、升级回填、候选过滤和访问验证页识别。真实邮箱的网络操作仍按 README 的平台验收口径执行，不在自动化测试中连接生产邮箱。
