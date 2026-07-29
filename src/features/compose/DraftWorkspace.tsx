import { useRef, useState, type FormEvent } from 'react';
import { Button } from '@fluentui/react-components';
import { PaperPlaneTilt, PencilSimple, Trash, WarningCircle, X } from '@phosphor-icons/react';
import { api } from '../../api';
import type { Account, Draft, Message } from '../../types';
import { AccountProviderMark, Overlay, providerLabel, relativeTime } from '../../components/shared';

export function DraftWorkspace({ drafts, accounts, onOpen, onDelete, onCreate }: { drafts: Draft[]; accounts: Account[]; onOpen: (draft: Draft) => void; onDelete: (id: string) => void | Promise<void>; onCreate: () => void }) {
  return <section className="draft-workspace"><header><div><span>本地 SQLite 草稿</span><h1>草稿</h1><p>未完成的邮件保存在本机，发送成功后会自动移除。</p></div><Button appearance="primary" icon={<PencilSimple size={17} />} onClick={onCreate}>新建邮件</Button></header>
    {drafts.length === 0 ? <div className="draft-empty"><PencilSimple size={44} weight="duotone" /><h2>没有未完成的邮件</h2><p>写信时选择“存为草稿”，之后可以继续编辑。</p><button onClick={onCreate}>开始写邮件</button></div> : <div className="draft-list">{drafts.map((draft) => { const account = accounts.find((item) => item.id === draft.accountId); return <article key={draft.id}><button className="draft-main" onClick={() => onOpen(draft)}><span className="draft-account">{account && <AccountProviderMark provider={account.provider} />}{account?.displayName ?? '未知邮箱'}</span><strong>{draft.subject || '（无主题）'}</strong><p>{draft.text || '还没有正文内容'}</p><small>收件人：{draft.to.join(', ') || '未填写'} · {relativeTime(draft.updatedAt)}</small></button><button className="draft-delete" title="删除草稿" aria-label="删除草稿" onClick={() => void onDelete(draft.id)}><Trash size={17} /></button></article>; })}</div>}
  </section>;
}


