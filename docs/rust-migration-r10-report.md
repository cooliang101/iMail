# R10 旧 Node 服务源码移除报告

日期：2026-08-11
分支：`codex/rust-service-migration`

## 结论

Windows Rust/Tauri 迁移、数据兼容、真实邮件闭环、资源长稳和 Rust-only 安装包验收已经完成。按用户决定，代码回滚由 Git 历史承担，当前工作树已删除旧 Node 服务实现及其双实现对照入口；测试数据、迁移快照和既有验收报告全部保留。

## 删除范围

- 删除整个 `server/`：旧 Express HTTP、认证、SQLite、邮件、OAuth、同步 Worker、Gateway、MCP、维护工具、SEA 入口及其测试，共 113 个文件。
- 删除 `desktop-runtime`、`server-runtime` 和 `legacy-node-baseline` 中的旧生成产物。
- 删除 Node/Rust 双宿主契约、资源对照、Node 数据摘要、Node 迁移校验和旧四账户数据包装脚本。
- 删除旧服务专用的 Express、IMAPFlow、Nodemailer、MCP Server SDK、SEA、代理、WebSocket、加密与对应类型依赖；`npm uninstall` 共移除 128 个包。
- 删除 `tsconfig.server.json`，保留 `tsconfig.tools.json` 用于前端构建脚本和 Rust 互操作测试的 Node 工具环境。

npm/Vite/TypeScript 仍是 React 前端与工程脚本的构建工具，不属于服务端运行时；官方 MCP TypeScript Client 继续用于对 Rust MCP 服务进行真实互操作验证。

## 长期门禁

- HTTP 路由清单改为固定契约对 Rust Axum 路由的单实现检查。
- MCP 工具清单由 Rust 实现嵌入并在 Rust 测试中校验，另由官方 TypeScript Client 连接 Rust fixture 验证协议互操作。
- MIME fixture、SQLite migration、凭据格式、真实 TLS IMAP/SMTP、附件、Worker/IDLE 和四邮箱闭集保护均由 Rust 测试继续覆盖。
- 历史 Node/Rust 内存对照报告继续保留；后续资源回归只以当前 Rust 实现为对象，不重新构建旧 Node 服务。

## 删除后验证

- `npm run typecheck`：通过。
- `npm test`：34 个文件、126 项通过。
- `npm run build`：通过。
- Rust workspace：140 项通过，1 项显式长稳测试按设计 ignored。
- Windows Tauri：22 项通过。
- Rust workspace 与 Windows Tauri rustfmt、严格 Clippy：通过。
- `npm audit --omit=dev`：0 个漏洞。
- Windows NSIS 构建和 release 冒烟：通过，`embeddedRustService=true`、`bundledNodeRuntime=false`。

新生成的不可覆盖 Windows 安装包：

- 路径：`output/rust-migration-tests/r10-node-source-removed-windows-2026-08-11/iMail_0.0.1_x64-setup.exe`
- 大小：8,762,445 bytes
- SHA-256：`2CBBF0268C7D519786987869C5EFA6347E2C9E80D8D11FE88E77CDE9F38D8A47`
- 7-Zip 扫描：`node.exe`、旧 service/worker/manager、`.cjs`、`server-runtime`、`desktop-runtime` 与 `legacy-node-baseline` 命中数为 0。

## 数据保留

- 活动 `.data/imail.sqlite` SHA-256 仍为 `074B3D437ADDDEBE3BD8020C3DAA18B825DE440EDA34AD01838979574BD72000`。
- 未删除或覆盖数据库、主密钥、Logo、邮件缓存、测试邮件、切换前后快照或历史验收报告。
- 本轮没有发送邮件；后续真实发送仍由 Rust 验收二进制强制限制在恰好四个唯一邮箱组成的闭集内。
