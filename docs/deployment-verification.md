# 部署模式完成度与内部测试验证矩阵

本文件把[部署模式路线图](./deployment-modes-roadmap.md)中的验收场景映射到可重复证据。单元测试证明协议和状态转换，打包冒烟证明真实进程与构建产物；需要操作系统重启、平台身份签名或容器运行时的项目不能用代码检查代替。

2026-08-04 Windows 本机复验已通过类型检查、43 个测试文件中的 270 项测试、生产依赖审计、Web/远程运行包构建、Rust 主程序与清理程序测试、NSIS 构建、已安装桌面启动、本地守护进程和远程同源服务冒烟。随后 [GitHub Actions 运行 30872253518](https://github.com/cooliang101/iMail/actions/runs/30872253518) 在提交 `bb9502f` 上通过 Ubuntu 远程运行时、真实容器、Windows NSIS 与用户守护进程、macOS arm64/x64 应用和 DMG 五项门禁。当前只交付有期限的内部测试产物；公网反向代理、正式平台签名/Apple notarization 属于延期的正式分发验收，操作系统注销登录仍需内测实机验证。

## 自动验证入口

| 命令 | 验证范围 |
| --- | --- |
| `npm run test:internal-release` | 类型、全量测试、远程构建、Windows 内部测试 NSIS 构建、本地 supervisor 真实进程链、远程同源运行链；旧命令 `test:deployment-release` 保留为兼容别名 |
| `npm run test:windows-installer` | 受控当前用户环境中的 NSIS 静默安装、sidecar 装配、已安装应用启动、`HKCU Run\\iMailService` 守护自启动项注销、卸载 hook 以及默认保留数据；脚本先拒绝任何既有安装、服务数据或同名自启动项，只在 CI 或显式授权时运行 |
| `npm run test:local-daemon` | 自动分配隔离端口，验证 supervisor 独占锁、API/Worker 父子关系、API 崩溃恢复、孤儿 Worker 退出、守护代次切换、持续启动失败的退避与诊断、卸载保留数据和越界配置拒绝；异常退出同步终止测试进程树；Rust 测试另覆盖服务程序原子切换、中断恢复与回滚 |
| `npm run test:remote-release` | 显式 `NODE_ENV=production` 下的 Web/SPA/CSP、公开 Host、不隐式允许 localhost 开发 CORS、同主机明文 WebSocket Origin 拒绝、可信代理 Cookie、初始化关注册、安全审计与 Token 不泄漏、两个独立设备会话共享数据、SSE、WebSocket、API fallback 边界 |
| `server/auth/store-security.test.ts` 与 `server/index.test.ts` | 登录限流跨存储重启保持；审计来源使用实例盐 HMAC；事件按应用用户隔离；HTTP 授权码管理及 MCP 管理调用留痕；响应不包含原始 Token |
| `npm run test:container-release` | 裸机及 HTTPS Compose 解析、Caddyfile 官方镜像校验、Docker 镜像构建、动态端口启动、健康端点、Web 外壳、非 root 用户、镜像健康检查，以及镜像内备份—完整性校验—恢复往返 |
| `npm run test:macos-bundle` | `.app` 深度签名、sidecar hardened runtime、`allow-jit` 且无未签名可执行内存 entitlement、桌面启动、已签名 SEA sidecar 身份、独立同步 Worker与控制令牌优雅退出；随后在干净 CI 用户中实际 bootstrap LaunchAgent，验证签名 supervisor、API/Worker、API 崩溃恢复、bootout 和进程树清理；仅允许在 macOS 测试构建机运行 |
| `npm run backup -- <目标目录>` | 在线 SQLite 一致性快照、同目录暂存后原子提交、数据目录内主密钥/实例身份/Logo 的同批备份，以及含服务与 schema 版本的逐文件 SHA-256 v2 清单 |
| `npm run restore:prepare -- <备份目录> <新目录>` | v1/v2 SHA-256 清单、清单与数据库 schema 交叉检查、当前发布 schema 上限、SQLite 完整性、iMail 核心表、主密钥与实例身份格式检查；复制到全新目录且拒绝覆盖当前数据，测试覆盖数据分叉、身份保留、快照往返、篡改、版本错配及未来 schema 拒绝；服务启动迁移另做同样上限检查 |
| `npm run upgrade:preflight -- <新备份目录> <新预检目录>` | 在线一致备份、非覆盖恢复、当前版本迁移、SQLite `quick_check`、外键检查与 schema 版本确认；成功时在线数据不变，失败时清理半迁移副本但保留回滚备份；远程运行包与容器门禁直接执行打包后命令 |
| GitHub Actions `Internal test verification` | 2026-08-04 的[绿色运行 30872253518](https://github.com/cooliang101/iMail/actions/runs/30872253518) 已通过 Ubuntu 远程运行时与真实容器、Windows NSIS 与真实用户守护进程、macOS arm64/x64 原生 `.app`/DMG 构建和冒烟；当前工作流把桌面 Artifacts 保留 14 天，并在 `main` 全门禁通过后创建带 SHA-256 校验文件的 Draft Release，首个草稿仍待合并后由 CI 验证 |

## 路线图验收映射

| 场景 | 当前自动证据 | 内部测试仍需实机验证 |
| --- | --- | --- |
| 1. 选择本地服务后一键安装并启动 | Tauri sidecar 构建、命令桥测试、服务身份测试、本地进程冒烟；Windows 测试构建机已在受控当前用户环境中真实安装并从安装目录启动 MSVC 产物；安装版 UI 已验证能进入本地选择并准确报告真实端口冲突；跨平台 CI 已取得首次绿色运行 | 全新 macOS 用户从 UI 首次启用；无端口冲突环境中的 Windows UI 成功态留档 |
| 2. 桌面 UI 退出后继续同步 | Windows supervisor 是不依赖 WebView 的独立服务管理程序；本地冒烟直接脱离 UI 启动 API 与 Worker；配置与事务测试证明用户显式暂停/移除后，应用重启检查不会静默重新启用，写入暂停意图失败则恢复后台服务 | 安装包中退出 UI 后用真实邮箱观察同步 |
| 3. 用户登录后自动启动 | Windows HKCU Run 与 macOS LaunchAgent 配置测试 | Windows 注销/登录；macOS 注销/登录 |
| 4. 守护进程崩溃后恢复 | 本地冒烟强制终止 API，验证新 PID 恢复、旧 Worker 退出、新 Worker 建立；缺失服务文件时 supervisor 保持退避并持久化安全诊断 | 设置页错误摘要和“打开日志目录”在安装包中的人工检查 |
| 5. 切到远程后暂停本地服务 | 模式事务测试验证远程先验身份、再暂停本地、最后提交配置；远程检查失败不暂停，配置提交或暂停部分失败会重新启用原本地服务；Tauri 命令只有同时确认 API 身份消失且 supervisor 锁释放后才报告暂停成功 | 桌面 UI 真实切换并观察本地进程退出 |
| 6. 远程断网不回退本地 | 服务配置测试证明模式与地址独立且无自动 fallback；启动门禁测试证明保存的远程端点必须先重新通过身份/协议握手，失败时不会发送登录状态请求 | 桌面断网错误文案人工检查 |
| 6a. 远程传输不降级 | 配置与事务测试覆盖 HTTPS、IPv4/IPv6 回环 HTTP、局域网/公网 HTTP 拒绝以及旧配置在身份请求前拦截；Rust 测试使用真实 307 响应确认网络桥不跟随重定向 | 受信任内部证书、过期证书和错误主机证书的桌面人工检查 |
| 7. 多设备和浏览器共享远程数据 | 远程发布冒烟使用两个独立登录 Cookie：设备 A 创建草稿、设备 B 读取并更新、设备 A 读取更新；设备 A 退出后设备 B 会话和数据继续有效，同时验证 SSE 与 WebSocket | 两台真实物理设备通过受信任 HTTPS 使用同一远程实例 |
| 8. 切回本地不合并数据 | 配置测试验证切换仅改变数据源并保留远程地址；模式事务测试验证本地守护和身份就绪后才提交，失败会重新暂停刚启用的本地服务；网络桥按基地址隔离并持久化 Cookie Jar，Rust 测试覆盖桌面重启恢复与跨服务不串会话；检查代次测试证明旧实例迟到响应不能覆盖新实例状态 | 各准备一组不同数据后做桌面往返切换 |
| 9. 移除守护项和运行文件、保留数据 | remove 命令和 NSIS pre-uninstall hook 只删除配置、锁、runtime、logs，不删除 `data`；本地进程冒烟验证数据保留；Windows 安装冒烟在无既有状态的当前用户中创建受控 `HKCU Run\\iMailService` 值，真实执行安装与卸载并确认该值和运行目录均消失；跨平台 CI 已取得首次绿色运行。永久删除使用独立命令和确认文字，Rust 测试覆盖未移除拒绝、错误确认拒绝、只删除固定普通 `data` 目录并保留同级状态 | 带真实用户数据的交互式卸载与独立删除确认界面检查 |
| 10. 升级失败可诊断和回滚 | 版本化运行文件、代次锁、旧 supervisor 自退及自动恢复旧配置逻辑；进程冒烟验证代次退出；Rust 文件事务测试验证暂存校验、原子激活、进程中断状态恢复、失败回滚和提交清理 | 用两个内部测试版本执行成功升级和进程级故障注入回滚 |

## 内部测试平台门禁

Windows 测试构建机：

1. 运行 `npm run test:internal-release`。
2. 构建 NSIS，使用全新当前用户安装。
3. 执行场景 1、2、3、5、8、9、10，并保存进程、注册项和数据目录证据。
4. 验证卸载和“移除运行文件”默认保留数据；再从独立危险入口输入确认文字，验证永久删除本地数据且不影响远程实例。

macOS 测试构建机：

1. GitHub Actions 分别在 `macos-15` arm64 和 `macos-15-intel` x64 原生 runner 执行 `npm run build:desktop:internal` 和 `npm run test:macos-bundle`，使用 ad-hoc 签名证明两种架构均可打包，并从签名后的 `.app` 直接启动桌面程序、SEA sidecar 与 Worker。
2. 执行登录启动、崩溃恢复、暂停、升级、移除和数据保留场景。
3. Developer ID Application、Apple notarization 与发行者身份检查不阻塞内部测试，留到正式分发阶段。

远程服务测试机：

1. GitHub Actions 的 `Remote container` 门禁以及内部测试机均运行 `npm run test:container-release`。
2. 跨设备内测时使用客户端信任的内部 HTTPS，并确认长连接超时和 Host 配置；公网正式域名不阻塞当前阶段。
3. 用持久卷完成“备份—升级—恢复—回退”演练。
4. 使用 `compose.https.example.yml` 和受控测试域名验证证书、HTTP 到 HTTPS 跳转、SSE 即时事件、WebSocket 升级及后端 8787 未直接暴露。
