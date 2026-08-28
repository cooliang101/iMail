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
    expect(tauri.app.windows[0].generalAutofillEnabled).toBe(false);
    expect(tauri.app.windows[0].minWidth).toBeGreaterThanOrEqual(1024);
    expect(tauri.app.windows[0].minHeight).toBeGreaterThanOrEqual(680);

    const appInput = await readFile(path.join(workspaceRoot, 'frontend', 'src', 'components', 'form-controls', 'app-input.tsx'), 'utf8');
    const appTextarea = await readFile(path.join(workspaceRoot, 'frontend', 'src', 'components', 'form-controls', 'app-textarea.tsx'), 'utf8');
    expect(appInput).toContain('autoComplete="off"');
    expect(appInput).toContain('data-form-type="other"');
    expect(appTextarea).toContain('autoComplete="off"');

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

  it('uses the shared custom select instead of native select controls', async () => {
    const sourceRoot = path.join(workspaceRoot, 'frontend', 'src');
    const entries = await readdir(sourceRoot, { recursive: true });
    const sourceFiles = entries.filter((entry) => typeof entry === 'string' && entry.endsWith('.tsx'));
    const contents = await Promise.all(sourceFiles.map((entry) => readFile(path.join(sourceRoot, entry), 'utf8')));
    expect(contents.join('\n')).not.toMatch(/<select\b/i);

    const appSelect = await readFile(path.join(sourceRoot, 'components', 'form-controls', 'app-select.tsx'), 'utf8');
    expect(appSelect).toContain('role="listbox"');
    expect(appSelect).toContain('createPortal');
    expect(appSelect).toContain('type="hidden"');
  });

  it('uses switches for immediate boolean settings while preserving confirmation checkboxes', async () => {
    const sourceRoot = path.join(workspaceRoot, 'frontend', 'src');
    const switchControl = await readFile(path.join(sourceRoot, 'components', 'form-controls', 'app-switch.tsx'), 'utf8');
    const general = await readFile(path.join(sourceRoot, 'features', 'settings', 'GeneralPanel.tsx'), 'utf8');
    const notifications = await readFile(path.join(sourceRoot, 'features', 'settings', 'NotificationPanel.tsx'), 'utf8');
    const authorizationExport = await readFile(path.join(sourceRoot, 'features', 'settings', 'AuthorizationExport.tsx'), 'utf8');
    expect(switchControl).toContain('role="switch"');
    expect(switchControl).toContain('return <label');
    expect(switchControl).not.toContain('return <span');
    expect(general).toContain('<AppSwitch');
    expect(notifications).toContain('<AppSwitch');
    expect(general).not.toContain('<AppCheckbox');
    expect(notifications).not.toContain('<AppCheckbox');
    expect(authorizationExport).toContain('<AppCheckbox');
  });

  it('keeps translation actions in the message header instead of covering the body', async () => {
    const sourceRoot = path.join(workspaceRoot, 'frontend', 'src');
    const reader = await readFile(path.join(sourceRoot, 'features', 'mail', 'MessageReader.tsx'), 'utf8');
    const settingsModal = await readFile(path.join(sourceRoot, 'features', 'settings', 'SettingsModal.tsx'), 'utf8');
    const translationSettings = await readFile(path.join(sourceRoot, 'features', 'translation', 'TranslationSettingsPanel.tsx'), 'utf8');
    const mailStyles = await readFile(path.join(sourceRoot, 'styles', 'mail.css'), 'utf8');
    expect(reader).toContain('className="sender-actions"');
    expect(reader).not.toContain('mail-body-view-action');
    expect(mailStyles).not.toContain('.mail-body-view-action');
    expect(translationSettings).toContain('editor.descriptor.credentialKinds.length === 0');
    expect(translationSettings).toContain("descriptor.kind !== 'edge-local'");
    expect(settingsModal).toContain("id: 'translation', label: '翻译服务'");
  });

  it('protects the mail reader while compacting navigation at desktop widths', async () => {
    const sourceRoot = path.join(workspaceRoot, 'frontend', 'src');
    const responsiveStyles = await readFile(path.join(sourceRoot, 'styles', 'typography-responsive.css'), 'utf8');
    const mailStyles = await readFile(path.join(sourceRoot, 'styles', 'mail.css'), 'utf8');
    const sidebar = await readFile(path.join(sourceRoot, 'features', 'navigation', 'AppSidebar.tsx'), 'utf8');
    expect(responsiveStyles).toContain('@media (min-width: 821px) and (max-width: 1180px)');
    expect(responsiveStyles).toContain('grid-template-columns: 64px 64px minmax(0, 1fr)');
    expect(responsiveStyles).toContain('.primary-sidebar .compose-button-label { display: none; }');
    expect(mailStyles).toContain('minmax(460px, 1fr)');
    expect(mailStyles).toContain('container: mail-reader / inline-size');
    expect(mailStyles).toContain('@container mail-reader (max-width: 620px)');
    expect(sidebar).toContain('className="compose-button-label"');
    expect(sidebar).toContain("title={t('统一收件箱')}");
  });

  it('keeps privacy settings focused on user actions instead of implementation facts', async () => {
    const privacy = await readFile(path.join(workspaceRoot, 'frontend', 'src', 'features', 'settings', 'PrivacyPanel.tsx'), 'utf8');
    expect(privacy).not.toContain('settings-facts');
    expect(privacy).not.toContain('白名单清洗');
    expect(privacy).not.toContain('仅 MCP Full');
    expect(privacy).toContain('value={`${accountCount} 个邮箱`}');
  });

  it('keeps shortcut key controls at a consistent size', async () => {
    const typography = await readFile(path.join(workspaceRoot, 'frontend', 'src', 'styles', 'typography-responsive.css'), 'utf8');
    const shortcuts = await readFile(path.join(workspaceRoot, 'frontend', 'src', 'features', 'settings', 'ShortcutPanel.tsx'), 'utf8');
    expect(typography).toContain('.shortcut-row > button { width: 96px; min-width: 96px; height: 36px; min-height: 36px;');
    expect(shortcuts).toContain('const shortcutIcons: Record<ShortcutActionId, Icon>');
    expect(shortcuts).not.toContain('<Keyboard size={18} />');
  });

  it('uses the shared settings surface for the remote service editor', async () => {
    const styles = await readFile(path.join(workspaceRoot, 'frontend', 'src', 'styles.css'), 'utf8');
    const editor = await readFile(path.join(workspaceRoot, 'frontend', 'src', 'features', 'service', 'ServiceAddressEditor.tsx'), 'utf8');
    expect(styles).toContain('.service-address-editor:not(.is-compact) .service-remote-form{border:1px solid var(--color-border);border-radius:var(--radius-lg);');
    expect(editor).toContain('<AppButton appearance="subtle" type="button" onClick={onCancel}>取消</AppButton>');
  });

  it('keeps Hide My Email as an independent settings feature', async () => {
    const settings = await readFile(path.join(workspaceRoot, 'frontend', 'src', 'features', 'settings', 'SettingsModal.tsx'), 'utf8');
    const accounts = await readFile(path.join(workspaceRoot, 'frontend', 'src', 'features', 'accounts', 'AccountSettingsModal.tsx'), 'utf8');
    const accountCard = await readFile(path.join(workspaceRoot, 'frontend', 'src', 'features', 'accounts', 'AccountSettingsCard.tsx'), 'utf8');
    const hmeSettings = await readFile(path.join(workspaceRoot, 'frontend', 'src', 'features', 'settings', 'AppleHmeSettingsPanel.tsx'), 'utf8');
    const hmePanel = await readFile(path.join(workspaceRoot, 'frontend', 'src', 'features', 'apple-hme', 'AppleHmePanel.tsx'), 'utf8');
    const settingsStyles = await readFile(path.join(workspaceRoot, 'frontend', 'src', 'styles', 'settings.css'), 'utf8');
    expect(settings).toContain("id: 'apple-hme'");
    expect(settings).toContain('<AppleHmeSettingsPanel');
    expect(accounts).not.toContain('AppleHmePanel');
    expect(accountCard).toContain('account-detail-view');
    expect(accountCard).not.toContain('settings-account-card');
    expect(await exists('frontend/src/features/apple-hme/AppleHmePanel.tsx')).toBe(true);
    expect(hmePanel).toContain("scrollIntoView({ behavior: 'smooth', block: 'nearest' })");
    expect(hmePanel).toContain('appleAccountLastSuccessfulKeepaliveAt');
    expect(hmePanel).toContain('icloudWebLastSuccessfulKeepaliveAt');
    expect(hmePanel.indexOf('className="apple-hme-error"')).toBeLessThan(hmePanel.indexOf('className="apple-hme-login"'));
    expect(hmePanel).toContain("export type AppleHmeView = 'overview' | 'addresses' | 'create'");
    expect(hmePanel).toContain("if (view === 'addresses')");
    expect(hmePanel).toContain('className="apple-hme-address-heading-actions"');
    expect(hmePanel).toContain("onClick={() => onViewChange('create')}");
    expect(hmePanel).toContain('<SettingsLinkRow');
    expect(hmePanel).not.toContain('className="apple-hme-view-switch"');
    expect(hmeSettings).toContain('icloudAccounts.map((account) => <AppleHmePanel');
    expect(hmeSettings).not.toContain('apple-hme-account-picker');
    expect(hmeSettings).toContain('新增托管');
    expect(settings).toContain("onAddAccount('icloud', 'apple-hme')");
    expect(hmePanel).toContain('apple-hme-account-settings-list');
    expect(settingsStyles).toContain('.apple-hme-login-actions');
    expect(settingsStyles).toContain('.settings-panel-body { min-width: 0; min-height: 0; overflow-x: hidden; overflow-y: auto');
    expect(settingsStyles).toContain('.apple-hme-settings-panel .settings-panel-body { display: block');
    expect(settingsStyles).toContain('scrollbar-gutter: stable');
  });
});
