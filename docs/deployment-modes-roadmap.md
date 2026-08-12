# Windows 本地嵌入与远程 Rust 服务模式

本文件记录当前生效的部署模式。旧 Node 用户级守护进程方案已经被 Rust/Tauri 迁移替代；历史决策和验收证据保存在各阶段报告中，不再作为实施指南。

## 当前模式

### Windows 桌面本地模式

- React WebView 通过类型化 Tauri command/event 直接调用同进程 Rust 服务。
- 不启动 Node、SEA、manager 或独立 Worker，不绑定常驻业务 HTTP 端口。
- Rust host 持有登录上下文、SQLite、同步 worker/scheduler/IDLE watcher 和二进制读取能力。
- 关闭窗口只隐藏到托盘并继续同步；显式退出才停止 Rust 运行时。
- OAuth 可以临时打开随机 loopback callback listener；该 listener 不提供业务 API。
- 卸载默认保留数据库、主密钥、Logo、日志和迁移快照。

### Windows 桌面远程模式

- 桌面通过 Rust 网络桥连接用户选择的 HTTPS Rust 服务。
- Cookie Jar、事件流、下载和请求按规范化服务地址隔离；Cookie 不返回 WebView。
- 切换模式只改变 adapter 和数据源，不复制、合并或删除本地/远程数据。
- 远程验证失败不会静默回退本地，避免用户误操作另一份数据。

### Rust HTTP 服务模式

- `http-service/` 是独立部署入口，`imail-server` 启动后默认监听 HTTP；主机和端口仍需显式按部署环境配置。
- Web API、Gateway 与 MCP 属于 `crates/` 的通用服务能力；独立入口只负责把它们挂到 REST、SSE、WebSocket、Streamable HTTP 和 OAuth callback，不复制领域实现。
- Tauri 应用通过类型化 command/event 复用相同应用能力；是否开放外部 HTTP listener 是 transport 配置，不决定 MCP/Web API 的领域能力是否存在。
- Gateway 与 MCP 默认关闭，必须由用户开关和 Token scope 同时授权。
- 正式容器由 Rust runtime 运行；Node 仅在镜像 build stage 生成静态 Web 资源。

## 数据与升级

旧版升级按以下单写入者事务执行：

1. 识别受管旧守护配置和数据目录。
2. 停止旧 API、Worker 与 supervisor，并确认端口不再监听。
3. 创建不可覆盖的完整数据快照。
4. 校验 SQLite、外键、主密钥和全部账户凭据。
5. 以无网络 Rust host 首启；数据库哈希不应因预检变化。
6. 成功后写入 `embedded-switch.json`；旧 runtime 和快照保留一个回退周期。

任何失败都保留现场。不得清空数据库、删除同步游标、重新添加邮箱或让 Node/Rust 同时写一个目录。

## Windows 验收

- 本地登录、四账户公开视图、邮件/草稿/联系人读取和同步状态可用。
- Tauri 直调覆盖当前 UI 的全部领域操作，未知操作立即拒绝。
- 窗口隐藏后同步继续，托盘恢复正常，显式退出后无 iMail worker 残留。
- 安装包扫描不包含 `node.exe`、Node CJS、sidecar、manager 或 worker。
- 覆盖安装和卸载不删除数据；重新安装可继续读取原实例。
- 运行状态没有 iMail Node 进程或常驻业务 listener。

Linux/WSL2、Docker 的真实运行门禁和原生 Linux/macOS 桌面不属于当前 Windows 验收；后续阶段见[跨平台支持路线](./cross-platform-support-roadmap.md)。这不改变正式 Dockerfile 已采用 Rust runtime 的代码状态，也不把未执行的运行门禁记为通过。

## 发布边界

- 当前桌面发布只支持 Windows x64。
- 普通分支推送和 pull request 不触发发布工作流。
- 未经用户明确授权，不创建版本标签，不运行 `workflow_dispatch`，不推送镜像。
- Windows 与 Docker 构建必须分别选择，不能连带执行另一平台。
