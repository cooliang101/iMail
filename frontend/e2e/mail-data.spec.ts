import { expect, test, type Page, type Route } from '@playwright/test';

const account = {
  id: 'account-1', provider: 'gmail', email: 'owner@example.test', displayName: 'Fixture Inbox',
  group: '工作', groupIcon: 'briefcase', color: '#427d6d', status: 'connected', lastSyncAt: '2026-08-13T08:00:00Z',
  mailboxes: [{ path: 'INBOX', name: 'Inbox', delimiter: '/', specialUse: '\\Inbox', selectable: true, subscribed: true, total: 70, unread: 2 }],
};

function fixtureMessage(index: number) {
  const id = `message-${String(index).padStart(3, '0')}`;
  return {
    id, accountId: account.id, mailbox: 'INBOX', mailboxRole: 'inbox',
    from: { name: `Fixture Sender ${index}`, address: `sender${index}@example.test`, logo: { url: '' } },
    to: index === 1
      ? [{ name: 'Owner', address: account.email }, { name: 'Project Archive', address: 'archive@example.test' }]
      : [{ name: 'Owner', address: account.email }], subject: `Fixture subject ${index}`, preview: `Fixture preview ${index}`,
    date: new Date(Date.UTC(2026, 7, 13, 12, 0, 70 - index)).toISOString(), unread: index === 2 || index === 3, flagged: false,
    hasAttachments: index === 1, attachments: index === 1 ? [{ filename: 'fixture.txt', contentType: 'text/plain', size: 18, index: 0 }] : [],
    labels: index === 1 ? ['重要'] : [],
  };
}

const messages = Array.from({ length: 70 }, (_, index) => fixtureMessage(index + 1));
const preferences = {
  theme: 'mint-fresh', startupView: 'inbox', markReadOnOpen: true, defaultMessageView: 'source',
  notificationKinds: { unread: true, snooze: true, error: true },
  shortcutBindings: { focusSearch: 'Ctrl+K', compose: 'C', sync: 'R', nextMessage: 'J', previousMessage: 'K', reply: 'A', forward: 'F', toggleStar: 'S', markUnread: 'U', archive: 'E', delete: 'Delete', openShortcutSettings: 'Ctrl+/' },
};

type FixtureState = {
  moveCalls: string[];
  patchCalls: Array<{ id: string; body: Record<string, unknown> }>;
  drafts: Array<Record<string, unknown>>;
  outbox: Array<Record<string, unknown>>;
  workItems: Array<Record<string, unknown>>;
  outboxListCalls: number;
  messagePageCalls: number;
  serviceInfoCalls: number;
  failNextPatch: boolean;
};

async function json(route: Route, body: unknown, status = 200) {
  await route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
}

