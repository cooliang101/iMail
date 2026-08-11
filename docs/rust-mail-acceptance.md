# Rust 专用邮箱验收协议

该驱动用于 R4/R5 真实 IMAP/SMTP 验收。它会产生远程副作用，只允许使用专用测试邮箱；不得配置生产邮箱或当前用户的日常邮箱。

## 安全约束

- 默认拒绝运行，必须同时设置 `IMAIL_ACCEPTANCE_ALLOW_REMOTE_WRITE=true` 与 `IMAIL_ACCEPTANCE_DEDICATED_ACCOUNT=true`。
- 密码、预置 access token 与交互式 OAuth 只能选择一种。交互式 OAuth 还必须单独设置 `IMAIL_ACCEPTANCE_ALLOW_INTERACTIVE_OAUTH=true`。
- OAuth callback 返回的身份必须与 `IMAIL_ACCEPTANCE_EMAIL` 大小写不敏感地完全一致；不一致时在任何邮件写入前终止，避免误用个人账户。
- 报告不包含邮箱地址、授权 URL/state、密码、Token、邮件 Message-ID、UID、文件夹名称或正文。
- 单账户入口仍可用于交互式 OAuth 凭据准备；当前数据迁移验收使用四账户闭环入口，每个账户只向闭集内另一个账户发送，不允许 CC/BCC，也不允许任何闭集外收件人。
- 成功接收后把验收邮件移动到服务商的 Archive/All Mail；不调用删除、不清空邮箱。投递确认失败时停止后续边，不自动重复发送。
- 报告路径已存在时拒绝覆盖。
- 运行失败时保留已发送邮件，便于定位；不得以清理邮件作为重试前置条件。

## 环境变量

必填：

- `IMAIL_ACCEPTANCE_ALLOW_REMOTE_WRITE=true`
- `IMAIL_ACCEPTANCE_DEDICATED_ACCOUNT=true`
- `IMAIL_ACCEPTANCE_EMAIL`
- `IMAIL_ACCEPTANCE_IMAP_HOST`
- `IMAIL_ACCEPTANCE_SMTP_HOST`
- 以下认证方式三选一：
  - `IMAIL_ACCEPTANCE_PASSWORD`
  - `IMAIL_ACCEPTANCE_ACCESS_TOKEN`
  - `IMAIL_ACCEPTANCE_OAUTH_INTERACTIVE=true`，并按下节配置交互式 OAuth

可选：

- `IMAIL_ACCEPTANCE_PROVIDER`，OAuth 时用于记录非敏感 provider 名称
- `IMAIL_ACCEPTANCE_DISPLAY_NAME`
- `IMAIL_ACCEPTANCE_IMAP_PORT`，默认 `993`
- `IMAIL_ACCEPTANCE_IMAP_SECURE`，默认 `true`
- `IMAIL_ACCEPTANCE_SMTP_PORT`，默认 `465`
- `IMAIL_ACCEPTANCE_SMTP_SECURE`，默认 `true`；587 STARTTLS 通常设置为 `false`
- `IMAIL_ACCEPTANCE_REPORT`，建议指向新的 `output/rust-migration-tests/*.json`

### 交互式 OAuth

启用交互式 OAuth 时必填：

- `IMAIL_ACCEPTANCE_ALLOW_INTERACTIVE_OAUTH=true`
- `IMAIL_ACCEPTANCE_OAUTH_INTERACTIVE=true`
- `IMAIL_ACCEPTANCE_OAUTH_PROVIDER`：`google`、`microsoft` 或 `yahoo`
- `IMAIL_ACCEPTANCE_OAUTH_CLIENT_ID`

Google 与 Yahoo 还必须设置 `IMAIL_ACCEPTANCE_OAUTH_CLIENT_SECRET`。Microsoft Desktop App 可以不设置 secret。Yahoo Mail 权限已经审核时还必须设置 `IMAIL_ACCEPTANCE_YAHOO_MAIL_OAUTH_APPROVED=true`。

可选：

