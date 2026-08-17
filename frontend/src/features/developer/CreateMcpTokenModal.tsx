import '../../styles/dialogs.css';
import { useState, type FormEvent } from 'preact/compat';
import { AppButton } from '../../components/AppButton';
import { Code, WarningCircle, X } from '../../components/icons';
import { api } from '../../services';
import { Overlay } from '../../components/shared';
import { AppInput, AppSelect } from '../../components/form-controls';
import { TokenCreatedResult } from './TokenCreatedResult';
import { useExternalAccessBaseUrl } from './external-access-endpoint';

export function CreateMcpTokenModal({ onClose, onCreated }: { onClose: () => void; onCreated: () => Promise<void> }) {
  const [raw, setRaw] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const { baseUrl } = useExternalAccessBaseUrl();

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault(); setBusy(true); setError('');
    const form = new FormData(event.currentTarget);
    try {
      const result = await api<{ token: string }>('/api/developer-tokens', { method: 'POST', body: JSON.stringify({ name: form.get('name'), mailboxes: [], scopes: ['mcp:full'], ttlSeconds: Number(form.get('ttlSeconds')) }) });
      setRaw(result.token); await onCreated();
    } catch (value) { setError(value instanceof Error ? value.message : 'MCP 授权码创建失败'); }
    finally { setBusy(false); }
  }

  return <Overlay onClose={onClose}>{raw ? <TokenCreatedResult raw={raw} kind="MCP 授权码" onClose={onClose} /> : <form className="token-modal mcp-token-modal" onSubmit={submit}>
    <div className="modal-header"><div><span>MCP 接入</span><h2>创建 MCP 授权码</h2><p>供可信 Agent 通过 MCP 管理 iMail。</p></div><button type="button" aria-label="关闭 MCP 授权码创建窗口" onClick={onClose}><X size={21} /></button></div>
    <div className="mcp-access-summary"><Code size={20} /><div><strong>完整控制权限</strong><span>可管理全部当前及未来邮箱、邮件、草稿与账户授权。</span><code>{baseUrl ? `${baseUrl}/mcp` : '正在准备 MCP 地址…'}</code></div></div>
    <label><span>Agent 或客户端名称</span><AppInput name="name" defaultValue="本地 MCP Agent" required /></label>
    <label><span>有效时间</span><AppSelect name="ttlSeconds" defaultValue="3600" options={[{ value: '1800', label: '30 分钟' }, { value: '3600', label: '1 小时' }, { value: '21600', label: '6 小时' }, { value: '86400', label: '24 小时' }, { value: '604800', label: '7 天' }]} /></label>
    <div className="inline-warning"><WarningCircle size={18} /><span><strong>仅签发给可信 Agent</strong> MCP 授权码以 <code>imail_mcp_</code> 开头，不要用于 REST API 请求。</span></div>
    {error && <div className="inline-error"><WarningCircle size={17} />{error}</div>}
    <div className="modal-footer"><button type="button" onClick={onClose}>取消</button><AppButton appearance="primary" type="submit" disabled={busy}>{busy ? '创建中…' : '创建 MCP 授权码'}</AppButton></div>
  </form>}</Overlay>;
}
