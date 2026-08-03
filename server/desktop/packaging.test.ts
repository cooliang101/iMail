import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

function json<T>(file: string) {
  return JSON.parse(readFileSync(file, 'utf8')) as T;
}

describe('desktop packaging configuration', () => {
  it('keeps web and server builds independent from the Tauri build', () => {
    const manifest = json<{ scripts: Record<string, string> }>('package.json');
    expect(manifest.scripts.build).toBe('tsc -b && vite build');
    expect(manifest.scripts['build:web']).not.toContain('tauri');
    expect(manifest.scripts['build:server']).not.toContain('tauri');
    expect(manifest.scripts['build:desktop']).toContain('tauri build');
  });

  it('packages only the client and does not embed the Node service', () => {
    const config = json<{ identifier: string; build: { beforeBuildCommand: string }; app: { windows: unknown[] }; bundle: { externalBin?: string[]; resources?: Record<string, string> } }>('src-tauri/tauri.conf.json');
    expect(config.identifier).toBe('com.cooliang.imail');
    expect(config.build.beforeBuildCommand).toBe('npm run build:web');
    expect(config.app.windows).toHaveLength(1);
    expect(config.bundle.externalBin).toBeUndefined();
    expect(config.bundle.resources).toBeUndefined();
  });

  it('defines a Windows NSIS package and a hardened macOS DMG', () => {
    const windows = json<{ bundle: { targets: string[]; windows: { nsis: { installMode: string } } } }>('src-tauri/tauri.windows.conf.json');
    const macos = json<{ bundle: { targets: string[]; macOS: { minimumSystemVersion: string; hardenedRuntime: boolean; entitlements: string } } }>('src-tauri/tauri.macos.conf.json');
    const entitlements = readFileSync('src-tauri/Entitlements.plist', 'utf8');
    expect(windows.bundle.targets).toEqual(['nsis']);
    expect(windows.bundle.windows.nsis.installMode).toBe('currentUser');
    expect(macos.bundle).toMatchObject({ targets: ['dmg'], macOS: { minimumSystemVersion: '11.0', hardenedRuntime: true, entitlements: 'Entitlements.plist' } });
    expect(entitlements).toContain('com.apple.security.cs.allow-jit');
    expect(entitlements).toContain('com.apple.security.network.client');
    expect(entitlements).toContain('com.apple.security.network.server');
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
