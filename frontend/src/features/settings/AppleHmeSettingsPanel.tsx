import { useEffect, useState } from 'preact/compat';
import type { Notice } from '../../app-model';
import { AppSelect } from '../../components/form-controls';
import { Cloud, Envelope } from '../../components/icons';
import type { Account } from '../../types';
import { AppleHmePanel } from '../apple-hme';
import { PanelHeading } from './PanelHeading';

export function AppleHmeSettingsPanel({ accounts, onAddAccount, setNotice }: {
  accounts: Account[];
  onAddAccount: () => void;
  setNotice: (notice: Notice) => void;
}) {
  const icloudAccounts = accounts.filter((account) => account.provider === 'icloud');
  const [selectedAccountId, setSelectedAccountId] = useState(icloudAccounts[0]?.id ?? '');

  useEffect(() => {
    if (!icloudAccounts.some((account) => account.id === selectedAccountId)) {
      setSelectedAccountId(icloudAccounts[0]?.id ?? '');
    }
  }, [icloudAccounts, selectedAccountId]);

  const selectedAccount = icloudAccounts.find((account) => account.id === selectedAccountId);

  return <section className="settings-feature-panel apple-hme-settings-panel">
    <PanelHeading
      eyebrow="iCloud+ 隐私"
      title="隐私邮箱"
      description="集中管理 Hide My Email 授权、隐藏地址和本地快照。地址只在你主动同步时从 Apple 更新。"
      syncNote={false}
    />
    <div className="settings-panel-body">
      {selectedAccount ? <>
        <section className="apple-hme-account-picker" aria-label="关联的 iCloud 邮箱">
          <span><Cloud size={22} weight="duotone" /></span>
          <div>
            <strong>关联的 iCloud 身份</strong>
            <small>用于隔离本地授权和地址数据，不会把隐藏地址添加成邮箱账户。</small>
          </div>
          <AppSelect
            aria-label="选择 iCloud 邮箱"
            value={selectedAccount.id}
            onValueChange={setSelectedAccountId}
            options={icloudAccounts.map((account) => ({ value: account.id, label: `${account.displayName} · ${account.email}` }))}
          />
        </section>
        <AppleHmePanel key={selectedAccount.id} account={selectedAccount} setNotice={setNotice} />
      </> : <div className="settings-empty apple-hme-settings-empty">
        <Envelope size={38} weight="duotone" />
        <h3>需要一个 iCloud 邮箱身份</h3>
        <p>添加 iCloud 邮箱后，可在这里单独授权并管理 Hide My Email；隐藏地址不会进入邮箱账户列表。</p>
        <button type="button" className="settings-primary-action" onClick={onAddAccount}>添加 iCloud 邮箱</button>
      </div>}
    </div>
  </section>;
}
