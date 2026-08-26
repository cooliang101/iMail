# imail-apple-hme

Apple Account、iCloud Web 与 Hide My Email 的独立 Rust 协议实现。

此 crate 不读取 iMail 数据库，也不自动保存会话。调用方必须把每次登录、刷新和 HME
操作后更新的 `AppleSession` 使用 iMail `master.key` 加密保存。

iMail 的正式接入位于 `imail-http::apple_hme`：schema v7 使用按邮箱账户归属的
`apple_hme_sessions` 表保存密文；schema v8 保存手动同步的地址快照。HTTP、桌面内嵌服务
与 `mcp:full` 控制面复用同一组管理操作。

## 会话能力

- `LoginStateKind::ICloudWeb`：HME 创建、列表、停用和永久删除。
- `LoginStateKind::AppleAccount`：Apple Account 新接口创建 HME。
- 完整管理通常需要分别完成两种登录，再通过 `AppleSession::merge` 合并。
- IMAP/SMTP 继续使用 iMail 已有的 Apple App 专用密码，不属于本 crate。

## 登录

```rust,no_run
use imail_apple_hme::{AppleAuthClient, LoginRequest};

let auth = AppleAuthClient::default();
let started = auth.start_login(LoginRequest::icloud_web(
    "owner@icloud.com",
    "apple-account-password",
))?;

let web_session = if let Some(session) = started.session {
    session
} else {
    auth.submit_two_factor(
        started.pending_id.as_deref().expect("pending id"),
        "123456",
        None,
    )?
};
# Ok::<(), imail_apple_hme::AppleHmeError>(())
```

Apple 主密码只用于 SRP 计算，不会放入 `AppleSession`。默认 pending store 在内存中保存
10 分钟；进程重启后需要重新登录。自定义 `PendingLoginStore` 如果把 payload 落盘，必须先
使用主密钥加密，因为 payload 含 Cookie、scnt 和 Session Token。

## HME

```rust,no_run
use imail_apple_hme::{
    AppleHmeClient, CreateChannel, CreateHmeRequest,
};
# let mut session = imail_apple_hme::AppleSession::empty("owner@icloud.com");

let client = AppleHmeClient::default();
let created = client.create(&mut session, CreateHmeRequest {
    label: "registration".into(),
    note: "created by iMail".into(),
    channel: CreateChannel::Auto,
})?;

let addresses = client.list(&mut session)?;
client.deactivate(&mut session, &created.address.anonymous_id)?;
# Ok::<(), imail_apple_hme::AppleHmeError>(())
```

`delete_inactive` 只允许删除已停用地址，避免把“停用”与不可恢复的永久删除混成一个操作。

## 协议边界

Apple 没有为这些网页端 HME 请求提供稳定的公共 API。User-Agent、Client Hints、build
number、Cookie 和响应结构都可能变化。生产接入必须保留 mock transport 契约测试，并把
真实账户验证放在显式启用的人工 canary 中，不能写入普通 CI。