- `IMAIL_ACCEPTANCE_ACCOUNT_PROVIDER`：默认按 OAuth provider 使用 `gmail`、`outlook` 或 `yahoo`；Microsoft 个人账户可明确设为 `hotmail`。
- `IMAIL_ACCEPTANCE_OAUTH_REDIRECT_URI`：默认 `http://127.0.0.1:0/oauth/callback`，运行时绑定随机回环端口。OAuth Desktop App 必须允许该类 loopback redirect。
- `IMAIL_ACCEPTANCE_OAUTH_TIMEOUT_SECONDS`：默认 300，只接受 30–900 秒。

驱动会直接打开系统浏览器，授权 URL 不写入 stdout/stderr；操作者在浏览器中完成专用账户授权。加密 state 使用仅存在于本次进程内的随机临时主密钥；callback、authorization code、access token 与 refresh token 均不写入报告或数据目录。完成身份校验后，驱动要求服务商返回 refresh token，并立即执行一次真实刷新交换；刷新成功才进入 IMAP/SMTP 验收。

凭据应通过当前 PowerShell 进程的环境变量注入，不写入命令行、仓库、报告或文档。

## 执行

```powershell
npm run rust:mail-acceptance
```

驱动按顺序验证：

1. 交互式模式执行 PKCE 授权、loopback callback、身份一致性检查和 refresh-token 交换；
2. IMAP 与 SMTP 登录；
3. Archive/All Mail 文件夹发现；
4. 唯一主题 HTML/文本邮件自投递；
5. 最近窗口增量收取与 MIME 解析；
6. 附件名称、类型和字节往返；
7. 已读与星标远程更新及 CHANGEDSINCE/普通 FLAGS 观察；
8. 在真实 IDLE/STATUS 等待中注入取消，要求返回 `CANCELLED`；
9. 取消后重新连接；
10. 将测试邮件移动到 Archive/All Mail，不删除；
11. 输出脱敏 JSON 报告。

交互式 OAuth 报告中的 `oauthAuthorized` 与 `oauthRefreshVerified` 必须为 `true`，所有模式中的 `deleted` 必须为 `false`。该驱动不打开 `.data`，也不启动 Node 或 Rust 后台同步，因此不会让两个实现同时保持同一账户的 IDLE。

## 本地真实 TLS 运行时长稳

公共邮箱验收之外，可先用隔离 loopback TLS fixture 持续运行正式 Rust worker、scheduler 与 IDLE watcher：

```powershell
npm run rust:tls-soak -- 3600 32 output/rust-migration-tests/r6-real-tls-soak-1h-v1.json
```

三个位置参数依次为持续秒数、允许的首尾 RSS 增长 MiB 和新报告路径。持续时间只接受 30–86400 秒；报告必须位于 `output/rust-migration-tests`、父目录已存在且目标不存在。入口不读取 `.data`，也不访问公共邮箱；它验证真实本地 TLS socket、首次 IDLE 断线恢复、`EXISTS`→recovery→worker FETCH、周期 scheduler 校准、队列/heartbeat 收敛、资源采样和优雅停机。loopback 长稳只能作为公共邮箱门禁前的协议栈回归，不能替代服务商限流、NAT/代理超时或 OAuth 策略。

## 现有四账户闭环

迁移阶段使用当前四账户完成的闭环报告继续保留在 `output/rust-migration-tests`，但依赖旧 Node 凭据读取器的矩阵包装脚本已随 Node 服务源码删除。后续如需再次发送，只能通过 `npm run rust:mail-acceptance` 逐边调用纯 Rust 验收二进制，并显式提供 `IMAIL_ACCEPTANCE_ALLOWED_RECIPIENTS_JSON`。Rust 入口强制该数组恰好包含四个唯一邮箱，发送方和收件方都必须属于闭集且不能相同，否则在连接和发送前失败；不得使用其他收件人。

使用当前桌面账户配置准备验收环境时，必须先显式退出 Tauri 桌面进程并确认嵌入式 worker/IDLE 已停止，同时确认没有 8787 listener。验收结束或失败后再启动桌面应用；禁止两个 Rust host 同时对四个账户保持同步连接。
