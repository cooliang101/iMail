import { describe, expect, it } from 'vitest';
import { newMailNotificationFromEvent } from './new-mail-notifications';

function event(overrides: Record<string, unknown> = {}) {
  return JSON.stringify({
    id: 42,
    payload: {
      message: {
        id: 'message-1',
        accountEmail: 'work@example.com',
        mailboxRole: 'inbox',
        from: { name: 'Alice', address: 'alice@example.com' },
        subject: '项目进展',
        preview: '新的里程碑已经完成。',
        unread: true,
        ...overrides,
      },
    },
  });
}

describe('new mail notifications', () => {
  it('builds a concise system notification for a newly received inbox message', () => {
    expect(newMailNotificationFromEvent(event())).toEqual({
      eventKey: '42',
      tag: 'imail-message-message-1',
      title: '新邮件 · Alice',
      body: '项目进展\n新的里程碑已经完成。\nwork@example.com',
    });
  });

  it('ignores sent mail, read mail, and malformed event data', () => {
    expect(newMailNotificationFromEvent(event({ mailboxRole: 'sent' }))).toBeNull();
    expect(newMailNotificationFromEvent(event({ unread: false }))).toBeNull();
    expect(newMailNotificationFromEvent('{')).toBeNull();
  });

  it('uses safe fallbacks when sender and subject are absent', () => {
    const result = newMailNotificationFromEvent(event({ from: {}, subject: '', preview: '' }));
    expect(result?.title).toBe('新邮件 · 未知发件人');
    expect(result?.body).toBe('（无主题）\nwork@example.com');
  });
});
