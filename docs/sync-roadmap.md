# 邮箱同步开发路线图

## 目标

iMail 的邮箱同步由后端持久化调度，不依赖前端页面、用户会话、SSE/WebSocket 连接或开发者网关订阅。前端只负责配置同步策略、查看运行状态和提交“立即同步”任务。

最终形态由 API 服务与独立同步 Worker 共同组成：API 负责账户、设置和本地缓存访问；Worker 负责调度、IMAP 连接、增量同步、失败恢复和新邮件事件。两者通过 SQLite 中的持久化策略、任务、租约和事件记录协作，不依赖仅存在于进程内存中的任务状态。

## 实施状态

截至 2026-07-30，M0–M6 已落地：同步提交使用细粒度 SQL 事务；策略、任务、租约、邮箱游标、事件和 Worker 心跳均已持久化；API 默认拉起并监管独立 Worker；HTTP、MCP 和前端设置已接入；UIDVALIDITY、CONDSTORE/QRESYNC、远端 UID 删除检查和收件箱 IDLE 已启用。周期调度始终作为 IDLE 断线时的可靠兜底。

保留的快照兼容接口在 `BEGIN IMMEDIATE` 内读取和提交，以确保与 Worker 跨进程串行；邮件同步热路径不再通过整库替换提交。

## 架构原则

1. **后端自治**：只要后端服务及同步 Worker 正在运行，就必须按策略同步，前端是否连接不影响任务生命周期。
2. **持久化调度**：同步策略、下次执行时间、邮箱游标、失败次数和任务租约都必须落库，进程重启后能够恢复。
3. **单邮箱互斥**：同一账户的同一邮箱文件夹在任意时刻最多有一个同步任务执行；手动请求不能制造重复并发。
4. **故障隔离**：单个账户、文件夹或 Worker 任务失败不影响 API 服务和其他账户。
5. **增量正确性优先**：按文件夹保存 `UIDVALIDITY`、UID 游标和可用时的 `HIGHESTMODSEQ`，不能只依赖账户级 `lastSyncAt`。
6. **控制面一致**：新增同步能力时同时评估 HTTP API、MCP 工具、前端设置、README 和 `docs/mcp-integration.md`。
7. **安全边界不变**：任何同步状态、任务、事件和错误响应都不得包含邮箱凭据、OAuth Token、加密字段或主密钥。

## 目标架构

```text
Frontend
   │  settings / status / sync-now
   ▼
API service ───────────────┐
   │                       │
   │ policy, jobs, events  │ cached messages
   ▼                       ▼
SQLite ◄──────────── Sync Worker ───────────► IMAP providers
   │                       │
   └──── status/events ────┘
```

- API 服务不再直接执行长时间 IMAP 同步；“立即同步”写入持久化任务队列。
- Worker 根据策略生成到期任务，也领取 API/MCP 创建的手动任务。
- Worker 将同步结果和事件写入数据库；前端可通过 SSE 获取通知，并以状态查询作为兜底。
- 开发者 WebSocket 网关只推送事件，不再承担或维持产品同步调度。

## 默认产品策略

| 设置 | 默认值 | 可选范围 |
| --- | --- | --- |
| 自动同步 | 开启 | 开启 / 暂停 |
| 收件箱间隔 | 5 分钟 | 1 / 5 / 15 / 30 / 60 分钟 / 仅手动 |
| 自动同步范围 | 收件箱 | 收件箱 / 标准文件夹 / 选择文件夹 |
| 服务启动后补同步 | 开启 | 开启 / 关闭 |
| 网络恢复后重试 | 开启 | 开启 / 关闭 |
| 持续失败通知 | 开启 | 开启 / 关闭 |

“标准文件夹”包括收件箱、已发送和归档。垃圾箱与自定义文件夹默认不自动同步，用户可显式选择。

## 里程碑

### M0：行为基线与风险测试

**目的**：在修改存储和同步逻辑前固定现有协议行为，避免结构重构改变 API 或邮件语义。

交付物：

- 为首次最近 80 封、后续 UID 增量、最近邮件标记刷新增加自动化测试。
- 覆盖多账户并发、同账户重复同步、IMAP 连接错误和进程重启场景。
- 记录当前 HTTP `sync` 接口和 MCP `mailbox_sync` 的响应契约。
- 增加测试用 IMAP 适配边界，测试不得连接生产邮箱。

完成标准：

- 当前行为有可重复的测试基线。
- 已知缺陷被测试明确记录，而不是在重构中被无意改变。

