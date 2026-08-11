# Windows 桌面打包与 Rust 嵌入边界

Windows 桌面版复用 React 前端并把 Rust 领域服务静态链接进 Tauri。安装包不携带 Node、SEA、服务 sidecar、manager 或独立 worker。

## 运行边界

- 本地模式：WebView 只使用类型化 command/event；Rust host 持有会话、SQLite、凭据、网络协议和同步生命周期。
- 远程模式：WebView 仍不直接访问网络会话，由 Rust bridge 连接经校验的 HTTPS 服务。
- 本地模式无服务 URL、Cookie Jar、SSE 或业务 listener；OAuth 只使用短生命周期随机 loopback callback。
- 窗口关闭隐藏到托盘并继续同步；托盘“退出”关闭事件任务和嵌入式 host 后结束进程。

## 包内容

- `tauri.conf.json` 的 `beforeBuildCommand` 只构建 Web。
- `externalBin` 为空；NSIS 安装目录只有 `imail.exe`、卸载程序与必需资源。
- 安装包扫描必须拒绝 `node.exe`、`.cjs`、`imail-service`、manager、worker 和 `service-runtime`。
- 交付平台只有 Windows x64；不生成 macOS 或原生 Linux 桌面包。

## 数据与卸载

- 用户数据继续位于 `%LOCALAPPDATA%\com.cooliang.imail\local-service\data`，避免覆盖升级改变既有路径。
- 覆盖安装与默认卸载保留数据库、主密钥、Logo、日志和迁移快照。
- 卸载 hook 调用当前 `imail.exe --imail-uninstall-cleanup`，只清理旧 runtime/启动注册等受管文件。
- 删除邮箱数据只能通过登录后的“隐私与数据”流程，并受重新认证和固定确认文本保护。

## 旧版迁移

首次发现受管 `daemon.json` 且尚无成功记录时，Tauri 执行一次性单写入者切换：停止并确认旧 Node 服务、创建不可覆盖快照、验证数据库/主密钥/全部凭据、无网络启动 Rust host，并写入 `embedded-switch.json`。失败时保留现场；只有数据库哈希未变才允许恢复旧服务。

退休的旧守护设置管理面已从默认 release 编译图移出。源码中的 `legacy-daemon-admin` feature 仅用于过渡期审计旧状态/启用/移除和 sidecar 部署逻辑；正式 Windows 包不启用它。首次升级仍依赖的停服、非覆盖快照、失败恢复、旧 supervisor 与卸载清理不受该 feature 控制，并保留一个发布周期。

完成一个回退发布周期前保留该兼容代码和旧 runtime 文件；它们不被调用为当前服务，也不进入新安装包。

## 验收

构建前从仓库根目录运行 `npm --prefix frontend run typecheck`、`npm --prefix frontend test`、`npm --prefix frontend run build`、Rust/Tauri tests、严格 Clippy 与 rustfmt。随后构建 NSIS，执行 release 冒烟、归档扫描、进程/端口检查及数据哈希/快照复核。当前结果见 [`rust-migration-r9-report.md`](./rust-migration-r9-report.md)。
