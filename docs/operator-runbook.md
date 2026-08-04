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
| `IMAIL_SYNC_WORKER_MODE` | `child` | `child` 由 API 启动器监管；`external` 由外部管理器运行；`disabled` 仅用于诊断 |
| `IMAIL_SYNC_CONCURRENCY` | `3` | Worker 最大并发同步任务数，范围 1–10 |
| `IMAIL_SYNC_WORKER_POLL_MS` | `1000` | Worker 领取任务间隔，最小 250ms |
| `IMAIL_SYNC_SCHEDULER_INTERVAL_MS` | `5000` | 到期策略扫描间隔，最小 5 秒 |
| `IMAIL_SYNC_STARTUP_DELAY_MS` | `1000` | 服务启动后的首次补同步延迟 |
| `IMAIL_SYNC_JOB_LEASE_MS` | `120000` | 任务租约时间，最小 10 秒；执行中会自动续租 |
| `IMAIL_SYNC_IDLE_ENABLED` | `true` | 是否启用收件箱 IMAP IDLE 实时唤醒；关闭后仍按周期轮询 |
| `IMAIL_SYNC_IDLE_RECONCILE_MS` | `5000` | IDLE 连接期望状态检查与断线重建间隔，最小 5 秒 |
| `IMAIL_SYNC_IDLE_REFRESH_MS` | `60000` | IDLE 保活刷新周期；不支持 IDLE 的服务商以此间隔执行 STATUS 兜底，最小 15 秒 |

## 本地启动

Streamable HTTP：

```bash
npm run dev
```

默认模式会由 API 启动器拉起并监管 Worker。Docker Compose 需要分别管理进程时：

```powershell
$env:IMAIL_SYNC_WORKER_MODE='external'
npm start
npm run worker
```

## 远程生产部署

`npm run build:remote` 生成同版本 Web、API 与 Worker 运行包，`npm run start:remote` 默认监听 `0.0.0.0:8787` 并托管 `dist`。容器以非 root 用户运行并把所有可变数据写入 `/data`。`compose.example.yml` 只把端口绑定到宿主机回环地址，适合本机验证或接入宿主机已有的反向代理，不应改成直接监听所有网卡。

仓库同时提供带 Caddy 自动 HTTPS 的 `compose.https.example.yml`。服务镜像由受控 GitHub Actions 发布到 `ghcr.io/cooliang101/imail`；复制环境变量模板，填写已解析到部署主机的域名，并将 `IMAIL_IMAGE` 固定到所需版本标签或 digest 后启动。若 GHCR 包保持私有，先使用具有 `read:packages` 权限的 Token 执行 `docker login ghcr.io`：

```bash
cp deploy/remote.env.example .env.remote
# 编辑 .env.remote，至少设置 IMAIL_PUBLIC_HOST，并在固定部署中替换 edge
docker compose --env-file .env.remote -f compose.https.example.yml up -d --pull always
```

该拓扑只向公网发布 Caddy 的 80/443（含 HTTP/3 UDP）端口，iMail 的 8787 只存在于 Compose 网络。Caddy 自动申请和续期证书，配置禁用上游响应缓冲以保证 SSE 实时送达；WebSocket 由 `reverse_proxy` 原生转发。部署前确认 DNS 已生效且防火墙允许 TCP 80/443 与 UDP 443。若已有反向代理，继续使用回环绑定的 `compose.example.yml`，并自行配置 SSE 禁用缓冲、WebSocket 升级和足够长的读取超时。

公网入口必须使用 HTTPS。仅当反向代理是服务的直接前一跳时设置 `IMAIL_TRUST_PROXY=true`，并把公开地址同步写入 `FRONTEND_URL` 和 `OAUTH_CALLBACK_BASE_URL`。`MCP_ALLOWED_HOSTS` 只列出实际主机名。生产环境默认只允许首个用户完成初始化；除非实例明确供多个互不信任用户共同使用，否则不要把 `IMAIL_REGISTRATION_MODE` 改成 `open`。

同源部署不需要设置 `CORS_ORIGIN`。生产运行时不会自动加入 `localhost:5173`；拆分 Web/API 域名时只填写完整 Origin，例如 `https://web.example.com`，不带路径、查询、片段或凭据。非回环 HTTP 来源会导致服务在启动时拒绝配置。

