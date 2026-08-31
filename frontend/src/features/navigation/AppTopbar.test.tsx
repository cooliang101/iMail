import { describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'preact-render-to-string';
import { parseHTML } from 'linkedom';
import { AppTopbar } from './AppTopbar';

const props = {
  sidebarCollapsed: false, sidebarOpen: false, search: '', searchShortcut: 'Ctrl + K', searchInputRef: { current: null },
  onToggleSidebar: vi.fn(), onOpenMobileSidebar: vi.fn(), onSearchChange: vi.fn(), onNotifications: vi.fn(), onAbout: vi.fn(),
};

describe('advanced search toolbar entry', () => {
  it.each([false, true])('uses the existing icon button with active=%s', (active) => {
    const { document } = parseHTML(renderToStaticMarkup(<AppTopbar {...props} onAdvancedSearch={vi.fn()} advancedActive={active} />));
    const button = document.querySelector('.advanced-search-trigger')!;
    expect(button.classList.contains('icon-button')).toBe(true);
    expect(button.getAttribute('title')).toBe('高级搜索');
    expect(button.getAttribute('aria-label')).toBe('高级搜索');
    expect(button.getAttribute('aria-pressed')).toBe(String(active));
    expect(button.querySelector('svg')?.getAttribute('width')).toBe('19');
    expect(button.textContent).toBe('');
  });

  it('omits the entry outside mail views', () => {
    const html = renderToStaticMarkup(<AppTopbar {...props} />);
    expect(html).not.toContain('advanced-search-trigger');
  });
});
