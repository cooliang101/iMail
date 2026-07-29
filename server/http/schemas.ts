import { z } from 'zod';

export const providerSchema = z.enum(['outlook', 'gmail', 'qq', 'yahoo', 'hotmail', 'icloud', 'custom']);

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

export const draftSchema = z.object({
  accountId: z.string().uuid(),
  to: z.array(z.string().email()).default([]),
  cc: z.array(z.string().email()).default([]),
  subject: z.string().max(500).default(''),
  text: z.string().max(2_000_000).default(''),
});

export const sendSchema = z.object({
  accountId: z.string().uuid(),
  to: z.array(z.string().email()).min(1),
  cc: z.array(z.string().email()).optional(),
  subject: z.string().min(1),
  text: z.string().min(1),
  html: z.string().optional(),
  draftId: z.string().uuid().optional(),
});

export const tokenSchema = z.object({
  name: z.string().min(1).max(80),
  scopes: z.array(z.enum(['messages:read', 'messages:send', 'accounts:read'])).min(1),
  mailboxes: z.array(z.string().email()).min(1),
  ttlSeconds: z.number().int().min(300).max(7 * 24 * 3600),
});