async function installMailFixture(page: Page) {
  // Keep lazy-module resource entries available even when the dev server emits
  // many fine-grained icon modules. Chromium's default buffer can evict the
  // first preload entries before the warmup assertion observes them.
  await page.addInitScript(() => performance.setResourceTimingBufferSize(2_000));
  const state: FixtureState = { moveCalls: [], patchCalls: [], drafts: [], outbox: [], workItems: [], outboxListCalls: 0, messagePageCalls: 0, serviceInfoCalls: 0, failNextPatch: false };
  await page.route('**/api/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const path = url.pathname;
    const method = request.method();
    if (path === '/api/auth/status') return json(route, { setupRequired: false, registrationOpen: true, user: { id: 'user-1', login: 'fixture', displayName: 'Fixture User', createdAt: '2026-08-13T00:00:00Z' } });
    if (path === '/api/accounts') return json(route, { accounts: [account] });
    if (path === '/api/smart-folders' && method === 'GET') return json(route, { folders: [] });
    if (path === '/api/developer-tokens') return json(route, { tokens: [] });
    if (path === '/api/external-access') return json(route, { settings: { mcpEnabled: false, gatewayEnabled: false } });
    if (path === '/api/preferences') return json(route, method === 'PATCH' ? { preferences: { ...preferences, ...request.postDataJSON() } } : { preferences });
    if (path === '/api/message-stats') return json(route, { total: 70, unread: 2, byAccount: [{ accountId: account.id, total: 70, unread: 2 }], byGroup: [{ group: account.group, total: 70, unread: 2 }] });
    if (path === '/api/drafts' && method === 'GET') return json(route, { drafts: state.drafts });
    if ((path === '/api/drafts' && method === 'POST') || (path.startsWith('/api/drafts/') && method === 'PUT')) {
      const body = request.postDataJSON() as Record<string, unknown>;
      const draft = { id: path.split('/').at(-1) === 'drafts' ? request.headers()['x-draft-id'] : path.split('/').at(-1), createdAt: '2026-08-13T00:00:00Z', updatedAt: new Date().toISOString(), ...body };
      state.drafts = [draft]; return json(route, { draft });
    }
    if (path === '/api/outbox' && method === 'GET') { state.outboxListCalls += 1; return json(route, { items: state.outbox }); }
    if (path === '/api/mail-work-items' && method === 'GET') return json(route, { items: state.workItems });
    if (path === '/api/outbox' && method === 'POST') {
      const body = request.postDataJSON() as Record<string, unknown>;
      expect(body.requestId).toEqual(expect.stringMatching(/^[0-9a-f-]{36}$/));
      const existing = state.outbox.find((item) => item.requestId === body.requestId);
      if (existing) return json(route, { item: existing }, 201);
      const item = {
        id: `outbox-${state.outbox.length + 1}`,
        requestId: body.requestId,
        accountId: body.accountId,
        to: body.to,
        cc: body.cc ?? [],
        bcc: body.bcc ?? [],
        subject: body.subject,
        scheduledAt: body.sendAt,
        status: 'scheduled',
        attempts: 0,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      };
      state.outbox = [item, ...state.outbox];
      state.drafts = state.drafts.filter((draft) => draft.id !== body.draftId);
      return json(route, { item }, 201);
    }
    const outboxRetry = path.match(/^\/api\/outbox\/(outbox-\d+)\/retry$/);
    if (outboxRetry && method === 'POST') {
      state.outbox = state.outbox.map((item) => item.id === outboxRetry[1] ? { ...item, status: 'scheduled', lastError: undefined } : item);
      return json(route, { queued: true });
    }
    const outboxResolve = path.match(/^\/api\/outbox\/(outbox-\d+)\/resolve$/);
    if (outboxResolve && method === 'POST') {
      const resolution = (request.postDataJSON() as { resolution: 'sent' | 'notSent' }).resolution;
      state.outbox = state.outbox.map((item) => item.id === outboxResolve[1]
        ? { ...item, status: resolution === 'sent' ? 'sent' : 'cancelled', lastError: undefined }
        : item);
      return json(route, { resolved: true, status: resolution === 'sent' ? 'sent' : 'cancelled' });
    }
    const outboxItem = path.match(/^\/api\/outbox\/(outbox-\d+)$/);
    if (outboxItem && method === 'DELETE') {
      state.outbox = state.outbox.map((item) => item.id === outboxItem[1] ? { ...item, status: 'cancelled', updatedAt: new Date().toISOString() } : item);
      return json(route, { cancelled: true });
    }
    if (path === '/api/labels') return json(route, { labels: ['重要', '待办'] });
    if (path === '/api/contacts') return json(route, { contacts: [
      { address: 'sender1@example.test', name: 'Wayne Fixture', messageCount: 18, lastContactAt: '2026-08-12T00:00:00Z', logo: { url: '' } },
      { address: 'recipient@example.test', name: 'Recipient', messageCount: 3, lastContactAt: '2026-08-12T00:00:00Z', logo: { url: '' } },
    ] });
    if (path === '/api/messages' && method === 'GET') {
      state.messagePageCalls += 1;
      const cursor = url.searchParams.get('cursor');
      const page = cursor ? messages.slice(60) : messages.slice(0, 60);
      return json(route, { messages: page, total: 70, nextOffset: cursor ? 70 : 60, nextCursor: cursor ? undefined : 'fixture-cursor', hasMore: !cursor });
    }
    const conversation = path.match(/^\/api\/messages\/(message-\d{3})\/conversation$/);
    if (conversation && method === 'GET') return json(route, { messages: messages.filter(item => item.id === conversation[1]) });
    const detail = path.match(/^\/api\/messages\/(message-\d{3})$/);
    if (detail && method === 'GET') {
      const message = messages.find((item) => item.id === detail[1])!;
      return json(route, { message: { ...message, text: `Full fixture body for ${message.subject}`, html: `<p>Full fixture body for <strong>${message.subject}</strong></p>` } });
    }
    if (detail && method === 'PATCH') {
      const body = request.postDataJSON() as Record<string, unknown>; state.patchCalls.push({ id: detail[1], body });
      if (state.failNextPatch) { state.failNextPatch = false; await new Promise((resolve) => setTimeout(resolve, 250)); return json(route, { error: 'fixture offline failure' }, 503); }
      return json(route, { message: { ...messages.find((item) => item.id === detail[1]), ...body } });
    }
    const workItem = path.match(/^\/api\/messages\/(message-\d{3})\/work-item$/);
    if (workItem && method === 'PUT') {
      const body = request.postDataJSON() as { status: string; dueAt?: string; note?: string };
      const message = messages.find((item) => item.id === workItem[1])!;
      const existing = state.workItems.find((entry) => (entry.item as { messageId: string }).messageId === workItem[1]);
      const item = { id: existing ? (existing.item as { id: string }).id : `work-${state.workItems.length + 1}`, messageId: workItem[1], accountId: account.id, status: body.status, dueAt: body.dueAt, note: body.note ?? '', createdAt: new Date().toISOString(), updatedAt: new Date().toISOString() };
      state.workItems = [{ item, message }, ...state.workItems.filter((entry) => (entry.item as { messageId: string }).messageId !== workItem[1])];
      return json(route, { item });
    }
    if (workItem && method === 'DELETE') {
      state.workItems = state.workItems.filter((entry) => (entry.item as { messageId: string }).messageId !== workItem[1]);
      return json(route, { completed: true, messageId: workItem[1] });
    }
    const move = path.match(/^\/api\/messages\/(message-\d{3})\/move$/);
    if (move) { state.moveCalls.push(move[1]); return json(route, { ok: true }); }
    if (path === '/api/messages/message-001/attachments/0/preview' && method === 'POST') return json(route, { previewId: 'preview-1', descriptor: { kind: 'text', filename: 'fixture.txt', contentType: 'text/plain', size: 18, archiveEntries: [] }, expiresInSeconds: 300 });
    if (path === '/api/attachment-previews/preview-1/content') return route.fulfill({ status: 200, contentType: 'text/plain', body: 'fixture attachment' });
    if (path === '/api/attachment-previews/preview-1' && method === 'DELETE') return route.fulfill({ status: 204, body: '' });
    if (path === '/api/system/info') { state.serviceInfoCalls += 1; return json(route, { service: 'imail', instanceId: 'fixture-instance', version: '0.0.2', protocolVersion: 1, capabilities: { gateway: true, mcp: true, syncWorker: true, webClient: true } }); }
    if (path === '/api/auth/logout') return route.fulfill({ status: 204, body: '' });
    return json(route, { error: `Unhandled fixture route: ${method} ${path}` }, 404);
  });
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Fixture subject 1', exact: true })).toBeVisible();
  return state;
}

