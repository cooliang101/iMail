import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

function json<T>(file: string) {
  return JSON.parse(readFileSync(file, 'utf8')) as T;
}

describe('desktop packaging configuration', () => {
  it('keeps web and server builds independently verifiable', () => {
    const manifest = json<{ scripts: Record<string, string> }>('package.json');
    expect(manifest.scripts.build).toBe('tsc -b && vite build');
    expect(manifest.scripts['build:web']).not.toContain('tauri');
    expect(manifest.scripts['build:server']).not.toContain('tauri');
    expect(manifest.scripts['build:service-runtime']).toContain('build-service-runtime.mjs');
    expect(readFileSync('scripts/build-service-runtime.mjs', 'utf8')).toContain('pc-windows-msvc');
    expect(manifest.scripts['build:desktop']).toContain('tauri build');
    expect(manifest.scripts['build:desktop:windows']).toContain('x86_64-pc-windows-msvc');
  });

  it('packages the managed service executable without embedding mutable data', () => {
    const config = json<{ identifier: string; build: { beforeBuildCommand: string }; app: { windows: unknown[] }; bundle: { externalBin?: string[]; resources?: Record<string, string> } }>('src-tauri/tauri.conf.json');
    expect(config.identifier).toBe('com.cooliang.imail');
    expect(config.build.beforeBuildCommand).toContain('npm run build:web');
    expect(config.build.beforeBuildCommand).toContain('npm run build:service-runtime');
    expect(config.app.windows).toHaveLength(1);
    expect(config.bundle.externalBin).toEqual(['binaries/imail-service']);
    expect(config.bundle.resources).toBeUndefined();
  });

  it('defines a user-level daemon lifecycle without installing a system service', () => {
    const runtime = readFileSync('src-tauri/src/local_service.rs', 'utf8');
    const main = readFileSync('src-tauri/src/main.rs', 'utf8');
    expect(runtime).toContain('HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run');
    expect(runtime).toContain('Library/LaunchAgents');
    expect(runtime).toContain('local_service_enable');
    expect(runtime).toContain('local_service_pause');
    expect(runtime).toContain('local_service_remove');
    expect(runtime).toContain('supervisor_id');
    expect(runtime).toContain('supervisor_lock_file');
    expect(runtime).toContain('imail-service-manager.exe');
    expect(runtime).toContain('--imail-daemon');
    expect(main).toContain('run_local_service_daemon_from_args');
    expect(runtime).not.toContain('HKLM\\');
    expect(runtime).not.toContain('LaunchDaemons');
  });

  it('builds the remote API, worker and same-origin web client as one release unit', () => {
    const manifest = json<{ scripts: Record<string, string> }>('package.json');
    const dockerfile = readFileSync('Dockerfile', 'utf8');
    const server = readFileSync('server/app.ts', 'utf8');
    expect(manifest.scripts['build:remote']).toContain('build:web');
    expect(manifest.scripts['build:remote']).toContain('build:remote-runtime');
    const runtimeBuilder = readFileSync('scripts/build-remote-runtime.mjs', 'utf8');
    expect(runtimeBuilder).toContain('imail-backup.mjs');
    expect(runtimeBuilder).toContain('imail-upgrade-preflight.mjs');
    expect(runtimeBuilder).toContain('imail-restore.mjs');
    expect(dockerfile).toContain('IMAIL_WEB_DIST=/app/dist');
    expect(dockerfile).toContain('IMAIL_BACKUP_DIR=/backups');
    expect(dockerfile).toContain('VOLUME ["/data", "/backups"]');
    expect(dockerfile).toContain('USER imail');
    expect(server).toContain('installWebClient');
  });

  it('ships a loopback-only bare compose and a TLS reverse-proxy deployment', () => {
    const bareCompose = readFileSync('compose.example.yml', 'utf8');
    const httpsCompose = readFileSync('compose.https.example.yml', 'utf8');
    const caddy = readFileSync('deploy/Caddyfile', 'utf8');
    expect(bareCompose).toContain('127.0.0.1:8787:8787');
    expect(httpsCompose).not.toMatch(/imail:[\s\S]*?ports:\s*\n\s*- ["']?8787:8787/);
    expect(httpsCompose).toContain('IMAIL_TRUST_PROXY: "true"');
    expect(httpsCompose).toContain('OAUTH_CALLBACK_BASE_URL: https://${IMAIL_PUBLIC_HOST');
    expect(httpsCompose).toContain('./deploy/Caddyfile:/etc/caddy/Caddyfile:ro');
    expect(caddy).toContain('reverse_proxy imail:8787');
    expect(caddy).toContain('flush_interval -1');
  });

  it('defines a Windows NSIS package and a hardened macOS DMG', () => {
    const windows = json<{ bundle: { targets: string[]; windows: { nsis: { installMode: string } } } }>('src-tauri/tauri.windows.conf.json');
    const macos = json<{ bundle: { targets: string[]; macOS: { minimumSystemVersion: string; hardenedRuntime: boolean; entitlements: string } } }>('src-tauri/tauri.macos.conf.json');
    const entitlements = readFileSync('src-tauri/Entitlements.plist', 'utf8');
    const macosSmoke = readFileSync('scripts/smoke-macos-bundle.mjs', 'utf8');
    const launchAgentSmoke = readFileSync('scripts/smoke-macos-launch-agent.mjs', 'utf8');
    expect(windows.bundle.targets).toEqual(['nsis']);
    expect(windows.bundle.windows.nsis.installMode).toBe('currentUser');
    expect((windows.bundle.windows.nsis as { installerHooks?: string }).installerHooks).toBe('./windows/hooks.nsh');
    expect((windows.bundle as { externalBin?: string[] }).externalBin).toEqual(['binaries/imail-service', 'binaries/imail-service-manager']);
    const hooks = readFileSync('src-tauri/windows/hooks.nsh', 'utf8');
    const windowsInstallerSmoke = readFileSync('scripts/smoke-windows-installer.mjs', 'utf8');
    expect(hooks).toContain('NSIS_HOOK_PREUNINSTALL');
    expect(hooks).toContain('imail-service-manager.exe');
    expect(hooks).toContain('--imail-uninstall-cleanup');
    expect(hooks).toContain('/SD IDOK');
    expect(windowsInstallerSmoke).toContain("const startupRegistryValue = 'iMailService'");
    expect(windowsInstallerSmoke).toContain('uninstallRemovedUserStartup: true');
    expect(macos.bundle).toMatchObject({ targets: ['dmg'], macOS: { minimumSystemVersion: '11.0', hardenedRuntime: true, entitlements: 'Entitlements.plist' } });
    expect(entitlements).toContain('com.apple.security.cs.allow-jit');
    expect(entitlements).not.toContain('com.apple.security.cs.allow-unsigned-executable-memory');
    expect(entitlements).toContain('com.apple.security.network.client');
    expect(entitlements).toContain('com.apple.security.network.server');
    expect(macosSmoke).toContain("['--verify', '--deep', '--strict', '--verbose=2', appBundle]");
    expect(macosSmoke).toContain("['--display', '--entitlements', ':-', service]");
    expect(macosSmoke).toContain("match[3].includes('--sync-worker')");
    expect(macosSmoke).toContain('/api/system/shutdown');
    expect(macosSmoke).toContain("import('./smoke-macos-launch-agent.mjs')");
    expect(launchAgentSmoke).toContain("['bootstrap', domain, plist]");
    expect(launchAgentSmoke).toContain("['bootout', domain, plist]");
    expect(launchAgentSmoke).toContain("process.kill(firstApi.pid, 'SIGKILL')");
    expect(launchAgentSmoke).toContain('拒绝覆盖');
  });

  it('gates native installers and the remote container on their real platforms', () => {
    const workflow = readFileSync('.github/workflows/deployment-release.yml', 'utf8');
    expect(workflow).toContain('runs-on: windows-latest');
    expect(workflow).toContain('runner: macos-15');
    expect(workflow).toContain('runner: macos-15-intel');
    expect(workflow).toContain("APPLE_SIGNING_IDENTITY: '-'");
    expect(workflow).toContain('cargo test --manifest-path src-tauri/Cargo.toml --lib --target x86_64-pc-windows-msvc');
    expect(workflow).toContain('cargo test --manifest-path src-tauri/cleanup-helper/Cargo.toml --locked');
    expect(workflow).toContain('npm run test:local-daemon');
    expect(workflow).toContain('npm run test:windows-installer');
    expect(workflow).toContain('npm run test:macos-bundle');
    expect(workflow).toContain('npm run test:container-release');
    expect(workflow).toContain('src-tauri/target/release/bundle/dmg/*.dmg');
    expect(workflow).toContain('src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/*.exe');
  });

  it('keeps one desktop instance and hides the main window to the system tray on close', () => {
    const manifest = readFileSync('src-tauri/Cargo.toml', 'utf8');
    const runtime = readFileSync('src-tauri/src/lib.rs', 'utf8');
    expect(manifest).toContain('tauri-plugin-single-instance');
    expect(manifest).toContain('features = ["tray-icon"]');
    expect(runtime).toContain('tauri_plugin_single_instance::init');
    expect(runtime).toContain('api.prevent_close()');
    expect(runtime).toContain('window.hide()');
    expect(runtime).toContain('TrayIconBuilder::with_id("main")');
    expect(runtime).toContain('MenuItem::with_id(app, "compose", "写邮件"');
    expect(runtime).toContain('app.emit("desktop-compose", ())');
    expect(runtime).toContain('"quit" => app.exit(0)');
  });
});