### M1：细粒度存储事务

**目的**：消除当前“读取完整快照、删除并重建整库”在多进程写入时产生的数据覆盖风险。

交付物：

- 为账户、邮件、联系人、草稿和 Token 提供按实体的查询与写入方法。
- 邮件写入使用 `(account_id, mailbox, uid)` 唯一键执行 UPSERT。
- 同步状态更新、邮件写入和游标推进在同一数据库事务中提交。
- 联系人 reconciliation 改为受影响联系人范围内的增量维护，或在事务后安全执行的独立任务。
- 保留兼容读取接口，逐步移除同步路径对 `readStore()` / `updateStore()` 完整快照的依赖。

完成标准：

- API 和同步操作并发写入时互不覆盖。
- SQLite WAL 模式下可以由两个进程分别读写。
- 现有 HTTP、MCP 和前端行为不变。

### M2：同步领域模型与持久化任务

**目的**：建立可恢复、可观测、可互斥的同步任务模型。

建议新增表：

- `sync_policies`
  - `account_id`
  - `enabled`
  - `interval_minutes`
  - `folder_mode`
  - `selected_mailboxes_json`
  - `sync_on_start`
  - `retry_on_recovery`
  - `notify_on_error`
  - `updated_at`
- `mailbox_sync_states`
  - `account_id`
  - `mailbox`
  - `uid_validity`
  - `last_seen_uid`
  - `highest_modseq`
  - `last_attempt_at`
  - `last_success_at`
  - `next_sync_at`
  - `consecutive_failures`
  - `last_error_code`
  - `last_error_message`
- `sync_jobs`
  - `id`
  - `account_id`
  - `mailbox`
  - `reason`：`scheduled | startup | manual | recovery`
  - `status`：`queued | running | succeeded | failed | cancelled`
  - `priority`
  - `not_before`
  - `locked_by`
  - `locked_until`
  - `attempts`
  - `created_at` / `started_at` / `finished_at`
  - 同步数量与安全裁剪后的错误信息

交付物：

- 策略默认值和账户级覆盖逻辑。
- 原子任务领取与租约续期。
- 过期租约回收、重复任务合并和幂等键。
- 连接状态与同步状态分离：
  - `connectionStatus`: `connected | unreachable | authRequired`
  - `syncState`: `idle | scheduled | running | backoff | paused`

完成标准：

- 同一账户和文件夹不会被两个执行器同时同步。
- Worker 被强制终止后，未完成任务可在租约过期后恢复。
- 重复点击“立即同步”最多产生一个有效待执行任务。

### M3：进程内调度器

**目的**：先验证完整同步策略，再承担独立进程带来的部署复杂度。

交付物：

- 独立于 WebSocket 的调度模块，由后端启动生命周期启动和停止。
- 启动时扫描到期账户，并在 15 秒内创建补同步任务。
- 每分钟扫描到期策略，使用随机抖动避免账户同时连接。
- 网络类错误采用 `1、5、15、30、60` 分钟退避并加入抖动。
- OAuth/授权码错误进入 `authRequired`，暂停自动重试，等待重新授权。
- 全局并发上限和每服务商并发上限均可配置。

完成标准：

- 未打开前端时，新邮件仍能在配置周期内进入本地缓存。
- 服务停止一晚后重启，可以补齐停机期间的新邮件。
- 网关没有订阅者时，同步策略照常执行。

### M4：HTTP、MCP 与前端设置

**目的**：让用户能够配置和理解同步行为，而不把任务执行绑定到前端。

HTTP API：

- `GET /api/sync-policy`
- `PATCH /api/sync-policy`
- `GET /api/accounts/:id/sync-policy`
- `PATCH /api/accounts/:id/sync-policy`
- `GET /api/sync-status`
- `POST /api/accounts/:id/sync` 改为创建任务，并返回任务 ID 和排队状态
- `GET /api/sync-jobs/:id`

MCP：

- 保持 `mailbox_sync` 名称兼容，将实现改为创建任务或等待有限时间返回结果。
- 新增 `sync_policy_get` 和 `sync_policy_update`；策略修改属于账户管理，只允许 `mcp:full`。
- 更新 `docs/mcp-integration.md`，确保参数、权限和异步语义与 HTTP 一致。

前端：

