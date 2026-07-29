import { useState, type FormEvent } from 'react';
import { Button } from '@fluentui/react-components';
import { ArrowRight, Bell, Check, Clock, Envelope, Tag, WarningCircle, X } from '@phosphor-icons/react';
import { api } from '../../api';
import type { Account, Message } from '../../types';
import type { MailNotification } from '../../app-model';
import { Overlay, ProviderIcon, providerLabel, relativeTime } from '../../components/shared';
import { AppInput } from '../../components/form-controls';

export function SnoozeModal({ onClose, onSave }: { onClose: () => void; onSave: (until: string | null) => void }) {
  const at = (days: number, hour: number) => { const value = new Date(); value.setDate(value.getDate() + days); value.setHours(hour, 0, 0, 0); return value.toISOString(); };
  const tomorrow = at(1, 9); const nextWeek = (() => { const value = new Date(); const days = ((8 - value.getDay()) % 7) || 7; value.setDate(value.getDate() + days); value.setHours(9, 0, 0, 0); return value.toISOString(); })();
  return <Overlay onClose={onClose}><section className="utility-modal snooze-modal"><div className="modal-header"><div><span>专注处理</span><h2>稍后提醒我</h2><p>到期前邮件会从收件箱隐藏，并保留在稍后处理。</p></div><button onClick={onClose} aria-label="关闭稍后处理窗口"><X size={21} /></button></div><div className="snooze-options"><button onClick={() => onSave(tomorrow)}><Clock size={19} /><span><strong>明天上午</strong><small>明天 09:00</small></span></button><button onClick={() => onSave(nextWeek)}><Clock size={19} /><span><strong>下周一</strong><small>下周一 09:00</small></span></button><label><Clock size={19} /><span><strong>自定义时间</strong><AppInput type="datetime-local" min={new Date().toISOString().slice(0, 16)} onChange={(event) => { if (event.target.value) onSave(new Date(event.target.value).toISOString()); }} /></span></label></div><button className="snooze-clear" onClick={() => onSave(null)}>取消稍后处理并返回收件箱</button></section></Overlay>;
}

