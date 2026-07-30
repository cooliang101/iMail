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

  it('bundles the Node sidecar and both desktop runtime resource trees', () => {
    const config = json<{ identifier: string; bundle: { externalBin: string[]; resources: Record<string, string> } }>('src-tauri/tauri.conf.json');
    expect(config.identifier).toBe('com.cooliang.imail');
    expect(config.bundle.externalBin).toEqual(['binaries/imail-node']);
    expect(config.bundle.resources).toMatchObject({ '../desktop-runtime/': 'desktop-runtime/', '../dist/': 'web/' });
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
});
