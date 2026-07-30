# iMail 样式系统与维护规范

本文是 iMail 客户端样式的维护契约，适用于 `src/` 下的所有界面。目标不是追求一套抽象的“设计系统”，而是让邮件客户端在高信息密度、长时间阅读、不同屏幕尺寸和多账户身份并存时，仍保持稳定、清楚且可演进。

## 1. 现状审计与设计方向

### 已有优势

- 产品以统一语义 token 支撑四套明确视觉语言：经典薄荷、冷青科技、海军蓝商业和柔和粗野主义；都保持深色账户轨、清晰工作区层级与低刺激操作色。
- 三栏桌面结构适合多账户邮件工作流；在平板收窄，在手机切换为列表/详情单页，响应策略符合任务模型。
- 邮件列表保持紧凑，阅读器留出更宽松的行高和内容宽度，信息密度有清晰分区。
- 状态、空状态、加载骨架、键盘焦点和 `prefers-reduced-motion` 已有实现基础。
- 图标统一使用 Phosphor；Fluent UI 负责基础交互控件，业务组件复用统一表单封装。

### 当前技术债

- 原 `styles.css` 中存在数百个直接色值，同一层级出现大量肉眼几乎相同的灰绿，修改主题需要逐个选择器排查。
- Fluent UI 品牌色阶原来写在 `main.tsx`，CSS 变量写在 `styles.css`，两个控制面没有共同的维护入口。
- 字号经过多轮可读性补丁后形成了有效层级，但命名只有局部别名，组件仍可继续写任意字号。
- 间距、圆角、阴影、动效和 z-index 多数是裸值，缺少尺度与选择依据。
- `styles.css` 同时承担基础样式、业务组件和响应式覆盖；在不拆动现有 feature 结构的前提下，需要先建立主题边界，再逐步按领域拆分。
- 邮件 HTML iframe、邮箱服务商品牌色和账户自定义色属于外部内容/身份数据，不能与产品 UI 强行使用同一个强调色。

### 设计原则

1. **内容优先**：主色只表达操作、选择和关键状态，不把每个图标都变成装饰色。
2. **主题内单一中性色族**：每套主题使用一致的冷灰、蓝灰或暖米色家族，禁止在同一主题中混入无语义的灰阶或蓝紫渐变。
3. **密度分区**：导航和邮件列表紧凑；阅读、编辑、设置和开发者文档使用更舒展的行距。
4. **身份色与状态色分离**：邮箱品牌、账户自定义色、工作空间色不能替代成功/警告/错误语义。
5. **先语义、后数值**：组件描述“正文文字”“弱边框”“危险背景”，不描述具体 hex。

## 2. 样式架构

样式入口顺序如下：

1. `src/theme.ts`：生成 Fluent UI 的 `Theme`，只维护 Fluent 品牌色阶和基础字体。
2. `src/theme.css`：产品主题 token，是颜色、排版、间距、形状、阴影、动效、层级和布局尺寸的唯一入口。
3. `src/styles.css`：组件和响应式规则，只消费语义 token，不定义主题。

`main.tsx` 必须先导入 `theme.css`，再导入 `styles.css`。`AppThemeProvider` 同时切换根 `FluentProvider` 品牌色和 `data-theme` 语义 token；不要在业务组件中判断主题并切换 class。

当前主题 ID 与定位：

| ID | 设置名称 | 视觉定位 |
| --- | --- | --- |
| `mint-fresh` | 薄荷清新 | 原有冷灰绿、深松石账户轨和低饱和薄荷强调色；默认主题 |
| `tech` | 霓虹终端 | 冷白、深墨青、青色信号光、细网格与利落几何 |
| `business-blue` | 深海蓝图 | 海军蓝、清晰操作层级和克制阴影 |
| `soft-neubrutalism` | 柔和撞色 | 奶油底、粉彩、深色描边和轻微错位阴影 |

主题元数据与安全归一化放在 `src/features/appearance/theme-model.ts`，Fluent 色阶放在 `src/theme.ts`，完整 CSS token 契约放在 `src/theme.css`。新增主题必须同时补齐这三处，并为无效或已移除的主题 ID 保留安全回退。

### Token 分层

- **Primitive**：如 `--brand-70`、`--neutral-50`，表达色阶中的固定位置。只在 `theme.css` 内使用。
- **Semantic**：如 `--color-text-secondary`、`--color-accent-subtle`，表达用途。组件只使用这一层。
- **Component/identity**：如运行时 `--avatar-color`、`--account-color`，由数据驱动并限制在组件局部。
- **Compatibility alias**：`--accent`、`--surface` 等只为旧选择器保留。新代码禁止使用；修改旧选择器时应顺手替换为完整语义名。

