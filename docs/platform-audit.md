# CP0 平台审计

状态：完成  
审计日期：2026-08-12

## 结论

iMail 的领域、存储、邮件、OAuth、同步与 HTTP 能力已经集中在根 Cargo workspace 的 `crates/` 和 `http-service/`，没有依赖桌面操作系统 API。当前平台耦合集中在 `src-tauri/` 的桌面壳、Windows NSIS 配置和明确标记为 Windows-only 的安装包脚本中。

近期跨平台范围是共享 Rust 核心、服务端 Docker 和 Linux 桌面准备。macOS 标记为未来支持，不进入当前实现、CI 或发布门禁。原生桌面平台没有完成对应原生验收前，不进入支持矩阵。

## 平台边界

| 能力 | 当前实现位置 | 当前状态 | Owner | 目标阶段与验证 |
| --- | --- | --- | --- | --- |
| 领域、存储、邮件、OAuth、同步 | `crates/` | 平台无关；不得读取桌面环境变量或调用系统命令 | Shared Core | CP1：Windows、Ubuntu 编译与契约测试；macOS 留待未来阶段 |
| 独立 HTTP 与 Docker | `http-service/` | 薄启动器；Unix 使用 SIGTERM，其他平台使用 Ctrl-C | Service Runtime | CP1/CP2：Ubuntu 编译、`linux/amd64` 容器运行门禁 |
| 桌面数据、日志目录 | Tauri path resolver；`desktop_platform.rs` 仅处理 Tauri 启动前路径 | 运行期目录平台无关；bootstrap 日志当前只实现 Windows | Desktop Adapter | CP3：XDG；macOS 未来阶段：Application Support |
| 打开应用目录 | `desktop_platform.rs` | 当前只调用 Windows Explorer；其他平台显式拒绝 | Desktop Adapter | CP3：Linux 原生实现与 UI 验收；macOS 未来阶段单独实现 |
| 托盘、通知、单实例、窗口生命周期 | `src-tauri/src/lib.rs` + Tauri plugins | 使用 Tauri API，没有直接调用 Win32 | Desktop Adapter | 每个原生桌面阶段在目标系统验证行为 |
| OAuth callback | `imail-oauth*`、`imail-http` | 临时 loopback listener，不依赖桌面常驻 HTTP | Shared Core / Desktop Adapter | CP1 验证协议；CP3 和未来 macOS 阶段验证系统浏览器行为 |
| Windows 安装与卸载 | `tauri.windows.conf.json`、`windows/hooks.nsh` | 明确隔离为 NSIS/current-user 配置 | Windows Release | Windows 本地安装、升级、卸载和数据保留门禁 |
| 旧守护迁移 | `local_service.rs`、`legacy-daemon-admin` | 仅服务 Windows 一次性升级、回退和卸载清理；不属于新平台实现 | Windows Compatibility | Windows 回归；完成保留周期后另行评审删除 |
| Linux 桌面 | 尚无平台 bundle 配置 | 未支持 | Linux Desktop | CP3：Ubuntu 原生环境实现和验收 |
| macOS 桌面 | 尚无平台 bundle 配置 | 未来支持；当前不实施 | Future macOS | 未来独立立项，必须使用原生 Mac、签名和公证门禁 |

## 已完成整改

- 新增 `src-tauri/src/desktop_platform.rs`，集中管理启动期日志路径、打开目录和旧 Windows 守护数据根；业务模块不再直接调用 Explorer 或读取 `LOCALAPPDATA`。
- 删除 `local_service.rs` 中未交付、且会诱导新平台复用旧守护方案的 macOS `launchctl`/LaunchAgent 实现与测试。
- 旧守护注册和卸载清理在非 Windows 平台明确拒绝，不再伪装为可移植桌面能力。
- Windows release 的 `windows_subsystem` crate 属性增加显式 `windows` 条件，避免平台意图隐含在 release profile 中。
- 当前数据目录、Cookie、嵌入式 Session、托盘、通知与单实例继续使用 Tauri 路径或插件接口；没有引入新的操作系统分支。

