import { describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'preact-render-to-string';
import { parseHTML } from 'linkedom';
import { GeneralPanel } from './GeneralPanel';
import { defaultAppPreferences } from './settings-model';
import { isTauriRuntime } from '../../platform/tauri-runtime';

vi.mock('../../platform/tauri-runtime', () => ({ isTauriRuntime: vi.fn(() => false) }));

describe('combined general, display and service settings', () => {
  it.each([false, true])('keeps one heading and themed scroll owner, desktop=%s', (desktop) => {
    vi.mocked(isTauriRuntime).mockReturnValue(desktop);
    const { document } = parseHTML(renderToStaticMarkup(<GeneralPanel preferences={defaultAppPreferences} onChange={vi.fn()} />));
    expect(document.querySelectorAll('.settings-panel-heading')).toHaveLength(1);
    expect(document.querySelector('h2')?.textContent).toBe('通用');
    expect(document.querySelectorAll('.settings-panel-body')).toHaveLength(1);
    expect(document.querySelector('.settings-panel-body')?.classList.contains('app-scrollbar')).toBe(true);
    expect(document.querySelector('.settings-section')?.textContent).toContain('启动页面');
    expect(document.querySelector('[aria-label="邮件展示"]')?.textContent).toContain('渲染邮件');
    expect(document.querySelectorAll('[aria-label="邮件展示"] [role="switch"]')).toHaveLength(1);
    expect(document.querySelector('[aria-label="服务连接"]')?.textContent).toContain('远程服务');
    expect(document.querySelector('.service-address-editor')).toBeNull();
    expect(document.querySelectorAll('.service-mode-grid')).toHaveLength(desktop ? 1 : 0);
    expect(document.querySelector('[aria-label="服务连接"]')?.textContent.includes('应用日志')).toBe(desktop);
  });
  it.each(['source', 'rendered'] as const)('reflects the saved message view: %s', (defaultMessageView) => {
    const { document } = parseHTML(renderToStaticMarkup(<GeneralPanel preferences={{ ...defaultAppPreferences, defaultMessageView }} onChange={vi.fn()} />));
    expect(document.querySelector('[aria-label="邮件展示"] [role="switch"]')?.hasAttribute('checked')).toBe(defaultMessageView === 'rendered');
  });
});