async function simulateDesktopFrame(page: Page) {
  await page.evaluate(() => {
    const root = document.querySelector('#root');
    const fluentRoot = root?.firstElementChild;
    if (!root || !fluentRoot || root.querySelector('.desktop-frame')) return;
    const frame = document.createElement('div');
    const titlebar = document.createElement('div');
    const content = document.createElement('div');
    frame.className = 'desktop-frame';
    titlebar.className = 'desktop-titlebar';
    content.className = 'desktop-content';
    root.append(frame);
    frame.append(titlebar, content);
    content.append(fluentRoot);
  });
}

test('mail pagination loads stable fixture pages and reads full bodies', async ({ page }) => {
  const state = await installMailFixture(page);
  await expect(page.getByText('Full fixture body for', { exact: false })).toBeVisible();
  await page.locator('.message-list').evaluate((element) => element.scrollTo(0, element.scrollHeight));
  await expect.poll(() => state.messagePageCalls).toBeGreaterThanOrEqual(2);
  await page.locator('.message-list').evaluate((element) => element.scrollTo(0, element.scrollHeight));
  await expect(page.locator('.message-row').filter({ hasText: 'Fixture subject 70' })).toBeVisible();
});

test('mail work queue adds, displays and completes a message', async ({ page }) => {
  const state = await installMailFixture(page);
  await page.getByRole('button', { name: '加入邮件处理队列' }).click();
  await expect.poll(() => state.workItems.length).toBe(1);
  await expect(page.getByText('邮件已加入处理队列', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: '邮件处理队列', exact: true }).click();
  await expect(page.locator('.work-queue-header strong')).toHaveText('邮件处理队列');
  await expect(page.locator('.work-queue-item').filter({ hasText: 'Fixture subject 1' })).toBeVisible();
  await page.locator('.work-queue-item').getByRole('button', { name: '完成' }).click();
  await expect.poll(() => state.workItems.length).toBe(0);
  await expect(page.getByText('当前没有处理项目', { exact: true })).toBeVisible();
});

