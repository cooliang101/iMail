# iMail 自定义主题生成规范

请为 iMail 生成一个自定义主题 JSON。只输出一个 JSON 对象，不要添加 Markdown 代码围栏、注释、CSS、解释文字或额外字段。

## JSON 结构

```json
{
  "name": "暮色珊瑚",
  "canvas": "#eef0f5",
  "surface": "#fffdfb",
  "surfaceSubtle": "#f7f3f1",
  "rail": "#262337",
  "text": "#252331",
  "textSecondary": "#6d6878",
  "border": "#ddd7df",
  "accent": "#d66f5f",
  "accentSubtle": "#f9e7e3",
  "radius": "balanced",
  "shadow": "soft",
  "typography": "system"
}
```

## 字段约束

- `name`：1–40 个字符的主题名称。
- 所有颜色必须是完整的六位十六进制颜色，格式为 `#RRGGBB`。
- `canvas`：应用最底层背景；`surface`：主内容面；`surfaceSubtle`：次级面板。
- `rail`：左侧账户轨背景，应与文字及图标有清晰对比。
- `text`：主文字；`textSecondary`：辅助文字；`border`：分隔线与控件边框。
- `accent`：唯一主强调色；`accentSubtle`：强调色的浅色背景。
- `radius` 只能是 `compact`、`balanced` 或 `rounded`。
- `shadow` 只能是 `none`、`soft` 或 `offset`。
- `typography` 只能是 `system`、`technical` 或 `rounded`。

## 设计要求

- 保证 `text` 在 `surface` 上、`rail` 内反色文字在 `rail` 上均清晰可读；iMail 会用 `rail`、`accent` 和 `accentSubtle` 自动派生账户栏按钮、选中态、边界和阴影。
- `surface`、`surfaceSubtle` 与 `canvas` 要能通过明度差区分，但避免刺眼的纯黑大面积背景。
- `accent` 用于按钮、选中态和焦点，不要再引入第二个高饱和主色。iMail 还会从安全颜色令牌派生新增邮箱弹窗的遮罩、服务商卡片、OAuth 提示、悬浮和选中状态。
- 不要生成渐变、透明色、CSS 变量、图片 URL 或可执行内容；iMail 会从这些安全令牌派生完整界面变量。

## 使用方式

在 iMail 的“设置 → 主题 → 自定义主题”中，将 AI 返回的 JSON 粘贴到“导入 AI 主题 JSON”，然后点击“校验并应用”。通过登录会话保存后，安全主题令牌会同步到服务端，并继续在当前设备保留本地缓存。

MCP 客户端可以调用 `theme_custom_update` 保存同一结构，调用 `theme_custom_get` 读取；它与 `/api/preferences` 共用同一份用户级安全主题存储。
