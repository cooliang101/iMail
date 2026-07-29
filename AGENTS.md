# iMail engineering guide

- `src/App.tsx` 只负责顶层状态、数据编排和 feature 组合；不要把完整业务组件或弹窗写回该文件。
- 客户端组件按领域放入 `src/features/<domain>/`，跨领域 UI 与纯展示工具放入 `src/components/`。
- 跨 feature 的客户端类型放入 `src/app-model.ts`；邮件领域和服务端共享的数据结构继续使用 `src/types.ts`。
- feature 可以依赖 `api.ts`、`types.ts` 和 `components/`，不要反向依赖 `App.tsx`。
- 保持现有本地优先、安全边界和响应式行为；结构重构不得改变 API 协议。
- 提交前运行 `npm run typecheck`、`npm test` 和 `npm run build`。
