import { useState } from 'preact/compat';
import { AppButton } from '../../components/AppButton';
import { Check, Copy, WarningCircle } from '../../components/icons';

export function TokenCreatedResult({ raw, kind, agentPayload, onClose }: { raw: string; kind: 'API Token' | 'MCP 授权码'; agentPayload?: string; onClose: () => void }) {
  const [copied, setCopied] = useState(false);
  const [agentCopied, setAgentCopied] = useState(false);
  const [error, setError] = useState('');

  return <div className="token-created">
    <div className="success-orbit"><Check size={28} weight="bold" /></div>
    <h2>{kind}已创建</h2>
    <p>请现在复制并保存，关闭后无法再次查看完整值。</p>
    <div className="raw-token"><code>{raw}</code><button onClick={async () => {
      try { await navigator.clipboard.writeText(raw); setCopied(true); }
      catch { setError('复制失败，请手动选择授权码'); }
    }}><Copy size={17} />{copied ? '已复制' : '复制'}</button></div>
    {agentPayload && <button className="copy-agent-payload" type="button" onClick={async () => {
      try { await navigator.clipboard.writeText(agentPayload); setAgentCopied(true); setError(''); }
      catch { setError('复制失败，请手动复制 Token 和网关地址'); }
    }}><Copy size={17} />{agentCopied ? '已复制，可粘贴给 Agent' : '复制完整调用信息给 Agent'}</button>}
    {error && <div className="inline-error"><WarningCircle size={17} />{error}</div>}
    <AppButton appearance="primary" onClick={onClose}>完成</AppButton>
  </div>;
}
