# 部署模式

iMail 当前只交付 Windows x64 桌面端和服务端 Docker 镜像。两种交付共用根 Cargo workspace 中的 Rust 领域能力和 `frontend/` 中的 Preact 前端，不维护第二套业务实现。

## Windows 桌面本地模式

- Preact WebView 通过类型化 Tauri command/event 直接调用同进程 Rust 服务。
- 本地模式不配置服务 URL，也不绑定常驻业务 HTTP 端口。
- Rust host 持有登录上下文、SQLite、同步 worker、scheduler、IDLE watcher 和二进制读取能力。
- 关闭窗口只隐藏到托盘并继续同步；显式退出才停止运行时。
- OAuth 仅在授权期间临时打开随机 loopback callback listener，该 listener 不提供业务 API。
- 覆盖安装和默认卸载保留数据库、主密钥、Logo 与日志；用户数据只能通过登录后的“隐私与数据”流程清除。

## Windows 桌面远程模式

- 桌面通过 Rust 网络桥连接用户明确选择的 HTTPS Rust 服务；只有回环开发地址可以使用 HTTP。
- Cookie Jar、事件流、下载和请求按规范化服务地址隔离，Cookie 不返回 WebView。
- 切换模式只改变 adapter 和数据源，不复制、合并或删除本地与远程数据。
- 远程连接失败不会静默回退到本地实例。

## Docker 服务端模式

- `http-service/` 是独立部署入口，负责装配 HTTP listener、Web、REST、SSE、WebSocket、Gateway、MCP 和 OAuth callback。
- 通用 Web API、Gateway 与 MCP 能力位于 `crates/imail-http/`；部署入口不复制领域逻辑。
- Gateway 与 MCP 默认关闭，必须同时满足用户开关和 Token scope。
- 正式镜像由 Rust runtime 提供服务；Node 只在镜像构建阶段生成静态 Web 资源，不进入最终镜像。
- 远程部署必须持久化 `/data` 与主密钥，并通过 HTTPS 暴露服务。完整操作见 [运维手册](operator-runbook.md)。

## 数据与安全边界

- 本地与远程实例是独立数据源；模式切换不执行隐式迁移、同步或故障回退。
- 同一数据目录任一时刻只能有一个 iMail 进程族写入。
- 升级前使用维护工具创建不可覆盖备份，并只在独立副本上执行恢复或升级预检。
- 邮箱凭据、OAuth Token、代理密码、主密钥和会话 Token 不得进入前端状态、日志或普通 API/MCP 响应。

## 发布边界

- Windows 桌面只构建 x64 NSIS；不维护原生 Linux 或 macOS 桌面构建。
- 服务端只发布 `linux/amd64` Docker 镜像。
- 普通分支推送和 pull request 不触发发布工作流。
- 手动工作流必须选择 `docker` 或 `windows`，不能连带执行另一平台。
- 未经用户明确授权，不创建版本 tag，不执行 `workflow_dispatch`，不推送镜像。
