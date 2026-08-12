# 运维手册

## 服务环境变量

| 变量 | 默认值 | 用途 |
| --- | --- | --- |
| `MCP_ALLOWED_HOSTS` | `localhost,127.0.0.1,::1` | HTTP MCP 允许的 Host 与 Origin 主机名，逗号分隔 |
| `HOST` | `127.0.0.1` | iMail API 监听地址；远程监听会扩大所有 API 的暴露面 |
| `PORT` | `8787` | API 与 `/mcp` 端口 |
| `IMAIL_WEB_DIST` | 未设置 | 远程服务托管的 Web 构建目录；桌面本地服务不设置 |
| `IMAIL_TRUST_PROXY` | `false` | HTTPS 反向代理位于服务前一跳时设为 `true` |
| `IMAIL_ALLOWED_HOSTS` | `localhost,127.0.0.1,::1` | 远程生产 API 与 Web 允许的请求主机名，不含端口 |
| `CORS_ORIGIN` | 开发模式内置回环前端，生产未设置 | 仅在 Web 与 API 不同源时列出完整 Origin，逗号分隔；非回环来源必须 HTTPS |
| `IMAIL_REGISTRATION_MODE` | 开发为 `open`，生产为 `initial-only` | 生产初始化后是否继续允许创建应用用户 |
| `IMAIL_SYNC_WORKER` | `true` | `http-service` 的 Rust `imail-server` 是否在同一进程装配 worker、scheduler 与 IDLE watcher；`false` 仅用于诊断或契约隔离 |
| `IMAIL_SYNC_CONCURRENCY` | `3` | Worker 最大并发同步任务数，范围 1–10 |
| `IMAIL_SYNC_WORKER_POLL_MS` | `1000` | Worker 领取任务间隔，最小 250ms |
| `IMAIL_SYNC_SCHEDULER_INTERVAL_MS` | `5000` | 到期校准任务扫描间隔，最小 5 秒 |
| `IMAIL_SYNC_STARTUP_DELAY_MS` | `1000` | 服务启动后的首次补同步延迟 |
| `IMAIL_SYNC_JOB_LEASE_MS` | `120000` | 任务租约时间，最小 10 秒；执行中会自动续租 |
| `IMAIL_SYNC_RECONCILE_MINUTES` | `30` | 后台一致性校准间隔，范围 5–1440 分钟；这是运维可靠性参数，不是用户同步频率 |
| `IMAIL_SYNC_IDLE_ENABLED` | `true` | 是否启用收件箱 IMAP IDLE 变化唤醒；关闭后仍由后台一致性校准保证最终一致 |
| `IMAIL_SYNC_IDLE_RECONCILE_MS` | `5000` | IDLE 连接期望状态检查与断线重建间隔，最小 5 秒 |
| `IMAIL_SYNC_IDLE_REFRESH_MS` | `60000` | IDLE 保活刷新周期；不支持 IDLE 的服务商以此间隔执行 STATUS 兜底，最小 15 秒 |

Rust 宿主读取 `IMAIL_SYNC_WORKER` 并复用其余 `IMAIL_SYNC_*` 调优项。对外报告 `syncWorker=true` 之前必须已成功启动运行时，启动失败会让整个服务失败，而不是只启动 HTTP 空壳。

Rust HTTP 宿主继续接受现有远程部署变量 `HOST`、`PORT`、`CORS_ORIGIN`、`IMAIL_TRUST_PROXY` 和 `IMAIL_REGISTRATION_MODE`。`--host`/`--port` 命令行值优先；`IMAIL_CORS_ORIGINS`、`IMAIL_TRUST_PROXY_ONE_HOP` 与 `IMAIL_REGISTRATION_OPEN` 是迁移期显式覆盖别名，不要求现有部署改名。

Windows 桌面本地模式平时只使用进程内领域调用；进入“外部接入”后，同一 Rust 宿主会按需增加一个仅监听 `127.0.0.1` 随机端口的 HTTP Adapter，供本机 MCP 与 Gateway 客户端使用。它不是旧版固定 8787 守护进程，并随桌面进程退出。`--daemon-control-file` 仅为迁移期兼容参数。Docker/远程部署不要配置该参数，应由容器 SIGTERM 或进程管理器停止。

## 本地启动

Streamable HTTP：

```bash
npm --prefix frontend run dev
```

