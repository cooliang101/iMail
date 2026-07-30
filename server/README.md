# Server 架构

服务端按 HTTP、业务协议和持久化三个边界组织。入口文件只装配模块，不承载业务逻辑。

```text
index.ts                 进程启动与监听
app.ts                   Express 应用装配、中间件和路由挂载
routes/                  按资源拆分的 HTTP 路由
http/                    参数模型、鉴权、响应转换和错误处理
gateway/                 开发者网关契约、服务、错误模型与轻量文档页
domain/                  HTTP、MCP 和后台任务共享的领域服务、错误与参数模型
mcp/                     MCP 传输、认证、装配器与按领域拆分的 tools/
mail/                    IMAP/SMTP 连接、同步、远程操作和发送
oauth/                   服务商配置、OAuth 客户端、授权流程和密钥刷新
storage/                 SQLite schema、行转换、快照、差异更新和迁移写入
store.ts                 存储门面与 SQLiteStore 协调器
contact-model.ts         联系人聚合、主域识别和共享 Logo 引用
sender-logo.ts           安全网站探测、图片缓存和采集审计
mail.ts / oauth.ts       稳定的公共导出入口
```

## 依赖方向

- `index.ts` 只依赖 `app.ts`。
- `app.ts` 只负责挂载 `routes/` 和全局中间件。
- `routes/` 负责协议适配，账户、草稿和通知等复用行为调用 `domain/`；业务模块不反向依赖路由。
- `http/` 不保存业务状态；共享的请求校验和响应裁剪统一放在这里。
- `gateway/` 维护公开 API 契约，不向外暴露内部账户 ID 或存储结构。
- `mcp/server.ts` 只装配工具；账户与草稿工具分别位于 `mcp/tools/`，并复用 `domain/`，只接受独立的 `mcp:full` 授权码，不返回邮箱凭据。
- `mail/`、`oauth/` 通过 `store.ts` 访问持久化，不直接操作 HTTP 请求或响应。
- `storage/` 只关心 SQLite 与领域数据之间的转换；schema 变更由带版本号的迁移推进，同步控制表通过外键随账户级联清理。
- 面向请求的存储门面必须存在明确用户上下文；只有 Worker、调度器等后台流程可以显式调用 `readAllStore`。后台写入必须使用账户 ID 定向方法，不提供通用全局快照更新。
- 同步层只写持久化领域事件，不依赖 HTTP presenter、Gateway presenter 或进程内事件总线；SSE 与 WebSocket 各自消费同一事件日志。
- 账户、草稿和同步资源的写操作必须先验证当前用户归属；HTTP 与 MCP 删除账户时统一清理邮件、草稿、Token 关联和同步控制数据。
- `contacts` 是联系人建议和邮件发件人资料的唯一来源；Logo 元数据属于联系人字段，优先引用子域缓存，缺失时引用可注册主域缓存。
- Logo 探测只访问发件人同主域，且 `logo_fetch_attempts` 中已有成功或失败记录的域名/子域名永不自动重试。

新增接口时优先放入对应资源路由；只有多个路由复用的参数模型或响应转换才进入 `http/`。新增邮件/OAuth 行为应放进对应目录，并通过顶层门面文件导出，避免调用方依赖内部实现路径。
