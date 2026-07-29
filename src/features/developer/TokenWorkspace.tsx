import { useEffect, useState, type FormEvent } from 'react';
import { Button } from '@fluentui/react-components';
import { AddressBook, ArrowRight, Code, Copy, Key, Plus, WarningCircle } from '@phosphor-icons/react';
import { api } from '../../api';
import type { Account, DeveloperToken } from '../../types';
import type { Notice } from '../../app-model';
import { Overlay, ProviderIcon, providerLabel } from '../../components/shared';
import { AppSelect } from '../../components/form-controls';

export function TokenWorkspace({ accounts, tokens, onCreate, onReload, setNotice }: { accounts: Account[]; tokens: DeveloperToken[]; onCreate: () => void; onReload: () => Promise<void>; setNotice: (notice: Notice) => void }) {
  const [gatewayAccount, setGatewayAccount] = useState(accounts[0]?.email ?? '');
  useEffect(() => {
    if (!accounts.some((account) => account.email === gatewayAccount)) setGatewayAccount(accounts[0]?.email ?? '');
  }, [accounts, gatewayAccount]);
  async function revoke(id: string) {
    try { await api(`/api/developer-tokens/${id}`, { method: 'DELETE' }); await onReload(); setNotice({ kind: 'success', text: 'Token 已撤销' }); }
    catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : 'Token 撤销失败' }); }
  }
  return <section className="token-workspace"><header><div><span>本地开发能力</span><h1>邮件网关</h1><p>让本地项目用一个短期 Token 安全读取或发送邮件，无需重复配置每个邮箱的 IMAP。</p></div><Button appearance="primary" icon={<Plus size={17} />} onClick={onCreate} disabled={accounts.length === 0}>创建临时 Token</Button></header>
    <div className="endpoint-strip"><Code size={21} /><span><small>开发 API 地址</small><code>http://127.0.0.1:8787/api/dev/v1</code></span><button onClick={() => void navigator.clipboard.writeText('http://127.0.0.1:8787/api/dev/v1')}><Copy size={17} />复制</button></div>
    <div className="token-columns"><div className="token-list"><div className="token-title"><h2>有效 Token</h2><span>{tokens.filter((token) => token.expiresAt > new Date().toISOString()).length} 个正在生效</span></div>{accounts.length === 0 ? <div className="token-empty"><Key size={38} weight="duotone" /><h3>接入邮箱后即可创建</h3><p>Token 只会访问你明确选择的邮箱和权限。</p></div> : tokens.length === 0 ? <div className="token-empty"><Key size={38} weight="duotone" /><h3>还没有临时 Token</h3><p>创建一个给本地应用使用，原始值只显示一次。</p><button onClick={onCreate}>创建第一个 Token</button></div> : tokens.map((token) => <article className="token-item" key={token.id}><div className="token-icon"><Key size={20} /></div><div><strong>{token.name}</strong><code>{token.prefix}••••••••••••</code><span>{token.scopes.map((scope) => scope.replace('messages:', '')).join(' · ')} · {token.mailboxes.length} 个邮箱</span></div><div className="token-time"><small>到期时间</small><span>{new Intl.DateTimeFormat('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' }).format(new Date(token.expiresAt))}</span></div><button className="revoke" onClick={() => void revoke(token.id)}>撤销</button></article>)}</div>
      <aside className="quickstart"><div className="quickstart-title"><AddressBook size={21} /><div><strong>快速调用</strong><span>指定邮箱读取最新 10 封邮件</span></div></div><label className="gateway-account-select"><span>API 使用的邮箱</span><AppSelect value={gatewayAccount} onValueChange={setGatewayAccount} options={accounts.map((account) => ({ value: account.email, label: `${providerLabel[account.provider]} · ${account.displayName} · ${account.email}` }))} /></label><pre><code><span className="code-muted">curl</span> http://127.0.0.1:8787/api/dev/v1/mailboxes/{gatewayAccount || 'user@example.com'}/messages?limit=10 \\{`\n`}  -H <span className="code-string">&quot;Authorization: Bearer imail_xxx&quot;</span></code></pre><div className="security-note"><WarningCircle size={18} /><p><strong>只使用邮箱地址</strong><span>网关请求和响应均不会暴露 iMail 内部邮箱 ID。</span></p></div><a href="/api/docs" target="_blank" rel="noreferrer">打开轻量 API Console <ArrowRight size={15} /></a></aside></div>
  </section>;
}