`npm --prefix frontend run dev` 同时启动 Vite 与 `http-service/` 中的 Rust `imail-server`。独立服务入口默认启用 HTTP，并在同一进程装配 worker、scheduler 与 IDLE watcher。

## 远程生产部署

`npm --prefix frontend run build:remote` 生成 `frontend/dist`、`http-service/` 独立启动器和 Rust 维护工具；`npm --prefix frontend run start:remote` 监听 `0.0.0.0:8787`。正式容器把 `frontend/dist` 复制为镜像内 `/app/dist`，runtime 不包含 Node，以非 root 用户运行并把所有可变数据写入 `/data`。`http-service/compose.example.yml` 只把端口绑定到宿主机回环地址。

仓库同时提供带 Caddy 自动 HTTPS 的 `http-service/compose.https.example.yml`。服务镜像由受控 GitHub Actions 发布到 `ghcr.io/cooliang101/imail`；复制环境变量模板，填写已解析到部署主机的域名，并将 `IMAIL_IMAGE` 固定到所需版本标签或 digest 后启动。若 GHCR 包保持私有，先使用具有 `read:packages` 权限的 Token 执行 `docker login ghcr.io`：

```bash
cp http-service/deploy/remote.env.example .env.remote
# 编辑 .env.remote，至少设置 IMAIL_PUBLIC_HOST，并在固定部署中替换 edge
docker compose --env-file .env.remote -f http-service/compose.https.example.yml up -d --pull always
```

该拓扑只向公网发布 Caddy 的 80/443（含 HTTP/3 UDP）端口，iMail 的 8787 只存在于 Compose 网络。Caddy 自动申请和续期证书，配置禁用上游响应缓冲以保证 SSE 实时送达；WebSocket 由 `reverse_proxy` 原生转发。部署前确认 DNS 已生效且防火墙允许 TCP 80/443 与 UDP 443。若已有反向代理，继续使用回环绑定的 `http-service/compose.example.yml`，并自行配置 SSE 禁用缓冲、WebSocket 升级和足够长的读取超时。

公网入口必须使用 HTTPS。仅当反向代理是服务的直接前一跳时设置 `IMAIL_TRUST_PROXY=true`，并把公开地址同步写入 `FRONTEND_URL` 和 `OAUTH_CALLBACK_BASE_URL`。`MCP_ALLOWED_HOSTS` 只列出实际主机名。生产环境默认只允许首个用户完成初始化；除非实例明确供多个互不信任用户共同使用，否则不要把 `IMAIL_REGISTRATION_MODE` 改成 `open`。

同源部署不需要设置 `CORS_ORIGIN`。生产运行时不会自动加入 `localhost:5173`；拆分 Web/API 域名时只填写完整 Origin，例如 `https://web.example.com`，不带路径、查询、片段或凭据。非回环 HTTP 来源会导致服务在启动时拒绝配置。

登录与注册限流保存在 SQLite 的 `auth_rate_limits` 表，重启不会清零；注册、登录、授权码与敏感管理动作写入 `security_audit_events`。审计来源只保存使用实例随机盐生成的 HMAC，不保存密码、会话、OAuth Token 或邮箱凭据。公网反向代理仍应提供独立的连接级限流和访问日志。

## 备份与恢复

在线备份使用 SQLite backup API 取得一致数据库快照，并同时复制自动生成的主密钥、持久实例身份与发件人 Logo。备份先写入同目录暂存项，全部成功后再原子提交，并生成包含 iMail 版本、数据库 schema 版本和逐文件 SHA-256 的 v2 完整性清单；恢复仍兼容已有 v1 清单。当前 schema v5 在 `accounts.proxy_json` 中保存每邮箱的非密码代理字段，代理密码仍位于加密凭据载荷，二者都会随数据库快照一起备份：

```bash
npm --prefix frontend run backup -- /safe/backups/imail-2026-08-03
```

Compose 将独立的 `imail-backups` 卷挂载到 `/backups`，容器使用 Rust 维护 CLI 在线备份：

```bash
docker compose --env-file .env.remote -f http-service/compose.https.example.yml exec imail \
  /app/imail-maintenance backup /backups/imail-2026-08-03
docker compose --env-file .env.remote -f http-service/compose.https.example.yml cp \
  imail:/backups/imail-2026-08-03 ./imail-2026-08-03
```

第二条命令把快照导出到 Docker 主机；只留在同一主机的命名卷不构成异地备份。

