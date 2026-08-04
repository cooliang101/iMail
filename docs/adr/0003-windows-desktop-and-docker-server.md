---
status: accepted
---

# 交付平台收敛为 Windows 桌面与 Docker 服务端

iMail 当前只维护 Windows x64 桌面安装包和服务端 Docker 镜像。Windows 桌面包内置用户级守护服务，Docker 镜像提供远程 Web、API 与同步 Worker；Linux 只作为容器运行环境，不提供原生安装、systemd 单元或桌面包，macOS 桌面构建、签名与发布不进入支持矩阵。这样可以把有限的构建和测试资源集中在实际交付路径上；代价是未来若恢复其他原生平台，需要重新建立对应的打包、守护进程和验收门禁。
