import { useEffect, useState, type KeyboardEvent } from 'react';
import { ArrowCounterClockwise, Check, Keyboard, X } from '@phosphor-icons/react';
import type { ShortcutActionId, ShortcutBindings } from '../../app-model';
import { Overlay } from '../../components/shared';
import { defaultShortcutBindings, shortcutConflict, shortcutDefinitions, shortcutFromEvent, shortcutLabel } from './shortcut-model';

export function ShortcutSettingsModal({ bindings, onChange, onClose }: { bindings: ShortcutBindings; onChange: (bindings: ShortcutBindings) => void; onClose: () => void }) {
  const [draft, setDraft] = useState(bindings);
  const [recording, setRecording] = useState<ShortcutActionId | null>(null);
  const [error, setError] = useState('');
  useEffect(() => setDraft(bindings), [bindings]);

  function capture(event: KeyboardEvent<HTMLButtonElement>, actionId: ShortcutActionId) {
    if (recording !== actionId) return;
    event.preventDefault(); event.stopPropagation();
    if (event.key === 'Escape') { setRecording(null); setError(''); return; }
    if (event.key === 'Backspace' || event.key === 'Delete') { setDraft((current) => ({ ...current, [actionId]: '' })); setRecording(null); setError(''); return; }
    const candidate = shortcutFromEvent(event.nativeEvent);
    if (!candidate) return;
    const conflict = shortcutConflict(draft, actionId, candidate);
    if (conflict) { setError(`“${shortcutLabel(candidate)}”已用于“${conflict.label}”`); return; }
    setDraft((current) => ({ ...current, [actionId]: candidate })); setRecording(null); setError('');
  }

  return <Overlay onClose={onClose} wide dialogClassName="shortcut-settings-shell"><section className="shortcut-settings-modal">
    <div className="modal-header"><div><span>键盘效率</span><h2>快捷键设置</h2><p>点击任意绑定后直接按下新组合键；按 Backspace 清除，Esc 取消录制。</p></div><button type="button" aria-label="关闭快捷键设置" onClick={onClose}><X size={21} /></button></div>
    <div className="shortcut-groups">
      {(['global', 'mail'] as const).map((scope) => <section key={scope}><h3>{scope === 'global' ? '全局操作' : '邮件操作'}</h3><div className="shortcut-list">
        {shortcutDefinitions.filter((item) => item.scope === scope).map((item) => <div className="shortcut-row" key={item.id}><i><Keyboard size={18} /></i><span><strong>{item.label}</strong><small>{item.description}</small></span><button type="button" className={recording === item.id ? 'is-recording' : ''} onClick={() => { setRecording(item.id); setError(''); }} onKeyDown={(event) => capture(event, item.id)}>{recording === item.id ? '请按键…' : <kbd>{shortcutLabel(draft[item.id])}</kbd>}</button></div>)}
      </div></section>)}
    </div>
    {error && <div className="shortcut-error">{error}</div>}
    <footer className="modal-footer shortcut-footer"><button type="button" onClick={() => { setDraft({ ...defaultShortcutBindings }); setError(''); }}><ArrowCounterClockwise size={15} />恢复默认</button><button type="button" className="shortcut-save" onClick={() => { onChange(draft); onClose(); }}><Check size={15} />保存快捷键</button></footer>
  </section></Overlay>;
}
