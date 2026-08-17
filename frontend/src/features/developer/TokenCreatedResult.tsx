import { useState } from 'preact/compat';
import { AppButton } from '../../components/AppButton';
import { Check, Copy, WarningCircle } from '../../components/icons';

export function TokenCreatedResult({ raw, kind, onClose }: { raw: string; kind: 'API Token' | 'MCP 授权码'; onClose: () => void }) {
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState('');

  return <div className="token-created">
    <div className="success-orbit"><Check size={28} weight="bold" /></div>
    <h2>{kind}已创建</h2>
    <p>请现在复制并保存，关闭后无法再次查看完整值。</p>
    <div className="raw-token"><code>{raw}</code><button onClick={async () => {
      try { await navigator.clipboard.writeText(raw); setCopied(true); }
      catch { setError('复制失败，请手动选择授权码'); }
    }}><Copy size={17} />{copied ? '已复制' : '复制'}</button></div>
    {error && <div className="inline-error"><WarningCircle size={17} />{error}</div>}
    <AppButton appearance="primary" onClick={onClose}>完成</AppButton>
  </div>;
}
