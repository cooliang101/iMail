import { z } from 'zod';

export const DEFAULT_ACCOUNT_COLOR = '#168f78';
export const providerSchema = z.enum(['outlook', 'gmail', 'qq', 'yahoo', 'hotmail', 'icloud', 'custom']);
export const oauthProviderSchema = z.enum(['outlook', 'gmail', 'yahoo', 'hotmail']);
export const workspaceIconSchema = z.enum(['folder', 'briefcase', 'building', 'home', 'users', 'code', 'heart', 'star']);
export const mailboxRoleSchema = z.enum(['inbox', 'sent', 'archive', 'drafts', 'trash', 'junk', 'custom']);
export const accountColorSchema = z.string().regex(/^#[0-9a-fA-F]{6}$/);
export const appPasswordSchema = z.string().min(1).max(512);

export const mailSettingsSchema = z.object({
  imapHost: z.string().min(1), imapPort: z.number().int().min(1).max(65535), imapSecure: z.boolean(),
  smtpHost: z.string().min(1), smtpPort: z.number().int().min(1).max(65535), smtpSecure: z.boolean(),
});

export const accountMetadataFieldsSchema = z.object({
  displayName: z.string().trim().min(1).max(80).optional(),
  group: z.string().trim().min(1).max(40).optional(),
  groupIcon: workspaceIconSchema.optional(),
  color: accountColorSchema.optional(),
});
export const accountMetadataSchema = accountMetadataFieldsSchema.refine((value) => Object.keys(value).length > 0, '至少提供一个要更新的字段');
