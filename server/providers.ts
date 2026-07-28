import type { MailSettings, ProviderId } from './types.js';

export const PROVIDERS: Record<Exclude<ProviderId, 'custom'>, MailSettings> = {
  outlook: { imapHost: 'outlook.office365.com', imapPort: 993, imapSecure: true, smtpHost: 'smtp.office365.com', smtpPort: 587, smtpSecure: false },
  hotmail: { imapHost: 'outlook.office365.com', imapPort: 993, imapSecure: true, smtpHost: 'smtp.office365.com', smtpPort: 587, smtpSecure: false },
  gmail: { imapHost: 'imap.gmail.com', imapPort: 993, imapSecure: true, smtpHost: 'smtp.gmail.com', smtpPort: 465, smtpSecure: true },
  qq: { imapHost: 'imap.qq.com', imapPort: 993, imapSecure: true, smtpHost: 'smtp.qq.com', smtpPort: 465, smtpSecure: true },
  yahoo: { imapHost: 'imap.mail.yahoo.com', imapPort: 993, imapSecure: true, smtpHost: 'smtp.mail.yahoo.com', smtpPort: 465, smtpSecure: true },
  icloud: { imapHost: 'imap.mail.me.com', imapPort: 993, imapSecure: true, smtpHost: 'smtp.mail.me.com', smtpPort: 587, smtpSecure: false },
};

export function settingsFor(provider: ProviderId, custom?: MailSettings): MailSettings {
  if (provider === 'custom') {
    if (!custom) throw new Error('通用邮箱需要完整的 IMAP/SMTP 配置');
    return custom;
  }
  return PROVIDERS[provider];
}
