import { useState } from 'react';
import { AppCheckbox, AppInput, AppSelect } from '../../components/form-controls';
import type { MailProxySettings } from '../../types';

export function ProxyFields({ proxy, compact = false }: { proxy?: MailProxySettings; compact?: boolean }) {
  const [enabled, setEnabled] = useState(Boolean(proxy));
  const [protocol, setProtocol] = useState(proxy?.protocol ?? 'http');
  return <section className={`account-proxy-fields ${compact ? 'is-compact' : ''}`}>
    <AppCheckbox name="proxyEnabled" checked={enabled} onChange={(_, data) => setEnabled(Boolean(data.checked))} label="通过代理连接邮件服务器" />
    {enabled && <div className="form-grid">
      <label><span>代理协议</span><AppSelect name="proxyProtocol" value={protocol} onValueChange={(value) => setProtocol(value as typeof protocol)} options={[
        { value: 'http', label: 'HTTP' }, { value: 'https', label: 'HTTPS' }, { value: 'socks5', label: 'SOCKS5' },
      ]} /></label>
      <label><span>代理主机</span><AppInput name="proxyHost" defaultValue={proxy?.host} placeholder="127.0.0.1" required /></label>
      <label><span>代理端口</span><AppInput name="proxyPort" type="number" min={1} max={65535} defaultValue={String(proxy?.port ?? (protocol === 'socks5' ? 1080 : 8080))} required /></label>
      <label><span>用户名（可选）</span><AppInput name="proxyUsername" defaultValue={proxy?.username} autoComplete="off" /></label>
      <label><span>密码（可选）</span><AppInput name="proxyPassword" type="password" placeholder={proxy ? '留空则保留现有密码' : '无认证可留空'} autoComplete="new-password" /></label>
    </div>}
  </section>;
}

export function proxyInputFromForm(form: FormData) {
  if (!form.has('proxyEnabled')) return { enabled: false as const };
  return {
    enabled: true as const,
    protocol: String(form.get('proxyProtocol')) as 'http' | 'https' | 'socks5',
    host: String(form.get('proxyHost') ?? '').trim(),
    port: Number(form.get('proxyPort')),
    username: String(form.get('proxyUsername') ?? '').trim() || undefined,
    password: String(form.get('proxyPassword') ?? '') || undefined,
  };
}
