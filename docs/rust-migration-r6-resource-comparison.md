# Rust 服务迁移 R6 资源对照

日期：2026-08-10
状态：Windows 同构 HTTP 空闲与大邮件查询对照完成；真实邮箱长稳负载仍待执行

## 对照口径

本轮比较正式 production 构建，而不是 `tsx`、Cargo debug 或只测 Rust 内部 runtime：

- Node 使用 `server-runtime/imail-server.cjs` 与 `imail-worker.cjs`。
- Rust 使用 release `imail-server.exe`。
- 两边均使用全新隔离数据目录、同一 Web 构建、Gateway、MCP、3 个 worker slot 和回环 HTTP。
- 每组启动后依次请求 health、service info、OpenAPI 和 Web 首页，共预热 50 次；随后空闲采样 15 次。
- “仅宿主”关闭同步 worker；“完整拓扑”启用 Node 子 worker 或 Rust 同进程 worker。
- Windows Working Set 以完整进程树求和，因此包含服务控制台宿主；Node 完整拓扑同时包含独立 worker。报告不把单个 Node 父进程数字冒充总占用。
- 四组均通过控制令牌端点停机，要求退出码 0、无 worker 重启、无遗留采样进程。

采样器为 `scripts/compare-service-resources.mjs`。它只在系统临时目录创建测试数据，拒绝覆盖报告文件，并在结束时校验后清理自己的临时根目录。可重复执行：

```powershell
npm run build:remote
cargo build --release --locked --manifest-path rust/Cargo.toml -p imail-http --bin imail-server
node scripts/compare-service-resources.mjs `
  --duration-seconds 15 `
  --report output/rust-migration-tests/<新的报告名>.json
```

`npm run rust:resource-compare` 用于构建并执行默认 30 秒采样；需要自定义时长和非覆盖报告路径时使用上面的直接命令，避免 npm 把命名参数解释为自身配置。

大邮件查询口径使用 `--workload large-mail`，也可运行 `npm run rust:resource-compare:large-mail`。采样器会在每个独立临时目录中注册测试用户并直接写入相同 schema v6 合成数据，随后先校验列表总数、摘要不含正文和 2 MiB 详情长度，再在每次采样前发起 8 个并发列表、账户列表、详情和统计请求。合成账户没有同步策略，且两边都显式关闭 IDLE，避免资源对照访问网络或把失败重连记入 HTTP 查询成本。

## 结果

正式报告保存在被 Git 忽略的 `output/rust-migration-tests/r6-service-resource-comparison-15s-v2.json`，SHA-256：

`772c47e712fee3852fc2c1f0906fa82608ae284d4cd20fda22eca985afb510cc`

| 拓扑 | 实现 | 首次 Working Set | 末次 | 中位数 | 峰值 | 峰值进程数 |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| HTTP 宿主 | Node | 135,659,520 B | 120,090,624 B | 114.53 MiB | 129.46 MiB | 2 |
| HTTP 宿主 | Rust | 21,929,984 B | 21,897,216 B | 20.88 MiB | 20.91 MiB | 2 |
| HTTP + 同步 | Node | 226,213,888 B | 204,296,192 B | 196.95 MiB | 216.11 MiB | 3 |
| HTTP + 同步 | Rust | 22,794,240 B | 22,814,720 B | 21.76 MiB | 21.76 MiB | 2 |

在该口径下：

- Rust HTTP 宿主峰值 Working Set 比 Node 低 83.85%，中位数低 81.77%。
- Rust 完整拓扑峰值比 Node 父子进程树低 89.93%，中位数低 88.95%。
- Rust 启用 3 个同步 worker 后相对仅宿主峰值只增加约 0.85 MiB；Node 独立 worker 使完整拓扑峰值增加约 86.65 MiB。
- 四组退出码均为 0，停机后遗留进程数为 0。

## 同时发现并修复的问题

首次采样发现 Node 生产包的 `imail-worker.cjs` 仍以源码文件名 `worker.ts` 判断是否为入口，导致打包后立即正常退出、父进程每秒重启。`server/sync/worker.ts` 现同时识别启动器注入的 `IMAIL_SYNC_WORKER_MODE=child`，并有单元测试覆盖源码入口、打包 child 入口和 disabled 模式。

Windows 上 Node 对自身调用 `process.kill(pid, 'SIGTERM')` 会直接以退出码 1 终止，绕过已安装的异步 close 流程。守护停服现在通过显式注册的宿主 shutdown handler 调用 `server.close`，复验中 Node API、Node worker 和 Rust 宿主均退出码 0，且无遗留进程。

