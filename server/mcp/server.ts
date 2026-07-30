import crypto from 'node:crypto';
import { McpServer, type CallToolResult } from '@modelcontextprotocol/server';
import { z } from 'zod';
import { encryptSecret } from '../crypto.js';
import { publicAccount } from '../http/presenters.js';
import { beginOAuth, beginOAuthReconnect, validateStoredAccountConnection } from '../oauth.js';
import { settingsFor } from '../providers.js';
import { downloadAttachment, moveRemoteMessage, sendMessage, testAccount, updateRemoteMessageFlags } from '../mail.js';
import { gatewayMessageDetail, gatewayMessageSummary } from '../gateway/presenters.js';
import { getCachedMessage, getMessageStats, listCachedMessages, readStore, updateStore } from '../store.js';
import type { Draft, MailAccount, MailSettings, ProviderId } from '../types.js';
import { getSyncStore } from '../sync/store.js';
import { canonicalSyncTarget } from '../mail/mailbox-role.js';
import { appPreferencesUpdateSchema, readAppPreferences, updateAppPreferences } from '../preferences.js';

const providerSchema = z.enum(['outlook', 'gmail', 'qq', 'yahoo', 'hotmail', 'icloud', 'custom']);
const mailboxRoleSchema = z.enum(['inbox', 'sent', 'archive', 'drafts', 'trash', 'junk', 'custom']);
const workspaceIconSchema = z.enum(['folder', 'briefcase', 'building', 'home', 'users', 'code', 'heart', 'star']);
const settingsSchema = z.object({
  imapHost: z.string().min(1), imapPort: z.number().int().min(1).max(65535), imapSecure: z.boolean(),
  smtpHost: z.string().min(1), smtpPort: z.number().int().min(1).max(65535), smtpSecure: z.boolean(),
});
const attachmentSchema = z.object({
  id: z.string().min(1).max(100).optional(), filename: z.string().min(1).max(255),
  contentType: z.string().min(1).max(150), data: z.string().max(21_000_000),
});

function output(data: Record<string, unknown>): CallToolResult {
  const safe = JSON.parse(JSON.stringify(data)) as Record<string, unknown>;
  return { content: [{ type: 'text', text: JSON.stringify(safe, null, 2) }], structuredContent: safe };
}

function messageAccount(data: Awaited<ReturnType<typeof readStore>>, accountId: string) {
  const account = data.accounts.find((item) => item.id === accountId);
  if (!account) throw new Error('邮箱账户不存在');
  return account;
}

function accountByEmail(data: Awaited<ReturnType<typeof readStore>>, email: string) {
  const account = data.accounts.find((item) => item.email.toLowerCase() === email.toLowerCase());
  if (!account) throw new Error(`邮箱账户不存在：${email}`);
  return account;
}

function draftSummary(draft: Draft, account: MailAccount) {
  return {
    id: draft.id, accountEmail: account.email, to: draft.to, cc: draft.cc, subject: draft.subject,
    textPreview: draft.text.replace(/\s+/g, ' ').slice(0, 180), attachments: draft.attachments.map(({ data: _data, ...item }) => item),
    createdAt: draft.createdAt, updatedAt: draft.updatedAt,
  };
}

