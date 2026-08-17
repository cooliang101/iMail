import '../../styles/dialogs.css';
import { useState, type FormEvent } from 'preact/compat';
import { AppButton } from '../../components/AppButton';
import { ArrowRight, Bell, Check, Clock, Envelope, Tag, WarningCircle, X } from '../../components/icons';
import { api } from '../../services';
import type { Account, Message } from '../../types';
import type { MailNotification } from '../../app-model';
import { Overlay, ProviderIcon, providerLabel, relativeTime } from '../../components/shared';
import { AppInput } from '../../components/form-controls';

export function LabelModal({ message, knownLabels, onClose, onSave }: { message: Message; knownLabels: string[]; onClose: () => void; onSave: (labels: string[]) => void }) {
  const [selected, setSelected] = useState(message.labels);
  const [custom, setCustom] = useState('');
  const toggle = (label: string) => setSelected((current) => current.includes(label) ? current.filter((item) => item !== label) : [...current, label]);
  const add = () => { const label = custom.trim(); if (!label) return; setSelected((current) => current.includes(label) ? current : [...current, label]); setCustom(''); };
  return <Overlay onClose={onClose}><section className="utility-modal"><div className="modal-header"><div><span>整理邮件</span><h2>管理标签</h2><p>{message.subject}</p></div><button onClick={onClose} aria-label="关闭标签窗口"><X size={21} /></button></div><div className="label-options">{knownLabels.map((label) => <button key={label} className={selected.includes(label) ? 'selected' : ''} onClick={() => toggle(label)}><Tag size={15} />{label}{selected.includes(label) && <Check size={14} />}</button>)}</div><div className="label-create"><AppInput value={custom} onChange={(event) => setCustom(event.currentTarget.value)} onKeyDown={(event) => { if (event.key === 'Enter') { event.preventDefault(); add(); } }} placeholder="输入新标签名称" maxLength={40} /><button onClick={add}>添加</button></div>{selected.length > 0 && <div className="selected-labels">{selected.map((label) => <button key={label} onClick={() => toggle(label)}>{label}<X size={12} /></button>)}</div>}<div className="modal-footer"><button onClick={onClose}>取消</button><AppButton appearance="primary" onClick={() => onSave(selected)}>保存标签</AppButton></div></section></Overlay>;
}
