import type { ComponentChildren } from 'preact';
import { useEffect, useRef, useState, type CSSProperties } from 'preact/compat';
import { AppButton } from '../../components/AppButton';
import { Archive, ArrowLeft, ArrowRight, CaretDown, Clock, Envelope, File, Star, Tag, Trash, Tray } from '../../components/icons';
import type { Account, Message } from '../../types';
import { virtualRange } from '../../utils/virtual';
import { AccountProviderMark, providerLabel, relativeTime, SenderAvatar } from '../../components/shared';
import { useAppTheme } from '../appearance';

const MESSAGE_ROW_HEIGHT = 108;
const SOFT_NEUBRUTALISM_ROW_PITCH = 116;
const MESSAGE_OVERSCAN = 6;

export function VirtualMessageList({ messages, accounts, selectedId, ready, loading, hasMore, emptyContent, onSelect, onContextMenu, onBackgroundContextMenu, onLoadMore, onAddAccount }: {
  messages: Message[]; accounts: Account[]; selectedId?: string; ready: boolean; loading: boolean; hasMore: boolean;
  onSelect: (id: string) => void; onContextMenu?: (message: Message, point: { x: number; y: number }) => void; onBackgroundContextMenu?: (point: { x: number; y: number }) => void; onLoadMore: () => void | Promise<void>; onAddAccount: () => void;
  emptyContent?: ComponentChildren;
}) {
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(600);
  const { themeId } = useAppTheme();
  const rowPitch = themeId === 'soft-neubrutalism' ? SOFT_NEUBRUTALISM_ROW_PITCH : MESSAGE_ROW_HEIGHT;
  const firstMessageId = messages[0]?.id;

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
  }, [firstMessageId]);

  const { start, end } = virtualRange(messages.length, scrollTop, viewportHeight, rowPitch, MESSAGE_OVERSCAN);

  useEffect(() => {
    if (hasMore && !loading && scrollTop + viewportHeight >= messages.length * rowPitch - rowPitch * 8) void onLoadMore();
  }, [hasMore, loading, messages.length, onLoadMore, rowPitch, scrollTop, viewportHeight]);

  return <div className="message-list virtual-message-list" ref={viewportRef} onScroll={(event) => setScrollTop(event.currentTarget.scrollTop)} onContextMenu={(event) => { if (!(event.target as Element).closest('.message-row')) { event.preventDefault(); onBackgroundContextMenu?.({ x: event.clientX, y: event.clientY }); } }}>
    {!ready ? Array.from({ length: 6 }).map((_, index) => <div className="message-skeleton" key={index}><i /><span /><b /></div>) : messages.length === 0 ? emptyContent ?? <div className="empty-state"><Tray size={42} weight="duotone" /><h3>{accounts.length === 0 ? '还没有接入邮箱' : '这里暂时很安静'}</h3><p>{accounts.length === 0 ? '点击左侧加号，连接你的第一个邮箱。' : '换一个邮箱或清除搜索条件试试。'}</p>{accounts.length === 0 && <button onClick={onAddAccount}>添加邮箱</button>}</div> : <div className="virtual-message-space" style={{ height: messages.length * rowPitch }}>
      {messages.slice(start, end).map((message, visibleIndex) => {
        const index = start + visibleIndex;
        const account = accounts.find((item) => item.id === message.accountId);
        const color = account?.color ?? '#66857d';
        return <button key={message.id} data-row-tone={index % 3} style={{ '--message-row-top': `${index * rowPitch}px`, '--message-row-height': `${MESSAGE_ROW_HEIGHT}px` } as CSSProperties} className={`message-row virtual-message-row ${selectedId === message.id ? 'selected' : ''} ${message.unread ? 'unread' : ''}`} onClick={() => onSelect(message.id)} onContextMenu={(event) => { event.preventDefault(); event.stopPropagation(); onContextMenu?.(message, { x: event.clientX, y: event.clientY }); }}>
          <SenderAvatar logo={message.from.logo} name={message.from.name || message.from.address} color={color} />
          <span className="message-copy"><span className="message-meta"><strong>{message.from.name || message.from.address}</strong><time>{relativeTime(message.date)}</time></span><b>{message.subject}</b><span>{message.preview}</span><small className="message-account"><span className="message-account-identity">{account ? <><AccountProviderMark provider={account.provider} className="message-provider-mark" /><b>{account.displayName}</b><em title={account.email}>{account.email}</em></> : '邮箱'}</span>{message.hasAttachments && <span className="message-attachment"><File size={13} />附件</span>}</small></span>
          {message.flagged && <Star className="row-star" size={15} weight="fill" />}
        </button>;
      })}
    </div>}
    {loading && ready && <div className="message-loading">正在读取本地缓存…</div>}
  </div>;
}
