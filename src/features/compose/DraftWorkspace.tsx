import { Button } from '@fluentui/react-components';
import { PencilSimple, Trash } from '@phosphor-icons/react';
import type { Account, Draft, Message } from '../../types';
import { AccountProviderMark, relativeTime } from '../../components/shared';

export function DraftWorkspace({ drafts, remoteDrafts, accounts, selectedRemoteId, onOpen, onOpenRemote, onDelete, onCreate }: {
  drafts: Draft[]; remoteDrafts: Message[]; accounts: Account[]; selectedRemoteId?: string;
  onOpen: (draft: Draft) => void; onOpenRemote: (draft: Message) => void; onDelete: (id: string) => void | Promise<void>; onCreate: () => void;
}) {
  const empty = drafts.length === 0 && remoteDrafts.length === 0;
  return <section className="message-pane draft-pane"><header className="draft-pane-header"><div><span>本地与邮箱草稿</span><strong>草稿</strong><small>{drafts.length} 封本地 · {remoteDrafts.length} 封邮箱草稿</small></div><Button appearance="primary" icon={<PencilSimple size={16} />} onClick={onCreate}>新建</Button></header>
    {empty ? <div className="draft-empty"><PencilSimple size={40} weight="duotone" /><h2>没有未完成的邮件</h2><p>开始写信后，内容会自动保存到这里。</p><button onClick={onCreate}>开始写邮件</button></div> : <div className="draft-list">
      {drafts.map((draft) => { const account = accounts.find((item) => item.id === draft.accountId); return <article key={draft.id}><button className="draft-main" onClick={() => onOpen(draft)}><span className="draft-account">{account && <AccountProviderMark provider={account.provider} />}{account?.displayName ?? '未知邮箱'}<em>{relativeTime(draft.updatedAt)}</em></span><strong>{draft.subject || '（无主题）'}</strong><p>{draft.text || (draft.attachments.length ? `${draft.attachments.length} 个附件` : '还没有正文内容')}</p><small>本地草稿 · 收件人：{draft.to.join(', ') || '未填写'}</small></button><button className="draft-delete" title="删除草稿" aria-label="删除草稿" onClick={() => void onDelete(draft.id)}><Trash size={17} /></button></article>; })}
      {remoteDrafts.map((draft) => { const account = accounts.find((item) => item.id === draft.accountId); return <article key={draft.id} className={selectedRemoteId === draft.id ? 'active' : ''}><button className="draft-main" onClick={() => onOpenRemote(draft)}><span className="draft-account">{account && <AccountProviderMark provider={account.provider} />}{account?.displayName ?? '未知邮箱'}<em>{relativeTime(draft.date)}</em></span><strong>{draft.subject || '（无主题）'}</strong><p>{draft.preview || '邮箱服务器中的草稿'}</p><small>邮箱草稿 · 点击预览</small></button></article>; })}
    </div>}
  </section>;
}
