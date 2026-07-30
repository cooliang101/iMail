import crypto from 'node:crypto';
import type { McpServer } from '@modelcontextprotocol/server';
import { z } from 'zod';
import { accountByEmail } from '../../domain/accounts.js';
import { deleteDraft, getDraft, listDrafts, saveDraft } from '../../domain/drafts.js';
import { conflict } from '../../domain/errors.js';
import { readStore } from '../../store.js';
import type { Draft, MailAccount } from '../../types.js';
import { attachmentSchema, output } from '../support.js';

function draftSummary(draft: Draft, account: MailAccount) {
  return {
    id: draft.id, accountEmail: account.email, to: draft.to, cc: draft.cc, subject: draft.subject,
    textPreview: draft.text.replace(/\s+/g, ' ').slice(0, 180), attachments: draft.attachments.map(({ data: _data, ...item }) => item),
    createdAt: draft.createdAt, updatedAt: draft.updatedAt,
  };
}

function draftAccount(accounts: MailAccount[], accountId: string) {
  const account = accounts.find((item) => item.id === accountId);
  if (!account) throw conflict('DRAFT_ACCOUNT_MISSING', '草稿关联的邮箱账户不存在');
  return account;
}

export function registerDraftTools(server: McpServer) {
  server.registerTool('drafts_list', {
    title: '列出草稿', description: '列出本地草稿摘要，不返回附件 Base64 内容。',
    inputSchema: z.object({ accountEmail: z.string().email().optional() }), annotations: { readOnlyHint: true, idempotentHint: true },
  }, async ({ accountEmail }) => {
    const data = await readStore();
    const account = accountEmail ? await accountByEmail(accountEmail) : undefined;
    const drafts = (await listDrafts()).filter((item) => !account || item.accountId === account.id).sort((a, b) => b.updatedAt.localeCompare(a.updatedAt));
    return output({ drafts: drafts.map((draft) => draftSummary(draft, draftAccount(data.accounts, draft.accountId))) });
  });

  server.registerTool('draft_get', {
    title: '读取草稿', description: '读取一份本地草稿的完整正文和附件内容。',
    inputSchema: z.object({ draftId: z.string().uuid() }), annotations: { readOnlyHint: true, idempotentHint: true },
  }, async ({ draftId }) => {
    const data = await readStore();
    const draft = await getDraft(draftId);
    return output({ draft: { ...draft, accountEmail: draftAccount(data.accounts, draft.accountId).email, accountId: undefined } });
  });

  server.registerTool('draft_save', {
    title: '保存草稿', description: '新建或覆盖本地草稿。传 draftId 时更新，否则新建。',
    inputSchema: z.object({
      draftId: z.string().uuid().optional(), accountEmail: z.string().email(), to: z.array(z.string().email()).default([]), cc: z.array(z.string().email()).default([]),
      subject: z.string().max(500).default(''), text: z.string().max(2_000_000).default(''), html: z.string().max(8_000_000).default(''), attachments: z.array(attachmentSchema).max(10).default([]),
    }), annotations: { idempotentHint: true },
  }, async ({ draftId, accountEmail, attachments, ...content }) => {
    const account = await accountByEmail(accountEmail);
    const total = attachments.reduce((sum, item) => sum + Buffer.byteLength(item.data, 'base64'), 0);
    if (total > 15 * 1024 * 1024) throw conflict('DRAFT_ATTACHMENTS_TOO_LARGE', '附件总大小不能超过 15 MB');
    if (draftId && (await getDraft(draftId)).accountId !== account.id) throw conflict('DRAFT_ACCOUNT_MISMATCH', '草稿与指定邮箱不匹配');
    const saved = await saveDraft({
      accountId: account.id, ...content,
      attachments: attachments.map((item) => ({ ...item, id: item.id ?? crypto.randomUUID(), size: Buffer.byteLength(item.data, 'base64') })),
    }, draftId);
    return output({ draft: draftSummary(saved, account) });
  });

  server.registerTool('draft_delete', {
    title: '删除草稿', description: '永久删除一份本地草稿。',
    inputSchema: z.object({ draftId: z.string().uuid() }), annotations: { destructiveHint: true, idempotentHint: true },
  }, async ({ draftId }) => {
    await getDraft(draftId); await deleteDraft(draftId);
    return output({ deleted: true, draftId });
  });
}