test('narrow desktop keeps the reader inside the viewport', async ({ page }) => {
  await page.setViewportSize({ width: 937, height: 817 });
  await installMailFixture(page);
  await expect(page.getByText('Full fixture body for', { exact: false })).toBeVisible();
  const geometry = await page.locator('.mail-layout').evaluate((layout) => {
    const reader = layout.querySelector<HTMLElement>('.reader');
    const content = layout.querySelector<HTMLElement>('.reader-content');
    if (!reader || !content) throw new Error('Reader layout is missing');
    const layoutRect = layout.getBoundingClientRect();
    const readerRect = reader.getBoundingClientRect();
    const contentRect = content.getBoundingClientRect();
    return {
      layoutRight: layoutRect.right,
      readerRight: readerRect.right,
      readerWidth: readerRect.width,
      contentLeft: contentRect.left,
      contentRight: contentRect.right,
      viewportWidth: window.innerWidth,
    };
  });
  expect(geometry.readerWidth).toBeGreaterThanOrEqual(320);
  expect(geometry.readerRight).toBeLessThanOrEqual(geometry.layoutRight + 1);
  expect(geometry.layoutRight).toBeLessThanOrEqual(geometry.viewportWidth + 1);
  expect(geometry.contentLeft).toBeGreaterThanOrEqual(0);
  expect(geometry.contentRight).toBeLessThanOrEqual(geometry.viewportWidth + 1);
});

test('reader shows complete routing details and opens the sender contact card', async ({ page }) => {
  await page.setViewportSize({ width: 1920, height: 900 });
  await installMailFixture(page);
  const senderTrigger = page.locator('.sender-contact-trigger');
  await expect(senderTrigger).toContainText('Wayne Fixture');
  await expect(senderTrigger).toContainText('sender1@example.test');
  await expect(page.locator('.recipient-line')).toContainText('Owner <owner@example.test>');
  await expect(page.locator('.recipient-line')).toContainText('Project Archive <archive@example.test>');
  await expect(page.locator('.mail-body')).toHaveCSS('background-color', 'rgba(0, 0, 0, 0)');
  const layout = await page.evaluate(() => {
    const content = document.querySelector('.reader-content')!.getBoundingClientRect();
    const sender = document.querySelector('.sender-contact-trigger')!.getBoundingClientRect();
    const recipients = document.querySelector('.recipient-line')!.getBoundingClientRect();
    const body = document.querySelector('.mail-plain-body')!.getBoundingClientRect();
    const reader = document.querySelector('.reader')!.getBoundingClientRect();
    return {
      recipientTop: recipients.top,
      senderBottom: sender.bottom,
      senderWidth: sender.width,
      contentWidth: content.width,
      readerWidth: reader.width,
      contentCenter: content.left + content.width / 2,
      bodyCenter: body.left + body.width / 2,
      bodyWidth: body.width,
    };
  });
  expect(layout.recipientTop).toBeGreaterThanOrEqual(layout.senderBottom - 2);
  expect(layout.senderWidth).toBeLessThan(layout.contentWidth / 2);
  expect(layout.contentWidth).toBeGreaterThan(1000);
  expect(layout.contentWidth / layout.readerWidth).toBeGreaterThan(0.75);
  expect(Math.abs(layout.contentCenter - layout.bodyCenter)).toBeLessThanOrEqual(2);
  expect(Math.abs(layout.contentWidth - layout.bodyWidth)).toBeLessThanOrEqual(2);

  await senderTrigger.click();
  const card = page.getByRole('dialog', { name: 'Wayne Fixture 的联系人名片' });
  await expect(card).toBeVisible();
  await expect(card).toContainText('18 封往来邮件');
  await expect(card.getByRole('button', { name: '复制邮箱' })).toBeVisible();
  await expect(card.getByRole('button', { name: '写邮件' })).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(card).toBeHidden();
  await expect(senderTrigger).toBeFocused();

  const recipientTrigger = page.getByRole('button', { name: '查看收件人 Owner <owner@example.test>' });
  await recipientTrigger.click();
  const recipientCard = page.getByRole('dialog', { name: 'Owner 的收件人信息' });
  await expect(recipientCard).toBeVisible();
  await expect(recipientCard).toContainText('owner@example.test');
  await expect(recipientCard).not.toContainText('封往来邮件');
  await expect(recipientCard.getByRole('button', { name: '复制邮箱' })).toBeVisible();
  await expect(recipientCard.getByRole('button', { name: '写邮件' })).toBeVisible();
  const recipientFilterRequest = page.waitForRequest((request) => new URL(request.url()).searchParams.get('recipient') === account.email);
  await recipientCard.getByRole('button', { name: '发往此地址' }).click();
  await recipientFilterRequest;
  const filterBar = page.getByRole('region', { name: '邮件参与者筛选条件' });
  await expect(filterBar).toContainText('发往');
  await expect(filterBar).toContainText(account.email);

  await senderTrigger.click();
  const senderFilterRequest = page.waitForRequest((request) => new URL(request.url()).searchParams.get('sender') === 'sender1@example.test');
  await card.getByRole('button', { name: '来自此地址' }).click();
  await senderFilterRequest;
  await expect(filterBar).toContainText('来自');
  await expect(filterBar).toContainText('sender1@example.test');
  await expect(filterBar.getByRole('button', { name: '清除全部' })).toBeVisible();

  await filterBar.getByRole('button', { name: `清除发往${account.email}的筛选` }).click();
  await expect(filterBar).not.toContainText('发往');
  await expect(filterBar).toContainText('sender1@example.test');
});

