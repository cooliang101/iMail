# iMail frontend

该目录是完整的 Preact/Vite/npm 前端工程边界：

- `src/App.tsx` / `src/main.tsx`：应用编排与启动入口；根目录只保留入口、共享模型、主题和全局样式契约。
- `src/services/`：客户端服务统一出口；`mail/` 按 HTTP 与 Tauri 环境拆分实现，`desktop/` 放桌面桥接能力。
- `src/features/`：按业务领域组织的界面与领域内逻辑。
- `src/components/`：跨领域 UI 与纯展示工具。
- `src/app/`：应用级选择器、懒加载和 feature 组合辅助逻辑。
- `src/config/`、`src/platform/`、`src/utils/`：静态配置、平台适配和纯工具函数。
- `public/`：图标、Web Manifest 和 Service Worker 等静态资源。
- `package.json` / `package-lock.json`：唯一 npm 清单与锁文件。
- `vite.config.ts` / `tsconfig*.json`：前端构建、Vitest 和 TypeScript 配置。
- `dist/` / `node_modules/`：本地生成目录，不进入 Git。

从仓库根目录运行：

```powershell
npm --prefix frontend install
npm --prefix frontend run dev
npm --prefix frontend run typecheck
npm --prefix frontend test
npm --prefix frontend run build
```

桌面构建仍由根目录 `src-tauri/` 承载，共享 Rust 能力位于根目录 `crates/`。npm 脚本会在需要时切换到仓库根目录调用它们；不要在仓库根目录重新创建 npm 清单、`src/` 或 `public/`。
