import { useEffect, useRef, useState, type CSSProperties } from 'react';
import { Button } from '@fluentui/react-components';
import { Archive, ArrowLeft, ArrowRight, CaretDown, Clock, Envelope, File, Star, Tag, Trash, Tray } from '@phosphor-icons/react';
import type { Account, Message } from '../../types';
import { virtualRange } from '../../virtual';
import { AccountProviderMark, initials, providerLabel, relativeTime } from '../../components/shared';

const MESSAGE_ROW_HEIGHT = 108;
const MESSAGE_OVERSCAN = 6;

export function VirtualMessageList({ messages, accounts, selectedId, ready, loading, hasMore, onSelect, onContextMenu, onBackgroundContextMenu, onLoadMore, onAddAccount }: {
  messages: Message[]; accounts: Account[]; selectedId?: string; ready: boolean; loading: boolean; hasMore: boolean;
  onSelect: (id: string) => void; onContextMenu?: (message: Message, point: { x: number; y: number }) => void; onBackgroundContextMenu?: (point: { x: number; y: number }) => void; onLoadMore: () => void | Promise<void>; onAddAccount: () => void;
}) {
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(600);

  useEffect(() => {
    const element = viewportRef.current;
    if (!element) return;
    const update = () => setViewportHeight(element.clientHeight);
    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (viewportRef.current) viewportRef.current.scrollTop = 0;
    setScrollTop(0);
  }, [messages[0]?.id]);

  const { start, end } = virtualRange(messages.length, scrollTop, viewportHeight, MESSAGE_ROW_HEIGHT, MESSAGE_OVERSCAN);

  useEffect(() => {
    if (hasMore && !loading && scrollTop + viewportHeight >= messages.length * MESSAGE_ROW_HEIGHT - MESSAGE_ROW_HEIGHT * 8) void onLoadMore();
  }, [hasMore, loading, messages.length, onLoadMore, scrollTop, viewportHeight]);

  return <div className="message-list virtual-message-list" ref={viewportRef} onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)} onContextMenu={(event) => { if (!(event.target as Element).closest('.message-row')) { event.preventDefault(); onBackgroundContextMenu?.({ x: event.clientX, y: event.clientY }); } }}>
    {!ready ? Array.from({ length: 6 }).map((_, index) => <div className="message-skeleton" key={index}><i /><span /><b /></div>) : messages.length === 0 ? <div className="empty-state"><Tray size={42} weight="duotone" /><h3>{accounts.length === 0 ? '还没有接入邮箱' : '这里暂时很安静'}</h3><p>{accounts.length === 0 ? '点击左侧加号，连接你的第一个邮箱。' : '换一个邮箱或清除搜索条件试试。'}</p>{accounts.length === 0 && <button onClick={onAddAccount}>添加邮箱</button>}</div> : <div className="virtual-message-space" style={{ height: messages.length * MESSAGE_ROW_HEIGHT }}>
      {messages.slice(start, end).map((message, visibleIndex) => {
        const index = start + visibleIndex;
        const account = accounts.find((item) => item.id === message.accountId);
        const color = account?.color ?? '#66857d';
        return <button key={message.id} style={{ top: index * MESSAGE_ROW_HEIGHT, height: MESSAGE_ROW_HEIGHT }} className={`message-row virtual-message-row ${selectedId === message.id ? 'selected' : ''} ${message.unread ? 'unread' : ''}`} onClick={() => onSelect(message.id)} onContextMenu={(event) => { event.preventDefault(); event.stopPropagation(); onContextMenu?.(message, { x: event.clientX, y: event.clientY }); }}>
          <span className="sender-avatar" style={{ '--avatar-color': color } as CSSProperties}>{initials(message.from.name || message.from.address)}</span>
          <span className="message-copy"><span className="message-meta"><strong>{message.from.name || message.from.address}</strong><time>{relativeTime(message.date)}</time></span><b>{message.subject}</b><span>{message.preview}</span><small className="message-account"><span className="message-account-identity">{account ? <><AccountProviderMark provider={account.provider} className="message-provider-mark" /><b>{account.displayName}</b><em title={account.email}>{account.email}</em></> : '邮箱'}</span>{message.hasAttachments && <span className="message-attachment"><File size={13} />附件</span>}</small></span>
          {message.flagged && <Star className="row-star" size={15} weight="fill" />}
        </button>;
      })}
    </div>}
    {loading && ready && <div className="message-loading">正在读取本地缓存…</div>}
  </div>;
}
