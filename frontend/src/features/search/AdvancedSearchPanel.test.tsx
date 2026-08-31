import { describe, expect, it, vi } from 'vitest';
import { renderToStaticMarkup } from 'preact-render-to-string';
import { parseHTML } from 'linkedom';
import { AdvancedSearchPanel } from './AdvancedSearchPanel';

describe('advanced search scroll layout', () => {
  it('keeps heading and submit actions outside the single themed scrolling body', () => {
    const { document } = parseHTML(renderToStaticMarkup(<AdvancedSearchPanel initial={{}} accounts={[]} labels={[]} onClose={vi.fn()} onApply={vi.fn()} onSave={vi.fn()} onDelete={vi.fn()} />));
    const body = document.querySelector('.advanced-search-body')!;
    expect(body.classList.contains('app-scrollbar')).toBe(true);
    expect(body.querySelector('.search-fields')).not.toBeNull();
    expect(body.querySelector('header, footer, button[type="submit"]')).toBeNull();
    expect(document.querySelector('.advanced-search-panel > header h2')?.textContent).toBe('高级搜索');
    expect(document.querySelector('.advanced-search-panel > form > footer button[type="submit"]')?.textContent).toBe('搜索');
  });
});
