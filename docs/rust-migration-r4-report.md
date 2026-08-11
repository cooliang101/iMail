# Rust 迁移 R4 自动化验收报告

日期：2026-08-10
分支：`codex/rust-service-migration`
结论：自动化实现与离线验收通过；专用真实邮箱往返待执行，R4 尚未最终关闭。

## 实现结果

R4 将外部邮件和 OAuth 能力拆成可直接嵌入 Tauri、也可由未来 HTTP Adapter 复用的端口：

- `imail-mail`：MIME 归一化、50 MiB 原始邮件上限、附件提取、协议安全错误、IMAP/SMTP 端口和远程操作服务。
- `imail-mail-network`：真实 IMAP/SMTP、TLS/STARTTLS、密码/XOAUTH2/Yahoo OAUTHBEARER、HTTP/HTTPS CONNECT、SOCKS5，以及连接/操作超时。
- `imail-oauth`：provider 配置、PKCE、加密 state、授权码交换/身份端口、Token 模型、刷新判定和账户级单航班协调。
- `imail-oauth-http`：Token/UserInfo HTTP、Microsoft RS256 JWKS 校验，以及仅授权期间存在的 loopback callback。
- `imail-core::mail_operations`：从现有用户作用域账户和缓存邮件解析安全连接配置，直接提供验证、发送、下载、标记和移动方法；不要求 HTTP。
- `imail-core::oauth_accounts`：OAuth 新增和重连、原邮箱校验、代理密码保留、连接状态持久化和安全公开视图。

默认运行实现仍是 Node，Rust 网络能力尚未连接当前同步调度器，也没有写入当前 `.data`。

## 兼容与安全证据

- `rust/fixtures/r4-mime-v1.eml` 与 `r4-mime-v1.json` 同时由 Node `mailparser` 和 Rust `mail-parser` 验证。
- Rust 保留 Node 的带尖括号 Message-ID、中文地址、正文 trim、HTML 可选值、UTC 毫秒日期、附件索引和 JavaScript UTF-16 预览边界。
- SMTP MIME 只接收内存附件；没有文件或 URL 读取入口。
- 协议错误会清除 URL 凭据、Bearer、access/refresh token、password 和 authorization 字段，折叠换行并限制为 500 个字符。
- OAuth state 使用现有 MasterKey AES-256-GCM 格式，包含 owner、provider、PKCE verifier 和 nonce；state 明文不会出现在 URL 或日志。
- Microsoft ID Token 强制 RS256、JWK `kid`、签名、audience、expiration、nonce 和 issuer 规则。
- loopback listener 拒绝非回环地址、HTTPS、错误 path 和错误 state；它不是常驻业务 HTTP API。
- TLS 同时加载平台系统根和公共 WebPKI 根，覆盖 Windows 企业信任与 Docker 公共 CA 场景。
- `NetworkMailAdapter` 现在在实例创建时构造并复用 TLS connector，不再为每个 IMAP/SMTP 连接重复加载系统与公共根。生产构造仍只信任相同的系统/WebPKI 根；测试构造才能注入隔离 CA。

## 自动化结果

| 门禁 | 结果 |
|---|---:|
| Rust workspace tests | 63 passed |
| Node/Vitest | 57 files, 318 passed |
| Rust strict Clippy | passed |
| Rust 1.77.2 workspace check | passed |
| TypeScript typecheck | passed |
| Vite production build | passed |
| R0 snapshot verification | all checks true |
| Rust/Node auth-store compatibility | all checks true |

数据保护结果：

- 当前数据库 SHA-256：`074b3d437adddebe3bd8020c3daa18b825de440eda34ad01838979574bd72000`
- R0 快照数据库 SHA-256：`dff6a56c17b3c4f15cf5745d52e8da871db528165425a28336240850c6de91d9`
- `snapshotUnchanged=true`
- `activeDataUnchanged=true`
- 所有写入、恢复、损坏数据和网络协议测试均使用 fixture、内存替身或临时目录。
- 新增 loopback 真实 TCP/TLS 协议 fixture：测试进程生成仅该用例信任的临时 CA，IMAP 端实际完成 LOGIN、CAPABILITY、LIST、STATUS、EXAMINE、FETCH literal 和 LOGOUT，随后从收到的 RFC822 执行 MIME 解析；SMTP 端实际完成 EHLO、AUTH、MAIL/RCPT、DATA 和 QUIT，并重新解析收到的出站邮件。该证据覆盖真实 socket、TLS、协议库和字节流，但不等同于公共服务商兼容性验收，也不访问当前 `.data`。

## 待执行真实邮箱验收

最终关闭 R4 前，需要专用验收账户执行：

1. IMAP 与 SMTP 连接验证。
2. 发送带唯一主题、文本、HTML 和小附件的邮件。
3. 从专用测试文件夹读取该邮件并下载附件，逐字节比较。
4. 往返切换已读和星标。
5. 移动到专用测试文件夹或归档并重新确认位置。
6. OAuth 授权、过期刷新、重连原邮箱和错误 state/nonce 验证。
7. 检查日志、错误、数据库公开字段和事件中不存在 Token 或密码。

验收不得删除邮件、清空邮箱、删除同步游标或让 Node/Rust 同时对同一数据目录运行同步。测试生成的邮件可以保留在测试文件夹，移动成功而不是删除作为清理条件。

自动验收驱动已经实现为 `npm run rust:mail-acceptance`。它要求专用账户和远程写入双显式 guard，使用唯一主题自投递并验证 MIME、附件、flags、真实取消、重连和归档；不删除邮件，脱敏报告不包含邮箱地址、凭据、UID、Message-ID、正文或文件夹名称。配置协议见 `docs/rust-mail-acceptance.md`，当前尚未注入专用账户凭据运行。

## 已知差异与后续约束

- IMAP MOVE 成功后，当前库不提供 UIDPLUS 映射，`RemoteMessageMoveResult.uid` 可能为空；R5 必须依赖目标文件夹增量确认，不能猜测 UID。
- SMTP 当前按整封投递成功返回全部 recipients；部分接受场景需要真实服务器验收，并在必要时扩展底层响应采集，但不改变公开 API 结构。
- `NetworkMailAdapter` 拥有小型 Tokio runtime，并通过同步端口供当前领域层调用。Tauri 接入必须在 blocking task 中运行，避免阻塞 WebView/UI 线程；R5 的长连接同步端口将保持异步任务模型。
- R4 不启动持久化 IDLE/QRESYNC，同步游标、租约和恢复属于 R5。