新增 token 前先确认现有语义能否满足。只有至少两个组件需要共享且无法用现有语义准确表达时，才新增 token。

## 3. 文本与排版

### 字体

- UI：`var(--font-family-ui)`，使用系统的 Segoe UI Variable，并为中文提供 Microsoft YaHei UI 回退。
- 展示标题：`var(--font-family-display)`，只用于页面标题、弹窗标题、邮件主题等强层级文本。
- 代码与固定宽度数据：`var(--font-family-mono)`。
- 数量、时间、配额等需要纵向比较的数字使用 `font-variant-numeric: tabular-nums`。

不要从业务组件加载远程字体。邮件客户端需要快速首屏、离线可用和稳定的中英文混排，系统变量字体比网络字体更合适。

### 字号层级

| Token | 尺寸 | 用途 |
| --- | ---: | --- |
| `--font-size-1` | 11px | 标签、计数、辅助元数据的下限 |
| `--font-size-2` | 12px | 次要说明、时间、快捷键 |
| `--font-size-3` | 13px | 控件文字、列表正文 |
| `--font-size-4` | 14px | 正文与编辑器默认文字 |
| `--font-size-5` | 16px | 小标题 |
| `--font-size-6` | 18px | 面板标题 |
| `--font-size-7` | 22px | 页面/弹窗标题 |
| `--font-size-8` | 28–36px | 开发者工作区等展示标题 |

11px 是产品 UI 的可读性下限。禁止新增 8–10px 文本；历史选择器已由末尾的排版收敛规则提升。正文默认使用 `--line-height-body`，长邮件与文档使用 `--line-height-reading`，控件使用 `--line-height-compact`。

标题应使用较紧的行高和 `--letter-spacing-heading`；分组标签可使用 `--letter-spacing-label`。正文段落建议控制在约 65–75 个拉丁字符宽，阅读器由 `--content-reading-width` 限制。可换行标题使用 `text-wrap: balance`，说明文字使用 `text-wrap: pretty`；列表行中的主题与地址则继续截断，避免破坏密度。

## 4. 颜色与表面

### 表面层级

- `--color-canvas`：整个应用底色。
- `--color-surface`：主工作区、阅读区。
- `--color-surface-subtle`：侧栏、分组区和轻微分层。
- `--color-surface-raised`：弹窗、菜单、输入框等浮起表面。
- `--color-surface-sunken`：搜索框、代码外围等内嵌表面。
- `--color-rail`：账户轨的唯一深色表面。

不要通过“白卡片 + 边框 + 阴影”同时叠加来制造层级。常规容器优先使用背景差；输入与分隔使用边框；菜单、弹窗等真正浮层才使用阴影。

### 文字和边框

- 主文字：`--color-text` / `--color-text-strong`
- 次要文字：`--color-text-secondary`
- 辅助文字：`--color-text-tertiary`
- 默认边框：`--color-border`
- hover 或强调边框：`--color-border-strong`

禁止使用透明度降低整个组件来表达次要层级，因为这会同时损害图标、边框和对比度。禁用态是例外，由全局规则处理。

### 语义色

- 主操作与选择：`--color-accent*`
- 信息：`--color-info*`
- 警告：`--color-warning*`
- 危险与错误：`--color-danger*`
- 成功与在线：`--color-success*`
- 键盘焦点：`--color-focus` 和 `--color-focus-ring`

每种状态优先组合“深色文字/图标 + 同族浅色背景”，不要只依赖颜色；错误、同步等状态还需要文字或图标。

### 允许的颜色例外

- `--avatar-color`、`--account-color`：来自账户数据，仅用于头像、账户标记和极浅的身份背景。
- `--color-provider-*`：第三方服务商的识别色。
- `--color-workspace-*`：工作空间的区分色，不能用于按钮或状态。
- 邮件 HTML iframe：属于不可信的隔离内容，必须保持独立的安全文档和阅读兜底样式，不能依赖父页面 CSS 变量。
- 透明阴影或混色：优先使用 `rgb(r g b / a)`、`color-mix()` 或主题阴影；只有运行时颜色才在局部计算。

## 5. 间距、形状、阴影与布局

### 间距