## Windows 专属资产清单

- `src-tauri/tauri.windows.conf.json`：NSIS、current-user 安装和 WebView2 bootstrapper。
- `src-tauri/windows/hooks.nsh`：卸载时清理旧 runtime，保留邮件数据。
- `scripts/build-internal-desktop.mjs`：只允许 Windows 构建内部 NSIS。
- `scripts/smoke-windows-installer.mjs`：Windows Registry、安装目录和卸载数据保留验收。
- `scripts/smoke-tauri-release.mjs`：Windows release 可执行文件冒烟。
- `.github/workflows/deployment-release.yml` 的 `windows` job：Windows x64 Artifact。

这些文件允许使用 Registry、`LOCALAPPDATA`、`.exe` 和 NSIS；共享 crate、HTTP 服务和通用前端测试不得依赖这些约定。

## CI 命名与职责

| 名称 | 环境 | 副作用边界 |
| --- | --- | --- |
| `verify-windows-x64` | Windows x64 | 前端、Rust、Tauri 与安装包验证；不发布 |
| `verify-shared-ubuntu` | Ubuntu x64 | 共享 workspace、HTTP 和契约验证；不构建桌面包 |
| `verify-docker-linux-amd64` | Ubuntu + Buildx | 构建和运行候选镜像；不推送 |
| `build-windows-x64` | Windows x64 | 仅显式手动构建内部 Artifact |
| `publish-docker-linux-amd64` | Ubuntu + Buildx | 仅明确授权或合规版本标签推送 GHCR |
| `verify-linux-desktop-x64` | Ubuntu 原生桌面环境 | CP3 后新增；构建与桌面行为验收 |

macOS job 不在当前矩阵中；进入未来支持阶段时再增加独立名称和受保护签名环境。

## Windows 基线门禁

从仓库根目录执行：

```powershell
npm --prefix frontend run typecheck
npm --prefix frontend test
npm --prefix frontend run build
npm --prefix frontend run rust:fmt
npm --prefix frontend run rust:clippy
npm --prefix frontend run rust:test
cargo test -p imail --lib --target x86_64-pc-windows-msvc
npm --prefix frontend run build:desktop:internal
npm --prefix frontend run test:desktop-release
```

安装/卸载验收只在确认测试机没有现有 iMail 安装后执行：

```powershell
$env:IMAIL_ALLOW_INSTALLER_SMOKE = "true"
npm --prefix frontend run test:windows-installer
```

所有测试必须使用临时目录或不可覆盖副本，不得修改 `.data`、正式平台数据目录或 `output/rust-migration-tests/`。

### 本次审计证据

2026-08-12 在 Windows x64 本机完成以下验证：

- 前端 `typecheck`、129 项 Vitest 测试和生产构建通过。
- Rust workspace 格式、全 feature Clippy、全 feature 测试通过；另有 21 项 Tauri lib 默认 feature 测试通过。
- 使用隔离的 `CARGO_TARGET_DIR` 完成 release 可执行文件与 NSIS 安装包构建，避免覆盖正在运行的正式客户端。
- 隔离 release 可执行文件通过桌面 smoke，确认嵌入式 Rust 服务存在且没有捆绑 Node runtime。
- 安装/卸载 smoke 未在本机执行：当前用户已有 iMail 安装、卸载注册和本地服务数据，安全前置检查要求拒绝覆盖。该项继续作为干净 Windows 测试机或 CI 的安装包门禁，不影响本次代码边界审计结论。

## CP1 输入

CP0 只确认边界和 Windows 基线，不宣称 Unix 已通过。CP1 必须在 Ubuntu 环境编译共享 workspace，清除测试中的 PowerShell、盘符和 Windows 可执行文件假设，并验证 Unix 权限、信号、TLS、代理、SQLite 与 loopback。macOS 只保留未来支持标记，不阻塞近期 CP1/CP2/CP3。
