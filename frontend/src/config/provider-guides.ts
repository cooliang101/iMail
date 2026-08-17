import type { ProviderId } from '../types';

export type CredentialGuide = {
  title: string;
  description: string;
  secretLabel: string;
  secretPlaceholder: string;
  actionLabel: string;
  helpUrl: string;
  steps: string[];
};

const guides: Partial<Record<ProviderId, CredentialGuide>> = {
  outlook: {
    title: '使用 Microsoft 应用专用密码',
    description: '仅适用于已开启两步验证、且账户或组织仍允许 IMAP/SMTP 密码验证的 Microsoft 账户。',
    secretLabel: 'Microsoft 应用专用密码',
    secretPlaceholder: '粘贴 Microsoft 生成的应用密码',
    actionLabel: '查看 Microsoft 官方说明',
    helpUrl: 'https://support.microsoft.com/account-billing/manage-app-passwords-for-two-step-verification-d6dc8c6d-4bf7-4851-ad95-6d07799387e9',
    steps: ['为 Microsoft 账户开启两步验证', '在安全设置中创建新的应用密码', '确认组织允许 IMAP 与 SMTP AUTH，然后粘贴生成的密码'],
  },
  gmail: {
    title: '使用 Google 应用专用密码',
    description: '仅在 Google 账户已开启两步验证、且当前无法使用 OAuth 时使用。',
    secretLabel: 'Google 应用专用密码',
    secretPlaceholder: '粘贴 16 位应用专用密码',
    actionLabel: '打开 Google 应用专用密码',
    helpUrl: 'https://myaccount.google.com/apppasswords',
    steps: ['登录 Google 账户并开启两步验证', '创建一个名为 iMail 的应用专用密码', '复制生成的 16 位密码并粘贴到下方'],
  },
  qq: {
    title: '获取 QQ 邮箱授权码',
    description: 'QQ 邮箱没有公开的第三方邮件 OAuth，官方接入方式是 IMAP/SMTP 授权码。',
    secretLabel: 'QQ 邮箱授权码',
    secretPlaceholder: '粘贴邮箱生成的授权码',
    actionLabel: '查看 QQ 官方授权码说明',
    helpUrl: 'https://help.mail.qq.com/detail/106/985',
    steps: ['登录 QQ 邮箱，进入“设置 → 账号与安全”', '开启 IMAP/SMTP 服务并按页面要求完成安全验证', '生成授权码并粘贴到下方，不要填写 QQ 登录密码'],
  },
  yahoo: {
    title: '获取 Yahoo 第三方应用密码',
    description: 'iMail 的 Yahoo OAuth 应用未获 mail-r/mail-w 审核时，可使用 Yahoo 官方第三方应用密码。',
    secretLabel: 'Yahoo 第三方应用密码',
    secretPlaceholder: '粘贴 Yahoo 生成的应用密码',
    actionLabel: '打开 Yahoo 账户安全',
    helpUrl: 'https://login.yahoo.com/account/security',
    steps: ['登录 Yahoo 账户安全页面', '在“External connections”中选择“Create app password”', '输入 iMail 作为应用名称，生成并粘贴密码'],
  },
  icloud: {
    title: '获取 Apple 应用专用密码',
    description: 'Apple 目前只向受支持应用开放账户授权；普通跨平台邮件客户端使用应用专用密码。',
    secretLabel: 'Apple 应用专用密码',
    secretPlaceholder: 'xxxx-xxxx-xxxx-xxxx',
    actionLabel: '打开 Apple 账户',
    helpUrl: 'https://account.apple.com/account/manage',
    steps: ['确认 Apple 账户已开启双重认证', '进入“登录与安全 → App 专用密码”并创建 iMail 密码', '复制生成的密码并粘贴到下方'],
  },
  hotmail: {
    title: '使用 Microsoft 应用专用密码',
    description: '仅适用于已开启两步验证、且账户仍允许 IMAP/SMTP 密码验证的 Outlook.com 个人账户。',
    secretLabel: 'Microsoft 应用专用密码',
    secretPlaceholder: '粘贴 Microsoft 生成的应用密码',
    actionLabel: '查看 Microsoft 官方说明',
    helpUrl: 'https://support.microsoft.com/account-billing/manage-app-passwords-for-two-step-verification-d6dc8c6d-4bf7-4851-ad95-6d07799387e9',
    steps: ['为 Microsoft 个人账户开启两步验证', '在高级安全选项中创建新的应用密码', '复制生成的密码并粘贴到下方'],
  },
};

export function credentialGuideFor(provider: ProviderId) {
  return guides[provider];
}

export function oauthCallbackOrigins(redirectUris: string[], currentOrigin: string) {
  const origins = new Set([currentOrigin]);
  for (const redirectUri of redirectUris) {
    try { origins.add(new URL(redirectUri).origin); } catch { /* 服务端会负责报告无效回调配置 */ }
  }
  return origins;
}
