# Rust 服务迁移 R5 进展报告

日期：2026-08-10
状态：持久化控制面、增量执行链、worker/watcher、策略校准与网络取消离线实现完成；真实邮箱验收未完成

## 本批范围

本批把 Node `server/sync/store.ts` 的核心持久化状态机迁移到 Rust，但不切换正式运行路径。Rust 和 Node 继续共享 schema v6 的表结构；当前 `.data` 仍只由 Node 正式运行路径写入。

实现位于 `rust/crates/imail-storage-sqlite/src/sync_runtime.rs`，桥接只读模型位于 `imail-protocol`。没有引入 HTTP、Tauri command 或新的数据库迁移。

## 已实现语义

- 用户级默认同步策略、账户策略初始化、策略停用/恢复和授权恢复。
- 同一账户/邮箱/角色的 queued/running 去重；queued 合并最高优先级与最早执行时间，running 设置 `rerun_requested`。
- 按优先级降序、创建时间升序领取任务；过期 running 租约重新排队并增加 attempts。
- worker 所有权约束下的续租、开始、完成、失败和取消；旧 worker 不能提交被其他 worker 回收的任务。
- 成功时原子写入计数、同步游标、下一次低频校准和完成事件；运行期间收到唤醒后追加 recovery 任务。
- 失败时区分授权暂停与临时不可达，持久化连续失败和下一次重试时间；重新授权后清理错误并恢复调度。
- worker 心跳、60 秒陈旧过滤、队列深度/最老排队时间和最多 500 条的增量事件读取。
- Rust 增量同步计划：初始最近 80 封、UID 增量、分批删除检测、CONDSTORE `CHANGEDSINCE`、UIDVALIDITY 重建、Gmail All Mail 标签过滤和 Node 兼容的 24 位消息缓存 ID。
- SQLite 邮箱差异事务：删除/重建、upsert、标记刷新、Message-ID 去重、本地标签与稍后处理保留、账户文件夹/同步状态更新、5000 封上限和裁剪正文后的变更摘要。
- 联系人重建已经并入邮箱差异事务，使用固定版本的内置 Mozilla PSL 处理 `co.uk` 和 `appspot.com` 等边界并保留主域 Logo；联系人触发器故障测试证明邮件与账户更新会一起回滚。
- 无 HTTP `MailboxSyncApplicationService`：在同一内部调用中完成用户归属检查、凭据解密、协议端口调用、差异规划和缓存提交；首次同步不产生新邮件通知。
- `RefreshingConnectionService` 在连接前按 90 秒窗口刷新 OAuth，使用账户级 singleflight 合并并发刷新，将旋转结果重新加密写回账户，并保留旧 refresh token 与代理密码；刷新后的配置可直接传给同步应用服务。
- IDLE/STATUS wake primitive：支持最长 60 秒 IDLE 刷新和无 IDLE 服务商的 STATUS fallback，只输出持久化任务唤醒原因。
- 独立 `imail-runtime` supervisor：取消令牌、周期任务、panic 隔离、重复任务拒绝、有界 shutdown 和不合作任务报告，为嵌入式 Tauri 与可选 HTTP 模式提供同一个进程生命周期。
- 持久同步 worker pool 已挂到 supervisor：每个 slot 使用独立 SQLite 连接领取任务、写入 worker 心跳、标记开始并持久化成功/失败；执行期间由独立租约线程自动续租，关闭时向执行器传播取消并把协作式退出记录为 cancelled，而不是伪造网络失败。
- worker 数量限制为 1–16，租约、轮询、低频校准和主机标识均由启动配置提供；生产级邮件执行器通过进程内 trait 注入，不引入 HTTP，也没有切换 Node 正式路径。
- `EmbeddedSyncExecutor` 已完成真实进程内装配：从数据目录加载主密钥和 owner，恢复既有 mailbox/role 游标，调用 OAuth 刷新、真实 IMAP 增量 Adapter、`MailboxSyncApplicationService` 与 SQLite 原子提交；排队后账户被删除会归类为不可重试的 `ACCOUNT_NOT_FOUND`。
- `EmbeddedSyncExecutor` 的同步 IMAP 创建已从内部硬编码改为 `SyncMailTransportFactory` 依赖，默认 factory 仍创建带取消探针的真实 `NetworkMailAdapter`。HTTP 宿主可把同一 factory 显式传给 worker；该边界让未来 Tauri 嵌入宿主、HTTP 宿主和隔离验收复用完全相同的执行器，而无需复制同步逻辑。
- `EmbeddedSyncExecutor` 的 OAuth provider 创建与配置解析也已改为宿主注入，默认仍使用标准服务商端点和真实 HTTP adapter；HTTP、worker、watcher 与未来 Tauri 嵌入宿主因此共享同一刷新和加密持久化路径。
- 账户 watcher manager 已挂到同一 supervisor：按启用的 `sync_policies` 动态增删每账户 watcher，停用策略或删除账户会取消并回收对应 watcher；真实装配复用 OAuth 刷新与 IMAP IDLE/STATUS Adapter。Changed 信号写成现有契约的高优先级 `recovery`，Reconcile 写成 `scheduled`，长连接本身不直接修改缓存。
- 无前端策略校准器已挂到 supervisor：启动扫描使用 `startup`，随后周期扫描使用 `scheduled`；展开 inbox/standard/selected，跳过 authRequired、paused 和未到 `next_sync_at` 的状态，失败到期改投 `recovery`，并按 Node 规则清理七天前同步事件。
- 真实网络 Adapter 支持注入取消探针；worker shutdown、租约丢失和 watcher 停用可中断 IMAP/SMTP future，不再等待完整 30/120 秒超时。QRESYNC 服务商会显式选择 RFC 7162 的 CONDSTORE/CHANGEDSINCE 安全路径；当前 async-imap 版本没有公开 QRESYNC SELECT/VANISHED API，因此仍保留分批 UID 删除核对。
- runtime 暴露无 HTTP 的进程内 health snapshot：累计 worker started/succeeded/failed/cancelled、scheduler scans/errors、active watchers 以及 watcher disconnect/reconnect。执行完成后的 SQLite 最终状态写入继续保持租约并重试，避免瞬时忙导致 running 空壳任务。
- 已完成当前数据的一致性 R5 snapshot 与独立写入副本预检：Rust/Node 字段及凭据摘要一致；4 个历史 due 目标与 rerun recovery 共执行 5 次且全部成功，最终 queued 为 0；16 个既有 UIDVALIDITY/UID/MODSEQ 与账户、1363 封邮件、联系人、草稿和 Token 均保持。详见 `docs/rust-migration-r5-data-copy-preflight.md`。
- 真实邮箱验收驱动已准备：双显式 guard 限制专用账户，使用唯一主题自投递并验证 MIME/附件/flags/真实取消/重连/归档，报告不含凭据或邮箱内容且从不删除邮件。协议见 `docs/rust-mail-acceptance.md`；尚未注入专用账户凭据执行。
- 已加入 release 资源门禁 `npm run rust:runtime-soak`，只接受带清单且无 queued job 的数据副本，拒绝 `.data` 和报告覆盖。2 个空闲 worker 的 60 秒采样从 9,633,792 bytes RSS 到 9,621,504 bytes，峰值 9,670,656 bytes，零任务、零遗留 worker/queue 且优雅关闭；边界与证据见 `docs/rust-migration-r5-resource-report.md`。

