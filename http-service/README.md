# iMail HTTP service

该目录是 iMail 的独立 HTTP 部署入口，不包含第二套业务实现。

- `src/main.rs`：装配并启动 HTTP bridge。
- `Dockerfile`、`compose*.yml`、`deploy/`：容器与 HTTPS 部署配置。
- 通用 Web API、MCP、Gateway 与 Web 托管实现位于 `crates/imail-http/`。
- 账户、存储、邮件、OAuth 和同步等底层服务位于 `crates/`，Windows Tauri 应用与本目录共同复用。

从仓库根目录运行：

```powershell
cargo run --locked -p imail-http-service -- --host 127.0.0.1 --port 8787
docker build --file http-service/Dockerfile --tag imail-http-service .
docker compose -f http-service/compose.example.yml up -d
```

独立服务入口默认启用 HTTP；`--host` 和 `--port` 可省略，默认监听 `127.0.0.1:8787`。Gateway 与 MCP 的开关、授权和领域行为继续由通用 Rust 服务实现维护。

GitHub Actions 的 `Build Windows or publish Docker` 工作流可在 `main` 手动选择 `docker`，发布 `ghcr.io/cooliang101/imail:edge` 和 `sha-<完整提交>`；推送与 `frontend/package.json` 版本一致的三段式 Git tag 时发布版本标签和提交标签。发布 job 会先运行完整容器生命周期 smoke，再以 `packages: write` 的 `GITHUB_TOKEN` 推送带 SBOM 和 provenance 的 `linux/amd64` 镜像。首次发布后是否公开由 GHCR 包设置控制。
