# 桌面内部测试构建

当前桌面交付只面向受控测试人员，不作为正式公开发行。Windows 产物是未配置商业代码签名的 NSIS 安装包；macOS 产物使用 ad-hoc 签名，以便验证应用、sidecar、同步 Worker 和 LaunchAgent 的完整运行链。

内部测试阶段不申请 Apple Developer 会员，不配置 Developer ID Application、Apple notarization、公开下载页或自动更新通道。CI 在 `main` 推送或对 `main` 手动运行成功后创建一条 Draft Release，并同时保存 14 天的 Actions 测试产物；草稿不会作为正式版本发布。

## 本机构建

构建机先安装 Node.js 22.5+、npm、Rust stable 和对应平台的原生工具链，然后执行：

```bash
npm ci
npm run build:desktop:internal
```

命令会按当前操作系统选择构建目标：

- Windows x64：生成 `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/*.exe`。
- macOS：强制使用 ad-hoc 签名并生成 `src-tauri/target/release/bundle/dmg/*.dmg`；构建机需要 Xcode Command Line Tools。
- Linux：当前不生成桌面内部测试包；远程服务仍可在 Linux 构建和运行。

提交测试包前还应执行项目门禁：

```bash
npm run typecheck
npm test
npm run build
```

Windows 构建机可运行完整内部发布检查：

```bash
npm run test:internal-release
```

macOS 构建后运行：

```bash
npm run test:macos-bundle
```

## CI 产物

GitHub Actions 的 [`Internal test verification`](https://github.com/cooliang101/iMail/actions/workflows/deployment-release.yml) 会验证远程运行时、容器、Windows 用户级守护进程及 macOS arm64/x64 包。在 `main` 推送或手动运行完成后，打开成功的任务并从页面底部的 Artifacts 下载：

- `imail-windows-x64-internal-test`
- `imail-macos-arm64-internal-test`
- `imail-macos-x64-internal-test`

Pull Request 只执行构建与验证，不上传可分发产物。功能分支手动任务的 Artifacts 保留 14 天，不创建 Release；只有 `main` 的成功任务会额外汇总为 Draft Release。

## Draft Release

`main` 的远程运行时、容器、Windows 和两个 macOS 架构任务全部通过后，汇总任务会从本次 CI 下载三个平台 Artifacts，生成 `SHA256SUMS.txt`，然后创建唯一的内部测试 Draft Release。版本标签格式为 `internal-v<版本>-build.<任务号>.<重试号>`。

从仓库的 [Releases 页面](https://github.com/cooliang101/iMail/releases)进入对应草稿，可以直接下载 `.exe`、arm64/x64 `.dmg` 和校验文件。只有具备仓库写权限的协作者负责查看、测试、删除或人工发布草稿；不要把内部测试草稿发布成正式 Release。

Draft Release 资产不像 Actions Artifact 那样按 14 天自动过期。确认某次构建不再使用后，应在 GitHub Release 页面删除对应草稿；CI 不自动删除旧草稿或已有资产。

## 标签测试版本

形如 `0.0.1` 的三段式版本标签会触发同一套完整跨平台门禁。标签必须与 `package.json`、Tauri 和 Rust 包版本完全一致；全部任务成功后，CI 将三个平台安装包和 `SHA256SUMS.txt` 发布为 GitHub Pre-release，并明确标注未配置 Windows 商业签名、Developer ID 或 Apple notarization。该版本不会被标记为 Latest。

## 安装限制

Windows 可能显示 SmartScreen 提示；只在确认文件来自本项目的受控测试人员中继续安装。macOS 会因为没有 Developer ID 与 notarization 而显示“无法验证开发者”；测试人员可在 Finder 中右键应用选择“打开”，或在“系统设置 → 隐私与安全性”中允许本次打开。不要为了内测全局关闭 Gatekeeper。

ad-hoc 签名只能验证内部构建链，不能证明发行者身份，也不适合交给不受控用户。正式公开发行前仍需单独完成平台证书、Apple notarization、发布渠道和升级签名设计。

## 服务模式测试边界

- 本地服务模式不需要 HTTPS。桌面应用连接回环地址上的用户级守护进程，桌面 UI 退出后守护进程继续同步。
- 同一台机器上的远程运行时开发测试可使用 `http://localhost`、`127.0.0.0/8` 或 `::1` 回环地址。
- 另一台设备连接远程服务时，即使属于内部测试，也必须使用客户端信任的 HTTPS。可以使用内部 DNS 与受信任的内部 CA，不要求现在建设公网正式域名；非回环 HTTP 会在前端和 Rust 网络桥两层被拒绝。
- 本地和远程实例仍是两份独立数据，切换模式不会迁移或合并数据。

## 内测验收重点

1. 全新安装选择本地服务后，守护进程能启动并通过身份握手。
2. 退出桌面 UI 后继续同步，重新打开后连接同一实例。
3. 暂停、切换远程、切回本地和移除运行文件的状态一致，移除默认保留数据。
4. Windows 注销登录和 macOS 注销登录后，用户级守护进程按平台注册项恢复。
5. 测试结束记录操作系统、CPU 架构、安装包来源、应用版本、复现步骤和日志摘要；日志不得包含邮箱凭据或 Token。
