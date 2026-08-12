---
status: accepted; amended by ADR-0005
---

# 交付平台收敛为 Windows 桌面与 Docker 服务端

iMail 当前只维护 Windows x64 桌面安装包和服务端 Docker 镜像。该决策中的平台范围仍有效，但 Windows 用户级守护服务已由 ADR-0005 的 Tauri 进程内 Rust 服务取代。Linux 只作为容器运行环境，不提供原生安装、systemd 单元或桌面包，macOS 桌面构建、签名与发布不进入支持矩阵。