先在服务外准备一个全新的恢复目录。命令会验证 SHA-256 清单、执行 SQLite `quick_check`、校验 iMail 核心表与本地 `master.key` 格式，并拒绝覆盖已有目录；它不会改写正在使用的数据卷：

```bash
npm --prefix frontend run restore:prepare -- /safe/backups/imail-2026-08-03 /safe/restore/imail-2026-08-03
```

容器内使用 `/app/imail-maintenance restore <备份目录> <全新恢复目录>` 执行相同校验。恢复目标必须是新目录或新数据卷，工具不会覆盖 `/data`。恢复工具和服务启动迁移都会拒绝高于当前发布版本支持上限的未来 schema。

正式 Rust 镜像不包含 Node，并内置 `/app/imail-maintenance`。该工具默认读取 `IMAIL_DATA_DIR`（镜像中为 `/data`），所有目标必须是不存在的新目录，绝不覆盖数据卷：

```bash
docker exec imail /app/imail-maintenance backup /backups/imail-2026-08-10
docker exec imail /app/imail-maintenance restore /backups/imail-2026-08-10 /backups/imail-restore-2026-08-10
docker exec imail /app/imail-maintenance upgrade-preflight /backups/imail-before-upgrade /backups/imail-upgrade-preflight
```

三条命令均输出机器可读 JSON。`upgrade-preflight` 先在线备份，再只在新恢复副本上执行当前 Rust schema 迁移、`quick_check` 和外键检查；失败不会修改 `/data`，已成功生成的备份继续保留用于诊断和恢复。

`master.key` 位于持久化 `/data` 中，并包含在维护工具生成的完整备份内。没有原主密钥，即使数据库恢复成功也无法解密邮箱凭据。

## 远程服务升级预检

替换远程服务前，使用将要发布的新版本执行预检。命令会在线创建一致性回滚备份，恢复到全新目录，只在副本上执行当前版本迁移，再运行 SQLite `quick_check` 和外键检查。在线数据目录不会被改写：

```bash
npm --prefix frontend run upgrade:preflight -- \
  /safe/backups/imail-before-upgrade \
  /safe/preflight/imail-new-version
```

Compose 部署应先拉取新镜像，但保持旧容器运行；然后用新镜像的一次性容器执行预检：

```bash
docker compose --env-file .env.remote -f http-service/compose.https.example.yml pull imail
docker compose --env-file .env.remote -f http-service/compose.https.example.yml run --rm --no-deps imail \
  /app/imail-maintenance upgrade-preflight \
  /backups/imail-before-upgrade /backups/imail-new-version-preflight
```

只有输出同时包含 `activeDataUntouched: true`、`sqliteQuickCheck: true` 和 `foreignKeysVerified: true` 时才执行 `docker compose up -d imail`。备份目录和预检目录必须是不存在的两个独立路径。预检失败时会删除半迁移副本，但保留已验证的回滚备份。不要把预检目录与在线 `/data` 合并或直接覆盖。

## 恢复切换与回滚

恢复前停止 API 和 Worker，将当前数据卷完整另存，再把经过 `restore:prepare` 验证的新目录作为完整数据目录切换进去。不要直接向运行中的数据卷复制文件，也不要混用不同时间点的数据库与主密钥。启动后先检查 `/api/system/info`、登录、账户列表和 Worker 心跳，再开放反向代理流量。升级前先执行备份；迁移失败时停止新版本，切换到准备好的旧数据快照并回退到原镜像或安装包。

## 冒烟检查

当前交付范围只有 Windows 桌面端与服务端 Docker；Linux 侧只运行 Docker。普通分支推送与 pull request 不触发工作流；未经用户明确授权，不创建版本 tag 或手动运行。手动工作流必须选择 `docker` 或 `windows`：前者只验证并发布 Rust `linux/amd64` 镜像，后者只生成 Rust-only Windows Artifact。发布前验证 Rust 持久卷重启、healthcheck、备份、非覆盖恢复、升级预检和优雅停机。

