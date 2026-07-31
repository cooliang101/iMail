import crypto from 'node:crypto';
import { readStore, updateStore } from '../store.js';
import type { Draft } from '../types.js';
import { notFound } from './errors.js';

export type DraftInput = Pick<Draft, 'accountId' | 'to' | 'cc' | 'subject' | 'text' | 'html' | 'attachments'>;

function assertAccount(data: Awaited<ReturnType<typeof readStore>>, accountId: string) {
  if (!data.accounts.some((item) => item.id === accountId)) throw notFound('ACCOUNT_NOT_FOUND', '发件邮箱不存在');
}

export async function listDrafts() { return (await readStore()).drafts ?? []; }

export async function getDraft(id: string) {
  const draft = (await readStore()).drafts?.find((item) => item.id === id);
  if (!draft) throw notFound('DRAFT_NOT_FOUND', '草稿不存在');
  return draft;
}

export async function createDraft(input: DraftInput, id: string = crypto.randomUUID()) {
  const now = new Date().toISOString();
  let saved: Draft | undefined;
  await updateStore((data) => {
    assertAccount(data, input.accountId);
    const drafts = data.drafts ??= [];
    saved = drafts.find((item) => item.id === id);
    if (saved) Object.assign(saved, input, { updatedAt: now });
    else {
      saved = { id, ...input, createdAt: now, updatedAt: now };
      drafts.push(saved);
    }
  });
  return saved!;
}

export async function saveDraft(input: DraftInput, id?: string) {
  if (!id) return createDraft(input);
  const now = new Date().toISOString();
  let saved: Draft | undefined;
  await updateStore((data) => {
    assertAccount(data, input.accountId);
    saved = (data.drafts ?? []).find((item) => item.id === id);
    if (!saved) throw notFound('DRAFT_NOT_FOUND', '草稿不存在');
    Object.assign(saved, input, { updatedAt: now });
  });
  return saved!;
}

export async function deleteDraft(id: string) {
  let deleted = false;
  await updateStore((data) => {
    const drafts = data.drafts ?? [];
    deleted = drafts.some((item) => item.id === id);
    data.drafts = drafts.filter((item) => item.id !== id);
  });
  return deleted;
}
