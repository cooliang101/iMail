# Rust 服务迁移 R6 浏览器验收

日期：2026-08-10
环境：Windows，Chromium/Chrome 151，Playwright CLI，有头模式
目标：验证当前 Web 构建无需修改即可由 Rust HTTP 宿主提供，并在真实浏览器安全模型下完成应用会话与跨 Origin 控制面操作。

## 数据边界

- Rust 仅打开 `output/playwright/r6-rust-browser-20260810-1830/data`、传输重连目录和 `output/playwright/r6-rust-https-20260810/data` 等合成验收目录。
- `IMAIL_SYNC_WORKER=false`，避免浏览器验收连接任何邮箱网络端点。
- 当前 `.data`、R0 snapshot 与 R5 snapshot 均未作为宿主数据目录。
- 合成用户、会话和测试授权码只属于该隔离目录；报告不记录密码、原始 Session 或完整授权码。

## 同源应用闭环

Rust 在 `127.0.0.1:18991` 同时提供生产 Web 资产和 API。真实浏览器完成：

1. 加载 SPA 与首次初始化页面。
2. 创建应用用户并进入统一收件箱。
3. 并行读取账户、Token、统计、草稿、标签、联系人、偏好、消息与 SSE。
4. 打开设置并更新“打开邮件时标记为已读”，浏览器观察到 `PATCH /api/preferences` 200。
5. 打开外部接入，启用 MCP，浏览器观察到 `PATCH /api/external-access` 200。
6. 创建一次性 MCP 授权码，随后列表只显示掩码，完整值不再出现在常规页面。
7. 重启 Rust HTTP 进程后刷新页面，原 Session 和应用状态继续可用。

初始化与业务加载阶段控制台为 0 error / 0 warning。为验证拒绝行为而主动发起的 CSP、CORS 和 421 请求会按预期产生浏览器错误日志，不计为应用启动错误。

浏览器实际保存的应用 Cookie 为 host-only、`HttpOnly`、`Secure`、`SameSite=Lax`。在 `127.0.0.1` 同源页面和后续 `localhost` 跨 Origin 登录中，浏览器均能保存对应 host-only Cookie；两个 Host 的 Cookie 不互相覆盖。

## 浏览器 CORS 与 Host 边界

另用无 CSP 的静态页面建立 `http://127.0.0.1:18992` 浏览器 Origin，并把它作为 Rust 唯一允许的 CORS Origin：

- 从该 Origin 请求 `http://localhost:18991/api/system/info` 成功返回 200。
- 带 JSON body 和 `credentials: include` 的跨 Origin 登录成功返回 200，随后 `/api/auth/session` 返回同一用户。
- 响应包含精确的 `Access-Control-Allow-Origin: http://127.0.0.1:18992`、`Access-Control-Allow-Credentials: true`、允许方法/头及 `Vary: Origin`。
- 从未列入白名单的 `http://localhost:18992` 发起同一请求被浏览器 CORS 层拒绝。
- 通过解析到本机但未列入 Host 白名单的 `lvh.me:18991` 访问 `/api/system/info`，Rust 返回 421 和固定安全错误，不返回 SPA。
- Rust 自己托管的应用 CSP 使用 `connect-src 'self' ws: wss:`，会在 CORS 之前阻止应用壳向其他 HTTP Origin 发请求；远程分离部署的 CORS 验收因此使用独立探针页。

## SSE 重连与 Gateway WebSocket

使用第二个全新隔离目录执行了浏览器传输生命周期验收，未连接真实邮箱：

- 浏览器以跨 Origin、带 Session Cookie 的 `EventSource` 建立 `/api/events`，收到首个 `connected`。
- 首轮验证发现 Ctrl-C 会停止 listener，但活跃 SSE 没有结束，Axum 因等待长连接而不能完成优雅退出。
- Rust 宿主现会在 shutdown 时广播连接取消；SSE stream 主动结束，Gateway WebSocket 以 1001 关闭，随后才停止同步运行时。
- 自动化回归保持真实 SSE TCP 连接不释放并触发 shutdown，要求宿主在 5 秒内返回，防止该问题复发。
- 修复后，浏览器观察到一次 SSE `error`，Rust 重新启动后同一个 `EventSource` 自动重连并收到第二个 `connected`，Session 无需重新登录。
- 浏览器 Gateway WebSocket 通过首帧 `authenticate` 完成连接；撤销对应短期 Token 后，现有连接在下一次在线校验中以策略码 1008 关闭。

浏览器页面只展示布尔结果和关闭码，不渲染完整 Token；Playwright 临时页面快照和控制台文件在验收后精确删除，隔离数据库继续保留。

## HTTPS 反向代理与 OpenAPI

使用独立 Node HTTPS 流式反向代理把 `https://localhost:19443` 和 `wss://localhost:19443` 转发到 Rust 的回环 HTTP listener。证书只用于本机验收且为一天有效的自签名证书；浏览器明确通过证书警告后继续，不把它当作生产证书验证。

- Rust 只设置现有 Node 部署变量 `HOST`、`PORT`、`IMAIL_TRUST_PROXY` 和 `IMAIL_REGISTRATION_MODE` 启动，证明迁移不要求运维变量改名。
- 浏览器通过 HTTPS 首次创建应用用户并保持 Session；Cookie 为 host-only、`HttpOnly`、`Secure`、`SameSite=Lax`。
- 经代理的 `/api/system/info` 返回 HSTS `max-age=31536000`，说明 Rust 只在显式信任直接前一跳时接受 `X-Forwarded-Proto: https`。
- 同源 HTTPS `EventSource` 在 5 秒门限内收到 `connected`，代理未缓冲 SSE 首帧。
- 同源 `wss://` Gateway 完成首帧认证，撤销 Token 后连接以 1008 关闭，证明 Upgrade 与在线授权校验可穿过 TLS 代理。
- `/gateway/docs` 在浏览器可见并能导航到 `/gateway/openapi.json`；JSON 为 OpenAPI 3.1.0、`Cache-Control: no-store`、相对 `/gateway/v1` server、7 条唯一 operation 路径及相对 WebSocket 地址。

OpenAPI 关键路径、安全方案、相对 server 和 WebSocket 扩展同时加入 Rust 自动化断言。Playwright 临时快照已精确删除；HTTPS 合成数据、自签名证书和代理脚本作为被 Git 忽略的验收证据保留。

## 视觉与证据

验收截图保存在 `output/playwright/r6-rust-browser-external-access.png`。截图确认 MCP 开关、地址、一个生效授权码的掩码和接入说明正常渲染，不包含完整授权码。

## 结论与剩余项

当前生产 Web 构建已能在不修改客户端代码的情况下，由 Rust 同源宿主完成初始化、会话恢复、主要读取、偏好写入和外部接入管理；真实浏览器的 Cookie、CSP、精确 CORS 与 Host 拒绝符合预期。

本验收不替代以下 R6 门禁：真实邮箱账户/OAuth 浏览器回调，以及 `linux/amd64` Docker 持久卷升级/备份恢复和发布拓扑中的有效证书/Caddy 复验。SSE 进程重启重连、HTTPS 代理和 Gateway WebSocket 浏览器客户端基础生命周期已经覆盖。
