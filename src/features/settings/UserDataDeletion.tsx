import { useState, type FormEvent } from 'react';
import { Button } from '@fluentui/react-components';
import { ArrowLeft, Trash, WarningCircle } from '@phosphor-icons/react';
import { api } from '../../api';
import type { Notice } from '../../app-model';
import { AppInput } from '../../components/form-controls';
import { CLEAR_USER_DATA_CONFIRMATION, clearUserDataReady } from './privacy-actions';

type Stage = 'idle' | 'review' | 'confirm';

export function UserDataDeletion({ accountCount, onCleared, setNotice }: { accountCount: number; onCleared: () => Promise<void>; setNotice: (notice: Notice) => void }) {
  const [stage, setStage] = useState<Stage>('idle');
  const [currentPassword, setCurrentPassword] = useState('');
  const [confirmation, setConfirmation] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  function reset() { setStage('idle'); setCurrentPassword(''); setConfirmation(''); setError(''); }

  async function clearData(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!clearUserDataReady(currentPassword, confirmation)) return;
    setBusy(true); setError('');
    try {
      await api('/api/security/clear-user-data', { method: 'POST', body: JSON.stringify({ currentPassword, confirmation }) });
      reset();
      setNotice({ kind: 'success', text: '当前账号的邮箱授权与本地邮件数据已清除，其他 iMail 用户未受影响' });
      try {
        await onCleared();
      } catch {
        setNotice({ kind: 'error', text: '数据已经清除，但界面刷新失败；请重新打开应用查看最新状态' });
      }
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : '无法清除当前账号的数据');
    } finally { setBusy(false); }
  }

  return <section className="privacy-action-section is-danger">
    <div className="privacy-action-heading"><Trash size={24} weight="duotone" /><div><strong>清除我的邮箱数据</strong><p>清除当前 iMail 登录账号的邮箱授权、邮件缓存、草稿、联系人、开发者令牌与同步状态。保留你的 iMail 登录账号、服务程序以及其他用户的数据。</p></div></div>
    {stage === 'idle' && <Button appearance="secondary" className="privacy-danger-button" type="button" icon={<Trash size={17} />} onClick={() => setStage('review')}>开始清除…</Button>}
    {stage === 'review' && <div className="privacy-clear-review" role="alert">
      <WarningCircle size={22} weight="duotone" /><div><strong>第一次确认：核对清除范围</strong><ul><li>{accountCount} 个邮箱及其授权凭据</li><li>已缓存邮件、草稿、联系人与同步记录</li><li>当前账号创建的 API 与 MCP 授权码</li></ul><p>其他 iMail 用户及其邮箱不会被删除，此操作无法撤销。</p><div className="privacy-action-buttons"><button type="button" onClick={reset}>取消</button><Button appearance="primary" className="privacy-danger-filled" type="button" onClick={() => setStage('confirm')}>我已了解，继续验证</Button></div></div>
    </div>}
    {stage === 'confirm' && <form className="privacy-action-form privacy-clear-confirm" onSubmit={clearData}>
      <header><button type="button" onClick={() => { setStage('review'); setError(''); }} disabled={busy}><ArrowLeft size={15} />返回上一步</button><strong>第二次确认：验证当前身份</strong></header>
      <label><span>当前 iMail 密码</span><AppInput type="password" value={currentPassword} onChange={(_, data) => setCurrentPassword(data.value)} autoComplete="current-password" required /></label>
      <label><span>输入“{CLEAR_USER_DATA_CONFIRMATION}”</span><AppInput value={confirmation} onChange={(_, data) => setConfirmation(data.value)} autoComplete="off" required /></label>
      {error && <div className="inline-error"><WarningCircle size={17} />{error}</div>}
      <div className="privacy-action-buttons"><button type="button" onClick={reset} disabled={busy}>取消</button><Button appearance="primary" className="privacy-danger-filled" type="submit" icon={<Trash size={17} />} disabled={busy || !clearUserDataReady(currentPassword, confirmation)}>{busy ? '正在清除…' : '确认清除我的数据'}</Button></div>
    </form>}
  </section>;
}
