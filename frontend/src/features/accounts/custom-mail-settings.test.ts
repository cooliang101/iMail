import { describe, expect, it } from 'vitest';
import { customMailSettingsFromForm } from './custom-mail-settings';

function settings(values: Record<string, string>) {
  const form = new FormData();
  for (const [name, value] of Object.entries(values)) form.set(name, value);
  return customMailSettingsFromForm(form);
}

describe('custom mail connection settings', () => {
  it('keeps implicit TLS independent from conventional port numbers', () => {
    expect(settings({
      imapHost: ' imap.example.com ', imapPort: '1993', imapSecurity: 'implicit-tls',
      smtpHost: ' smtp.example.com ', smtpPort: '1465', smtpSecurity: 'implicit-tls',
    })).toEqual({
      imapHost: 'imap.example.com', imapPort: 1993, imapSecure: true,
      smtpHost: 'smtp.example.com', smtpPort: 1465, smtpSecure: true,
    });
  });

  it('supports STARTTLS explicitly for IMAP and SMTP', () => {
    expect(settings({
      imapHost: 'imap.example.com', imapPort: '143', imapSecurity: 'starttls',
      smtpHost: 'smtp.example.com', smtpPort: '587', smtpSecurity: 'starttls',
    })).toMatchObject({ imapPort: 143, imapSecure: false, smtpPort: 587, smtpSecure: false });
  });
});
