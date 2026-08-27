import { describe, expect, it } from 'vitest';
import { renderToStaticMarkup } from 'preact-render-to-string';
import { ParticipantFilterBar } from './ParticipantFilterBar';

describe('ParticipantFilterBar', () => {
  it('renders both participant conditions with independent clear actions', () => {
    const html = renderToStaticMarkup(<ParticipantFilterBar
      filters={{
        sender: { name: 'Wayne', address: 'sender@example.test' },
        recipient: { name: 'Owner', address: 'owner@example.test' },
      }}
      onClear={() => undefined}
      onClearAll={() => undefined}
    />);

    expect(html).toContain('aria-label="邮件参与者筛选条件"');
    expect(html).toContain('来自');
    expect(html).toContain('发往');
    expect(html).toContain('sender@example.test');
    expect(html).toContain('owner@example.test');
    expect(html).toContain('清除来自sender@example.test的筛选');
    expect(html).toContain('清除发往owner@example.test的筛选');
    expect(html).toContain('清除全部');
  });

  it('does not render when no participant condition is active', () => {
    expect(renderToStaticMarkup(<ParticipantFilterBar
      filters={{ sender: null, recipient: null }}
      onClear={() => undefined}
      onClearAll={() => undefined}
    />)).toBe('');
  });
});
