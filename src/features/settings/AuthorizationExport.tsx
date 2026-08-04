import { useState, type FormEvent } from 'react';
import { Button } from '@fluentui/react-components';
import { DownloadSimple, FileLock, WarningCircle } from '@phosphor-icons/react';
import { api } from '../../api';
import type { Notice } from '../../app-model';
import { AppCheckbox, AppInput } from '../../components/form-controls';
import { usePlatform } from '../../platform/runtime';
import { authorizationExportPasswordError } from './privacy-actions';

type PreparedAuthorizationExport = {
  downloadPath: string;
  filename: string;
  accountCount: number;
  expiresAt: string;
};

export function AuthorizationExport({ accountCount, setNotice }: { accountCount: number; setNotice: (notice: Notice) => void }) {
  const platform = usePlatform();
  const [open, setOpen] = useState(false);
  const [acknowledged, setAcknowledged] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const formElement = event.currentTarget;
    const form = new FormData(formElement);
    const currentPassword = String(form.get('currentPassword') ?? '');
    const exportPassword = String(form.get('exportPassword') ?? '');
    const repeated = String(form.get('exportPasswordConfirm') ?? '');
    const validationError = authorizationExportPasswordError(exportPassword, repeated);
    if (validationError) { setError(validationError); return; }
    if (!acknowledged) { setError('请先确认你了解导出文件包含敏感授权信息'); return; }
    setBusy(true); setError('');
    try {
      const prepared = await api<PreparedAuthorizationExport>('/api/security/mail-authorization-exports', {
        method: 'POST', body: JSON.stringify({ currentPassword, exportPassword }),
      });
      await platform.saveDownload({ url: prepared.downloadPath, filename: prepared.filename });
      setOpen(false); setAcknowledged(false); formElement.reset();
      setNotice({ kind: 'success', text: `已准备 ${prepared.accountCount} 个邮箱的加密授权文件；不包含邮件内容` });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : '无法导出邮箱授权信息');
    } finally { setBusy(false); }
  }

  return <section className="privacy-action-section">
    <div className="privacy-action-heading"><FileLock size={24} weight="duotone" /><div><strong>邮箱授权信息导出</strong><p>导出当前账号下全部邮箱的连接配置与授权凭据，不包含邮件、附件、草稿、联系人或 iMail 登录密码。</p></div></div>
    {!open ? <Button appearance="secondary" type="button" icon={<DownloadSimple size={17} />} disabled={accountCount === 0} onClick={() => setOpen(true)}>{accountCount === 0 ? '暂无可导出的邮箱' : `导出 ${accountCount} 个邮箱`}</Button>
      : <form className="privacy-action-form" onSubmit={submit}>
        <div className="privacy-sensitive-note"><WarningCircle size={18} /><p>文件包含应用专用密码、OAuth Token 和代理密码。iMail 会使用你单独设置的导出密码加密文件。</p></div>
        <div className="privacy-form-grid">
          <label><span>当前 iMail 密码</span><AppInput name="currentPassword" type="password" autoComplete="current-password" required /></label>
          <label><span>导出文件密码</span><AppInput name="exportPassword" type="password" minLength={12} maxLength={256} autoComplete="new-password" placeholder="至少 12 个字符" required /></label>
          <label><span>再次输入导出文件密码</span><AppInput name="exportPasswordConfirm" type="password" minLength={12} maxLength={256} autoComplete="new-password" required /></label>
        </div>
        <AppCheckbox checked={acknowledged} onChange={(_, data) => setAcknowledged(Boolean(data.checked))} label="我了解持有此文件和导出密码的人可以登录这些邮箱" />
        {error && <div className="inline-error"><WarningCircle size={17} />{error}</div>}
        <div className="privacy-action-buttons"><button type="button" onClick={() => { setOpen(false); setAcknowledged(false); setError(''); }} disabled={busy}>取消</button><Button appearance="primary" type="submit" icon={<DownloadSimple size={17} />} disabled={busy || !acknowledged}>{busy ? '正在加密…' : '加密并下载'}</Button></div>
      </form>}
  </section>;
}
