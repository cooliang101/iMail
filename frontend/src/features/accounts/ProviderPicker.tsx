import { Check } from '../../components/icons';
import type { ProviderId } from '../../types';
import { ProviderIcon, providers } from '../../components/shared';

export function ProviderPicker({ value, busy, onChange }: { value: ProviderId; busy: boolean; onChange: (provider: ProviderId) => void }) {
  return <div className="provider-grid" aria-label="选择邮箱平台">{providers.map((item) => <button type="button" key={item.id} disabled={busy} aria-pressed={value === item.id} className={value === item.id ? 'selected' : ''} onClick={() => onChange(item.id)}><i className={`provider-mark provider-${item.id}`}><ProviderIcon provider={item.id} /></i><span>{item.name}</span>{value === item.id && <Check className="provider-selected-check" size={16} weight="bold" />}</button>)}</div>;
}
