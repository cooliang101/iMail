import { z } from 'zod';

export const providerSchema = z.enum(['outlook', 'gmail', 'qq', 'yahoo', 'hotmail', 'icloud', 'custom']);
export const workspaceIconSchema = z.enum(['folder', 'briefcase', 'building', 'home', 'users', 'code', 'heart', 'star']);

export const settingsSchema = z.object({
  imapHost: z.string().min(1),
  imapPort: z.number().int().min(1).max(65535),
  imapSecure: z.boolean(),
  smtpHost: z.string().min(1),
  smtpPort: z.number().int().min(1).max(65535),
  smtpSecure: z.boolean(),
});

export const accountSchema = z.object({
  provider: providerSchema,
  email: z.string().email(),
  displayName: z.string().min(1).max(80),
  group: z.string().min(1).max(40).default('个人'),
  groupIcon: workspaceIconSchema.default('folder'),
  color: z.string().regex(/^#[0-9a-fA-F]{6}$/).default('#17a887'),
  password: z.string().optional(),
  accessToken: z.string().optional(),
  settings: settingsSchema.optional(),
}).refine((value) => Boolean(value.password || value.accessToken), '请填写应用专用密码或 OAuth Access Token');

export const oauthStartSchema = z.object({
  provider: z.enum(['outlook', 'gmail', 'yahoo', 'hotmail']),
  displayName: z.string().max(80).optional(),
  group: z.string().min(1).max(40).default('个人'),
  color: z.string().regex(/^#[0-9a-fA-F]{6}$/).default('#168f78'),
});

export const mailboxRoleSchema = z.enum(['inbox', 'sent', 'archive', 'trash', 'custom']);

const draftAttachmentSchema = z.object({
  id: z.string().min(1).max(100),
  filename: z.string().min(1).max(255),
  contentType: z.string().min(1).max(150),
  size: z.number().int().nonnegative().max(5 * 1024 * 1024),
  data: z.string().max(7_000_000),
});

export const draftSchema = z.object({
  accountId: z.string().uuid(),
  to: z.array(z.string().trim().min(1).max(320)).default([]),
  cc: z.array(z.string().trim().min(1).max(320)).default([]),
  subject: z.string().max(500).default(''),
  text: z.string().max(2_000_000).default(''),
  html: z.string().max(8_000_000).default(''),
  attachments: z.array(draftAttachmentSchema).max(10).default([]),
}).superRefine((draft, context) => {
  if (draft.attachments.reduce((total, attachment) => total + attachment.size, 0) > 15 * 1024 * 1024) context.addIssue({ code: 'custom', path: ['attachments'], message: '附件总大小不能超过 15 MB' });
});

export const sendSchema = z.object({
  accountId: z.string().uuid(),
  to: z.array(z.string().email()).min(1),
  cc: z.array(z.string().email()).optional(),
  subject: z.string().min(1),
  text: z.string().min(1),
  html: z.string().optional(),
  attachments: z.array(draftAttachmentSchema).max(10).optional(),
  draftId: z.string().uuid().optional(),
}).superRefine((message, context) => {
  if ((message.attachments ?? []).reduce((total, attachment) => total + attachment.size, 0) > 15 * 1024 * 1024) context.addIssue({ code: 'custom', path: ['attachments'], message: '附件总大小不能超过 15 MB' });
});

export const tokenSchema = z.object({
  name: z.string().min(1).max(80),
  scopes: z.array(z.enum(['messages:read', 'messages:send', 'accounts:read', 'mcp:full'])).min(1),
  mailboxes: z.array(z.string().email()).default([]),
  ttlSeconds: z.number().int().min(300).max(7 * 24 * 3600),
}).refine((value) => value.mailboxes.length > 0 || value.scopes.includes('mcp:full'), { message: '非 MCP Token 至少需要选择一个邮箱', path: ['mailboxes'] });
