# React 到 Preact 迁移方案

> 实施状态（2026-08-17）：阶段 1–5 的代码迁移已经完成；React、React DOM、Fluent UI 和 TipTap React binding 均已从直接依赖移除。每一阶段均通过 typecheck、lint、Vitest、生产构建、Playwright E2E 与无障碍回归。Windows WebView2 内存、冷启动与长时间运行指标仍需在固定测试机上按本文性能验收方法采集，不能由 bundle 体积代替。

## 目标

iMail 在不分叉 Windows 桌面与 Docker Web 界面的前提下，将客户端运行时从 React 迁移到 Preact，减少前端依赖、JavaScript 下载与解析成本，并降低 WebView2 renderer 的常驻 JavaScript 开销。

迁移完成后的目标栈：

```text
Preact
├── preact/hooks：组件生命周期与局部状态
├── @preact/signals：高频、细粒度共享状态（仅在实测有收益时引入）
├── iMail components：按钮、弹窗、菜单、表单和反馈组件
├── CSS theme tokens：内置主题与安全自定义主题
├── @tiptap/core：富文本编辑器
└── Tauri adapter / HTTP adapter：桌面本地、桌面远程与浏览器模式
```

本次迁移不改变：

- HTTP API、MCP、Gateway 或类型化桌面调用协议。
- 本地与远程服务的数据隔离和选择语义。
- Rust 同步任务、SQLite、OAuth、凭据和 Cookie 安全边界。
- Windows 桌面与 Docker Web 共用一套界面的产品边界。
- 主题 ID、回退规则和自定义主题的安全字段集合。
- 邮件 HTML sanitizer、附件预览策略和富文本邮件格式。

## 当前基线

迁移开始前的生产构建基线如下。文件名带内容哈希，后续比较应按 chunk 职责而不是完整文件名匹配。

| 产物 | 原始大小 | gzip |
| --- | ---: | ---: |
| React、React DOM、Scheduler | 188.5 KB | 59.1 KB |
| 主应用与其他首屏依赖 | 465.8 KB | 135.7 KB |
| TipTap、ProseMirror | 391.0 KB | 124.5 KB |
| 设置功能 | 72.7 KB | 22.5 KB |

代码耦合基线：

- `frontend/src/` 中约 54 个文件直接导入 React。
- 27 个文件导入 `@fluentui/react-components`。
- `@tiptap/react` 只由 `RichTextEditor.tsx` 直接使用。
- 当前有 34 个前端单元测试文件，约 1025 行测试代码。

产物大小不是唯一成功指标。迁移前应在同一台测试机、同一 WebView2 Runtime 和同一测试数据上记录：

1. 冷启动到登录界面和邮件工作区可交互的时间。
2. 登录后空闲五分钟的应用进程、WebView2 进程组和总 working set。
3. 打开包含至少一万封邮件的文件夹后的列表滚动帧时间。
4. 连续切换 100 封邮件前后的 renderer working set 和 JS heap。
5. 打开、编辑和关闭写信窗口前后的内存回落。
6. 窗口隐藏到托盘五分钟后的 CPU、计时器和订阅活动。

## 为什么选择 Preact

Preact 保留 JSX、函数组件、Hooks、Context、Error Boundary、Suspense 和 lazy loading，能够复用当前 feature 划分以及大部分组件结构。迁移可以先通过 `preact/compat` 验证第三方兼容性，再逐步改为 Preact 原生导入。

不选择完全手写 DOM，原因是 iMail 已经包含持续同步事件、认证门禁、虚拟列表、多个工作区、弹窗焦点、草稿状态和异步竞态。手写这些生命周期会形成一套没有生态和测试工具支持的内部框架。

暂不选择 Solid、Lit 或其他不同组件模型，是因为它们需要重写现有 TSX 的状态和生命周期语义，无法提供 Preact 兼容迁移路径。

参考资料：