使用 4px 基础节奏：`--space-1/2/3/4/5/6/8/10/12`。组件内部通常使用 8–16px，面板间距使用 16–24px，页面级留白使用 24–48px。光学对齐允许 1–2px 微调，但需要与图标基线或边框有关，不能创建新的全局尺度。

### 圆角

- `--radius-xs`：标签、微型标记。
- `--radius-sm`：图标按钮、内部小控件。
- `--radius-md`：输入框、列表项、普通按钮。
- `--radius-lg`：主面板、卡片、账户头像。
- `--radius-xl`：移动端弹窗等大容器。
- `--radius-round`：状态点、确实需要胶囊形的控件。

外层容器圆角应大于内部控件。不要给所有元素统一圆角，也不要把普通操作按钮无差别做成胶囊。

### 阴影和层级

`--shadow-sm/md/lg` 跟随各主题的环境色与形态，分别用于悬浮提示、菜单/浮层和弹窗；Soft Neubrutalism 使用错位实色阴影。普通邮件行、设置条目和静态面板不使用阴影，主题预览与柔和粗野主义的描边卡片属于明确例外。

z-index 必须从 `--z-sticky/dropdown/sidebar/overlay/modal/toast` 中选择。组件内部可使用 1–2 的相对层级；禁止新增 999、9999 等值。

### 应用布局

桌面结构是 `账户轨 / 主导航 / 工作区`；工作区内是 `邮件列表 / 阅读或编辑面板`。关键尺寸由 `--layout-*` 管理。布局规则：

- `> 1050px`：完整三栏，消息列表以 `--layout-message-list` 为上限。
- `821–1050px`：收窄账户轨、侧栏和消息列表；阅读内容留白同步收窄。
- `651–820px`：主导航变为抽屉，保留账户轨与列表/阅读并列。
- `≤ 650px`：单列任务流，列表与阅读/写信互斥显示；顶部栏保持 sticky。

新增 feature 必须在 1050、820、650 三个现有断点验证。不要为单个组件随意添加相邻断点；确需新增时先说明无法使用容器自适应的原因。

## 6. 组件与交互规范

- 表单继续统一使用 `AppInput`、`AppSelect`、`AppTextarea`、`AppCheckbox`，不要直接新增原生表单控件。
- 可点击元素必须具备 hover、active/pressed、focus-visible 和 disabled 状态；触控目标建议不小于 36px。
- 动效使用 `--duration-*` 与 `--ease-standard`，只动画 `transform`、`opacity`、颜色和阴影。新增长动效前先验证 reduced-motion。
- 加载态应匹配真实内容形状；空状态说明下一步；错误提示直接说明失败原因和恢复方式。
- 图标默认使用 Phosphor，常规 UI 统一相近视觉尺寸和描边重量。图标颜色服从文本或 `data-icon-tone`，不逐个写色值。
- 业务组件保持语义 HTML；完整 feature 仍放在 `src/features/<domain>/`，跨领域展示组件放在 `src/components/`。

## 7. 新增或修改样式的流程

1. 确认样式应属于主题、共享组件还是某个 feature。
2. 优先复用现有语义 token；不要从设计稿直接复制 hex、阴影或任意字号。
3. 同时实现默认、hover、pressed、focus-visible、disabled，以及适用的 loading/empty/error 状态。
4. 检查 1050px、820px、650px 和窄于 390px 的布局，不允许横向溢出遮住主任务。
5. 使用键盘完成关键路径，并检查焦点是否可见。
6. 运行 `npm run typecheck`、`npm test`、`npm run build`。

建议审查命令：

```powershell
# 新业务样式中不应继续增加裸色值；主题文件和隔离邮件 HTML 是例外。
rg -n "#[0-9a-fA-F]{3,8}|rgb\(|hsl\(" src --glob "*.css" --glob "*.tsx"

# 检查任意字号、z-index 和原生表单控件。
rg -n "font-size:|z-index:|<(input|select|textarea)" src
```

## 8. 后续拆分策略

主题层已经独立，组件规则目前按领域拆在 `src/styles/*.css`，由 `styles.css` 统一导入。后续修改某个 feature 时，可把对应规则原样迁到 `src/features/<domain>/<domain>.css`，由该 feature 的入口导入；跨领域基础样式继续留在共享样式目录。拆分只改变归属，不应同时重命名全部 class 或改变 API/交互。

每次迁移旧规则时应完成三件事：删除兼容别名使用、把残留裸值替换成语义 token、合并重复的后置覆盖。完成全部领域迁移后，再删除 `theme.css` 中的 compatibility aliases。