test('participant filtering returns a narrow reader to the message list', async ({ page }) => {
  await installMailFixture(page);
  await page.setViewportSize({ width: 600, height: 820 });
  await page.locator('.message-row').filter({ has: page.getByText('Fixture subject 1', { exact: true }) }).click();
  await expect(page.locator('.mail-layout')).toHaveClass(/mobile-reader-open/);
  await page.locator('.sender-contact-trigger').click();
  const filterRequest = page.waitForRequest((request) => new URL(request.url()).searchParams.get('sender') === 'sender1@example.test');
  await page.getByRole('dialog', { name: 'Wayne Fixture 的联系人名片' }).getByRole('button', { name: '来自此地址' }).click();
  await filterRequest;
  await expect(page.locator('.mail-layout')).not.toHaveClass(/mobile-reader-open/);
  await expect(page.getByRole('region', { name: '邮件参与者筛选条件' })).toContainText('sender1@example.test');
  await expect(page.locator('.message-pane')).toBeVisible();
});

test('mail notices and external access keep their layout before compose is opened', async ({ page }) => {
  await installMailFixture(page);
  await simulateDesktopFrame(page);
  const initialGeometry = await page.evaluate(() => {
    const content = document.querySelector('.desktop-content')!.getBoundingClientRect();
    const shell = document.querySelector('.app-shell')!.getBoundingClientRect();
    const mail = document.querySelector('.mail-layout')!.getBoundingClientRect();
    return { viewport: window.innerHeight, contentBottom: content.bottom, shellBottom: shell.bottom, mailBottom: mail.bottom };
  });
  expect(initialGeometry.contentBottom).toBeLessThanOrEqual(initialGeometry.viewport);
  expect(initialGeometry.shellBottom).toBe(initialGeometry.contentBottom);
  expect(initialGeometry.mailBottom).toBeLessThan(initialGeometry.contentBottom);

  await page.locator('.message-row').filter({ hasText: 'Fixture subject 2' }).click();
  const notice = page.getByText('邮件已标记为已读', { exact: true });
  await expect(notice).toBeVisible();
  await expect(notice.locator('..')).toHaveCSS('position', 'fixed');
  await expect(page.getByRole('button', { name: '打开设置' })).toBeVisible();

  await page.evaluate(() => {
    document.documentElement.dataset.sawFeatureLoading = 'false';
    const observer = new MutationObserver(() => {
      if (document.querySelector('.feature-loading')) {
        document.documentElement.dataset.sawFeatureLoading = 'true';
        observer.disconnect();
      }
    });
    observer.observe(document.body, { childList: true, subtree: true });
  });
  await page.getByRole('button', { name: /外部接入/ }).click();
  await expect(page.locator('.feature-loading')).toHaveCount(0);
  await expect(page.locator('html')).toHaveAttribute('data-saw-feature-loading', 'false');
  await expect(page.getByRole('heading', { name: 'MCP Agent 接入' })).toBeVisible();
  await expect(page.locator('.token-workspace')).toHaveCSS('overflow', 'auto');
  await expect(page.locator('.access-tabs')).toHaveCSS('display', 'flex');
  await expect(page.getByRole('button', { name: '打开设置' })).toBeVisible();
  const externalGeometry = await page.evaluate(() => {
    const content = document.querySelector('.desktop-content')!.getBoundingClientRect();
    const workspace = document.querySelector('.token-workspace') as HTMLElement;
    const bounds = workspace.getBoundingClientRect();
    return { contentBottom: content.bottom, workspaceBottom: bounds.bottom, workspaceHeight: bounds.height, workspaceScrollHeight: workspace.scrollHeight };
  });
  expect(externalGeometry.workspaceBottom).toBeLessThan(externalGeometry.contentBottom);
  expect(externalGeometry.workspaceHeight).toBeGreaterThan(0);
  expect(externalGeometry.workspaceScrollHeight).toBeGreaterThanOrEqual(externalGeometry.workspaceHeight);
});

