import { useState, type FormEvent } from 'react';
import { Button } from '@fluentui/react-components';
import { CheckCircle, PlugsConnected } from '@phosphor-icons/react';
import { AppInput } from '../../components/form-controls';
import { configuredServiceUrl, normalizeServiceUrl, saveServiceUrl } from '../../service-config';
import { desktopTestService } from '../../desktop-http';
import { isTauriRuntime } from '../../platform/tauri-runtime';

export function ServiceAddressEditor({ compact = false, onCancel, onSaved }: { compact?: boolean; onCancel?: () => void; onSaved?: () => void }) {
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError('');
    try {
      const normalized = normalizeServiceUrl(String(new FormData(event.currentTarget).get('serviceUrl') ?? ''));
      if (!normalized) throw new Error('请输入 iMail 服务地址');
      if (isTauriRuntime()) {
        const response = await desktopTestService(normalized);
        if (response.status < 200 || response.status >= 300) throw new Error(`服务返回 ${response.status}`);
      } else {
        const response = await fetch(`${normalized}/api/auth/status`, { credentials: 'include', signal: AbortSignal.timeout(8_000) });
        if (!response.ok) throw new Error(`服务返回 ${response.status}`);
      }
      saveServiceUrl(normalized);
      window.dispatchEvent(new Event('imail:service-changed'));
      onSaved?.();
    } catch (reason) {
      setError(reason instanceof Error ? `无法连接：${reason.message}` : '无法连接 iMail 服务');
    } finally { setBusy(false); }
  }

  return <form className={`service-address-editor ${compact ? 'is-compact' : ''}`} onSubmit={submit}>
    <label><span><CheckCircle size={15} weight="fill" />服务地址</span><AppInput name="serviceUrl" type="url" defaultValue={configuredServiceUrl()} placeholder="http://127.0.0.1:8787" autoFocus required /></label>
    {error && <div className="auth-error" role="alert">{error}</div>}
    <div className="service-address-actions">{onCancel && <button type="button" onClick={onCancel}>取消</button>}<Button appearance="primary" type="submit" disabled={busy} icon={<PlugsConnected size={16} />}>{busy ? '正在连接…' : '测试并保存'}</Button></div>
  </form>;
}