- [Preact 项目目标](https://preactjs.com/about/project-goals/)
- [Preact 与 React 的差异](https://preactjs.com/guide/v11/differences-to-react/)
- [React 生态兼容配置](https://preactjs.com/guide/v10/getting-started/#aliasing-react-to-preact)

## 迁移原则

### 每个阶段必须可独立回滚

兼容层、UI 组件、业务组件和编辑器分开提交。不得在同一个大提交中同时替换运行时、主题系统和富文本编辑器。

### 先消除 React 专属库，再删除 React

`preact/compat` 可以帮助验证，但不是第三方 React 组件的长期担保。Fluent UI 和 TipTap React binding 必须在最终删除 React 包之前移除。

### 不以关闭类型检查换取兼容

不得把 `skipLibCheck` 当作长期解决方案，也不得使用大范围 `any` 掩盖 Preact 与 React 类型差异。临时兼容声明必须集中、注明删除阶段并有测试覆盖。

### 只在数据证明需要时使用 Signals

普通局部交互继续使用 Hooks。邮件选择、未读计数、同步状态等高频跨组件状态只有在 Profiler 证明 Context 或顶层状态造成多余渲染时，才迁到 Signals。不得在迁移期同时重写所有状态管理。

## 分阶段实施

### 阶段 0：补齐性能基线

在修改依赖之前完成基线采集，并保存测试环境信息：

- iMail 版本与 Git commit。
- Windows 版本、CPU、内存和缩放比例。
- WebView2 Runtime 版本。
- 邮件数量、HTML 邮件样本和附件样本。
- 冷启动和热启动分别测量至少五次，使用中位数比较。

阶段门禁：能够重复采集启动时间、working set、JS heap 和列表滚动指标。没有可重复基线时不得用主观感受宣布迁移成功。

### 阶段 1：Preact compatibility spike

仅建立兼容构建，不删除 React，也不修改业务组件：

1. 在 `frontend/` 安装 `preact` 和 Preact 的 Vite preset。
2. 将生产构建中的 `react`、`react-dom`、`react-dom/client`、`react/jsx-runtime` 和测试入口解析到 Preact 对应模块。
3. 为 TypeScript 配置一致的 JSX runtime 和模块路径。
4. 保持 React 包暂时存在，以满足尚未移除的第三方 peer dependency。
5. 检查构建产物，确认应用代码没有意外打包第二份 React。

必须专项验证：

- `Suspense` 和所有 lazy feature。
- `ErrorBoundary` 的捕获与 reset。
- Overlay、ContextMenu、弹窗焦点和 Escape 关闭。
- React ref、`forwardRef`、`useImperativeHandle`。
- Tauri 标题栏、托盘菜单和 desktop event listener 清理。
- Fluent UI Provider、Button 和主题切换。
- TipTap 编辑、工具栏状态、撤销和销毁。

阶段门禁：现有 typecheck、Vitest、构建、Playwright 和无障碍测试全部通过；产物中只有 Preact runtime。任何无法解释的 Fluent UI 或 TipTap 行为差异都应先阻止继续迁移。

### 阶段 2：移除 Fluent UI

在 `frontend/src/components/` 建立跨 feature 的展示组件：

- `AppButton`
- `AppIconButton`
- `AppSpinner`
- `AppDialog` / 现有 `Overlay`
- `AppMenu` / 现有 `ContextMenu`
- 已有 `AppInput`、`AppSelect`、`AppTextarea`、`AppCheckbox`

迁移要求：

- 业务 feature 不直接新增原生表单控件。
- 所有按钮保留 `type`、disabled、loading、icon、primary/subtle/danger 语义。
- 焦点环、键盘操作、ARIA name 和高对比度模式不能退化。
- 不复制 Fluent DOM 结构或样式；组件使用 iMail 自有 CSS token。
- `AppThemeProvider` 不再创建 Fluent Theme，只负责主题选择、持久化、根属性和安全 token 派生。
- 内置主题继续同步维护 `frontend/src/theme.ts` 与 `frontend/src/theme.css`。
- 自定义主题字段不变，不改动 Rust、MCP 或文档协议。

按 feature 分批替换，每一批保持测试通过。全部调用点删除后再卸载 `@fluentui/react-components`。

阶段门禁：代码搜索中不存在 Fluent UI import；四套内置主题和 custom 主题完成桌面、窄屏和 Web 视觉回归。

### 阶段 3：迁移到 Preact 原生 API

逐 feature 将类型和运行时导入替换为 Preact：

- Hooks 从 `preact/hooks` 导入。
- `lazy`、`Suspense`、`Component`、`createContext` 等从 `preact/compat` 过渡到合适的 Preact API。
- `ReactNode` 改为 `ComponentChildren`。
- React DOM 事件类型改为 Preact JSX 对应类型或领域级回调参数。
- `CSSProperties` 改为 Preact JSX CSS 属性类型。
- 服务、领域模型和纯函数不得依赖 UI 框架类型。

重点检查标准 DOM 事件与 React synthetic event 的差异，尤其是：

- `onInput` / `onChange`
- Portal 内事件传播
- focus、blur 和 related target
- checkbox、search input 和文件输入
- compositionstart、compositionupdate、compositionend

阶段门禁：业务源文件不再直接导入 `react` 或 `react-dom`；兼容层只允许为尚未迁移的编辑器或明确记录的边界存在。

### 阶段 4：移除 TipTap React binding

保留 TipTap/ProseMirror 编辑内核，将 `RichTextEditor` 改为 Preact 生命周期管理的 `@tiptap/core` 实例：

1. 使用 ref 提供编辑器 DOM 容器。
2. 组件挂载时创建 `Editor`。
3. 订阅 selection、transaction 和 update，派生工具栏状态。
4. props 改变时只更新必要内容，避免重建 editor。
5. 组件卸载时解除全部订阅并调用 `editor.destroy()`。
6. 文件选择、内联图片、链接弹层和附件回调继续使用统一表单与 Overlay 组件。

需要覆盖：中文 IME、跨段选择、粘贴 HTML、撤销/重做、链接、列表、引用、正文图片、附件和草稿恢复。

阶段门禁：关闭写信窗口后不存在残留 editor、MutationObserver、DOM listener 或 object URL；`@tiptap/react` 已从依赖和 lockfile 删除。

### 阶段 5：删除 React 兼容依赖

只有前述阶段完成后才执行：

- 删除 `react`、`react-dom`、`@types/react`、`@types/react-dom`。
- 删除 `@vitejs/plugin-react`。
- 删除不再需要的 alias 和临时类型声明。
- 将服务端静态渲染类测试迁到 `preact-render-to-string` 或 DOM 行为测试。
- 更新 bundle chunk 规则，删除 `react-vendor`，按实际需要决定是否单列 `preact-vendor`。
- 更新 `frontend/README.md`、根 README、架构文档和工程交接说明中的 React 描述。

阶段门禁：`package.json`、`package-lock.json`、源代码和构建产物均不再包含 React runtime。

### 自动化迁移结果

同一开发机上的生产构建对比：

| 产物 | 迁移前 gzip | 迁移后 gzip | 变化 |
| --- | ---: | ---: | ---: |
| UI runtime vendor | 60.57 KB | 8.25 KB | -52.32 KB（-86.4%） |
| 主应用 chunk | 138.96 KB | 58.87 KB | -80.09 KB（-57.6%） |
| TipTap/ProseMirror | 127.51 KB | 122.18 KB | -5.33 KB（-4.2%） |

图标层改用框架无关的 `@iconify-icons/ph` SVG 数据，由 `frontend/src/components/icons.tsx` 的 Preact 适配器统一渲染。`npm ls react react-dom --all` 为空，普通 `npm ci` 无需忽略 peer dependency，生产产物只有 Preact runtime。

## 验证矩阵

### 每个阶段的基础检查

从仓库根目录运行：

```powershell
npm --prefix frontend run typecheck
npm --prefix frontend test
npm --prefix frontend run build
```

涉及组件、焦点、认证或响应式布局的阶段还必须运行：

```powershell
npm --prefix frontend run lint
npm --prefix frontend run test:e2e
npm --prefix frontend run test:a11y
```

Windows 交付候选还必须按内部测试流程构建 NSIS 并执行桌面 smoke test；未经用户明确授权，不创建 tag，不触发 `workflow_dispatch`。

### 必测业务流程

- 首次注册、登录、退出和 401 回到登录页。
- 本地模式与远程模式切换，失败时不隐式回退。
- 添加、编辑、重连和删除邮箱账户。
- 多账户统一列表、搜索、分页、虚拟滚动和键盘选择。
- 已读、星标、归档、垃圾箱、标签和稍后处理。
- 写信、回复、转发、草稿、附件和内联图片。
- HTML 邮件 sanitizer、外链、远程图片和附件预览。
- 联系人、发件人 Logo 和地址建议。
- 同步事件、新邮件通知和托盘隐藏后继续同步。
- 设置、主题、自定义主题、快捷键、MCP Token 和敏感数据操作。

### 平台与无障碍

- Windows 100%、125%、150% DPI。
- 880×600 最小窗口和现有窄屏断点。
- 键盘完整操作、Tab 顺序、焦点恢复和 Escape。
- Narrator 的按钮名称、列表位置、表单错误和弹窗语义。
- 中文 IME、复制粘贴、拖放和文件选择。
- Docker Web 的 Chromium 与至少一个非 Chromium 浏览器回归。

## 性能验收

最终是否值得合并由实测决定。至少满足：

- 构建产物中不再存在 React runtime，且初始 JavaScript gzip 体积下降。
- 冷启动中位数不得比基线恶化超过 5%。
- 登录后空闲 renderer working set 应有稳定、可重复的下降；若改善不足 10%，需要解释迁移的其他长期收益。
- 连续切换邮件和反复打开写信窗口后，内存能回落且趋势不持续增长。
- 一万封邮件列表的滚动和选择延迟不得退化。
- 窗口隐藏到托盘时，前端非必要定时器、动画和订阅停止，Rust 后台同步继续运行。

不能用 bundle 体积推算内存收益，也不能只比较任务管理器中的单个进程。WebView2 使用多进程模型，应比较同一 iMail 实例关联的完整进程组。

## 风险与应对

| 风险 | 应对 |
| --- | --- |
| Fluent UI 在 compat 下出现边缘行为 | 阶段 1 只验证，阶段 2 完全移除 Fluent UI |
| React 和 Preact 被同时打包 | 构建分析检查模块解析和 vendor chunk |
| Portal 事件或弹窗焦点退化 | 为 Overlay、ContextMenu 和设置弹窗增加键盘/E2E 测试 |
| TypeScript 类型被兼容声明掩盖 | 禁止长期 `skipLibCheck` 和散落的模块补丁 |
| TipTap editor 卸载后泄漏 | 显式 destroy、订阅清理和重复开关内存测试 |
| Signals 造成第二套状态模式 | 仅用于 profiler 证明的高频共享状态，并记录适用边界 |
| 主题视觉漂移 | 保持安全 token schema，执行四套主题与 custom 视觉回归 |
| Web 与桌面行为分叉 | 两种运行时继续使用相同 feature 和平台 adapter 接口 |

## 回滚策略

- 阶段 1 的 compat spike 必须是独立提交，可以直接回滚而不影响业务代码。
- Fluent UI 按 feature 分批迁移，不跨批次删除公共兼容组件。
- React 依赖保留到阶段 5；在此前出现阻断问题可以恢复 React runtime 而不回滚业务功能。
- 不迁移数据库、偏好 schema 或 API，因此运行旧前端不需要数据回滚。
- 发布期间不得同时合入无关的大型 UI 重构，以便定位回归。

## 完成定义

满足以下全部条件才算迁移完成：

1. React、React DOM、Fluent UI React 和 TipTap React binding 已从依赖与 lockfile 删除。
2. 客户端源代码不再导入 React runtime 或 React 类型。
3. Windows 桌面与 Docker Web 继续共用同一套 feature 代码。
4. 本地/远程协议、主题 schema 和安全边界未改变。
5. typecheck、lint、Vitest、build、E2E、a11y 和 Windows 内部交付测试通过。
6. 性能指标达到本方案的验收门槛，且没有持续增长的内存趋势。
7. README、架构说明、样式系统和工程交接文档已与实现同步。