登录与注册限流保存在 SQLite 的 `auth_rate_limits` 表，重启不会清零；注册、登录、授权码与敏感管理动作写入 `security_audit_events`。审计来源只保存使用实例随机盐生成的 HMAC，不保存密码、会话、OAuth Token 或邮箱凭据。公网反向代理仍应提供独立的连接级限流和访问日志。

## 备份与恢复

在线备份使用 SQLite backup API 取得一致数据库快照，并同时复制自动生成的主密钥、持久实例身份与发件人 Logo。备份先写入同目录暂存项，全部成功后再原子提交，并生成包含 iMail 版本、数据库 schema 版本和逐文件 SHA-256 的 v2 完整性清单；恢复仍兼容已有 v1 清单。当前 schema v5 在 `accounts.proxy_json` 中保存每邮箱的非密码代理字段，代理密码仍位于加密凭据载荷，二者都会随数据库快照一起备份：

```bash
npm run backup -- /safe/backups/imail-2026-08-03
```

远程运行包同时生成 `server-runtime/imail-backup.mjs` 和 `server-runtime/imail-restore.mjs`。Compose 将独立的 `imail-backups` 卷挂载到 `/backups`，因此容器部署可在线执行：

```bash
docker compose --env-file .env.remote -f compose.https.example.yml exec imail \
  node server-runtime/imail-backup.mjs /backups/imail-2026-08-03
docker compose --env-file .env.remote -f compose.https.example.yml cp \
  imail:/backups/imail-2026-08-03 ./imail-2026-08-03
```

第二条命令把快照导出到 Docker 主机；只留在同一主机的命名卷不构成异地备份。

先在服务外准备一个全新的恢复目录。命令会验证 SHA-256 清单、执行 SQLite `quick_check`、校验 iMail 核心表与本地 `master.key` 格式，并拒绝覆盖已有目录；它不会改写正在使用的数据卷：

```bash
npm run restore:prepare -- /safe/backups/imail-2026-08-03 /safe/restore/imail-2026-08-03
```

容器内也可以用 `node server-runtime/imail-restore.mjs <备份目录> <全新恢复目录>` 执行相同校验。恢复目标必须是新目录或新数据卷，工具不会覆盖 `/data`。恢复工具和服务启动迁移都会拒绝高于当前发布版本支持上限的未来 schema；遇到此错误必须升级服务或选择兼容快照，不能强行启动旧版本。

使用环境变量提供 `APP_MASTER_KEY` 时，密钥不在数据目录中，必须由密钥管理系统另行备份。没有原主密钥，即使数据库恢复成功也无法解密邮箱凭据。

## 远程服务升级预检

替换远程服务前，使用将要发布的新版本执行预检。命令会在线创建一致性回滚备份，恢复到全新目录，只在副本上执行当前版本迁移，再运行 SQLite `quick_check` 和外键检查。在线数据目录不会被改写：

```bash
npm run upgrade:preflight -- \
  /safe/backups/imail-before-upgrade \
  /safe/preflight/imail-new-version
```

Compose 部署应先拉取新镜像，但保持旧容器运行；然后用新镜像的一次性容器执行预检：

```bash
docker compose --env-file .env.remote -f compose.https.example.yml pull imail
docker compose --env-file .env.remote -f compose.https.example.yml run --rm --no-deps imail \
  node server-runtime/imail-upgrade-preflight.mjs \
  /backups/imail-before-upgrade /backups/imail-new-version-preflight
```

只有输出同时包含 `activeDataUntouched: true`、`sqliteQuickCheck: true` 和 `foreignKeysVerified: true` 时才执行 `docker compose up -d imail`。备份目录和预检目录必须是不存在的两个独立路径。预检失败时会删除半迁移副本，但保留已验证的回滚备份。不要把预检目录与在线 `/data` 合并或直接覆盖。

## 恢复切换与回滚

