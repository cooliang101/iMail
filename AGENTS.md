# iMail engineering guide

- 前端工程统一位于 `frontend/`，使用 npm 与 `frontend/package-lock.json`；不要在仓库根目录新增 `package.json`、`src/`、`public/` 或 `node_modules/`，不要运行 pnpm、yarn 或生成其他包管理器的锁文件。
- `frontend/src/App.tsx` 只负责顶层状态、数据编排和 feature 组合；不要把完整业务组件或弹窗写回该文件。
- 客户端组件按领域放入 `frontend/src/features/<domain>/`，跨领域 UI 与纯展示工具放入 `frontend/src/components/`。
- 表单统一使用 `frontend/src/components/form-controls.tsx` 的 `AppInput`、`AppSelect`、`AppTextarea` 和 `AppCheckbox`；不要在业务组件中直接新增原生表单控件。
- 跨 feature 的客户端类型放入 `frontend/src/app-model.ts`；邮件领域共享的数据结构继续使用 `frontend/src/types.ts`。
- feature 可以依赖 `services/`、`types.ts` 和 `components/`，不要反向依赖 `App.tsx`。环境相关服务统一从 `frontend/src/services/` 导出，桌面、HTTP 等实现放入对应子目录，不要把 service 文件放回 `frontend/src/` 根目录。
- 主题 ID 与回退由 `frontend/src/features/appearance/` 维护；新增内置主题必须同步 `frontend/src/theme.ts` 和 `frontend/src/theme.css`。自定义主题只接受安全令牌，字段变更须同步 `theme-runtime.ts`、`crates/imail-core/src/theme.rs`、`crates/imail-http/src/mcp.rs` 与 `docs/custom-theme.md`，不得扩展为任意 CSS，也不得加入 HTTP 网关。
- 保持现有本地优先、安全边界和响应式行为；结构重构不得改变 API 协议。
- 通用 Web API、MCP 与 Gateway 能力统一放在 `crates/imail-http/`，由 Tauri 应用和 `http-service/` 共同复用；账户管理只允许 `mcp:full` 授权码，任何响应都不得暴露邮箱凭据、OAuth Token 或加密字段。
- 新增邮件或账户管理行为时，同步评估 HTTP API、MCP 工具与 `docs/mcp-integration.md`，避免两个控制面能力漂移。
- 联系人与邮件发件人必须复用 `contacts` 数据和其中的 Logo 字段；Logo 使用子域键与可注册主域兜底键，已有成功或失败采集记录的域名不得自动重试。
- 当前交付平台仅包含 Windows 桌面端和服务端 Docker 镜像；不维护原生 Linux 或 macOS 桌面构建、安装与发布流程。
- Windows 内部测试日常在本机构建和验证；GitHub Actions 手动运行必须选择 `docker` 或 `windows`，不得连带执行另一平台，三段式版本标签只发布服务端 `linux/amd64` Docker 镜像。普通分支推送与 pull request 不触发；未经用户明确授权，不要创建版本 tag 或执行 `workflow_dispatch`。
- 提交前从仓库根目录运行 `npm --prefix frontend run typecheck`、`npm --prefix frontend test` 和 `npm --prefix frontend run build`。
