# Rust 模块职责边界

拆分以职责和变化原因为依据。700～800 行是检查信号，不是强制上限；优先处理同时承担配置、协议、生命周期和测试的入口文件。先在现有 crate 内建立私有模块，只有出现独立复用或依赖隔离需求时才新增 crate。

## HTTP 适配层

`crates/imail-http/src/lib.rs` 保留公开导出、领域模块声明及私有共享状态，既有 `imail_http::…` 调用路径不变。

| 模块 | 职责 |
| --- | --- |
| `config.rs` | 启动模式、配置构建和 Host / Origin 校验 |
| `ports.rs` | 可注入的邮件连接接口及默认网络、OAuth 工厂 |
| `error.rs` | HTTP 宿主和嵌入式操作的公开错误类型 |
| `instance.rs` | 数据目录的持久实例身份 |
| `boundary.rs` | Host、代理头、CORS 和安全响应头 |
| `router.rs` | 共享状态初始化、领域路由组合和中间件装配 |
| `host.rs` | TCP 服务启停及两种宿主共用的同步运行时启动 |
| `embedded.rs` | 原生调用门面、运行时所有权及关闭 |
| `tests/` | 按账户、邮件、鉴权、MCP 等领域组织的契约测试；`mod.rs` 提供共享测试夹具 |

领域处理器继续调用 core 应用逻辑。配置和依赖工厂不依赖路由或宿主；路由不依赖宿主；TCP 与嵌入式宿主复用相同路由和应用状态。内部协作使用私有或 `pub(super)` 可见性，不增加外部 API。

## 邮件网络适配层

`crates/imail-mail-network/src/lib.rs` 保留 `NetworkMailAdapter` 的构建、统一超时和取消执行，以及既有公开导出。

| 模块 | 职责 |
| --- | --- |
| `imap.rs` | IMAP 连接认证、原文定位、标记修改和移动 |
| `sync.rs` | 增量拉取、游标协调及 IDLE / 轮询唤醒 |
| `smtp.rs` | SMTP 连接认证、MIME 构建和信封投递 |
| `tunnel.rs` | 直连、SOCKS5、HTTP CONNECT 和底层流 |
| `tls.rs` | 信任根与 TLS 握手 |
| `error.rs` | 网络错误到邮件协议错误的转换 |
| `tests.rs` | 本地 TLS 服务夹具及跨协议、超时、取消测试 |

同步复用 IMAP 会话；IMAP 和 SMTP 复用隧道与 TLS。协议细节测试留在所属模块内，跨协议测试使用本地夹具。生产默认工厂与测试注入点保持一致，不引入真实账户验证。

## 回归测试

此次拆分的回归测试分为模块内边界测试和跨模块契约测试：

| 测试位置 | 补充覆盖 |
| --- | --- |
| `imail-http/src/config.rs` | 启动参数错误、Host 规范化、Origin 中的凭据及路径、远端明文 Origin 拒绝 |
| `imail-http/src/instance.rs` | UUID v4 首次持久化及重读、损坏身份不覆盖、读取规范化不改写文件 |
| `imail-http/src/tests/boundary_contracts.rs` | CORS 预检不能绕过 Host / 代理校验、IPv6 和端口、缺失 Host、安全响应头 |
| `imail-mail-network/src/tunnel.rs` | IPv6 CONNECT、代理头后首段数据不被吞掉、响应大小边界及截断响应 |
| `imail-mail-network/src/sync.rs` | 显式邮箱缺失或不可选择时禁止回退到其他邮箱 |
| `imail-mail-network/src/tests.rs` | 操作已经启动后触发取消，并确认正在执行的 future 被释放 |

运行 `cargo test -p imail-http -p imail-mail-network --offline` 验证上述边界及已有跨模块契约。网络测试使用本地 TLS 夹具或内存双工流，不依赖真实邮箱。

## 后续维护

- 新行为放入负责该协议或领域的模块，入口只装配和导出。
- 保留现有 port / adapter 和门面模式，不为文件拆分增加无实际替换需求的 trait 或泛型层。
- 共用代码按业务含义命名，避免堆入 `utils.rs`、`common.rs`。
- 优先检查其他大文件中的独立职责，例如 MCP 工具目录与分发、SQLite 同步队列与调度；职责统一的实现可以继续保留。
- 模块迁移必须保留既有测试、公开签名和协议契约，并检查 `include_str!` 等相对路径及文档引用。
