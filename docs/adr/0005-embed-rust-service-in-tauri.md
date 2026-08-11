---
status: accepted
---

# Windows 本地模式直接嵌入 Rust 服务

iMail Windows 桌面本地模式把 Rust 领域服务静态链接进 Tauri，通过类型化 command/event 直接调用，不启动 Node、sidecar、用户级守护进程或常驻业务 HTTP listener。窗口隐藏时 Tauri 托盘进程继续运行同步任务，用户显式退出后服务停止。远程桌面和 Web 使用显式启用的 Rust HTTP Adapter，且与本地模式复用同一领域服务、SQLite 存储和同步运行时。本地与远程实例保持独立数据源，不做隐式复制、合并或故障回退。

当前交付平台仍为 Windows x64 桌面端和服务端 Docker 镜像。原生 Linux/macOS 桌面支持及交叉平台构建另立计划，不属于当前交付门禁。
