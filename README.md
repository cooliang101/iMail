# iMail

English | [简体中文](README_zh.md)

iMail is a local-first, multi-account email client. It brings Gmail, Outlook, QQ Mail, Yahoo Mail, iCloud Mail, and other IMAP accounts into one interface for reading, searching, composing, contact management, and everyday inbox organization.

The project is currently in internal testing. Its supported deliverables are the Windows x64 desktop app and the server Docker image. Linux is supported only as a server container environment; native Linux and macOS desktop builds are not maintained.

## Key features

- View and manage multiple email accounts in one inbox
- Support for Gmail, Outlook, Hotmail, QQ Mail, Yahoo Mail, iCloud Mail, and generic IMAP/SMTP accounts
- OAuth sign-in, app-specific passwords, and email authorization codes
- Continuous background mail delivery and synchronization while the window is hidden
- Search, stars, read status, archive, trash, and custom labels
- Compose, reply, forward, drafts, and attachment downloads
- Contacts, sender logos, and recipient suggestions
- Hide My Email address management for iCloud accounts
- Snooze, notification center, keyboard shortcuts, and multiple themes
- Per-account HTTP, HTTPS, or SOCKS5 proxy settings
- Optional API and MCP access for trusted tools and agents

## Two ways to use iMail

### Windows desktop app

The desktop app is designed for personal use on your own computer. Email data and credentials are stored locally. When the window is closed, iMail stays in the system tray and continues receiving mail; it stops only when you choose **Quit iMail**.

The desktop app can also connect to a self-hosted remote iMail service. Switching between local and remote mode changes only the data source—it does not automatically copy or merge data between the two instances.

### Docker server

The server edition is designed for deployment on a server or home device and is accessed through a browser. The image includes the web interface, mail service, and maintenance tools, and does not require Node.js at runtime.

Current image:

```text
ghcr.io/cooliang101/imail:edge
```

Authentication with GHCR may currently be required before pulling the image. For stable deployments, use a version tag, full commit tag, or digest instead of depending on `edge` indefinitely.

Run it locally:

```bash
docker compose -f http-service/compose.example.yml up -d
```

This example exposes the service only at `http://127.0.0.1:8787`. Public deployments must use HTTPS and persist `/data` and `/backups`. See the [operations runbook](docs/operator-runbook.md) for full instructions.

## Data and privacy

- Email passwords, authorization codes, and OAuth tokens are stored encrypted
- Email credentials are never returned through regular APIs, MCP, frontend logs, or error messages
- Desktop uninstallations and application upgrades preserve mail data by default
- A complete instance can be backed up and restored, including validation on a copy before an upgrade
- **Privacy & Data** can clear the current user's email data without affecting other users
- Account credentials can be exported to a separately password-protected file; message bodies are not included

iMail is a local-first product and does not provide automatic multi-device data synchronization. Local desktop instances and remote service instances remain independent.

## Agents and external integrations

The remote service can optionally enable an API Gateway or MCP endpoint, allowing trusted applications to read and send email, manage accounts, and trigger synchronization. Each authorization code has its own purpose, expiration, and revocation control.

These interfaces are disabled by default. MCP account-management features accept only a dedicated `mcp:full` authorization code. See the [MCP integration guide](docs/mcp-integration.md) for details.

## Supported deliverables

| Target | Status |
| --- | --- |
| Windows x64 desktop app | Supported and currently used for internal testing |
| Docker `linux/amd64` | Published to GHCR; deployment and security validation are ongoing |

## Local development

Node.js 22.5+, npm, and Rust are required. The frontend workspace is located exclusively in `frontend/`.

```bash
npm ci --prefix frontend
npm --prefix frontend run dev
```

Open `http://localhost:5173` in a browser. The development server listens only on localhost by default and is not directly exposed to the local network.

Windows desktop development:

```bash
npm --prefix frontend run dev:desktop
npm --prefix frontend run build:desktop:internal
```

Checks to run before committing:

```bash
npm --prefix frontend run typecheck
npm --prefix frontend test
npm --prefix frontend run build
```

Full Docker release validation:

```bash
npm --prefix frontend run test:container-release
```

## Project structure

```text
frontend/       Shared user interface
crates/         Mail, storage, sync, security, and integration capabilities
src-tauri/      Windows desktop application
http-service/   Docker and remote service entry point
docs/           Architecture, deployment, operations, and development plans
```

## Documentation

- [Documentation index](docs/README.md)
- [Deployment modes](docs/deployment-modes.md)
- [Operations runbook](docs/operator-runbook.md)
- [MCP integration guide](docs/mcp-integration.md)
- [Architecture](docs/architecture.md)
- [Internal testing](docs/internal-testing.md)
