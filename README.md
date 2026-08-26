# iMail

iMail 是一个本地优先的多邮箱客户端。它把 Gmail、Outlook、QQ、Yahoo、iCloud 和其他 IMAP 邮箱放进同一个界面，统一处理收信、搜索、写信、联系人和日常整理。

项目目前处于内部测试阶段，只交付 Windows x64 桌面端和服务端 Docker 镜像。Linux 仅作为服务端容器运行环境，不维护原生 Linux 或 macOS 桌面构建。

## 主要能力

- 在一个收件箱中查看和管理多个邮箱
- 支持 Gmail、Outlook、Hotmail、QQ、Yahoo、iCloud 与通用 IMAP/SMTP
- 支持 OAuth 登录、应用专用密码和邮箱授权码
- 后台持续接收新邮件，窗口隐藏后仍可同步
- 支持搜索、星标、已读、归档、垃圾箱和自定义标签
- 支持写信、回复、转发、草稿和附件下载
- 支持联系人、发件人 Logo 和写信建议
- 支持为 iCloud 邮箱管理 Hide My Email 地址
- 支持稍后处理、通知中心、快捷键和多套主题
- 每个邮箱可单独使用 HTTP、HTTPS 或 SOCKS5 代理
- 可为可信工具和 Agent 开启独立的 API 或 MCP 接入

## 两种使用方式

### Windows 桌面端

适合个人在自己的电脑上使用。邮件数据和授权信息保存在本机，关闭窗口后应用会留在系统托盘继续收信，选择“退出 iMail”才会停止。

桌面端也可以连接自己部署的远程 iMail 服务。切换本地和远程模式只会切换数据来源，不会自动复制或合并两边的数据。

### Docker 服务端

适合在服务器或家用设备上部署，然后通过浏览器访问。镜像同时包含 Web 界面、邮件服务和维护工具，运行时不需要 Node.js。

当前镜像：

```text
ghcr.io/cooliang101/imail:edge
```

镜像目前可能需要登录 GHCR 后拉取。固定部署建议使用版本标签、完整提交标签或 digest，不要长期依赖 `edge`。

本机试运行：

```bash
docker compose -f http-service/compose.example.yml up -d
```

该示例只允许本机访问 `http://127.0.0.1:8787`。公网部署必须使用 HTTPS，并持久化 `/data` 和 `/backups`；完整步骤见[运维手册](docs/operator-runbook.md)。

## 数据与隐私

- 邮箱密码、授权码和 OAuth Token 会加密保存
- 不会把邮箱凭据返回给普通 API、MCP、前端日志或错误信息
- 桌面卸载和应用升级默认保留邮件数据
- 可以备份和恢复完整实例，升级前可先在副本上检查数据
- “隐私与数据”可以清除当前用户的邮箱数据，不影响其他用户
- 邮箱授权信息可以导出为单独密码保护的文件，邮件正文不会进入该文件

iMail 是本地优先产品，不提供自动的多设备数据同步。本地桌面实例与远程服务实例彼此独立。

## Agent 与外部接入

远程服务可以按需开启 API Gateway 或 MCP，让可信程序读取邮件、发送邮件、管理邮箱和执行同步。每个授权码都有独立用途、有效期和撤销入口。

这些入口默认关闭。MCP 的账户管理能力只接受专用的 `mcp:full` 授权码，具体接入方式见 [MCP 指南](docs/mcp-integration.md)。

## 当前交付平台

| 目标 | 状态 |
| --- | --- |
| Windows x64 桌面端 | 已支持，当前用于内部测试 |
| Docker `linux/amd64` | 已发布到 GHCR，持续补充部署与安全验收 |

## 本地开发

需要 Node.js 22.5+、npm 和 Rust。前端工程统一位于 `frontend/`。

```bash
npm ci --prefix frontend
npm --prefix frontend run dev
```

浏览器打开 `http://localhost:5173`。开发服务默认只监听本机，不会直接暴露给局域网。

Windows 桌面开发：

```bash
npm --prefix frontend run dev:desktop
npm --prefix frontend run build:desktop:internal
```

提交前检查：

```bash
npm --prefix frontend run typecheck
npm --prefix frontend test
npm --prefix frontend run build
```

Docker 完整验收：

```bash
npm --prefix frontend run test:container-release
```

## 项目结构

```text
frontend/       共用界面
crates/         邮件、存储、同步、安全和外部接入能力
src-tauri/      Windows 桌面应用
http-service/   Docker 与远程服务入口
docs/           架构、部署、运维和开发计划
```

## 文档

- [文档索引](docs/README.md)
- [部署模式](docs/deployment-modes.md)
- [运维手册](docs/operator-runbook.md)
- [MCP 接入指南](docs/mcp-integration.md)
- [架构说明](docs/architecture.md)
- [内部测试说明](docs/internal-testing.md)