1. 在“外部接入”的“MCP”标签页签发 `mcp:full` 授权码。
2. 用 MCP Inspector 或任意标准客户端连接页面显示的桌面回环地址，或远程 Rust 服务的 `https://mail.example.com/mcp`。
3. 确认 `tools/list` 包含 `accounts_list`、`messages_list`、`message_send`、`account_remove`、`theme_custom_get` 和 `theme_custom_update`。
4. 调用 `imail_status` 与 `accounts_list`，确认响应不含 `encryptedSecret`、密码或 OAuth Token。
5. 使用普通 `messages:read` Token 连接，预期得到 HTTP 401。
6. 在 UI 撤销授权码，再次请求，预期得到 HTTP 401。
7. 请求 `GET /api/sync-status`，确认 `worker.workers` 至少有一个十秒内更新的心跳。
8. 向测试邮箱发送一封新邮件，在不点击“立即同步”的情况下确认数秒内出现；服务日志不应持续出现 `[sync-idle]` 重连错误。
9. 关闭浏览器，等待一个同步周期后再次查询，确认 `lastSuccessAt` 和 `nextSyncAt` 继续推进。
10. 调用 `theme_custom_update` 写入测试主题，再用 `theme_custom_get` 读取并确认相等；`GET /api/preferences` 不应出现 `customTheme`。

仓库级自动验证：

```bash
npm --prefix frontend run typecheck
npm --prefix frontend test
npm --prefix frontend run build
npm --prefix frontend audit --omit=dev
```

部署模式内部测试验证：

```bash
npm --prefix frontend run test:internal-release
# 正式 Rust worker/scheduler/IDLE 的隔离真实 TLS 长稳；参数为秒、增长 MiB、新报告
npm --prefix frontend run rust:tls-soak -- 3600 32 output/rust-migration-tests/r6-real-tls-soak-1h-v1.json
# 仅限明确允许改写当前用户安装状态的 Windows 测试机
IMAIL_ALLOW_INSTALLER_SMOKE=true npm --prefix frontend run test:windows-installer
```

资源报告使用不可覆盖写入；目标已存在时必须换用新的版本化文件名，不能删除或覆盖旧证据。持续资源门禁只测当前 Rust 实现，并继续使用系统临时目录或数据副本，不得挂载或修改活动 `.data`。

真实 TLS 长稳同样拒绝覆盖报告，并且只使用系统临时数据目录。日常回归可运行 60 秒；进入切换评审前应至少运行一份数小时报告。该本地 fixture 不含真实服务商、OAuth 或公网设备行为，不能替代专用公共邮箱验收。

Windows 与 Docker 的人工验收要求见[内部测试](internal-testing.md)，邮件网络副作用门禁见[邮件验收协议](rust-mail-acceptance.md)。

不要为了生成 Windows 内部测试包创建标签或触发 GitHub Actions。Windows 交付直接使用本机 `build:desktop:internal` 产物并记录版本、构建提交、平台/架构与 SHA-256；三段式版本标签只负责把已确认版本的服务端镜像发布到 GHCR。

`crates/imail-http/src/mcp.rs` 的测试覆盖授权拒绝、MCP 初始化、工具清单、工具调用和凭据不泄漏；`scripts/mcp-rust-sdk-interop.test.ts` 使用官方 TypeScript 客户端与 Rust 服务进行互操作验证。

## 故障排查

### 登录限流与安全审计

登录和注册限流保存在主 SQLite 数据库中，重启容器或服务不会绕过窗口。不要通过删除 `auth_rate_limits` 处理普通登录失败；先检查客户端地址、可信代理配置和账户名。确需应急解除时，应先停止服务、完成备份并记录变更原因。

登录后的用户可以查询自己的最近安全事件：

```bash
curl -fsS --cookie 'imail_session=<当前会话>' \
  'https://mail.example.com/api/security/audit-events?limit=100'
```

事件来源是使用实例内随机盐生成的 HMAC，便于关联同一来源但不能还原原始 IP。详情只包含账户 ID、授权码 ID、权限名、MCP 工具名等非敏感元数据；密码、邮箱授权码、OAuth Token 和 MCP Token 不得写入。事件保留 90 天，全实例超过 10,000 条时优先删除最旧记录。数据库备份会包含这些事件。

### 邮件没有自动同步

