import '../../styles/dialogs.css';
import { useState, type FormEvent } from 'preact/compat';
import { AppButton } from '../../components/AppButton';
import { ArrowRight, Bell, Check, Clock, Envelope, Tag, WarningCircle, X } from '../../components/icons';
import { api } from '../../services';
import type { Account, Message } from '../../types';
import type { MailNotification } from '../../app-model';
import { Overlay, ProviderIcon, providerLabel, relativeTime } from '../../components/shared';

export function NotificationsModal({ notifications, accounts, onClose, onOpenMessage }: { notifications: MailNotification[]; accounts: Account[]; onClose: () => void; onOpenMessage: (notification: MailNotification) => void }) {
  return <Overlay onClose={onClose}><section className="utility-modal notification-modal"><div className="modal-header"><div><span>账户与邮件动态</span><h2>通知中心</h2><p>连接异常、返回收件箱的稍后邮件和最近未读邮件。</p></div><button onClick={onClose} aria-label="关闭通知中心"><X size={21} /></button></div>{notifications.length === 0 ? <div className="utility-empty"><Bell size={38} weight="duotone" /><h3>暂无新通知</h3><p>邮箱连接和稍后处理状态都正常。</p></div> : <div className="notification-list">{notifications.map((notification) => { const account = accounts.find((item) => item.id === notification.accountId); return <button key={notification.id} disabled={!notification.messageId} onClick={() => onOpenMessage(notification)}><i className={`notification-kind notification-${notification.kind}`}>{notification.kind === 'error' ? <WarningCircle size={18} /> : notification.kind === 'snooze' ? <Clock size={18} /> : <Envelope size={18} />}</i><span><strong>{notification.title}</strong><small>{notification.detail}</small><em>{account ? `${providerLabel[account.provider]} · ${account.displayName}` : '邮箱'} · {relativeTime(notification.date)}</em></span>{notification.messageId && <ArrowRight size={16} />}</button>; })}</div>}</section></Overlay>;
}
