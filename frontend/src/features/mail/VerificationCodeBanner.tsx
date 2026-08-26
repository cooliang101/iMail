import { useEffect, useState } from 'preact/compat';
import { CheckCircle, Copy, Key } from '../../components/icons';

export function VerificationCodeBanner({ code }: { code: string }) {
  const [copied, setCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);

  useEffect(() => { setCopied(false); setCopyFailed(false); }, [code]);

  async function copyCode() {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setCopyFailed(false);
    } catch {
      setCopied(false);
      setCopyFailed(true);
    }
  }

  return <aside className="verification-code-banner" aria-label="邮件验证码">
    <span className="verification-code-icon"><Key size={19} weight="duotone" /></span>
    <span className="verification-code-copy"><small>检测到验证码</small><code>{code}</code></span>
    <span className="verification-code-action"><button type="button" onClick={() => void copyCode()}><Copy size={16} />复制验证码</button>{copied && <span className="verification-code-success" role="status" aria-label="复制成功" title="复制成功"><CheckCircle size={19} weight="fill" /></span>}</span>
    {copyFailed && <small className="verification-code-error" role="alert">复制失败，请手动选择验证码</small>}
  </aside>;
}