- 请求 `GET /api/sync-status`；先检查账户策略的 `enabled`、邮箱状态的 `nextSyncAt`，以及 `worker.workers[].heartbeatAt`。
- `queuedJobs` 持续增加但没有新心跳，说明 Rust 同步运行时未正常工作；检查应用日志或远程服务日志中的 worker/scheduler 启动与 panic 记录。
- `connectionStatus=authRequired` 时自动重试会暂停，应在邮箱设置中重新授权或更新凭据；验证成功后调度器会恢复该账户。
- `syncState=backoff` 表示网络或服务商错误，按 1、5、15、30、60 分钟退避。不要通过频繁点击手动同步绕过服务商限流。
- 持续出现 `[sync-idle]` 表示长连接无法稳定建立或被服务商/网络设备关闭；Worker 会按 0.5–30 秒退避重连，同时保留一分钟周期同步兜底。不支持 IDLE 的服务器会按 `IMAIL_SYNC_IDLE_REFRESH_MS` 执行 `STATUS`。
- 前端 SSE 仅用于刷新界面；断开 SSE 不会影响 Worker。不要把网关订阅状态当成同步健康指标。

桌面端可在“设置 → 服务连接”打开应用轮转日志：

- “应用日志”打开 `%LOCALAPPDATA%\com.cooliang.imail\logs`。`app.log` 记录桌面进程启动、Tauri 初始化、前端就绪、窗口/托盘操作、本地服务生命周期、正常退出、Rust panic，以及 WebView 的全局错误、未处理 Promise 和 `console.warn/error`。单文件上限 5 MB，最多保留 3 份。

应用和服务对外部错误文本执行统一脱敏，邮箱地址、Authorization/Cookie、密码、OAuth code/state/Token、client secret 和加密字段不得进入日志。提交问题时优先提供相关时间段的日志，不要通过关闭脱敏或手工打印凭据补充信息。

卸载桌面应用默认保留 `local-service/data`。“设置 → 服务连接”只选择嵌入式本地或远程 Rust 服务，不提供数据删除。登录用户若进入“设置 → 隐私与数据 → 清除我的邮箱数据”，必须先核对范围，再提交当前 iMail 密码和固定确认文字；服务只清除该用户的数据，不删除登录账号、主密钥或其他用户的数据。

同一页面的“邮箱授权信息导出”会把当前用户全部邮箱的连接配置、应用专用密码/OAuth Token 与代理凭据写入独立密码保护的 `.imailauth` 文件；邮件、附件、草稿、联系人和 iMail 登录密码不在其中。创建前必须重新验证当前密码，导出密码至少 12 个字符；下载地址与当前用户绑定、只允许下载一次且两分钟后过期。该能力只属于登录会话 HTTP UI，运维人员不得通过 MCP、API Gateway、日志或数据库查询替代它来交付凭据。

### 发件人 Logo 缺失或错误

- 查看服务端 `[sender-logo]` 日志；每行包含 `success` / `failed`、目标 origin、共享主域键和结果说明。访问验证页会记录为失败并回退到联系人首字母。
- 持久化审计在 `.data/imail.sqlite` 的 `logo_fetch_attempts` 表，联系人当前引用在 `contacts.logo_key` 等字段；图片文件位于 `.data/sender-logos/`。
- 同一 origin 有任何采集记录后都不会自动重试，失败主域的负缓存也不会过期。这是防止高频访问和错误图标的安全边界。
- 若管理员确认旧版本缓存了错误图标，需要在 iMail 停止后备份 `.data`，再同时删除该主域对应的 `contacts` Logo 字段、`logo_fetch_attempts` 记录以及 `.data/sender-logos/` 中对应缓存文件。三处必须一起处理；不要只删图片，否则仍会因审计记录而跳过采集。
- 不要通过定时任务批量清空采集记录。重新采集属于显式运维动作，执行前应确认目标站允许自动访问。

### HTTP 401

- 确认授权码以 `imail_mcp_` 开头且包含 `mcp:full`。
- 确认未超过签发时选择的有效期，且 Token 未在 UI 中撤销。
- 确认请求头是 `Authorization: Bearer <code>`，不要把邮箱服务商授权码放在这里。

### HTTP 403

- 检查请求 Host/Origin 是否在 `MCP_ALLOWED_HOSTS`。
- IPv6 回环可写 `::1` 或 `[::1]`。
- 远程主机名必须显式加入 allowlist；同时启用 HTTPS。

### 工具返回邮箱协议错误

- `IMAP 验证失败` / `SMTP 验证失败` 来自既有协议适配层。
- 对非 OAuth 账户使用 `account_update_authorization_code` 更新服务商授权码。
- 对 OAuth 账户使用 `account_reconnect_oauth` 获取新的官方登录网址。

## 撤销与轮换

MCP 授权码最长 7 天。建议每个 Agent 单独签发并使用可识别名称；任务结束立即撤销。Streamable HTTP 每次请求都会重新验证，撤销即时生效。