test('compose autosaves recipients, body and uploaded attachments', async ({ page }) => {
  const state = await installMailFixture(page);
  await page.getByRole('button', { name: '写邮件' }).click();
  await page.getByPlaceholder('输入姓名或邮箱').first().fill('recipient@example.test');
  await page.getByPlaceholder('邮件主题').fill('Autosaved fixture draft');
  await page.getByLabel('邮件正文').fill('Draft body');
  await page.locator('input[type=file][multiple]').setInputFiles({ name: 'upload.txt', mimeType: 'text/plain', buffer: Buffer.from('upload fixture') });
  await expect(page.getByText('upload.txt', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: '关闭写信' }).click();
  await expect.poll(() => state.drafts.length, { timeout: 5_000 }).toBe(1);
  expect(state.drafts).toHaveLength(1);
  expect(state.drafts[0].subject).toBe('Autosaved fixture draft');
  expect(state.drafts[0].attachments).toEqual(expect.arrayContaining([expect.objectContaining({ filename: 'upload.txt' })]));
});

test('scheduled send persists in the outbox and can be cancelled before delivery', async ({ page }) => {
  const state = await installMailFixture(page);
  await page.getByRole('button', { name: '写邮件' }).click();
  await page.getByPlaceholder('输入姓名或邮箱').first().fill('recipient@example.test');
  await page.getByPlaceholder('邮件主题').fill('Scheduled fixture message');
  await page.getByLabel('邮件正文').fill('Send this later');
  await page.getByRole('button', { name: '定时发送' }).click();
  const picker = page.getByRole('dialog', { name: '选择发送时间' });
  await expect(picker).toBeVisible();
  const geometry = await picker.evaluate((element) => {
    const bounds = element.getBoundingClientRect();
    return { width: bounds.width, right: bounds.right, viewport: window.innerWidth };
  });
  expect(geometry.width).toBeLessThanOrEqual(305);
  expect(geometry.right).toBeLessThanOrEqual(geometry.viewport);
  await picker.getByRole('button', { name: '1 小时后' }).click();
  await expect(page.locator('.outbox-pane .draft-pane-header').getByText('发件箱', { exact: true })).toBeVisible();
  await expect(page.getByText('Scheduled fixture message', { exact: true })).toBeVisible();
  await expect.poll(() => state.outbox.length).toBe(1);
  expect(state.outbox[0].status).toBe('scheduled');
  await page.getByRole('button', { name: '取消并返回草稿' }).click();
  await expect(page.getByText('已取消', { exact: true })).toBeVisible();
  expect(state.outbox[0].status).toBe('cancelled');
});

test('outbox refreshes when the background scheduler changes task status', async ({ page }) => {
  const state = await installMailFixture(page);
  state.outbox = [{
    id: 'outbox-1', accountId: account.id, to: ['recipient@example.test'], cc: [], bcc: [],
    subject: 'Background delivery fixture', scheduledAt: new Date().toISOString(), status: 'scheduled', attempts: 0,
    createdAt: new Date().toISOString(), updatedAt: new Date().toISOString(),
  }];
  await page.reload();
  await page.getByRole('button', { name: '发件箱' }).click();
  await expect(page.getByText('等待发送', { exact: true })).toBeVisible();
  const callsBeforeDelivery = state.outboxListCalls;
  state.outbox = state.outbox.map((item) => ({ ...item, status: 'sent', sentAt: new Date().toISOString() }));
  await expect.poll(() => state.outboxListCalls).toBeGreaterThan(callsBeforeDelivery);
  await expect(page.locator('.outbox-pane .outbox-status').getByText('已发送', { exact: true })).toBeVisible({ timeout: 5_000 });
});

