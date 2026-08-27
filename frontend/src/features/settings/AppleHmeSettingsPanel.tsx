import { useEffect, useState } from 'preact/compat';
import type { Notice } from '../../app-model';
import { Envelope, Plus } from '../../components/icons';
import type { Account } from '../../types';
import { AppleHmePanel, type AppleHmeView } from '../apple-hme';
import { SettingsPanelHeading } from '../../components/settings-navigation';

export function AppleHmeSettingsPanel({ accounts, onAddAccount, setNotice }: {
  accounts: Account[];
  onAddAccount: () => void;
  setNotice: (notice: Notice) => void;
}) {
  const icloudAccounts = accounts.filter((account) => account.provider === 'icloud');
  const [selectedAccountId, setSelectedAccountId] = useState(icloudAccounts[0]?.id ?? '');
  const [detailOpen, setDetailOpen] = useState(false);
  const [hmeView, setHmeView] = useState<AppleHmeView>('overview');

  useEffect(() => {
    if (!icloudAccounts.some((account) => account.id === selectedAccountId)) {
      setSelectedAccountId(icloudAccounts[0]?.id ?? '');
      setDetailOpen(false);
      setHmeView('overview');
    }
  }, [icloudAccounts, selectedAccountId]);

  const selectedAccount = icloudAccounts.find((account) => account.id === selectedAccountId);

  const detailTitle: Record<AppleHmeView, string> = {
    overview: '隐私邮箱',
    addresses: '地址管理',
    create: '创建地址',
    appleAccountLogin: '连接 Apple 地址创建',
    icloudWebLogin: '连接 iCloud 地址管理',
  };

  if (selectedAccount && detailOpen) return <section className="settings-feature-panel apple-hme-settings-panel">
    <SettingsPanelHeading title={detailTitle[hmeView]} ancestors={['隐私邮箱', selectedAccount.displayName]} onBack={() => { setHmeView('overview'); setDetailOpen(false); }} />
    <div className="settings-panel-body settings-detail-body"><AppleHmePanel account={selectedAccount} setNotice={setNotice} view={hmeView} onViewChange={(nextView) => {
      setHmeView(nextView);
      if (nextView === 'overview') setDetailOpen(false);
    }} /></div>
  </section>;

  return <section className="settings-feature-panel apple-hme-settings-panel">
    <SettingsPanelHeading title="隐私邮箱" />
    <div className="settings-panel-body">
      {icloudAccounts.length > 0 ? <div className="apple-hme-account-regions">
        <div className="apple-hme-overview-toolbar">
          <span><strong>托管的 iCloud</strong><small>{icloudAccounts.length} 个身份</small></span>
          <button type="button" className="settings-primary-action" onClick={onAddAccount}><Plus size={15} />新增托管</button>
        </div>
        {icloudAccounts.map((account) => <AppleHmePanel
          key={account.id}
          account={account}
          setNotice={setNotice}
          view="overview"
          onViewChange={(nextView) => {
            if (nextView === 'overview') return;
            setSelectedAccountId(account.id);
            setHmeView(nextView);
            setDetailOpen(true);
          }}
        />)}
      </div> : <div className="settings-empty apple-hme-settings-empty">
        <Envelope size={38} weight="duotone" />
        <h3>还没有托管 iCloud</h3>
        <p>先接入主 iCloud 邮箱，再连接 Apple 服务并管理它的隐私邮箱地址。</p>
        <button type="button" className="settings-primary-action" onClick={onAddAccount}><Plus size={15} />新增托管 iCloud</button>
      </div>}
    </div>
  </section>;
}
