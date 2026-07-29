# Server 架构

服务端按 HTTP、业务协议和持久化三个边界组织。入口文件只装配模块，不承载业务逻辑。

```text
index.ts                 进程启动与监听
app.ts                   Express 应用装配、中间件和路由挂载
routes/                  按资源拆分的 HTTP 路由
http/                    参数模型、鉴权、响应转换和错误处理
mail/                    IMAP/SMTP 连接、同步、远程操作和发送
oauth/                   服务商配置、OAuth 客户端、授权流程和密钥刷新
storage/                 SQLite schema、行转换、快照和事务写入
store.ts                 存储门面与 SQLiteStore 协调器
mail.ts / oauth.ts       稳定的公共导出入口
```

## 依赖方向

- `index.ts` 只依赖 `app.ts`。
- `app.ts` 只负责挂载 `routes/` 和全局中间件。
- `routes/` 可以调用邮件、OAuth、Token 和存储能力，但业务模块不反向依赖路由。
- `http/` 不保存业务状态；共享的请求校验和响应裁剪统一放在这里。
- `mail/`、`oauth/` 通过 `store.ts` 访问持久化，不直接操作 HTTP 请求或响应。
- `storage/` 只关心 SQLite 与领域数据之间的转换。

新增接口时优先放入对应资源路由；只有多个路由复用的参数模型或响应转换才进入 `http/`。新增邮件/OAuth 行为应放进对应目录，并通过顶层门面文件导出，避免调用方依赖内部实现路径。
