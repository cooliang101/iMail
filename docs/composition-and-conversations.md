# 写信与会话

iMail 的写信数据在 Windows 本地嵌入模式和 HTTP 远程模式使用同一组 Rust 领域模型。本文记录 schema 12 起的行为。

## 写信

- “回复”优先使用 Reply-To，没有时使用 From；“回复全部”再加入原 To/Cc，按地址大小写不敏感去重，并排除当前用户已接入的全部邮箱身份。
- Bcc 随本地草稿保存、恢复并进入 SMTP envelope。iMail 不生成 Bcc 投递头；协议测试同时检查 SMTP 收件人与最终 MIME。发送结果只返回给当前已授权用户。
- 回复草稿保存规范化后的 In-Reply-To 与 References，邮件头值受数量和注入校验。
- 账户签名和模板作为当前应用用户的偏好保存。内容是纯文本，插入编辑器前转义；新邮件/回复可分别控制签名。草稿恢复不重新插入签名；切换发件账户时只替换尚未编辑的自动签名，否则保留正文并提示核对。
- 模板在光标处插入，主题仅在当前主题为空时填充，插入后与普通正文一样可编辑。
- 正文提到附件而没有附件时显示应用内确认。检测排除引用历史和签名；用户仍可显式继续发送。
- 地址未完成、草稿保存失败或正在发送时，不允许通过关闭写信或切换邮件丢弃当前编辑；保存失败可在编辑器内修正后重试。发送期间编辑区暂时不可操作，避免发送快照与屏幕上的内容发生变化。

应用 HTTP 的 `/api/send` 与草稿接口、MCP 的 `message_send` / `draft_save` 接受 `bcc`、`inReplyTo`、`references`。MCP 的 `settings_get` / `settings_update` 通过 `composition` 读写签名和模板。Developer Gateway 的 `/send` 同样支持写信 envelope 字段；它不提供草稿、模板或签名管理。

## 会话

`GET /api/messages/:id/conversation` 和 MCP `conversation_get` 根据有效的 Message-ID、In-Reply-To、References 查询当前用户本地缓存中的会话摘要。

- 不使用主题兜底，因此同主题的无关邮件不会串联。
- 父邮件不在本地时，具有同一有效祖先的邮件仍可关联。
- 循环引用使用非递归并查集合并，不会无限遍历。
- 相同 Message-ID 的记录只有在核心元数据和缓存正文指纹一致时作为跨账户/文件夹副本关联；冲突记录及指向该歧义 ID 的边被隔离。
- 每条记录保留账户、文件夹、已读和附件等真实属性。查询和展开不修改邮件；回复动作准确绑定展开的那条记录。
- 会话发现主要读取元数据；重复 Message-ID 才逐条读取缓存正文计算指纹，内存只保留摘要。响应不含正文，阅读正文和附件在用户展开单封邮件时按 ID 延迟加载。

阅读器会折叠纯文本引用、已知原邮件分隔符，以及 HTML 的 blockquote 和常见提供商引用容器。HTML 折叠发生在严格清理之前，不会绕过现有脚本、URL、属性和 CSS 安全规则。

## 迁移

schema 12 为邮件加入安全解析后的头部 JSON，为草稿加入写信 envelope JSON。旧邮件只从原始 RFC 822 副本回填；原始内容不被改写。没有原始副本或解析失败时保持空关联头，避免凭主题猜测。

## 验证覆盖

自动化回归覆盖以下边界：

| 范围 | 测试入口与覆盖 |
| --- | --- |
| 回复、签名、提醒 | `frontend/src/features/compose/reply-model.test.ts`：Reply-To、自地址排除、To/Cc 去重、头部注入、签名转义与提醒排除引用 |
| 引用展示安全 | `frontend/src/features/mail/quoted-history.test.ts`：多层引用、行内回复、服务商容器及 HTML 清理 |
| 会话关系 | `crates/imail-core/src/messages/conversation.rs`：缺失祖先、循环、重复 ID 冲突、账户/文件夹副本与一万封长链 |
| 数据与隔离 | `crates/imail-storage-sqlite/src/tests.rs`：schema 12 回填、原始邮件不变、Bcc 草稿往返、配置持久化及用户隔离 |
| 投递隐私 | `crates/imail-mail-network/src/lib.rs` 的真实 TLS fixture：Bcc 进入 SMTP 收件人且不进入投递 MIME，回复头正确 |
| 控制面 | HTTP 会话所有权和无正文摘要、MCP/Gateway 写信输入校验、Tauri 路由及字段保真、官方 MCP SDK 互通 |

常规检查从仓库根目录运行：

```powershell
npm --prefix frontend run typecheck
npm --prefix frontend test
npm --prefix frontend run build
npm --prefix frontend run test:e2e
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --target x86_64-pc-windows-msvc -- -D warnings
cargo test --workspace --all-features --target x86_64-pc-windows-msvc
```

浏览器专项验收使用隔离 fixture，检查密送草稿关闭恢复、附件提醒取消/确认发送、模板插入与管理、签名切换/编辑保护、会话多封展开和精确回复目标、保存失败恢复及窄屏布局。常规 E2E 与专项验收分开记录，不能仅凭既有 E2E 通过推断新增功能已覆盖。真实邮箱 canary、安装包发布和首屏性能预算不属于这些检查。