## 大邮件查询增量结果

正式增量报告保存在 `output/rust-migration-tests/r6-service-resource-large-mail-5s-v2.json`，SHA-256：

`cafe9a5be3e69039684675175e9942132bd8e1171093f98e49f47855659c86bc`

口径为 5 个账户、500 封缓存邮件、每封 16 KiB 文本加 16 KiB HTML、单封 2 MiB 详情；预热后执行 6 批、每批 8 个并发请求，四组各返回约 41 MiB HTTP 数据。完整拓扑保留 3 个 worker slot，但关闭 IDLE，不访问任何邮箱网络。

| 拓扑 | 实现 | 中位 Working Set | 峰值 | 末次 | 返回数据 |
| --- | --- | ---: | ---: | ---: | ---: |
| HTTP 宿主 | Node | 239.45 MiB | 242.43 MiB | 242.43 MiB | 41.03 MiB |
| HTTP 宿主 | Rust | 24.95 MiB | 25.46 MiB | 24.86 MiB | 41.06 MiB |
| HTTP + 3 worker | Node | 609.49 MiB | 610.54 MiB | 325.64 MiB | 41.03 MiB |
| HTTP + 3 worker | Rust | 27.04 MiB | 27.40 MiB | 27.16 MiB | 41.06 MiB |

在这次短时压力口径下，Rust 仅宿主峰值比 Node 低 89.50%，完整拓扑峰值低 95.51%；中位数分别低 89.58% 与 95.56%。四组退出码均为 0、无遗留采样进程。完整 Node 组末次采样明显回落，说明 5 秒窗口包含 GC/页缓存波动，因此峰值和中位数只用于确认数量级差异，不能外推长期容量。

## 完整拓扑持续负载结果

采样器现支持 `--topology host|full|both`，报告首尾最多 10 个采样点的中位 Working Set 和漂移，并在负载结束后再次执行数据契约。可重复的五分钟入口为 `npm run rust:resource-compare:soak`；本轮先执行 60 个完整拓扑负载后采样点：

```powershell
node scripts/compare-service-resources.mjs `
  --workload large-mail `
  --topology full `
  --duration-seconds 60 `
  --message-count 500 `
  --body-bytes 16384 `
  --report output/rust-migration-tests/r6-service-resource-soak-60s-v3.json
```

报告 SHA-256：

`5c3770b6595bf62b027548d3a24308a19699df2f78b9476791d645f5a00f6707`

| 实现 | 采样点 | 实际采样跨度 | 中位 Working Set | 峰值 | 首窗口中位 | 末窗口中位 | 窗口漂移 | 返回数据 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Node 完整拓扑 | 60 | 106.27 s | 384.64 MiB | 555.77 MiB | 388.23 MiB | 362.01 MiB | -6.75% | 492.31 MiB |
| Rust 完整拓扑 | 60 | 96.37 s | 28.87 MiB | 36.31 MiB | 27.32 MiB | 32.96 MiB | +20.65% | 492.77 MiB |

两组均完成 61 批请求，负载前后都通过 500 封总数、列表正文裁剪和 2 MiB 详情长度契约；退出码均为 0、优雅停机且没有残留采样进程。Rust 峰值和中位 Working Set 分别比 Node 低 93.47% 和 92.49%。Rust 首尾窗口增加约 5.64 MiB，但采样值在约 29–36 MiB 间往复且窗口较短；这项证据只说明持续缓存读取期间保持低内存和正确性，不能把正漂移解释为泄漏，也不能据此宣称已经排除泄漏。`--duration-seconds` 对大邮件负载表示“负载后采样点数”；每点还包含 8 路请求完成时间，因此实际墙钟跨度大于参数值，并完整记录在 `samples[].elapsedMs`。

## 限制

本报告证明“当前 Node 常驻内存明显高于候选 Rust 服务”在同构 production HTTP 空闲负载下成立，但仍不是最终生产容量结论：

- 15 秒空闲对照与 60 点合成持续负载都不能替代数小时或数天的真实 IMAP IDLE、断网恢复和周期校准。
- 大邮件增量覆盖缓存列表与 2 MiB 正文响应，但隔离目录没有真实邮箱账户，因此仍没有 TLS、OAuth 刷新、远程 MIME 解析或附件下载峰值。
- 本机口径是 Windows Working Set；Linux 容器仍需在相同 cgroup 限额下记录 RSS、CPU 和 I/O。
- 本结果不能单独授权正式切换；真实邮箱和 `linux/amd64` 容器门禁仍必须完成。
