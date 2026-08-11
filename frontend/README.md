# iMail frontend

该目录是完整的 React/Vite/npm 前端工程边界：

- `src/`：界面、客户端 adapter、领域 feature 与测试。
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

桌面构建仍由根目录 `src-tauri/` 承载，Rust 服务位于根目录 `rust/`。npm 脚本会在需要时切换到仓库根目录调用它们；不要在仓库根目录重新创建 npm 清单、`src/` 或 `public/`。
