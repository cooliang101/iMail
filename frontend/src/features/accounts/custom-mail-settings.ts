export type CustomMailSettings = {
  imapHost: string;
  imapPort: number;
  imapSecure: boolean;
  smtpHost: string;
  smtpPort: number;
  smtpSecure: boolean;
};

export function customMailSettingsFromForm(form: FormData): CustomMailSettings {
  return {
    imapHost: String(form.get('imapHost') ?? '').trim(),
    imapPort: Number(form.get('imapPort')),
    imapSecure: form.get('imapSecurity') === 'implicit-tls',
    smtpHost: String(form.get('smtpHost') ?? '').trim(),
    smtpPort: Number(form.get('smtpPort')),
    smtpSecure: form.get('smtpSecurity') === 'implicit-tls',
  };
}