## 自动化证据

- Rust workspace：93 项通过。
- 严格 Clippy：`--workspace --all-targets --all-features -D warnings` 通过。
- MSRV：Rust 1.77.2 workspace 全目标/全 feature 检查通过。
- Node：57 个测试文件、318 项通过。
- TypeScript typecheck 和 Vite production build 通过。
- 当前数据库 SHA-256：`074b3d437adddebe3bd8020c3daa18b825de440eda34ad01838979574bd72000`。
- R0 快照数据库 SHA-256：`dff6a56c17b3c4f15cf5745d52e8da871db528165425a28336240850c6de91d9`。

测试数据库由每项 Rust 测试在系统临时目录创建并在结束时移除；本批代码没有打开或写入上述两个保留数据库。

## 未完成与门禁

R5 仍未完成，以下能力不得以当前报告替代验收：

- 持久 worker slots、生产装配器、动态 watcher、策略校准、网络取消、运行观测、QRESYNC 安全降级、现有数据副本游标续跑与 60 秒离线空闲资源门禁均已通过；仍缺专用真实邮箱上的断网重连/取消实测、长时间资源观测与 Node/Rust 最终网络增量快照对照。
- 数小时以上的长期空闲、真实网络抖动和连接/内存泄漏测试。
- 专用邮箱上的 Node/Rust 最终网络增量快照对照。

在这些项目完成前，Node 仍是正式数据的唯一同步调度器和 worker；不得让 Node 与 Rust 同时连接同一验收账户或写入同一原始数据目录。
