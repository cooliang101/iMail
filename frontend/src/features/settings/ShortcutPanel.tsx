import { useEffect, useState, type KeyboardEvent } from 'preact/compat';
import { Keyboard } from '../../components/icons';
import type { ShortcutActionId, ShortcutBindings } from '../../app-model';
import { shortcutConflict, shortcutDefinitions, shortcutFromEvent, shortcutLabel } from '../shortcuts';
import { SettingsPanelHeading } from '../../components/settings-navigation';

export function ShortcutPanel({ bindings, onChange }: { bindings: ShortcutBindings; onChange: (bindings: ShortcutBindings) => void }) {
  const [draft, setDraft] = useState(bindings);
  const [recording, setRecording] = useState<ShortcutActionId | null>(null);
  const [error, setError] = useState('');
  useEffect(() => { setDraft(bindings); setRecording(null); setError(''); }, [bindings]);
  function save(next: ShortcutBindings) { setDraft(next); onChange(next); }
  function capture(event: KeyboardEvent<HTMLButtonElement>, actionId: ShortcutActionId) {
    if (recording !== actionId) return;
    event.preventDefault(); event.stopPropagation();
    if (event.key === 'Escape') { setRecording(null); setError(''); return; }
    if (event.key === 'Backspace' || event.key === 'Delete') { save({ ...draft, [actionId]: '' }); setRecording(null); setError(''); return; }
    const candidate = shortcutFromEvent(event);
    if (!candidate) return;
    const conflict = shortcutConflict(draft, actionId, candidate);
    if (conflict) { setError(`“${shortcutLabel(candidate)}”已用于“${conflict.label}”`); return; }
    save({ ...draft, [actionId]: candidate }); setRecording(null); setError('');
  }
  return <section className="settings-feature-panel"><SettingsPanelHeading title="快捷键" />
    <div className="settings-panel-body"><div className="shortcut-groups">{(['global', 'mail'] as const).map((scope) => <section key={scope}><h3>{scope === 'global' ? '全局操作' : '邮件操作'}</h3><div className="shortcut-list">{shortcutDefinitions.filter((item) => item.scope === scope).map((item) => <div className="shortcut-row" key={item.id}><i><Keyboard size={18} /></i><span><strong>{item.label}</strong><small>{item.description}</small></span><button type="button" className={recording === item.id ? 'is-recording' : ''} onClick={() => { setRecording(item.id); setError(''); }} onKeyDown={(event) => capture(event, item.id)}>{recording === item.id ? '请按键…' : <kbd>{shortcutLabel(draft[item.id])}</kbd>}</button></div>)}</div></section>)}</div>
    {error && <div className="shortcut-error">{error}</div>}</div>
  </section>;
}
