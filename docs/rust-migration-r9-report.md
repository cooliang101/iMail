# R9 Windows Rust-only 收尾报告

日期：2026-08-11
分支：`codex/rust-service-migration`

## 结论

Windows 桌面本地模式已完成 Rust 进程内直连收尾：安装包不再携带或启动 Node SEA、service manager、worker 或本地 HTTP 服务；只有选择远程模式时才经过 Rust 网络桥访问外部 HTTP(S) 服务。关闭窗口继续由 Tauri 托盘进程保持嵌入式同步，显式退出才停止进程。

正式 `Dockerfile` 已改为 Rust HTTP Adapter，但本次升级任务的交付和完成范围仅为 Windows x64。Linux/WSL2、Docker 运行门禁与交叉平台构建不属于本任务，后续必须另建独立计划。本报告不把中止的交叉平台构建记为成功。

## 本轮变更

- 删除 Windows Node sidecar 构建脚本、本地 daemon 冒烟、独立 cleanup helper 和对应 CI 步骤。
- Tauri 构建只运行 Web 前端构建，不配置 `externalBin`；NSIS 只安装 `imail.exe` 与卸载程序。
- 删除前端本地端口选择、端口冲突恢复、守护暂停/删除/日志入口；本地服务选择写入 `tauri://embedded`，旧 `127.0.0.1:8787` 配置仍识别为迁移前本地模式。
- `api()`、同步事件、附件读取与下载在本地模式使用类型化 Tauri command/event；远程模式保留经过校验的 HTTP(S) 桥。
- `Dockerfile` 的最终 runtime 为 Debian + `imail-server`/`imail-maintenance`，Node 只用于 Web build stage，不进入运行镜像；容器必须显式 `--http` 启动。
- 默认开发、远程启动、备份、恢复预检和升级预检入口已全部切换到 Rust 二进制；生产依赖审计为 0 个漏洞。
- “外部访问”界面在 Tauri 本地嵌入模式明确显示“无 HTTP 地址”，并禁用 Gateway、MCP、Token 创建与复制；远程模式才展示实际 HTTP(S) 服务地址。
- README、架构、运维、部署验证、内部测试、MCP 与交接文档已同步到“本地直连、远程可选 HTTP”边界。
- Windows 迁移、数据兼容和资源对照验收完成后，按用户决定删除旧 Node 服务源码、Express 路由、Worker、双实现测试、基准构建和仅供该实现使用的 npm 依赖；旧代码通过 Git 历史追溯，既有验收报告与数据快照不删除。
- Tauri 默认 release 不再编译退休的旧守护管理面；`local_service_status/enable/remove/open_logs`、旧 sidecar 部署和对应回滚部署器仅保留在显式 `legacy-daemon-admin` 审计 feature 中。正常包只保留一次性切换所需的停服、快照、失败恢复、旧 supervisor 兼容和卸载清理。

## Windows 验收

- `npm run typecheck`：通过。
- `npm test`：67 个测试文件、327 项通过。
- `npm run build` 与 `npm run build:remote`：通过。
- Rust workspace：135 项通过，1 项需显式长稳条件的测试 ignored。
- Tauri 默认 release feature 图：22 项通过；显式 `legacy-daemon-admin` 审计 feature 图：24 项通过。
- Rust workspace 与 Tauri 严格 Clippy：通过。
- Rust workspace 与 Tauri rustfmt check：通过。
- Rust-only NSIS 构建及无界面启动冒烟：通过。
- Windows Rust 常驻资源回归：60 秒、60 个样本，门禁通过。

R9 最终安装包（包含本地无 HTTP 界面与旧守护默认编译图收敛）：

- 路径：`output/rust-migration-tests/r9-rust-only-target-final-v2-2026-08-11/x86_64-pc-windows-msvc/release/bundle/nsis/iMail_0.0.1_x64-setup.exe`
- 大小：8,762,301 bytes
- SHA-256：`9179272C477FDD8240D9FBB9F85C9B99E6913CE657177D6706349E10D05D3DFE`
- 7-Zip 内容扫描：`node.exe`、`imail-service`、manager、worker、`.cjs` 与 `service-runtime` 命中数为 0。
- release 冒烟输出：`embeddedRustService=true`、`bundledNodeRuntime=false`。

此前的 `r9-rust-only-target-2026-08-11` 与 `r9-rust-only-target-final-2026-08-11` 候选包继续保留，未被最终构建覆盖。

运行状态复核：iMail legacy service/manager/Node CJS 进程为 0，8787 监听为 0。系统中用于开发工具的其他 `node.exe` 不属于 iMail 服务，未做终止。

## Windows 常驻资源回归

最终通过报告保存在 `output/rust-migration-tests/r9-windows-runtime-soak-v3.json`，使用 R0 快照的独立副本，不访问活动 `.data`：