恢复前停止 API 和 Worker，将当前数据卷完整另存，再把经过 `restore:prepare` 验证的新目录作为完整数据目录切换进去。不要直接向运行中的数据卷复制文件，也不要混用不同时间点的数据库与主密钥。启动后先检查 `/api/system/info`、登录、账户列表和 Worker 心跳，再开放反向代理流量。升级前先执行备份；迁移失败时停止新版本，切换到准备好的旧数据快照并回退到原镜像或安装包。

## 冒烟检查

当前交付范围只有 Windows 桌面端与服务端 Docker；Linux 侧只运行 Docker，不维护原生部署单元。Actions 月度额度接近上限，普通分支推送与 pull request 不触发工作流；未经用户明确授权，不要创建版本 tag 或手动运行。手动工作流必须选择 `docker` 或 `windows`：前者只发布 `linux/amd64` GHCR 镜像，后者只生成 Windows Artifact。Windows 日常仍使用本机 `npm run test:internal-release`，有 Docker 的测试机可执行 `npm run test:container-release`。

1. 在“外部接入”的“MCP”标签页签发 `mcp:full` 授权码。
2. 用 MCP Inspector 或任意标准客户端连接 `http://127.0.0.1:8787/mcp`；桌面本地服务改过端口时使用设置页显示的当前地址。
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
npm run typecheck
npm test
npm run build
npm audit --omit=dev
```

部署模式内部测试验证：

```bash
npm run test:internal-release
# 安装并启动 Docker 的测试机额外执行
npm run test:container-release
# 仅限明确允许改写当前用户安装状态的 Windows 测试机
IMAIL_ALLOW_INSTALLER_SMOKE=true npm run test:windows-installer
```

逐项证据和需要在真实平台执行的检查见 [`deployment-verification.md`](./deployment-verification.md)。

不要为了生成 Windows 内部测试包创建标签或触发 GitHub Actions。Windows 交付直接使用本机 `build:desktop:internal` 产物并记录版本、构建提交、平台/架构与 SHA-256；三段式版本标签只负责把已确认版本的服务端镜像发布到 GHCR。

`server/index.test.ts` 覆盖授权拒绝、MCP 初始化、工具清单、工具调用和凭据不泄漏；`server/tokens.test.ts` 覆盖 `imail_mcp_` 格式和 scope 隔离。

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
- `queuedJobs` 持续增加但没有新心跳，说明 Worker 未运行。默认 `child` 模式查看 API 控制台中的 `[sync-worker]` 日志；外部模式确认 `npm run worker` 或对应服务单元已启动。
- `connectionStatus=authRequired` 时自动重试会暂停，应在邮箱设置中重新授权或更新凭据；验证成功后调度器会恢复该账户。
- `syncState=backoff` 表示网络或服务商错误，按 1、5、15、30、60 分钟退避。不要通过频繁点击手动同步绕过服务商限流。
- 持续出现 `[sync-idle]` 表示长连接无法稳定建立或被服务商/网络设备关闭；Worker 会按 0.5–30 秒退避重连，同时保留一分钟周期同步兜底。不支持 IDLE 的服务器会按 `IMAIL_SYNC_IDLE_REFRESH_MS` 执行 `STATUS`。
- 前端 SSE 仅用于刷新界面；断开 SSE 不会影响 Worker。不要把网关订阅状态当成同步健康指标。

本地模式可在“设置 → 服务连接 → 打开日志目录”查看轮转日志。`supervisor-status.json` 仅记录失败次数、固定原因代码、退出码和发生时间，可用于判断服务是否处于持续退避；它不包含邮箱凭据、会话或 Token。重新启用成功后旧诊断会自动清除。

卸载桌面应用或点击“移除运行文件”默认保留 `local-service/data`。“设置 → 服务连接”只管理服务模式、端口和守护生命周期，不提供数据删除。登录用户若进入“设置 → 隐私与数据 → 清除我的邮箱数据”，必须先核对范围，再提交当前 iMail 密码和固定确认文字；服务只清除该用户的邮箱账户与授权、邮件缓存、草稿、联系人、开发者令牌和同步状态，不删除登录账号、服务程序、主密钥或其他用户的数据。该操作无法撤销：需要保留邮件等完整内容时应事先创建服务数据备份；“邮箱授权信息导出”只保留连接配置与凭据，不包含邮件、草稿或联系人。

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
