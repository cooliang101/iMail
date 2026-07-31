# 桌面客户端打包与部署边界

iMail 桌面版是纯客户端。它复用 React 前端和 Platform Adapter，提供系统浏览器、保存对话框与通知等桌面能力，但不携带 Node.js、Express、SQLite、同步 Worker、MCP 或邮件凭据。

## 部署拓扑

```text
Windows / macOS 客户端 ─┐
                        ├─ HTTPS / REST / SSE ─► 独立 iMail 服务 ─► SQLite + Worker + IMAP/SMTP
Web 客户端 ─────────────┘
```

- 服务端通过 `npm start` 启动 API，并按 `IMAIL_SYNC_WORKER_MODE` 管理 Worker。
- Web 与桌面客户端默认连接 `http://127.0.0.1:8787`，也可用 `VITE_API_BASE_URL` 或客户端设置覆盖服务地址。
- 登录页以轻量“远程服务”展开项提供地址编辑，不把低频连接配置设为首次启动阻塞页；地址保存在该客户端的本地存储中。
- 客户端功能请求、SSE、附件下载、Gateway 文档与 MCP 地址全部从同一服务地址派生。

## 安全边界

- 生产服务地址应使用 HTTPS；跨源会话 Cookie 使用 `SameSite=None; Secure`。
- Web 客户端来源必须通过服务端 `CORS_ORIGIN` 明确允许；Tauri 客户端的 API、会话、SSE 与附件请求统一由 Rust 网络桥发出，不依赖 WebView CORS。
- Tauri CSP 允许客户端连接 HTTP/HTTPS 服务，但系统能力只授权给随包发布的本地窗口。
- 邮箱凭据、OAuth Token、SQLite 数据与后台任务绝不写入桌面安装目录。
- 桌面退出不会停止服务端同步；服务端升级也不要求重新发布桌面客户端。

## 构建

Windows：

```bash
npm run build:desktop:windows
npm run test:desktop-release
```

安装包输出到 `src-tauri/target/release/bundle/nsis/`。桌面构建只执行 `build:web`，不得重新加入 Node sidecar、服务端 bundle 或数据库资源。

macOS：

```bash
npm run build:desktop:macos
```

正式分发仍需在对应平台完成签名与 notarization。