export function createMailMcpServer() {
  const server = new McpServer({ name: 'imail', version: '1.0.0' }, {
    instructions: 'iMail 是本地邮箱控制面。执行移动、删除账户、更新凭据或发送邮件前，先读取目标并确认邮箱地址与邮件 ID。授权码和邮箱凭据属于敏感信息，不得回显、记录或写入邮件内容。',
  });

  server.registerTool('imail_status', {
    title: '查看 iMail 状态', description: '查看账户、邮件、未读、草稿数量与最近同步状态。',
    inputSchema: z.object({}), annotations: { readOnlyHint: true, idempotentHint: true },
  }, async () => {
    const [data, stats] = await Promise.all([readStore(), getMessageStats()]);
    return output({ accounts: data.accounts.length, messages: data.messages.length, unread: stats.unread, drafts: (data.drafts ?? []).length, lastSyncAt: data.accounts.map((item) => item.lastSyncAt).filter(Boolean).sort().at(-1) ?? null, syncWorker: getSyncStore().workerHealth() });
  });

  server.registerTool('settings_get', {
    title: '读取 iMail 设置', description: '读取启动页面、阅读、通知、邮件展示和快捷键偏好。',
    inputSchema: z.object({}), annotations: { readOnlyHint: true, idempotentHint: true },
  }, async () => output({ preferences: await readAppPreferences() }));

  server.registerTool('settings_update', {
    title: '更新 iMail 设置', description: '更新启动页面、阅读、通知、邮件展示或快捷键偏好；未提供的字段保持不变。',
    inputSchema: appPreferencesUpdateSchema, annotations: { idempotentHint: true },
  }, async (changes) => output({ preferences: await updateAppPreferences(changes) }));

  server.registerTool('accounts_list', {
    title: '列出邮箱账户', description: '列出所有邮箱账户、连接状态和文件夹，不返回任何凭据。',
    inputSchema: z.object({}), annotations: { readOnlyHint: true, idempotentHint: true },
  }, async () => output({ accounts: (await readStore()).accounts.map(publicAccount) }));

  server.registerTool('account_add_with_code', {
    title: '用邮箱授权码添加账户', description: '使用服务商生成的授权码或应用专用密码添加并验证 IMAP/SMTP 邮箱。凭据加密保存且不会返回。',
    inputSchema: z.object({
      provider: providerSchema, email: z.string().email(), displayName: z.string().min(1).max(80),
      authorizationCode: z.string().min(1).max(2048), group: z.string().min(1).max(40).default('个人'),
      groupIcon: workspaceIconSchema.default('folder'), color: z.string().regex(/^#[0-9a-fA-F]{6}$/).default('#168f78'),
      settings: settingsSchema.optional().describe('provider=custom 时必须提供完整 IMAP/SMTP 设置'),
    }), annotations: { idempotentHint: false },
  }, async (input) => {
    const existing = await readStore();
    const email = input.email.toLowerCase();
    if (existing.accounts.some((item) => item.email === email)) throw new Error('这个邮箱已经添加');
    const settings = settingsFor(input.provider as ProviderId, input.settings as MailSettings | undefined);
    const account: MailAccount = {
      id: crypto.randomUUID(), provider: input.provider, email, displayName: input.displayName.trim(), group: input.group.trim(),
      groupIcon: input.groupIcon, color: input.color, settings,
      encryptedSecret: await encryptSecret({ authType: 'app-password', password: input.authorizationCode }),
      authMethod: 'app-password', createdAt: new Date().toISOString(), status: 'connected',
    };
    await testAccount(account);
    await updateStore((data) => {
      if (data.accounts.some((item) => item.email === email)) throw new Error('这个邮箱刚刚被其他操作添加');
      data.accounts.push(account);
    });
    return output({ account: publicAccount(account) });
  });

  server.registerTool('account_start_oauth', {
    title: '开始邮箱 OAuth 授权', description: '为 Gmail、Outlook、Hotmail 或已获准的 Yahoo 生成官方 OAuth 授权网址。用户需在浏览器完成登录。',
    inputSchema: z.object({
      provider: z.enum(['outlook', 'gmail', 'yahoo', 'hotmail']), displayName: z.string().max(80).optional(),
      group: z.string().min(1).max(40).default('个人'), color: z.string().regex(/^#[0-9a-fA-F]{6}$/).default('#168f78'),
    }), annotations: { readOnlyHint: false, idempotentHint: false },
  }, async (input) => output(await beginOAuth(input) as unknown as Record<string, unknown>));

  server.registerTool('account_reconnect_oauth', {
    title: '重新授权 OAuth 邮箱', description: '为已有 OAuth 邮箱生成重新授权网址。',
    inputSchema: z.object({ email: z.string().email() }), annotations: { readOnlyHint: false, idempotentHint: false },
  }, async ({ email }) => {
    const account = accountByEmail(await readStore(), email);
    return output(await beginOAuthReconnect(account) as unknown as Record<string, unknown>);
  });

  server.registerTool('account_update', {
    title: '更新邮箱账户', description: '更新邮箱显示名称、工作空间、图标或颜色。',
    inputSchema: z.object({
      email: z.string().email(), displayName: z.string().trim().min(1).max(80).optional(), group: z.string().trim().min(1).max(40).optional(),
      groupIcon: workspaceIconSchema.optional(), color: z.string().regex(/^#[0-9a-fA-F]{6}$/).optional(),
    }).refine((value) => Object.keys(value).some((key) => key !== 'email'), '至少提供一个要更新的字段'),
    annotations: { idempotentHint: true },
  }, async ({ email, ...changes }) => {
    let updated: MailAccount | undefined;
    await updateStore((data) => { updated = accountByEmail(data, email); Object.assign(updated, changes); });
    return output({ account: publicAccount(updated!) });
  });

  server.registerTool('account_update_authorization_code', {
    title: '更新邮箱授权码', description: '验证并替换非 OAuth 邮箱的授权码或应用专用密码。新凭据加密保存且不会返回。',
    inputSchema: z.object({ email: z.string().email(), authorizationCode: z.string().min(1).max(2048) }), annotations: { idempotentHint: false },
  }, async ({ email, authorizationCode }) => {
    const account = accountByEmail(await readStore(), email);
    if (account.authMethod === 'oauth2') throw new Error('OAuth 邮箱请使用重新授权工具');
    const candidate: MailAccount = {
      ...account, encryptedSecret: await encryptSecret({ authType: 'app-password', password: authorizationCode }),
      authMethod: 'app-password', status: 'syncing', lastError: undefined,
    };
    await testAccount(candidate);
    const connected: MailAccount = { ...candidate, status: 'connected' };
    await updateStore((data) => {
      const index = data.accounts.findIndex((item) => item.id === account.id);
      if (index < 0) throw new Error('邮箱账户已被移除');
      data.accounts[index] = connected;
    });
    return output({ account: publicAccount(connected) });
  });

  server.registerTool('account_test_connection', {
    title: '测试邮箱连接', description: '使用已保存凭据测试邮箱的 IMAP 与 SMTP 连接。',
    inputSchema: z.object({ email: z.string().email() }), annotations: { readOnlyHint: false, idempotentHint: true },
  }, async ({ email }) => output({ account: publicAccount(await validateStoredAccountConnection(accountByEmail(await readStore(), email))) }));

  server.registerTool('account_remove', {
    title: '移除邮箱账户', description: '从 iMail 移除账户，并删除该账户的本地邮件缓存。不会删除服务商服务器上的账户。',
    inputSchema: z.object({ email: z.string().email() }), annotations: { destructiveHint: true, idempotentHint: true },
  }, async ({ email }) => {
    const account = accountByEmail(await readStore(), email);
    await updateStore((data) => {
      data.accounts = data.accounts.filter((item) => item.id !== account.id);
      data.messages = data.messages.filter((item) => item.accountId !== account.id);
      data.drafts = (data.drafts ?? []).filter((item) => item.accountId !== account.id);
      data.tokens.forEach((token) => { token.accountIds = token.accountIds.filter((id) => id !== account.id); });
    });
    getSyncStore().deleteAccountData(account.id);
    return output({ removed: true, email: account.email });
  });

  server.registerTool('mailbox_sync', {
    title: '同步邮箱', description: '从源 IMAP 同步一个或全部邮箱的收件箱、已发送、归档、垃圾箱或指定自定义文件夹。',
    inputSchema: z.object({
      email: z.string().email().optional().describe('不填时同步全部账户'), mailboxRole: mailboxRoleSchema.default('inbox'),
      mailboxPath: z.string().trim().min(1).max(500).optional().describe('指定自定义 IMAP 文件夹路径'),
    }), annotations: { idempotentHint: true },
  }, async ({ email, mailboxRole, mailboxPath }) => {
    const data = await readStore();
    const accounts = email ? [accountByEmail(data, email)] : data.accounts;
    const syncStore = getSyncStore();
    const results = accounts.map((account) => {
      const target = canonicalSyncTarget(account, mailboxRole, mailboxPath);
      const job = syncStore.enqueueJob({ accountId: account.id, ...target, reason: 'manual', priority: 100 });
      return { accountEmail: account.email, status: 'queued', synced: 0, jobId: job.id };
    });
    return output({ results });
  });

  server.registerTool('sync_policy_get', {
    title: '读取同步策略', description: '读取默认同步策略、账户级策略、邮箱同步状态和最近任务。',
    inputSchema: z.object({ email: z.string().email().optional() }), annotations: { readOnlyHint: true, idempotentHint: true },
  }, async ({ email }) => {
    const data = await readStore(); const syncStore = getSyncStore();
    const accounts = email ? [accountByEmail(data, email)] : data.accounts;
    return output({
      defaultPolicy: syncStore.getDefaultPolicy(),
      accounts: accounts.map((account) => ({ accountEmail: account.email, policy: syncStore.ensurePolicy(account.id), states: syncStore.listMailboxStates(account.id), jobs: syncStore.listJobs({ accountId: account.id, limit: 10 }) })),
    });
  });

  server.registerTool('sync_policy_update', {
    title: '更新同步策略', description: '更新全局默认策略或指定邮箱的后端自动同步策略。仅影响调度，不依赖任何前端连接。',
    inputSchema: z.object({
      email: z.string().email().optional().describe('不填时修改新账户使用的默认策略'),
      enabled: z.boolean().optional(), intervalMinutes: z.number().int().min(1).max(60).optional(),
      folderMode: z.enum(['inbox', 'standard', 'selected']).optional(), selectedMailboxes: z.array(z.string().trim().min(1).max(500)).max(100).optional(),
      syncOnStart: z.boolean().optional(), retryOnRecovery: z.boolean().optional(), notifyOnError: z.boolean().optional(),
    }).refine((value) => Object.keys(value).some((key) => key !== 'email'), '至少提供一个同步设置'),
    annotations: { idempotentHint: true },
  }, async ({ email, ...changes }) => {
    const syncStore = getSyncStore();
    if (!email) return output({ policy: syncStore.updateDefaultPolicy(changes) });
    const account = accountByEmail(await readStore(), email);
    return output({ accountEmail: account.email, policy: syncStore.updatePolicy(account.id, changes) });
  });

  server.registerTool('messages_list', {
    title: '查询邮件', description: '分页查询本地邮件缓存，可按邮箱、文件夹、搜索词、未读、星标、附件、标签和稍后处理过滤。',
    inputSchema: z.object({
      email: z.string().email().optional(), group: z.string().max(40).optional(), query: z.string().max(200).optional(),
      mailboxRole: mailboxRoleSchema.optional(), mailboxPath: z.string().max(500).optional(), unread: z.boolean().optional(),
      flagged: z.boolean().optional(), hasAttachments: z.boolean().optional(), snoozed: z.boolean().optional(), label: z.string().max(80).optional(),
      limit: z.number().int().min(1).max(100).default(25), offset: z.number().int().min(0).default(0),
    }), annotations: { readOnlyHint: true, idempotentHint: true },
  }, async (input) => {
    const data = await readStore();
    const accountId = input.email ? accountByEmail(data, input.email).id : undefined;
    const page = await listCachedMessages({
      accountId, group: input.group, query: input.query, mailboxRole: input.mailboxPath ? undefined : input.mailboxRole,
      mailbox: input.mailboxPath, unread: input.unread, flagged: input.flagged, hasAttachments: input.hasAttachments,
      snoozed: input.snoozed, label: input.label, limit: input.limit, offset: input.offset,
    });
    const accounts = new Map(data.accounts.map((item) => [item.id, item.email]));
    return output({ messages: page.messages.map((message) => gatewayMessageSummary(message, accounts.get(message.accountId) ?? '')), total: page.total, offset: input.offset, nextOffset: input.offset + page.messages.length, hasMore: input.offset + page.messages.length < page.total });
  });

  server.registerTool('message_get', {
    title: '读取邮件', description: '读取一封邮件的完整正文、HTML、标签和附件元数据。',
    inputSchema: z.object({ messageId: z.string().min(1) }), annotations: { readOnlyHint: true, idempotentHint: true },
  }, async ({ messageId }) => {
    const data = await readStore();
    const message = data.messages.find((item) => item.id === messageId);
    if (!message) throw new Error('邮件不存在');
    return output({ message: gatewayMessageDetail(message, messageAccount(data, message.accountId).email) });
  });

  server.registerTool('message_update', {
    title: '更新邮件状态', description: '更新源邮箱的已读/星标状态，以及 iMail 本地标签或稍后处理时间。',
    inputSchema: z.object({
      messageId: z.string().min(1), unread: z.boolean().optional(), flagged: z.boolean().optional(),
      labels: z.array(z.string().trim().min(1).max(40)).max(12).optional(), snoozedUntil: z.string().datetime().nullable().optional(),
    }).refine((value) => value.unread !== undefined || value.flagged !== undefined || value.labels !== undefined || value.snoozedUntil !== undefined, '至少提供一个要更新的字段'),
    annotations: { idempotentHint: true },
  }, async ({ messageId, ...changes }) => {
    if (changes.unread !== undefined || changes.flagged !== undefined) await updateRemoteMessageFlags(messageId, { unread: changes.unread, flagged: changes.flagged });
    let updated;
    await updateStore((data) => {
      updated = data.messages.find((item) => item.id === messageId);
      if (!updated) throw new Error('邮件不存在');
      Object.assign(updated, changes);
      if ('snoozedUntil' in changes) updated.snoozedUntil = changes.snoozedUntil ?? undefined;
    });
    const data = await readStore();
    return output({ message: gatewayMessageDetail(updated!, messageAccount(data, updated!.accountId).email) });
  });

  server.registerTool('message_move', {
    title: '移动邮件', description: '将邮件在源邮箱中归档或移至垃圾箱，并更新本地缓存。',
    inputSchema: z.object({ messageId: z.string().min(1), destination: z.enum(['archive', 'trash']) }),
    annotations: { destructiveHint: true, idempotentHint: false },
  }, async ({ messageId, destination }) => {
    const result = await moveRemoteMessage(messageId, destination);
    let moved;
    await updateStore((data) => {
      moved = data.messages.find((item) => item.id === messageId);
      if (!moved) throw new Error('邮件不存在');
      moved.mailbox = result.mailbox; moved.mailboxRole = destination; moved.snoozedUntil = undefined;
      if (result.uid) moved.uid = result.uid;
    });
    return output({ moved: true, messageId, destination, mailbox: result.mailbox });
  });

  server.registerTool('message_send', {
    title: '发送邮件', description: '通过指定邮箱发送文本或 HTML 邮件，可携带 Base64 附件。',
    inputSchema: z.object({
      accountEmail: z.string().email(), to: z.array(z.string().email()).min(1).max(100), cc: z.array(z.string().email()).max(100).optional(),
      subject: z.string().trim().min(1).max(500), text: z.string().min(1).max(2_000_000), html: z.string().max(8_000_000).optional(),
      attachments: z.array(attachmentSchema).max(10).optional(),
    }), annotations: { destructiveHint: false, idempotentHint: false },
  }, async ({ accountEmail, attachments, ...message }) => {
    const account = accountByEmail(await readStore(), accountEmail);
    const total = (attachments ?? []).reduce((sum, item) => sum + Buffer.byteLength(item.data, 'base64'), 0);
    if (total > 15 * 1024 * 1024) throw new Error('附件总大小不能超过 15 MB');
    return output({ delivery: await sendMessage({ ...message, accountId: account.id, attachments: attachments?.map(({ filename, contentType, data }) => ({ filename, contentType, data })) }) });
  });

  server.registerTool('attachment_download', {
    title: '下载邮件附件', description: '从源邮箱按需下载附件，返回文件名、MIME 类型和 Base64 内容。',
    inputSchema: z.object({ messageId: z.string().min(1), index: z.number().int().min(0) }), annotations: { readOnlyHint: true, idempotentHint: true },
  }, async ({ messageId, index }) => {
    if (!await getCachedMessage(messageId)) throw new Error('邮件不存在');
    const attachment = await downloadAttachment(messageId, index);
    return output({ filename: attachment.filename, contentType: attachment.contentType, size: attachment.content.length, data: attachment.content.toString('base64') });
  });

  server.registerTool('drafts_list', {
    title: '列出草稿', description: '列出本地草稿摘要，不返回附件 Base64 内容。',
    inputSchema: z.object({ accountEmail: z.string().email().optional() }), annotations: { readOnlyHint: true, idempotentHint: true },
  }, async ({ accountEmail }) => {
    const data = await readStore();
    const account = accountEmail ? accountByEmail(data, accountEmail) : undefined;
    const drafts = (data.drafts ?? []).filter((item) => !account || item.accountId === account.id).sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));
    return output({ drafts: drafts.map((draft) => draftSummary(draft, messageAccount(data, draft.accountId))) });
  });

  server.registerTool('draft_get', {
    title: '读取草稿', description: '读取一份本地草稿的完整正文和附件内容。',
    inputSchema: z.object({ draftId: z.string().uuid() }), annotations: { readOnlyHint: true, idempotentHint: true },
  }, async ({ draftId }) => {
    const data = await readStore();
    const draft = (data.drafts ?? []).find((item) => item.id === draftId);
    if (!draft) throw new Error('草稿不存在');
    return output({ draft: { ...draft, accountEmail: messageAccount(data, draft.accountId).email, accountId: undefined } });
  });

  server.registerTool('draft_save', {
    title: '保存草稿', description: '新建或覆盖本地草稿。传 draftId 时更新，否则新建。',
    inputSchema: z.object({
      draftId: z.string().uuid().optional(), accountEmail: z.string().email(), to: z.array(z.string().email()).default([]), cc: z.array(z.string().email()).default([]),
      subject: z.string().max(500).default(''), text: z.string().max(2_000_000).default(''), html: z.string().max(8_000_000).default(''), attachments: z.array(attachmentSchema).max(10).default([]),
    }), annotations: { idempotentHint: true },
  }, async ({ draftId, accountEmail, attachments, ...content }) => {
    const data = await readStore(); const account = accountByEmail(data, accountEmail); const now = new Date().toISOString();
    const total = attachments.reduce((sum, item) => sum + Buffer.byteLength(item.data, 'base64'), 0);
    if (total > 15 * 1024 * 1024) throw new Error('附件总大小不能超过 15 MB');
    let saved: Draft | undefined;
    await updateStore((store) => {
      if (draftId) {
        saved = (store.drafts ?? []).find((item) => item.id === draftId);
        if (!saved) throw new Error('草稿不存在');
        Object.assign(saved, content, { accountId: account.id, attachments: attachments.map((item) => ({ ...item, id: item.id ?? crypto.randomUUID(), size: Buffer.byteLength(item.data, 'base64') })), updatedAt: now });
      } else {
        saved = { id: crypto.randomUUID(), accountId: account.id, ...content, attachments: attachments.map((item) => ({ ...item, id: item.id ?? crypto.randomUUID(), size: Buffer.byteLength(item.data, 'base64') })), createdAt: now, updatedAt: now };
        (store.drafts ??= []).push(saved);
      }
    });
    return output({ draft: draftSummary(saved!, account) });
  });

  server.registerTool('draft_delete', {
    title: '删除草稿', description: '永久删除一份本地草稿。',
    inputSchema: z.object({ draftId: z.string().uuid() }), annotations: { destructiveHint: true, idempotentHint: true },
  }, async ({ draftId }) => {
    const data = await readStore();
    if (!(data.drafts ?? []).some((item) => item.id === draftId)) throw new Error('草稿不存在');
    await updateStore((store) => { store.drafts = (store.drafts ?? []).filter((item) => item.id !== draftId); });
    return output({ deleted: true, draftId });
  });

  server.registerTool('labels_list', {
    title: '列出邮件标签', description: '列出 iMail 本地使用过的全部邮件标签。',
    inputSchema: z.object({}), annotations: { readOnlyHint: true, idempotentHint: true },
  }, async () => output({ labels: Array.from(new Set((await readStore()).messages.flatMap((message) => message.labels ?? []))).sort((a, b) => a.localeCompare(b, 'zh-CN')) }));

  server.registerTool('notifications_list', {
    title: '列出邮箱通知', description: '列出连接异常、到期稍后邮件和最近未读邮件。',
    inputSchema: z.object({ limit: z.number().int().min(1).max(100).default(30) }), annotations: { readOnlyHint: true, idempotentHint: true },
  }, async ({ limit }) => {
    const data = await readStore(); const now = new Date().toISOString();
    const connection = data.accounts.filter((item) => item.status === 'error').map((account) => ({ id: `account-${account.id}`, kind: 'error', title: `${account.displayName} 连接异常`, detail: account.lastError || account.email, date: account.lastSyncAt || account.createdAt, accountEmail: account.email }));
    const returned = data.messages.filter((item) => item.snoozedUntil && item.snoozedUntil <= now).map((message) => ({ id: `snooze-${message.id}`, kind: 'snooze', title: message.subject, detail: '稍后处理的邮件已返回收件箱', date: message.snoozedUntil!, messageId: message.id, accountEmail: messageAccount(data, message.accountId).email }));
    const unread = data.messages.filter((item) => (item.mailboxRole ?? 'inbox') === 'inbox' && item.unread && (!item.snoozedUntil || item.snoozedUntil <= now)).map((message) => ({ id: `unread-${message.id}`, kind: 'unread', title: message.subject, detail: message.from.name || message.from.address, date: message.date, messageId: message.id, accountEmail: messageAccount(data, message.accountId).email }));
    return output({ notifications: [...connection, ...returned, ...unread].sort((a, b) => b.date.localeCompare(a.date)).slice(0, limit) });
  });

  return server;
}
