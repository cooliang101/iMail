import type { McpServer } from '@modelcontextprotocol/server';
import { z } from 'zod';
import {
  accountByEmail, createAccount, removeAccount, replaceAccountPassword, updateAccountMetadata, updateAccountProxy,
} from '../../domain/accounts.js';
import {
  accountColorSchema, accountMetadataFieldsSchema, appPasswordSchema, DEFAULT_ACCOUNT_COLOR,
  accountProxyUpdateSchema, mailProxySchema, mailSettingsSchema, oauthProviderSchema, providerSchema, workspaceIconSchema,
} from '../../domain/schemas.js';
import { publicAccount } from '../../http/presenters.js';
import { beginOAuth, beginOAuthReconnect, validateStoredAccountConnection } from '../../oauth.js';
import { readStore } from '../../store.js';
import { output } from '../support.js';

export function registerAccountTools(server: McpServer) {
  server.registerTool('accounts_list', {
    title: '列出邮箱账户', description: '列出所有邮箱账户、连接状态和文件夹，不返回任何凭据。',
    inputSchema: z.object({}), annotations: { readOnlyHint: true, idempotentHint: true },
  }, async () => output({ accounts: (await readStore()).accounts.map(publicAccount) }));

  server.registerTool('account_add_with_code', {
    title: '用邮箱授权码添加账户', description: '使用服务商生成的授权码或应用专用密码添加并验证 IMAP/SMTP 邮箱。凭据加密保存且不会返回。',
    inputSchema: z.object({
      provider: providerSchema, email: z.string().email(), displayName: z.string().min(1).max(80),
      authorizationCode: appPasswordSchema, group: z.string().min(1).max(40).default('个人'),
      groupIcon: workspaceIconSchema.default('folder'), color: accountColorSchema.default(DEFAULT_ACCOUNT_COLOR),
      settings: mailSettingsSchema.optional().describe('provider=custom 时必须提供完整 IMAP/SMTP 设置'),
      proxy: mailProxySchema.optional().describe('可选的 HTTP、HTTPS 或 SOCKS5 代理；密码只会加密保存'),
    }), annotations: { idempotentHint: false },
  }, async (input) => output({ account: publicAccount(await createAccount({ ...input, password: input.authorizationCode })) }));

  server.registerTool('account_start_oauth', {
    title: '开始邮箱 OAuth 授权', description: '为 Gmail、Outlook、Hotmail 或已获准的 Yahoo 生成官方 OAuth 授权网址。用户需在浏览器完成登录。',
    inputSchema: z.object({
      provider: oauthProviderSchema, displayName: z.string().max(80).optional(),
      group: z.string().min(1).max(40).default('个人'), color: accountColorSchema.default(DEFAULT_ACCOUNT_COLOR),
      proxy: mailProxySchema.optional().describe('可选的 HTTP、HTTPS 或 SOCKS5 邮件代理'),
    }), annotations: { readOnlyHint: false, idempotentHint: false },
  }, async (input) => output(await beginOAuth(input) as unknown as Record<string, unknown>));

  server.registerTool('account_reconnect_oauth', {
    title: '重新授权 OAuth 邮箱', description: '为已有 OAuth 邮箱生成重新授权网址。',
    inputSchema: z.object({ email: z.string().email() }), annotations: { readOnlyHint: false, idempotentHint: false },
  }, async ({ email }) => output(await beginOAuthReconnect(await accountByEmail(email)) as unknown as Record<string, unknown>));

  server.registerTool('account_update', {
    title: '更新邮箱账户', description: '更新邮箱显示名称、工作空间、图标或颜色。',
    inputSchema: accountMetadataFieldsSchema.extend({ email: z.string().email() })
      .refine((value) => Object.keys(value).some((key) => key !== 'email'), '至少提供一个要更新的字段'),
    annotations: { idempotentHint: true },
  }, async ({ email, ...changes }) => {
    const account = await accountByEmail(email);
    return output({ account: publicAccount(await updateAccountMetadata(account.id, changes)) });
  });

  server.registerTool('account_update_authorization_code', {
    title: '更新邮箱授权码', description: '验证并替换非 OAuth 邮箱的授权码或应用专用密码。新凭据加密保存且不会返回。',
    inputSchema: z.object({ email: z.string().email(), authorizationCode: appPasswordSchema }), annotations: { idempotentHint: false },
  }, async ({ email, authorizationCode }) => {
    const account = await accountByEmail(email);
    return output({ account: publicAccount(await replaceAccountPassword(account.id, authorizationCode)) });
  });

  server.registerTool('account_test_connection', {
    title: '测试邮箱连接', description: '使用已保存凭据测试邮箱的 IMAP 与 SMTP 连接。',
    inputSchema: z.object({ email: z.string().email() }), annotations: { readOnlyHint: false, idempotentHint: true },
  }, async ({ email }) => output({ account: publicAccount(await validateStoredAccountConnection(await accountByEmail(email))) }));

  server.registerTool('account_proxy_update', {
    title: '更新邮箱代理', description: '启用、修改、关闭账户级代理，或复用另一邮箱的代理。代理密码加密保存且不会返回。',
    inputSchema: z.union([
      accountProxyUpdateSchema.and(z.object({ email: z.string().email() })),
      z.object({ email: z.string().email(), enabled: z.literal(true), sourceEmail: z.string().email() }),
    ]),
    annotations: { idempotentHint: false },
  }, async ({ email, ...input }) => {
    const account = await accountByEmail(email);
    const update = 'sourceEmail' in input
      ? { enabled: true as const, sourceAccountId: (await accountByEmail(input.sourceEmail)).id }
      : input;
    return output({ account: publicAccount(await updateAccountProxy(account.id, update)) });
  });

  server.registerTool('account_remove', {
    title: '移除邮箱账户', description: '从 iMail 移除账户，并删除该账户的本地邮件缓存。不会删除服务商服务器上的账户。',
    inputSchema: z.object({ email: z.string().email() }), annotations: { destructiveHint: true, idempotentHint: true },
  }, async ({ email }) => {
    const account = await accountByEmail(email);
    await removeAccount(account.id);
    return output({ removed: true, email: account.email });
  });
}
