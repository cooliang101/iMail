import { access, readFile, readdir } from 'node:fs/promises';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

const workspaceRoot = path.resolve(import.meta.dirname, '..');

async function exists(relativePath: string) {
  return access(path.join(workspaceRoot, relativePath)).then(() => true, () => false);
}

describe('frontend workspace layout', () => {
  it('keeps the complete npm and Vite project under frontend', async () => {
    for (const required of [
      'frontend/src',
      'frontend/public',
      'frontend/index.html',
      'frontend/package.json',
      'frontend/package-lock.json',
      'frontend/tsconfig.json',
      'frontend/tsconfig.app.json',
      'frontend/tsconfig.tools.json',
      'frontend/vite.config.ts',
    ]) {
      expect(await exists(required), required).toBe(true);
    }
    for (const forbidden of ['src', 'public', 'package.json', 'package-lock.json', 'node_modules', 'dist']) {
      expect(await exists(forbidden), forbidden).toBe(false);
    }
  });

  it('keeps the frontend source root limited to application entry contracts', async () => {
    const sourceRoot = path.join(workspaceRoot, 'frontend', 'src');
    const entries = await readdir(sourceRoot, { withFileTypes: true });
    const rootFiles = entries.filter((entry) => entry.isFile()).map((entry) => entry.name).sort();
    expect(rootFiles).toEqual([
      'App.tsx',
      'app-model.ts',
      'main.tsx',
      'raw-imports.d.ts',
      'styles.css',
      'theme.css',
      'theme.ts',
      'types.ts',
      'vite-env.d.ts',
    ]);
    for (const required of [
      'frontend/src/app',
      'frontend/src/components',
      'frontend/src/config',
      'frontend/src/features',
      'frontend/src/platform',
      'frontend/src/services',
      'frontend/src/services/desktop',
      'frontend/src/services/mail',
      'frontend/src/utils',
    ]) {
      expect(await exists(required), required).toBe(true);
    }
  });

  it('uses a root Rust workspace and keeps HTTP deployment isolated', async () => {
    for (const required of [
      'Cargo.toml',
      'Cargo.lock',
      'crates/imail-core',
      'crates/imail-http',
      'http-service/Cargo.toml',
      'http-service/src/main.rs',
      'http-service/Dockerfile',
      'http-service/compose.example.yml',
      'http-service/compose.https.example.yml',
      'http-service/deploy/Caddyfile',
    ]) {
      expect(await exists(required), required).toBe(true);
    }
    for (const forbidden of ['rust', 'Dockerfile', 'compose.example.yml', 'compose.https.example.yml', 'deploy']) {
      expect(await exists(forbidden), forbidden).toBe(false);
    }

    const workspaceManifest = await readFile(path.join(workspaceRoot, 'Cargo.toml'), 'utf8');
    expect(workspaceManifest).toContain('"src-tauri"');
    expect(workspaceManifest).toContain('"http-service"');
    expect(await exists('src-tauri/Cargo.lock')).toBe(false);
  });

  it('points Tauri, the standalone HTTP service and CI at the frontend workspace', async () => {
    const tauri = JSON.parse(await readFile(path.join(workspaceRoot, 'src-tauri', 'tauri.conf.json'), 'utf8'));
    expect(tauri.$schema).toBe('../frontend/node_modules/@tauri-apps/cli/config.schema.json');
    expect(tauri.build.frontendDist).toBe('../frontend/dist');
    expect(tauri.build.beforeBuildCommand).toBe('npm run build:web');

    const dockerfile = await readFile(path.join(workspaceRoot, 'http-service', 'Dockerfile'), 'utf8');
    expect(dockerfile).toContain('COPY frontend/package.json frontend/package-lock.json ./');
    expect(dockerfile).toContain('COPY --from=web-build /app/frontend/dist ./dist');

    const workflow = await readFile(
      path.join(workspaceRoot, '.github', 'workflows', 'deployment-release.yml'),
      'utf8',
    );
    expect(workflow).toContain('cache-dependency-path: frontend/package-lock.json');
    expect(workflow).toContain('npm ci --prefix frontend');
  });

  it('loads shell-affecting toast and workspace styles before lazy features render', async () => {
    const styles = await readFile(path.join(workspaceRoot, 'frontend', 'src', 'styles.css'), 'utf8');
    expect(styles).toContain("@import './styles/compose.css';");
    const composeStyles = await readFile(path.join(workspaceRoot, 'frontend', 'src', 'styles', 'compose.css'), 'utf8');
    expect(composeStyles).toContain('.toast {');
    expect(composeStyles).toContain('.token-workspace {');
  });

  it('keeps Hide My Email as an independent settings feature', async () => {
    const settings = await readFile(path.join(workspaceRoot, 'frontend', 'src', 'features', 'settings', 'SettingsModal.tsx'), 'utf8');
    const accounts = await readFile(path.join(workspaceRoot, 'frontend', 'src', 'features', 'accounts', 'AccountSettingsModal.tsx'), 'utf8');
    const hmeSettings = await readFile(path.join(workspaceRoot, 'frontend', 'src', 'features', 'settings', 'AppleHmeSettingsPanel.tsx'), 'utf8');
    const hmePanel = await readFile(path.join(workspaceRoot, 'frontend', 'src', 'features', 'apple-hme', 'AppleHmePanel.tsx'), 'utf8');
    const settingsStyles = await readFile(path.join(workspaceRoot, 'frontend', 'src', 'styles', 'settings.css'), 'utf8');
    expect(settings).toContain("id: 'apple-hme'");
    expect(settings).toContain('<AppleHmeSettingsPanel');
    expect(accounts).not.toContain('AppleHmePanel');
    expect(await exists('frontend/src/features/apple-hme/AppleHmePanel.tsx')).toBe(true);
    expect(hmePanel).toContain("scrollIntoView({ behavior: 'smooth', block: 'nearest' })");
    expect(hmePanel).toContain('appleAccountLastSuccessfulKeepaliveAt');
    expect(hmePanel).toContain('icloudWebLastSuccessfulKeepaliveAt');
    expect(hmePanel.indexOf('className="apple-hme-error"')).toBeLessThan(hmePanel.indexOf('className="apple-hme-login"'));
    expect(hmePanel).toContain("export type AppleHmeView = 'overview' | 'addresses' | 'create'");
    expect(hmePanel).toContain("if (view === 'addresses')");
    expect(hmePanel).toContain('<SettingsLinkRow');
    expect(hmePanel).not.toContain('className="apple-hme-view-switch"');
    expect(hmeSettings).toContain('icloudAccounts.map((account) => <AppleHmePanel');
    expect(hmeSettings).not.toContain('apple-hme-account-picker');
    expect(hmePanel).toContain('apple-hme-account-settings-list');
    expect(settingsStyles).toContain('.apple-hme-login-actions');
    expect(settingsStyles).toContain('.settings-panel-body { min-width: 0; min-height: 0; overflow-x: hidden; overflow-y: auto');
    expect(settingsStyles).toContain('.apple-hme-settings-panel .settings-panel-body { display: block');
    expect(settingsStyles).toContain('scrollbar-gutter: stable');
  });
});
