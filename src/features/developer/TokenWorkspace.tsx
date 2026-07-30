import { useEffect, useState } from 'react';
import { Button } from '@fluentui/react-components';
import { AddressBook, ArrowRight, Code, Copy, Key, Plus, WarningCircle } from '@phosphor-icons/react';
import { api } from '../../api';
import type { Account, DeveloperToken } from '../../types';
import type { Notice } from '../../app-model';
import { providerLabel } from '../../components/shared';
import { AppSelect } from '../../components/form-controls';
import { McpIntegrationGuide } from './McpIntegrationGuide';

type AccessTab = 'api' | 'mcp';

export function TokenWorkspace({ accounts, tokens, onCreateApi, onCreateMcp, onReload, setNotice }: { accounts: Account[]; tokens: DeveloperToken[]; onCreateApi: () => void; onCreateMcp: () => void; onReload: () => Promise<void>; setNotice: (notice: Notice) => void }) {
  const [activeTab, setActiveTab] = useState<AccessTab>('mcp');
  const [gatewayAccount, setGatewayAccount] = useState(accounts[0]?.email ?? '');
  const visibleTokens = tokens.filter((token) => token.scopes.includes('mcp:full') === (activeTab === 'mcp'));

  useEffect(() => {
    if (!accounts.some((account) => account.email === gatewayAccount)) setGatewayAccount(accounts[0]?.email ?? '');
  }, [accounts, gatewayAccount]);

  async function revoke(id: string) {
    try { await api(`/api/developer-tokens/${id}`, { method: 'DELETE' }); await onReload(); setNotice({ kind: 'success', text: activeTab === 'mcp' ? 'MCP 授权码已撤销' : 'API Token 已撤销' }); }
    catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : '凭据撤销失败' }); }
  }

  async function copyEndpoint(value: string) {
    try { await navigator.clipboard.writeText(value); setNotice({ kind: 'success', text: '接入地址已复制' }); }
    catch { setNotice({ kind: 'error', text: '复制失败，请手动选择地址' }); }
  }

  return <section className="token-workspace">
    <header><div><span>外部接入</span><h1>{activeTab === 'mcp' ? 'MCP Agent 接入' : '邮件 API 网关'}</h1><p>{activeTab === 'mcp' ? '为可信 Agent 创建独立授权码，通过标准 MCP 工具安全管理邮箱。' : '为本地项目创建细粒度 API Token，通过 REST 接口读取或发送邮件。'}</p></div><Button appearance="primary" icon={<Plus size={17} />} onClick={activeTab === 'mcp' ? onCreateMcp : onCreateApi} disabled={activeTab === 'api' && accounts.length === 0}>{activeTab === 'mcp' ? '创建 MCP 授权码' : '创建 API Token'}</Button></header>

    <nav className="access-tabs" role="tablist" aria-label="外部接入方式"><button role="tab" aria-selected={activeTab === 'mcp'} className={activeTab === 'mcp' ? 'active' : ''} onClick={() => setActiveTab('mcp')}><Code size={18} /><span><strong>MCP</strong><small>Agent 工具调用</small></span></button><button role="tab" aria-selected={activeTab === 'api'} className={activeTab === 'api' ? 'active' : ''} onClick={() => setActiveTab('api')}><AddressBook size={18} /><span><strong>API 网关</strong><small>REST 接口调用</small></span></button></nav>

    <div className="endpoint-strip"><Code size={21} /><span><small>{activeTab === 'mcp' ? 'Streamable HTTP 地址' : 'REST API 地址'}</small><code>{activeTab === 'mcp' ? 'http://127.0.0.1:8787/mcp' : 'http://127.0.0.1:8787/gateway/v1'}</code></span><button onClick={() => void copyEndpoint(activeTab === 'mcp' ? 'http://127.0.0.1:8787/mcp' : 'http://127.0.0.1:8787/gateway/v1')}><Copy size={17} />复制</button></div>

    <div className="token-columns"><div className="token-list"><div className="token-title"><h2>{activeTab === 'mcp' ? 'MCP 授权码' : 'API Token'}</h2><span>{visibleTokens.filter((token) => token.expiresAt > new Date().toISOString()).length} 个正在生效</span></div>{visibleTokens.length === 0 ? <div className="token-empty"><Key size={38} weight="duotone" /><h3>{activeTab === 'mcp' ? '还没有 MCP 授权码' : '还没有 API Token'}</h3><p>{activeTab === 'mcp' ? '创建独立授权码后，按下方配置即可让 Agent 连接。' : accounts.length === 0 ? '接入邮箱后即可创建 API Token。' : '选择邮箱与权限，为本地应用创建最小权限 Token。'}</p><button disabled={activeTab === 'api' && accounts.length === 0} onClick={activeTab === 'mcp' ? onCreateMcp : onCreateApi}>{activeTab === 'mcp' ? '创建 MCP 授权码' : '创建 API Token'}</button></div> : visibleTokens.map((token) => <article className="token-item" key={token.id}><div className={`token-icon ${activeTab === 'mcp' ? 'is-mcp' : ''}`}><Key size={20} /></div><div><strong>{token.name}</strong><code>{token.prefix}••••••••••••</code><span>{activeTab === 'mcp' ? '全部当前与未来邮箱' : `${token.scopes.map((scope) => scope.replace('messages:', '')).join(' · ')} · ${token.mailboxes.length} 个邮箱`}</span></div><div className="token-time"><small>到期时间</small><span>{new Intl.DateTimeFormat('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' }).format(new Date(token.expiresAt))}</span></div><button className="revoke" onClick={() => void revoke(token.id)}>撤销</button></article>)}</div>
      {activeTab === 'api' ? <aside className="quickstart"><div className="quickstart-title"><AddressBook size={21} /><div><strong>快速调用</strong><span>指定邮箱读取最新 10 封邮件</span></div></div><label className="gateway-account-select"><span>API 使用的邮箱</span><AppSelect value={gatewayAccount} onValueChange={setGatewayAccount} options={accounts.map((account) => ({ value: account.email, label: `${providerLabel[account.provider]} · ${account.displayName} · ${account.email}` }))} /></label><pre><code><span className="code-muted">curl</span> http://127.0.0.1:8787/gateway/v1/mailboxes/{gatewayAccount || 'user@example.com'}/messages?limit=10 \\{`\n`}  -H <span className="code-string">&quot;Authorization: Bearer imail_xxx&quot;</span></code></pre><div className="security-note"><WarningCircle size={18} /><p><strong>只使用邮箱地址</strong><span>网关请求和响应均不会暴露 iMail 内部邮箱 ID。</span></p></div><a href="/gateway/docs" target="_blank" rel="noreferrer">打开轻量 API Console <ArrowRight size={15} /></a></aside> : <aside className="quickstart mcp-quickstart"><div className="quickstart-title"><Code size={21} /><div><strong>接入前准备</strong><span>授权码与 API Token 不互通</span></div></div><ol><li>创建并立即复制 <code>imail_mcp_</code> 授权码。</li><li>选择 HTTP 或 stdio 配置。</li><li>让 Agent 连接并自动发现工具。</li><li>任务完成后撤销授权码。</li></ol><div className="security-note"><WarningCircle size={18} /><p><strong>完整控制权限</strong><span>可管理账户、邮件和草稿，仅签发给可信 Agent。</span></p></div></aside>}
    </div>
    {activeTab === 'mcp' && <McpIntegrationGuide setNotice={setNotice} />}
  </section>;
}
