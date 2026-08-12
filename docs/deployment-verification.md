# 当前交付验收

更新日期：2026-08-11

当前任务的唯一交付目标是 Windows x64 Rust-only 桌面端。Linux/WSL2、Docker 运行验收和交叉编译不属于本任务，后续必须另建独立计划；历史 Node 守护与早期发布证据不代表当前架构。

## 自动门禁

| 命令 | 证明范围 |
| --- | --- |
| `npm --prefix frontend run typecheck` | React、Tauri adapter 与共享 TypeScript 契约类型检查 |
| `npm --prefix frontend test` | 前端行为、Rust HTTP/MCP 契约及数据安全测试 |
| `npm --prefix frontend run build` | 生产 Web 资源，输出到 `frontend/dist` |
| `npm --prefix frontend run rust:test` | Rust 核心、SQLite、邮件网络、同步运行时和 HTTP adapter |
| `npm --prefix frontend run rust:clippy` | Rust workspace 全 target/feature 严格 lint |
| `cargo test -p imail --lib --target x86_64-pc-windows-msvc` | Windows Tauri 直调、会话、事件、迁移和安全边界 |
| `cargo clippy -p imail --all-targets --all-features --target x86_64-pc-windows-msvc -- -D warnings` | Windows 宿主严格 lint |
| `npm --prefix frontend run build:desktop:windows` | Rust-only NSIS 构建 |
| `npm --prefix frontend run test:desktop-release` | release Tauri 无界面启动与退出 |

提交前至少从仓库根目录运行工程指南要求的 `npm --prefix frontend run typecheck`、`npm --prefix frontend test` 和 `npm --prefix frontend run build`。

## Windows 安装包门禁

1. NSIS 只包含 `imail.exe`、卸载程序和必需资源。
2. 归档扫描拒绝 `node.exe`、`.cjs`、`imail-service`、manager、worker 和 `service-runtime`。
3. 当前用户覆盖安装保留 `%LOCALAPPDATA%\com.cooliang.imail\local-service\data`、主密钥、Logo 和迁移快照。
4. 卸载清理只删除当前 runtime/注册项；默认保留数据。
5. 安装目录启动后无需 Node，且本地模式没有常驻业务 listener。

## 功能门禁

- 初始化、注册、登录、登出和重启会话恢复。
- 账户列表/编辑/连接测试、OAuth 动态 callback 和凭据不泄露。
- 邮件列表/详情/标记/移动、草稿、附件和发送。
- 全局、账户、文件夹和角色同步；事件订阅取消与重连。
- 偏好、主题、Token、外部访问设置、授权导出与隐私清理。
- 本地/远程往返切换只改变 adapter；失败不回退或合并数据。

真实发送验收只能在用户指定的四个邮箱闭环内执行，不得向其他收件人外发。投递未确认时停止后续边，不自动重复发送。

## 数据门禁

- 自动化测试只操作唯一临时目录或不可覆盖迁移副本。
- 活动 `.data`、Windows 用户数据、切换前快照和切换后快照不得被测试清理。
- 首次真实 Rust 写入前后记录 schema、完整性、外键、账户/邮件/草稿/联系人/Token/Logo/同步摘要。
- 旧 Node 与 Rust 不能同时写同一 SQLite 目录。

## 当前证据

R9 Windows 迁移结果见 [`rust-migration-r9-report.md`](./rust-migration-r9-report.md)；迁移验证后删除旧 Node 服务源码的范围、回归结果和新安装包哈希见 [`rust-migration-r10-report.md`](./rust-migration-r10-report.md)。后续 Linux、macOS 与多架构容器工作见 [`cross-platform-support-roadmap.md`](./cross-platform-support-roadmap.md)。

## 独立后续计划（不属于当前 Windows 验收）

- WSL2/Docker `linux/amd64` 正式镜像运行门禁。
- 公网证书、Caddy/反向代理和正式镜像发布。
- Windows 正式代码签名与公开分发。

这些项目的阶段、数据不变量与完成门禁已经整理到 [`cross-platform-support-roadmap.md`](./cross-platform-support-roadmap.md)。它们不影响当前 Windows 任务完成状态，也不授权创建 tag、推送镜像或触发 GitHub Actions。
