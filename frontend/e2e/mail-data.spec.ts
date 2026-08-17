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
    to: [{ name: 'Owner', address: account.email }], subject: `Fixture subject ${index}`, preview: `Fixture preview ${index}`,
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
  messagePageCalls: number;
  serviceInfoCalls: number;
  failNextPatch: boolean;
};

async function json(route: Route, body: unknown, status = 200) {
  await route.fulfill({ status, contentType: 'application/json', body: JSON.stringify(body) });
}

async function installMailFixture(page: Page) {
  const state: FixtureState = { moveCalls: [], patchCalls: [], drafts: [], messagePageCalls: 0, serviceInfoCalls: 0, failNextPatch: false };
  await page.route('**/api/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const path = url.pathname;
    const method = request.method();
    if (path === '/api/auth/status') return json(route, { setupRequired: false, registrationOpen: true, user: { id: 'user-1', login: 'fixture', displayName: 'Fixture User', createdAt: '2026-08-13T00:00:00Z' } });
    if (path === '/api/accounts') return json(route, { accounts: [account] });
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
    if (path === '/api/labels') return json(route, { labels: ['重要', '待办'] });
    if (path === '/api/contacts') return json(route, { contacts: [{ address: 'recipient@example.test', name: 'Recipient', messageCount: 3, lastContactAt: '2026-08-12T00:00:00Z', logo: { url: '' } }] });
    if (path === '/api/messages' && method === 'GET') {
      state.messagePageCalls += 1;
      const cursor = url.searchParams.get('cursor');
      const page = cursor ? messages.slice(60) : messages.slice(0, 60);
      return json(route, { messages: page, total: 70, nextOffset: cursor ? 70 : 60, nextCursor: cursor ? undefined : 'fixture-cursor', hasMore: !cursor });
    }
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
  await page.getByRole('button', { name: '查看' }).click();
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

test('remote service selection verifies the endpoint before switching', async ({ page }) => {
  const state = await installMailFixture(page);
  await page.getByRole('button', { name: '打开设置' }).click();
  await page.getByRole('button', { name: /服务连接/ }).click();
  await expect(page.getByText(/实例 fixture/)).toBeVisible();
  await page.getByLabel('服务地址').fill('http://127.0.0.1:18787');
  await page.getByRole('button', { name: '连接远程服务' }).click();
  await expect.poll(() => state.serviceInfoCalls).toBeGreaterThanOrEqual(2);
  await expect.poll(() => page.evaluate(() => localStorage.getItem('imail.service-mode'))).toBe('remote');
});
