# Windows 桌面与 Docker 内部测试

当前交付只面向受控测试人员，不作为正式公开发行。桌面端只生成未配置商业代码签名的 Windows x64 NSIS；远程服务只交付 Docker 镜像。原生 Linux 与 macOS 桌面安装包不在支持范围内。

普通分支推送与 pull request 不触发 GitHub Actions。`.github/workflows/deployment-release.yml` 的手动入口必须二选一：`docker` 只构建并推送 `linux/amd64` 服务端镜像，`windows` 只构建 Windows x64 NSIS 并上传 14 天 Artifact；两者不会互相连带执行。三段式版本标签只发布 Docker。未经用户明确授权，不要创建版本 tag 或执行手动工作流。

## 本机构建

Windows 构建机先安装 Node.js 22.5+、npm、Rust stable、Microsoft C++ Build Tools 与 WebView2，然后执行：

```bash
npm ci --prefix frontend
npm --prefix frontend run build:desktop:internal
```

命令只接受 Windows，并生成 `target/x86_64-pc-windows-msvc/release/bundle/nsis/*.exe`。Linux 只需要 Docker 引擎来构建服务端镜像，不维护额外的原生部署流程。

提交测试包前还应执行项目门禁：

```bash
npm --prefix frontend run typecheck
npm --prefix frontend test
npm --prefix frontend run build
```

Windows 构建机可运行完整内部发布检查：

```bash
npm --prefix frontend run test:internal-release
```

服务端 Docker 验证：

```bash
npm --prefix frontend run test:container-release
```

授权执行手动工作流后，镜像发布到 `ghcr.io/cooliang101/imail`，标签为 `edge` 与完整 `sha-<提交>`。未来三段式版本标签只发布对应完整版本和提交 SHA，不移动已有 `0.0.1` 标签，也不生成 `latest`。Compose 默认读取 `IMAIL_IMAGE`；内测可用 `edge`，可复现部署应使用版本标签或工作流输出的 digest。GHCR 首次发布后的可见性由包设置决定，不在工作流中自动改为公开。

需要把 Windows 产物交给测试人员时，直接从本机输出目录复制，并在交付记录中填写应用版本、构建提交、架构和 SHA-256。Docker 服务端应记录 GHCR 镜像标签与 digest。不要用额外的 GitHub Actions 运行替代本机验证；版本标签只用于发布已确认版本的 Docker 镜像。

## 安装限制

Windows 可能显示 SmartScreen 提示；只在确认文件来自本项目的受控测试人员中继续安装。正式公开发行前仍需配置 Windows 代码签名、发布渠道和升级签名。

## 服务模式测试边界

- 本地模式通过 Tauri 进程内 Rust 服务运行，不使用服务 URL、HTTP、sidecar 或独立 Worker。关闭窗口隐藏到托盘并继续同步，显式退出才停止。
- 同一台机器上的远程运行时开发测试可使用 `http://localhost`、`127.0.0.0/8` 或 `::1` 回环地址。
- 另一台设备连接远程服务时，即使属于内部测试，也必须使用客户端信任的 HTTPS。可以使用内部 DNS 与受信任的内部 CA，不要求现在建设公网正式域名；非回环 HTTP 会在前端和 Rust 网络桥两层被拒绝。
- 本地和远程实例仍是两份独立数据，切换模式不会迁移或合并数据。

## 内测验收重点

1. 全新安装选择本地模式后，由嵌入式 Rust 完成身份检查；安装目录和进程树不含 sidecar、manager 或独立 worker，8787 无 iMail listener。
2. 关闭窗口后继续同步，托盘重新打开仍是同一实例；显式退出后嵌入式 worker/IDLE 全部停止。
3. 切换远程、切回本地的状态一致；远程失败不回退，模式切换不合并数据。
4. Windows 注销并重新登录后，应用不会由旧 `iMailService` 启动项拉起；用户手动启动应用后恢复持久任务。
5. “设置 → 服务连接”与登录门禁都不显示数据删除；进入“隐私与数据”后，第一次确认展示准确范围，第二次必须通过当前密码和固定文字。完成后当前用户的邮箱、邮件缓存、草稿、联系人、开发者令牌和同步状态消失，但 iMail 登录账号、服务与另一测试用户的数据仍存在。
6. 在“隐私与数据”导出授权时，验证文件使用单独密码加密，包含当前用户全部邮箱连接配置与凭据，但不包含邮件、附件、草稿、联系人或 iMail 登录密码；同一下载地址只能成功一次，MCP/Gateway 无对应能力。
7. 在“设置 → 邮箱管理”分别为两个邮箱配置不同代理，重启服务后确认各自配置仍存在；从 schema v4 数据副本升级到 v5 后旧账户保持直连，再次备份/恢复时 `proxy_json` 不丢失。
8. 测试结束记录操作系统、CPU 架构、安装包来源、应用版本、复现步骤和日志摘要；日志不得包含邮箱凭据或 Token。
9. 从“服务连接”打开应用日志：验证 `app.log` 包含本次启动、前端就绪、嵌入式服务和退出阶段；制造一个不含真实凭据的错误，确认邮箱、Bearer Token、OAuth code/state、Cookie 和密码均被脱敏。
