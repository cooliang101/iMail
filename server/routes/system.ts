import { Router } from 'express';
import { oauthProviderCatalog } from '../oauth.js';

export const systemRouter = Router();

systemRouter.get('/health', (_req, res) => res.json({ ok: true, service: 'imail' }));

systemRouter.get('/providers', (_req, res) => res.json({
  providers: [
    { id: 'outlook', name: 'Outlook / Microsoft 365', hint: '优先使用 Microsoft 安全登录，也可使用账户允许的应用专用密码', authMode: 'oauth2', oauthProvider: 'microsoft', fallbackAuthMode: 'app-password', helpUrl: 'https://support.microsoft.com/account-billing/manage-app-passwords-for-two-step-verification-d6dc8c6d-4bf7-4851-ad95-6d07799387e9' },
    { id: 'gmail', name: 'Gmail', hint: '使用 Google 安全登录', authMode: 'oauth2', oauthProvider: 'google', fallbackAuthMode: 'app-password' },
    { id: 'qq', name: 'QQ 邮箱', hint: 'QQ 未公开邮件 OAuth，请使用授权码', authMode: 'authorization-code', oauthProvider: null, helpUrl: 'https://help.mail.qq.com/detail/106/985' },
    { id: 'yahoo', name: 'Yahoo', hint: 'OAuth 需要 Yahoo Mail 接入审核；未审核可使用第三方应用密码', authMode: 'oauth2', oauthProvider: 'yahoo', fallbackAuthMode: 'app-password', helpUrl: 'https://login.yahoo.com/account/security' },
    { id: 'hotmail', name: 'Hotmail / Outlook.com', hint: '优先使用 Microsoft 个人账户安全登录，也可使用账户允许的应用专用密码', authMode: 'oauth2', oauthProvider: 'microsoft', oauthTenant: 'consumers', fallbackAuthMode: 'app-password', helpUrl: 'https://support.microsoft.com/account-billing/manage-app-passwords-for-two-step-verification-d6dc8c6d-4bf7-4851-ad95-6d07799387e9' },
    { id: 'icloud', name: 'iCloud', hint: '普通跨平台客户端使用 Apple 应用专用密码', authMode: 'app-password', oauthProvider: null, helpUrl: 'https://account.apple.com/account/manage' },
    { id: 'custom', name: '其他邮箱', hint: '自定义 IMAP / SMTP', authMode: 'custom' },
  ],
  oauth: oauthProviderCatalog(),
}));