test('uncertain delivery requires an explicit manual resolution', async ({ page }) => {
  const state = await installMailFixture(page);
  state.outbox = [{
    id: 'outbox-1', accountId: account.id, to: ['recipient@example.test'], cc: [], bcc: [],
    subject: 'Needs review fixture', scheduledAt: new Date().toISOString(), status: 'needsReview', attempts: 1,
    lastError: 'SMTP 发送结果不确定，请核对已发送邮件后人工处理。',
    createdAt: new Date().toISOString(), updatedAt: new Date().toISOString(),
  }];
  await page.reload();
  await page.getByRole('button', { name: '发件箱' }).click();
  await expect(page.getByText('Needs review fixture', { exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '已在已发送中找到' })).toBeVisible();
  await expect(page.getByRole('button', { name: '确认未发送，返回草稿' })).toBeVisible();
  await page.getByRole('button', { name: '已在已发送中找到' }).click();
  await expect(page.locator('.outbox-pane .outbox-status').getByText('已发送', { exact: true })).toBeVisible();
  expect(state.outbox[0].status).toBe('sent');
});

test('idle warmup preloads deferred interaction bundles without a loading flash', async ({ page }) => {
  await installMailFixture(page);
  await page.waitForFunction(() => {
    const resources = performance.getEntriesByType('resource').map((entry) => entry.name);
    return resources.some((name) => name.includes('/features/compose/ComposePane.tsx'))
      && resources.some((name) => name.includes('/features/settings/SettingsModal.tsx'))
      && resources.some((name) => name.includes('/features/developer/CreateApiTokenModal.tsx'));
  }, undefined, { timeout: 15_000 });
  await page.evaluate(() => {
    document.documentElement.dataset.sawFeatureLoading = 'false';
    const observer = new MutationObserver(() => {
      if (document.querySelector('.feature-loading')) document.documentElement.dataset.sawFeatureLoading = 'true';
    });
    observer.observe(document.body, { childList: true, subtree: true });
  });

  await page.getByRole('button', { name: '写邮件' }).click();
  await expect(page.getByPlaceholder('邮件主题')).toBeVisible();
  await expect(page.locator('html')).toHaveAttribute('data-saw-feature-loading', 'false');
  await page.getByRole('button', { name: '关闭写信' }).click();
  await page.getByRole('button', { name: '打开设置' }).click();
  await expect(page.getByRole('heading', { name: '设置' })).toBeVisible();
  await expect(page.locator('html')).toHaveAttribute('data-saw-feature-loading', 'false');
});

test('attachment preview and download affordance use the message fixture', async ({ page }) => {
  await installMailFixture(page);
  await expect(page.getByRole('link', { name: /下载/ })).toHaveAttribute('download', 'fixture.txt');
  await page.getByRole('button', { name: '查看', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'fixture.txt' })).toBeVisible();
  await expect(page.getByText('fixture attachment')).toBeVisible();
});

test('archive can be undone before the remote commit', async ({ page }) => {
  const state = await installMailFixture(page);
  await page.getByRole('button', { name: '归档邮件' }).click();
  await expect(page.getByText('邮件即将归档')).toBeVisible();
  await page.getByRole('button', { name: '撤销' }).click();
  await expect(page.getByRole('heading', { name: 'Fixture subject 1', exact: true })).toBeVisible();
  await page.waitForTimeout(4_700);
  expect(state.moveCalls).toEqual([]);
});

test('failed optimistic updates roll back and rapid duplicates are suppressed', async ({ page }) => {
  const state = await installMailFixture(page);
  state.failNextPatch = true;
  const star = page.getByRole('button', { name: '添加星标' });
  await star.evaluate((button: HTMLButtonElement) => { button.click(); button.click(); });
  await expect(page.getByRole('alert')).toContainText('fixture offline failure');
  await expect(page.getByRole('button', { name: '添加星标' })).toBeVisible();
  expect(state.patchCalls.filter(({ body }) => 'flagged' in body)).toHaveLength(1);
});

test('search, labels and snooze update the selected message', async ({ page }) => {
  const state = await installMailFixture(page);
  await page.getByPlaceholder('搜索当前范围内的邮件').fill('Fixture subject 1');
  await expect(page.getByRole('heading', { name: 'Fixture subject 1', exact: true })).toBeVisible();
  await page.getByRole('button', { name: '管理邮件标签' }).last().click();
  const labelDialog = page.getByRole('dialog');
  await labelDialog.getByRole('button', { name: '待办' }).click();
  await labelDialog.getByRole('button', { name: '保存标签' }).click();
  await page.locator('.reader-actions').getByRole('button', { name: '稍后处理' }).click();
  await page.getByRole('button', { name: '明天上午' }).click();
  expect(state.patchCalls.some(({ body }) => Array.isArray(body.labels))).toBe(true);
  expect(state.patchCalls.some(({ body }) => typeof body.snoozedUntil === 'string')).toBe(true);
});

