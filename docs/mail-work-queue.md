# 邮件处理队列与回复自动化

## 交付范围

iMail 为每个应用用户维护独立的邮件处理队列。一个邮件最多对应一个活动项目，状态包括：

- `needsReply`：待回复；
- `needsReview`：草稿或处理结果待确认；
- `followUp`：到期后需要主动跟进；
- `waiting`：已安排回复，等待对方。

项目可保存 RFC 3339 `dueAt` 和最多 4000 个字符的备注。邮件或账户删除时，关联项目通过 SQLite 外键同步清除。桌面侧栏展示队列总数，队列页支持筛选、状态切换、到期提示、打开关联草稿和完成项目；邮件阅读页可直接加入“待回复”。

## HTTP API

| 方法 | 路径 | 用途 |
| --- | --- | --- |
| `GET` | `/api/mail-work-items` | 列出当前用户队列；可用 `status` 筛选 |
| `PUT` | `/api/messages/:id/work-item` | 新增或更新状态、期限和备注 |
| `DELETE` | `/api/messages/:id/work-item` | 完成并移出队列，不删除邮件或草稿 |
| `POST` | `/api/messages/:id/reply-draft` | 创建安全回复或回复全部草稿 |
| `POST` | `/api/drafts/:id/schedule` | 显式确认后安排草稿发送 |

HTTP、Tauri 嵌入式服务和 MCP 复用同一 Rust 业务入口，避免本地桌面与远程服务语义漂移。

## 回复与发送闭环

`mail_reply_draft_create` 接受原邮件 ID、`reply`/`replyAll` 模式和 Agent 生成的正文。服务端固定使用原邮件所属账户，优先采用 `Reply-To`，回复全部时合并原 To/Cc 并排除当前用户的全部邮箱地址，同时规范化 `In-Reply-To` 与 `References`。调用只创建草稿，并把队列项目设为 `needsReview`。

`mail_draft_schedule` 读取已保存草稿的不可变发送快照，要求：

- `confirmed` 必须为 `true`；
- `requestId` 必须是 UUID，并沿用发件箱幂等冲突检查；
- `sendAt` 必须是现在至一年内的 RFC 3339 时间；
- 收件人、正文、回复头和附件继续使用现有发信校验。

安排成功后处理项目变为 `waiting`。SMTP 的明确成功、明确失败和不确定结果仍由既有发件箱负责；应用不会把“已加入发件箱”误报为“SMTP 已发送”。

## 安全边界

- 邮件正文中的任何指令均是不可信数据，不会改变工具授权或确认要求。
- 队列、原邮件、草稿与账户都按当前应用用户验证所有权。
- MCP 写操作需要 `mcp:full`，并只在安全审计中记录工具名与授权码 ID，不记录正文和参数。
- iMail 不内置模型调用；Agent 负责生成建议正文，服务只负责确定性收件人、线程关系、草稿和受控发送。
- 自动立即外发仍不属于此闭环；需要发送时必须经过草稿与 `confirmed: true`。
