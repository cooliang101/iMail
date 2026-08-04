import { useState } from 'react';
import { Button } from '@fluentui/react-components';
import { Trash, WarningCircle } from '@phosphor-icons/react';
import { AppInput } from '../../components/form-controls';
import {
  desktopDeleteLocalServiceData,
  desktopEnableLocalService,
  desktopRemoveLocalService,
  type LocalServiceStatus,
} from '../../local-service';
import { saveLocalServiceSuspended } from '../../service-config';
import { serviceErrorMessage } from './service-connection';
import { suspendManagedLocalService } from './service-transition';

const CONFIRMATION = '永久删除本地数据';

export function LocalDataDeletion({ status, onDeleted }: {
  status: LocalServiceStatus;
  onDeleted?: (status: LocalServiceStatus) => void;
}) {
  const [open, setOpen] = useState(false);
  const [confirmation, setConfirmation] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  if (!status.dataPresent) return null;

  async function deleteData() {
    if (confirmation !== CONFIRMATION) return;
    setBusy(true); setError('');
    try {
      if (status.installed) {
        await suspendManagedLocalService({
          suspendLocal: desktopRemoveLocalService,
          enableLocal: desktopEnableLocalService,
          saveSuspended: saveLocalServiceSuspended,
        });
      } else {
        saveLocalServiceSuspended(true);
      }
      const next = await desktopDeleteLocalServiceData(confirmation);
      setOpen(false); setConfirmation(''); onDeleted?.(next);
    } catch (reason) {
      setError(serviceErrorMessage(reason, '删除本地邮件数据失败'));
    } finally { setBusy(false); }
  }

  return <div className="service-data-deletion">
    {!open ? <Button type="button" appearance="subtle" className="service-danger-button" icon={<Trash size={16} />} onClick={() => setOpen(true)}>
      永久删除本地数据
    </Button> : <div className="service-data-confirmation">
      <WarningCircle size={22} weight="duotone" />
      <div>
        <strong>此操作不可恢复</strong>
        <p>将删除本机数据库、邮件缓存、邮箱凭据、主密钥和联系人 Logo。{status.installed ? '本地守护程序也会先被移除。' : ''}</p>
        <label><span>输入“{CONFIRMATION}”以继续</span><AppInput value={confirmation} onChange={(_, data) => setConfirmation(data.value)} autoComplete="off" /></label>
        {error && <div className="auth-error" role="alert">{error}</div>}
        <div className="service-data-confirmation-actions">
          <Button type="button" appearance="subtle" onClick={() => { setOpen(false); setConfirmation(''); setError(''); }} disabled={busy}>取消</Button>
          <Button type="button" appearance="primary" className="service-danger-button" onClick={() => void deleteData()} disabled={busy || confirmation !== CONFIRMATION} icon={<Trash size={16} />}>
            {busy ? '正在永久删除…' : status.installed ? '移除服务并永久删除' : '永久删除'}
          </Button>
        </div>
      </div>
    </div>}
  </div>;
}
