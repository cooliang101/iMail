import { useState } from 'preact/compat';
import { CaretDown, Check } from '../../components/icons';
import type { Account } from '../../types';
import { AccountProviderMark, providerLabel } from '../../components/shared';

export function SenderField({ accounts, value, onChange }: { accounts: Account[]; value: string; onChange: (value: string) => void }) {
  const [open, setOpen] = useState(false);
  const selected = accounts.find((account) => account.id === value) ?? accounts[0];
  if (!selected) return null;

  return <div className="compose-row compose-sender-row"><span>发件人</span><span className="compose-sender-control" onBlur={() => window.setTimeout(() => setOpen(false), 120)}>
    <button className="compose-sender-tag" type="button" aria-haspopup="listbox" aria-expanded={open} onClick={() => setOpen((current) => !current)}>
      <AccountProviderMark provider={selected.provider} />
      <span className="compose-sender-copy"><strong>{selected.displayName}</strong><small>{selected.email}</small></span>
      <CaretDown size={13} />
    </button>
    {open && <span className="sender-suggestions" role="listbox" aria-label="选择发件人">
      {accounts.map((account) => <button key={account.id} type="button" role="option" aria-selected={account.id === value} onClick={() => { onChange(account.id); setOpen(false); }}>
        <AccountProviderMark provider={account.provider} />
        <span className="sender-option-copy"><strong>{account.displayName}</strong><small>{providerLabel[account.provider]} · {account.email}</small></span>
        {account.id === value && <Check size={15} />}
      </button>)}
    </span>}
  </span></div>;
}