- 在 `src/features/accounts/` 增加默认同步策略和账户级覆盖设置。
- 使用 `AppInput`、`AppSelect`、`AppTextarea` 和 `AppCheckbox`，不新增原生业务表单控件。
- 展示上次尝试、上次成功、下次计划、退避时间、失败次数和当前任务。
- “立即同步”只创建任务；前端关闭后任务继续执行。
- 使用 SSE 接收 `sync.started`、`sync.completed`、`sync.failed` 和 `message.created`；SSE 断开时用状态轮询兜底。

完成标准：

- 修改策略后无需重启服务即可生效。
- 前端断开或刷新不会取消正在执行的任务。
- API 与 MCP 响应不暴露凭据和加密字段。

### M5：独立同步 Worker

**目的**：将 IMAP 连接、邮件解析和调度故障与 API 事件循环隔离。

交付物：

- 新增长期运行的 Worker 入口，例如 `server/sync/worker.ts`。
- API 进程停止直接执行 IMAP 同步，只负责读写策略和创建任务。
- Worker 使用唯一实例 ID 领取任务、续租并记录心跳。
- Worker 对单任务设置连接、下载、解析和总执行超时。
- 启动器或外部进程管理器负责 Worker 崩溃自动重启。
- 增加 Worker 健康状态与积压指标：最后心跳、排队任务数、最老任务等待时间。

部署方式：

- 桌面/单机版：统一启动器拉起 API 和 Worker，并分别监管。
- 服务端版：由 systemd、Docker Compose 或其他进程管理器分别运行 API 和 Worker。
- Worker 不依赖前端连接，也不应依赖 API 进程内存；两者仅通过持久化状态协作。

完成标准：

- 人为终止 Worker 不影响 API 读取本地邮件和账户设置。
- Worker 自动恢复后能够继续领取过期任务。
- API 重启期间 Worker 可以继续执行已持久化的同步策略。

### M6：IMAP 正确性与实时增强

**目的**：在稳定轮询基础上提高实时性和远端状态一致性。

交付物：

- 检测 `UIDVALIDITY` 变化并安全重建对应文件夹游标。
- 支持服务商提供的 `CONDSTORE/QRESYNC`，同步标记变化和删除；不支持时使用兼容扫描。
- 为支持的账户增加 IMAP IDLE，断线后自动回退到正常轮询。
- IDLE 只作为提前唤醒机制，不能替代持久化周期调度。
- 增加文件夹重命名、删除、退订和 Gmail All Mail 特殊语义测试。

完成标准：

- UID 重置、UID 空洞、重复投递和进程重启不会造成永久漏信或重复邮件。
- IDLE 断线不会使账户停止接收新邮件。
- 远端已读、星标和删除状态能在声明的同步范围内最终一致。

## 测试矩阵

每个里程碑至少覆盖：

- 首次同步、正常增量、无新邮件、UID 不连续。
- 两个账户同时到期、同一账户重复手动请求。
- IMAP 超时、DNS/网络失败、授权失效、限流。
- API、Worker 在任务不同阶段分别崩溃和重启。
- 前端始终关闭、前端中途断开、SSE 重连。
- 策略暂停、恢复、修改间隔和切换文件夹范围。
- 响应、日志和数据库任务记录不包含敏感凭据。

真实邮箱验收至少覆盖 Gmail、Outlook 和一个应用专用密码账户；自动化测试使用协议替身，不连接生产邮箱。

## 发布与回滚

1. 新表和新列只做向前兼容迁移，旧读取路径在迁移期间继续可用。
2. M3 首先通过功能开关启用调度器，观察重复任务、失败率和数据库锁等待。
3. M5 上线初期保留进程内执行器作为关闭状态的应急回退，但任一时刻只能启用一种任务执行器。
4. Worker 异常时可以暂停领取新任务；不得通过删除邮件缓存或同步游标进行回滚。
5. 每阶段发布前执行：

```bash
npm run typecheck
npm test
npm run build
```

## 总体验收标准

- 前端从未打开时，后端仍按策略持续同步。
- API、Worker 任一进程重启后不会丢失策略、任务和增量游标。
- 单账户故障不会阻塞其他账户，也不会高频重试。
- 手动同步、定时同步、启动补同步和网络恢复同步使用同一任务模型。
- 同步状态足以回答“是否开启、正在做什么、上次何时成功、下次何时执行、为何失败”。
- HTTP、MCP、前端和运维文档对同步能力的描述一致。

## 推荐开发顺序

严格按 `M0 → M1 → M2 → M3 → M4 → M5 → M6` 推进。M1 是拆分 Worker 的前置条件；M3 是解决“隔天不再同步”的最早可发布版本；M5 是完成故障隔离的目标版本；M6 不应阻塞基础轮询策略上线。