- 初始 RSS：9,666,560 bytes（约 9.22 MiB）。
- 末次 RSS：9,617,408 bytes（约 9.17 MiB）。
- 峰值 RSS：9,666,560 bytes（约 9.22 MiB）。
- 门禁计入的首尾增长：0 bytes，低于 16 MiB 预算。
- 运行期间没有待执行任务；停止后 worker、queued job 均为 0，优雅关闭成功。
- 报告 SHA-256：`8C4642AAB151434A44609BC50B8EABFA85CB17EB62ABF989609324F6BA3EDD6C`。

`v1` 与 `v2` 诊断报告及数据副本也继续保留。它们的内存均在预算内，但 R0 快照含有 1 条历史遗留 `running` 同步任务，运行时恢复后按预期失败，因此生命周期门禁拒绝通过。`v3` 只在新复制的数据副本中把该遗留状态收敛为 `failed`，验证状态计数为 `failed=64`、`succeeded=25314` 后再运行；没有修改 R0 快照或活动数据库。

## Windows 大邮件与多账户长稳预算

`output/rust-migration-tests/r9-windows-large-mail-soak-300s-v1.json` 使用完全隔离的合成数据，对 Node/Rust 完整拓扑分别执行 300 个采样周期和 301 个请求批次：5 个账户、1,000 封双 32 KiB 正文邮件、2 MiB 单封详情、每批 8 个并发请求。两端各传输约 2.41 GiB 响应数据，负载前后列表裁剪、总数和详情长度契约一致；两端退出码均为 0，停止后无遗留 PID。

| 指标 | Node 完整拓扑 | Rust 完整拓扑 |
| --- | ---: | ---: |
| 实际采样墙钟 | 949.913 秒 | 568.067 秒 |
| 中位 Working Set | 1,652,379,648 bytes | 37,748,736 bytes |
| 峰值 Working Set | 1,665,982,464 bytes | 48,709,632 bytes |
| 稳定窗口绝对漂移 | 16,338,944 bytes | 10,371,072 bytes |
| CPU 累计 | 129.8125 秒 | 21.859375 秒 |

Rust 峰值降低 97.08%，中位数降低 97.72%。报告 SHA-256 为 `A6134D58836EE196D52740909B83BBADBC816AE2786E7DDBE4C514A9EE116843`。

该场景现已成为会失败的持续门禁：Rust 峰值不得超过 64 MiB，首末十样本中位数的绝对漂移不得超过 16 MiB，峰值和中位数相对 Node 均至少降低 90%；同时要求前后数据契约、两端优雅停机和零遗留进程全部通过。当前报告满足所有检查。标准入口为：

```powershell
npm run rust:resource-compare:soak -- output/rust-migration-tests/<新的不可覆盖报告>.json
```

## Windows TLS/IDLE 调度长稳

`output/rust-migration-tests/r9-windows-real-tls-soak-300s-v1.json` 在本机临时 CA、真实 TLS socket、隔离 IMAP/SMTP fixture 和临时数据库上运行 300 秒：

- 294 个资源样本，RSS 从 20,201,472 增至 21,266,432 bytes，增长 1,064,960 bytes，峰值 21,585,920 bytes，低于 32 MiB 增长预算。
- 1 次 watcher 断线、1 次重连、1 个 recovery job 和 17 个 scheduled job；18 个任务全部成功，无失败或取消。
- 最大 queued job 为 0；停机后 queued job 与 worker heartbeat 均为 0，优雅关闭成功。
- 报告 SHA-256：`2BAB6964D808D54F45B68DEBE76A7C33FFD7E3920A85F751B10BC06AE9223D47`。

大附件路径由同一正式 TLS fixture 的 2 MiB 入站 FETCH/MIME/下载和出站 SMTP round-trip 自动化覆盖；本轮完整 Rust 测试已再次通过该项。所有上述长稳负载均未连接公共邮箱或发送邮件。

## 数据保留

- 工作区 `.data/imail.sqlite` 在本轮前后 SHA-256 均为 `074B3D437ADDDEBE3BD8020C3DAA18B825DE440EDA34AD01838979574BD72000`。
- 已有切换前快照与切换后快照继续原地保留；未删除数据库、主密钥、Logo、邮件缓存或测试邮件。
- 本轮没有发送邮件；四账户互发限制保持不变。

## 后续阶段

以下内容均不影响本次 Windows 任务完成状态：

1. Linux/WSL2、Docker `linux/amd64` 和其他平台的构建、运行与发布门禁另建独立跨平台计划，不在本路线中继续执行。
2. 完成一个发布周期后，确认不再需要旧版回滚，再删除 Tauri 中仅用于一次性 Node 守护迁移的兼容实现。
3. 旧 Node 对照宿主源码已在迁移验证完成后删除；历史测试数据、报告和迁移快照继续保留。

范围调整时已终止正在进行的 WSL2 冷构建并关闭 `Ubuntu-22.04`；该次尝试未生成候选镜像、运行容器、测试数据卷或验收报告，也未清理共享构建缓存和既有 Docker 数据。
