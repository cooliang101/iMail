import { z } from 'zod';

export const DEFAULT_ACCOUNT_COLOR = '#168f78';
export const providerSchema = z.enum(['outlook', 'gmail', 'qq', 'yahoo', 'hotmail', 'icloud', 'custom']);
export const oauthProviderSchema = z.enum(['outlook', 'gmail', 'yahoo', 'hotmail']);
export const workspaceIconSchema = z.enum(['folder', 'briefcase', 'building', 'home', 'users', 'code', 'heart', 'star']);
export const mailboxRoleSchema = z.enum(['inbox', 'sent', 'archive', 'drafts', 'trash', 'junk', 'custom']);
export const accountColorSchema = z.string().regex(/^#[0-9a-fA-F]{6}$/);
export const appPasswordSchema = z.string().min(1).max(512);

export const proxyProtocolSchema = z.enum(['http', 'https', 'socks5']);
const proxyHostSchema = z.string().trim().min(1).max(253).refine((value) => !value.includes('://') && !/[\s/?#]/.test(value), '代理主机格式无效');
export const mailProxySchema = z.object({
  protocol: proxyProtocolSchema,
  host: proxyHostSchema,
  port: z.number().int().min(1).max(65535),
  username: z.string().trim().max(256).optional().transform((value) => value || undefined),
  password: z.string().max(512).optional(),
});

export const accountProxyUpdateSchema = z.discriminatedUnion('enabled', [
  z.object({ enabled: z.literal(false) }),
  mailProxySchema.extend({ enabled: z.literal(true) }),
]);

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