test('general settings include message display and persist changes without nested scrolling', async ({ page }) => {
  await installMailFixture(page);
  await page.getByRole('button', { name: '打开设置' }).click();
  const navigation = page.getByRole('navigation', { name: '设置分类' });
  await expect(navigation.getByRole('button', { name: /邮件展示|服务连接/ })).toHaveCount(0);
  await expect(page.getByRole('heading', { name: '通用', exact: true })).toBeVisible();
  const renderSwitch = page.getByRole('switch', { name: '渲染邮件' });
  const renderSwitchControl = renderSwitch.locator('..');
  await expect(renderSwitch).not.toBeChecked();
  const save = page.waitForResponse((response) => response.url().endsWith('/api/preferences') && response.request().method() === 'PATCH');
  await renderSwitchControl.click();
  const response = await save;
  expect(response.ok()).toBe(true);
  expect(response.request().postDataJSON().defaultMessageView).toBe('rendered');
  await expect(renderSwitch).toBeChecked();
  await page.getByRole('button', { name: '关闭设置' }).click();
  await page.getByRole('button', { name: '打开设置' }).click();
  await expect(renderSwitch).toBeChecked();
  for (const width of [1280, 820, 390]) {
    await page.setViewportSize({ width, height: 800 });
    await expect(page.locator('.settings-content .settings-panel-body')).toHaveCount(1);
    const geometry = await page.locator('.settings-content').evaluate((content) => {
      const scrollOwners = [...content.querySelectorAll<HTMLElement>('*')].filter((element) => /auto|scroll/.test(getComputedStyle(element).overflowY) && element.scrollHeight > element.clientHeight + 1);
      const body = content.querySelector<HTMLElement>('.settings-panel-body')!;
      return { scrollOwners: scrollOwners.length, themed: scrollOwners.every((element) => element.classList.contains('app-scrollbar')), width: body.clientWidth, scrollWidth: body.scrollWidth };
    });
    expect(geometry.scrollOwners).toBeLessThanOrEqual(1);
    expect(geometry.themed).toBe(true);
    expect(geometry.scrollWidth).toBeLessThanOrEqual(geometry.width + 1);
    await page.getByRole('button', { name: /^远程服务/ }).click();
    await page.getByRole('button', { name: '返回通用' }).click();
    await expect(renderSwitch).toBeChecked();
  }
  const restore = page.waitForResponse((response) => response.url().endsWith('/api/preferences') && response.request().method() === 'PATCH');
  await renderSwitchControl.click();
  expect((await restore).request().postDataJSON().defaultMessageView).toBe('source');
  await expect(renderSwitch).not.toBeChecked();
});

test('remote service selection verifies the endpoint before switching', async ({ page }) => {
  const state = await installMailFixture(page);
  await page.getByRole('button', { name: '打开设置' }).click();
  await expect(page.getByRole('heading', { name: '通用', exact: true })).toBeVisible();
  await expect(page.getByRole('navigation', { name: '设置分类' }).getByRole('button', { name: /服务连接/ })).toHaveCount(0);
  await page.getByRole('button', { name: /^远程服务/ }).click();
  await expect(page.getByRole('heading', { name: '远程服务' })).toBeVisible();
  await page.getByRole('button', { name: '返回通用' }).click();
  await expect(page.getByRole('heading', { name: '通用', exact: true })).toBeVisible();
  await page.getByRole('button', { name: /^远程服务/ }).click();
  await page.getByRole('button', { name: '取消', exact: true }).click();
  await expect(page.getByRole('heading', { name: '通用', exact: true })).toBeVisible();
  await page.getByRole('button', { name: /^远程服务/ }).click();
  await page.getByLabel('服务地址').fill('http://127.0.0.1:18787');
  await page.getByRole('button', { name: '连接远程服务' }).click();
  await expect.poll(() => state.serviceInfoCalls).toBeGreaterThanOrEqual(2);
  await expect.poll(() => page.evaluate(() => localStorage.getItem('imail.service-mode'))).toBe('remote');
});
