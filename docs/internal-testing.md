# 桌面内部测试构建

当前桌面交付只面向受控测试人员，不作为正式公开发行。Windows 产物是未配置商业代码签名的 NSIS 安装包；macOS 产物使用 ad-hoc 签名，以便验证应用、sidecar、同步 Worker 和 LaunchAgent 的完整运行链。

内部测试阶段不申请 Apple Developer 会员，不配置 Developer ID Application、Apple notarization、公开下载页或自动更新通道。CI 不创建 GitHub Release，只在 `main` 推送或手动运行时保存 14 天的 Actions 测试产物。

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

Pull Request 只执行构建与验证，不上传可分发产物。内测产物保留 14 天，也不会自动变成 Release。

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
