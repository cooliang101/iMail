---
status: accepted
---

# 邮件翻译采用环境感知的多提供商架构

iMail 的邮件翻译不绑定单一厂商。Edge WebView2 本地模型、正式云端 API 与非官方实验协议的可用性、隐私边界、凭据形式和运行位置不同，因此翻译能力由稳定的领域协议和可扩展 Provider Profile 驱动。

## 决策

- 每个翻译服务配置为一个 `TranslationProviderProfile`。Profile 同时标识提供商类型与执行位置；桌面 WebView、本机 Rust 服务和远程 iMail 服务中的同类提供商是不同 Profile。
- 可跨环境同步的翻译偏好只保存默认目标语言、自动翻译开关和缓存开关。默认 Profile 选择属于当前执行环境，不进入通用偏好同步。
- WebView2 本地 Translator 由前端能力适配器执行。DeepL、Google Cloud、Azure Translator 和 Bing Web 等网络提供商由 Rust 翻译服务执行。
- 每个应用用户只允许一个 Edge 本地 Profile；其他服务仍可按凭据、区域或执行环境维护多个 Profile。
- 提供商的非敏感配置使用有类型的协议结构。密钥和服务账号不属于共享协议，只允许 Profile 引用使用 `master.key` 加密保存的凭据。
- Edge 本地翻译不把正文发送出设备。所有网络提供商都必须在首次使用及披露文本变更后获得按 Profile 记录的明确同意。
- 自动选择不得跨隐私边界静默降级。本地翻译失败时，iMail 只能提示选择已配置的云端 Profile，不能自行发送正文。
- Bing Web 是需要单独正文外发授权的实验提供商，不参与未授权的自动选择；其非官方协议不得成为邮件翻译的基础依赖。
- 翻译输入从第一版开始按稳定 ID 分段。第一版可以用纯文本展示结果，但协议必须允许后续恢复安全清洗后的 HTML 结构、排除引用历史并增量复用译文。
- 译文缓存使用用户、邮件、正文哈希、语言对、Profile、Provider 修订和分段版本共同隔离。删除邮箱数据时必须删除相应译文缓存。
- HTTP、Tauri 和未来 MCP 适配器复用同一 Rust 领域服务。任何控制面都不得读取凭据原文；MCP 翻译能力在提供商和隐私边界稳定后另行开放。

## 提供商类别

1. 本地：Edge WebView2 Translator，运行时检测，不承诺所有 WebView2 Runtime 可用。
2. 官方云端：DeepL、Google Cloud Translation、Azure Translator，使用公开且受支持的 API。
3. 实验协议：Bing Web，隔离实现、单独版本、有限重试并在反爬响应后冷却。

## 推进顺序

1. 共享协议、Profile 模型与架构文档。
2. Provider Registry、设置页面、加密凭据存储和状态检测。
3. 正文分段、阅读器交互和译文缓存。
4. WebView2 本地翻译。
5. DeepL 官方 API。
6. Google Cloud 与 Azure Translator。
7. Bing Web 实验提供商。
8. HTTP/MCP 能力与最终隐私审查。

## 当前实现说明

- DeepL 使用 Rust 侧统一网络执行入口，Free 与 Pro Profile 分别访问官方对应域名。API Key 只在服务端从 `master.key` 加密存储中解密，并仅写入 `Authorization` 请求头。
- 正文按稳定分段顺序批量发送；每批不超过 50 段，并在官方 128 KiB 总请求上限内保留安全余量。返回段数或顺序不完整时不写入缓存。
- 429 与 5xx 使用有限指数退避；鉴权、额度、限流、语言和上游故障转换为不包含供应商响应正文的安全错误。
- Google API Key 配置调用 Cloud Translation Basic v2，并通过 `x-goog-api-key` 请求头传递；Service Account JSON 在 Rust 内签发短期 JWT、换取 OAuth Token 后调用 Advanced v3。Service Account 的 Token 地址固定为 Google 官方端点，防止把签名断言发送到任意地址。
- Azure Translator 通过订阅密钥头和可选区域头调用 v3 文本翻译；全局资源与自定义域名分别使用官方路径规则，每批正文控制在 45,000 字符以内，为官方 50,000 字符上限保留余量。
- Bing Web 仅作为显式启用的实验 Profile：Rust 从 Bing Translator 页面提取短期 `IG`、`IID` 与防滥用参数，每段按 900 字符再切片；状态 205 只刷新会话并重试一次，401/429 会触发进程内五分钟冷却。重定向后的会话域必须仍属于 `bing.com`，协议变化或响应不完整时不会写入译文缓存。
- MCP 只在 `mcp:full` 下公开安全 Profile 列表和服务端邮件翻译。返回结果移除用户 ID、正文哈希和供应商原始响应；WebView 本地 Profile 不会被服务端代执行，翻译调用只记录不含参数的安全审计事件。
- HTML 邮件的双语模式先复用严格清洗后的原始排版，再按稳定分段顺序把纯文本译文插入对应正文块下方。只有全部分段都能可靠映射时才启用原排版双语；映射不完整时整体回退到纯文本阅读版。引用历史保留原文但不进入翻译请求，译文节点不接受供应商 HTML。

## 不采用的方案

- 不把 EdgeTranslate 或 Bing 网页协议直接写入邮件阅读器，因为这会把 UI、非官方协议和隐私决策耦合在一起。
- 不把 API Key 放入 `AppPreferences`、浏览器存储或前端请求日志。
- 不把提供商类型当作唯一配置标识，因为同一类型可以在多个执行环境中存在。
- 不默认自动翻译外语邮件，也不允许失败后无提示切换到另一个云端厂商。
