import { McpServer } from '@modelcontextprotocol/server';
import { z } from 'zod';
import { buildNotifications } from '../domain/notifications.js';
import { invalid, notFound } from '../domain/errors.js';
import {
  mailboxRoleSchema,
} from '../domain/schemas.js';
import { downloadAttachment, moveRemoteMessage, sendMessage, updateRemoteMessageFlags } from '../mail.js';
import { gatewayMessageDetail, gatewayMessageSummary } from '../gateway/presenters.js';
import { getCachedMessage, getMessageStats, listCachedMessages, readStore, updateStore } from '../store.js';
import { getSyncStore } from '../sync/store.js';
import { canonicalSyncTarget } from '../mail/mailbox-role.js';
import { appPreferencesUpdateSchema, readAppPreferences, updateAppPreferences } from '../preferences.js';
import { customThemeSchema, readMcpCustomTheme, updateMcpCustomTheme } from './custom-theme.js';
import { registerAccountTools } from './tools/accounts.js';
import { registerDraftTools } from './tools/drafts.js';
import { attachmentSchema, output } from './support.js';

function messageAccount(data: Awaited<ReturnType<typeof readStore>>, accountId: string) {
  const account = data.accounts.find((item) => item.id === accountId);
  if (!account) throw notFound('ACCOUNT_NOT_FOUND', '邮箱账户不存在');
  return account;
}

function accountByEmail(data: Awaited<ReturnType<typeof readStore>>, email: string) {
  const account = data.accounts.find((item) => item.email.toLowerCase() === email.toLowerCase());
  if (!account) throw notFound('ACCOUNT_NOT_FOUND', `邮箱账户不存在：${email}`);
  return account;
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
    title: '读取 iMail 设置', description: '读取主题、启动页面、阅读、通知、邮件展示和快捷键偏好。',
    inputSchema: z.object({}), annotations: { readOnlyHint: true, idempotentHint: true },
  }, async () => output({ preferences: await readAppPreferences() }));

  server.registerTool('settings_update', {
    title: '更新 iMail 设置', description: '更新主题、启动页面、阅读、通知、邮件展示或快捷键偏好；未提供的字段保持不变。',
    inputSchema: appPreferencesUpdateSchema, annotations: { idempotentHint: true },
  }, async (changes) => output({ preferences: await updateAppPreferences(changes) }));

  server.registerTool('theme_custom_get', {
    title: '读取自定义主题', description: '读取当前应用账号由 MCP 保存的安全自定义主题令牌，可复制到 iMail 自定义主题编辑器中。',
    inputSchema: z.object({}), annotations: { readOnlyHint: true, idempotentHint: true },
  }, async () => output({ theme: await readMcpCustomTheme() }));

  server.registerTool('theme_custom_update', {
    title: '保存自定义主题', description: '校验并保存颜色、圆角、阴影和字体令牌；不接受 CSS、URL 或可执行内容，也不经过 HTTP 网关。',
    inputSchema: customThemeSchema, annotations: { idempotentHint: true },
  }, async (theme) => output({ theme: await updateMcpCustomTheme(theme) }));

  registerAccountTools(server);

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
    title: '读取自动同步设置', description: '读取默认及账户级自动同步设置、邮箱校准状态和最近任务。',
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
    title: '更新自动同步设置', description: '更新全局默认设置或指定邮箱的自动同步开关、文件夹范围和失败通知。变化唤醒与后台校准不依赖任何前端连接。',
    inputSchema: z.object({
      email: z.string().email().optional().describe('不填时修改新账户使用的默认策略'),
      enabled: z.boolean().optional(),
      folderMode: z.enum(['inbox', 'standard', 'selected']).optional(), selectedMailboxes: z.array(z.string().trim().min(1).max(500)).max(100).optional(),
      notifyOnError: z.boolean().optional(),
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
    if (!message) throw notFound('MESSAGE_NOT_FOUND', '邮件不存在');
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
      if (!updated) throw notFound('MESSAGE_NOT_FOUND', '邮件不存在');
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
      if (!moved) throw notFound('MESSAGE_NOT_FOUND', '邮件不存在');
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
    if (total > 15 * 1024 * 1024) throw invalid('ATTACHMENTS_TOO_LARGE', '附件总大小不能超过 15 MB');
    return output({ delivery: await sendMessage({ ...message, accountId: account.id, attachments: attachments?.map(({ filename, contentType, data }) => ({ filename, contentType, data })) }) });
  });

  server.registerTool('attachment_download', {
    title: '下载邮件附件', description: '从源邮箱按需下载附件，返回文件名、MIME 类型和 Base64 内容。',
    inputSchema: z.object({ messageId: z.string().min(1), index: z.number().int().min(0) }), annotations: { readOnlyHint: true, idempotentHint: true },
  }, async ({ messageId, index }) => {
    if (!await getCachedMessage(messageId)) throw notFound('MESSAGE_NOT_FOUND', '邮件不存在');
    const attachment = await downloadAttachment(messageId, index);
    return output({ filename: attachment.filename, contentType: attachment.contentType, size: attachment.content.length, data: attachment.content.toString('base64') });
  });

  registerDraftTools(server);

  server.registerTool('labels_list', {
    title: '列出邮件标签', description: '列出 iMail 本地使用过的全部邮件标签。',
    inputSchema: z.object({}), annotations: { readOnlyHint: true, idempotentHint: true },
  }, async () => output({ labels: Array.from(new Set((await readStore()).messages.flatMap((message) => message.labels ?? []))).sort((a, b) => a.localeCompare(b, 'zh-CN')) }));

  server.registerTool('notifications_list', {
    title: '列出邮箱通知', description: '列出连接异常、到期稍后邮件和最近未读邮件。',
    inputSchema: z.object({ limit: z.number().int().min(1).max(100).default(30) }), annotations: { readOnlyHint: true, idempotentHint: true },
  }, async ({ limit }) => {
    const data = await readStore();
    return output({ notifications: buildNotifications(data, limit).map(({ accountId, ...item }) => ({ ...item, accountEmail: messageAccount(data, accountId).email })) });
  });

  return server;
}
