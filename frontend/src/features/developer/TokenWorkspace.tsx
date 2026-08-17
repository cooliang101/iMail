import { useEffect, useState } from 'preact/compat';
import { AppButton } from '../../components/AppButton';
import { AddressBook, ArrowRight, Code, Copy, Key, Plus, WarningCircle } from '../../components/icons';
import { api, desktopLog, describeDesktopLogValue } from '../../services';
import type { Account, DeveloperToken, ExternalAccessSettings } from '../../types';
import type { Notice } from '../../app-model';
import { providerLabel } from '../../components/shared';
import { AppCheckbox, AppSelect } from '../../components/form-controls';
import { McpIntegrationGuide } from './McpIntegrationGuide';
import { useExternalAccessBaseUrl } from './external-access-endpoint';

type AccessTab = 'api' | 'mcp';

export function TokenWorkspace({ accounts, tokens, onCreateApi, onCreateMcp, onReload, setNotice }: { accounts: Account[]; tokens: DeveloperToken[]; onCreateApi: () => void; onCreateMcp: () => void; onReload: () => Promise<void>; setNotice: (notice: Notice) => void }) {
  const [activeTab, setActiveTab] = useState<AccessTab>('mcp');
  const [gatewayAccount, setGatewayAccount] = useState(accounts[0]?.email ?? '');
  const [accessSettings, setAccessSettings] = useState<ExternalAccessSettings>();
  const [accessBusy, setAccessBusy] = useState(false);
  const visibleTokens = tokens.filter((token) => token.scopes.includes('mcp:full') === (activeTab === 'mcp'));
  const { baseUrl, error: endpointError } = useExternalAccessBaseUrl();
  const endpoint = baseUrl ? `${baseUrl}${activeTab === 'mcp' ? '/mcp' : '/gateway/v1'}` : '';
  const activeEnabled = activeTab === 'mcp' ? accessSettings?.mcpEnabled === true : accessSettings?.gatewayEnabled === true;
  const available = Boolean(baseUrl);

  useEffect(() => {
    if (!accounts.some((account) => account.email === gatewayAccount)) setGatewayAccount(accounts[0]?.email ?? '');
  }, [accounts, gatewayAccount]);

  useEffect(() => {
    void api<{ settings: ExternalAccessSettings }>('/api/external-access')
      .then((result) => setAccessSettings(result.settings))
      .catch((error) => setNotice({ kind: 'error', text: error instanceof Error ? error.message : '外部接入状态加载失败' }));
  }, [setNotice]);

  useEffect(() => {
    if (endpointError) setNotice({ kind: 'error', text: endpointError });
  }, [endpointError, setNotice]);

  async function setAccessEnabled(enabled: boolean) {
    const key = activeTab === 'mcp' ? 'mcpEnabled' : 'gatewayEnabled';
    setAccessBusy(true);
    try {
      const result = await api<{ settings: ExternalAccessSettings }>('/api/external-access', {
        method: 'PATCH', body: JSON.stringify({ [key]: enabled }),
      });
      setAccessSettings(result.settings);
      setNotice({ kind: 'success', text: `${activeTab === 'mcp' ? 'MCP' : '本地网关'}已${enabled ? '启用' : '关闭'}` });
    } catch (error) {
      setNotice({ kind: 'error', text: error instanceof Error ? error.message : '外部接入设置保存失败' });
    } finally { setAccessBusy(false); }
  }

  async function revoke(id: string) {
    try { await api(`/api/developer-tokens/${id}`, { method: 'DELETE' }); await onReload(); setNotice({ kind: 'success', text: activeTab === 'mcp' ? 'MCP 授权码已撤销' : 'API Token 已撤销' }); }
    catch (error) { setNotice({ kind: 'error', text: error instanceof Error ? error.message : '凭据撤销失败' }); }
  }

  async function copyEndpoint(value: string) {
    try { await navigator.clipboard.writeText(value); setNotice({ kind: 'success', text: '接入地址已复制' }); }
    catch (error) {
      void desktopLog('warn', 'external_access.copy_failed', describeDesktopLogValue(error));
      setNotice({ kind: 'error', text: '复制失败，请手动选择地址' });
    }
  }

  return <section className="token-workspace">
    <header><div><span>外部接入</span><h1>{activeTab === 'mcp' ? 'MCP Agent 接入' : '邮件 API 网关'}</h1><p>{activeTab === 'mcp' ? '为可信 Agent 创建独立授权码，通过标准 MCP 工具安全管理邮箱。' : '为项目创建细粒度 API Token，通过 REST 接口读取或发送邮件。'}</p></div><AppButton appearance="primary" icon={<Plus size={17} />} onClick={activeTab === 'mcp' ? onCreateMcp : onCreateApi} disabled={!available || !activeEnabled || (activeTab === 'api' && accounts.length === 0)}>{activeTab === 'mcp' ? '创建 MCP 授权码' : '创建 API Token'}</AppButton></header>

    <nav className="access-tabs" role="tablist" aria-label="外部接入方式"><button role="tab" aria-selected={activeTab === 'mcp'} className={activeTab === 'mcp' ? 'active' : ''} onClick={() => setActiveTab('mcp')}><Code size={18} /><span><strong>MCP</strong><small>Agent 工具调用</small></span></button><button role="tab" aria-selected={activeTab === 'api'} className={activeTab === 'api' ? 'active' : ''} onClick={() => setActiveTab('api')}><AddressBook size={18} /><span><strong>API 网关</strong><small>REST 接口调用</small></span></button></nav>

    <div className={`external-access-toggle${available && activeEnabled ? ' is-enabled' : ''}`}><span><strong>{activeTab === 'mcp' ? '启用 MCP 接入' : '启用 API 网关'}</strong><small>{!available ? endpointError || '正在启动本机 HTTP Adapter…' : activeEnabled ? '外部客户端可使用有效授权码连接' : '当前关闭，已有授权码也无法访问'}</small></span><AppCheckbox aria-label={activeTab === 'mcp' ? '启用 MCP 接入' : '启用 API 网关'} checked={available && activeEnabled} disabled={!available || !accessSettings || accessBusy} onChange={(_, data) => void setAccessEnabled(Boolean(data.checked))} /></div>

    <div className={`endpoint-strip${available && activeEnabled ? '' : ' is-disabled'}`}><Code size={21} /><span><small>{activeTab === 'mcp' ? 'Streamable HTTP 地址' : 'REST API 地址'}</small><code>{endpoint || '正在准备本机回环地址…'}</code></span><button disabled={!available || !activeEnabled} onClick={() => void copyEndpoint(endpoint)}><Copy size={17} />复制</button></div>

    <div className="token-columns"><div className="token-list"><div className="token-title"><h2>{activeTab === 'mcp' ? 'MCP 授权码' : 'API Token'}</h2><span>{visibleTokens.filter((token) => token.expiresAt > new Date().toISOString()).length} 个正在生效</span></div>{visibleTokens.length === 0 ? <div className="token-empty"><Key size={38} weight="duotone" /><h3>{activeTab === 'mcp' ? '还没有 MCP 授权码' : '还没有 API Token'}</h3><p>{!activeEnabled ? '先开启上方接入开关，再创建授权码。' : activeTab === 'mcp' ? '创建独立授权码后，按下方配置即可让 Agent 连接。' : accounts.length === 0 ? '接入邮箱后即可创建 API Token。' : '选择邮箱与权限，为本地应用创建最小权限 Token。'}</p><button disabled={!activeEnabled || (activeTab === 'api' && accounts.length === 0)} onClick={activeTab === 'mcp' ? onCreateMcp : onCreateApi}>{activeTab === 'mcp' ? '创建 MCP 授权码' : '创建 API Token'}</button></div> : visibleTokens.map((token) => <article className="token-item" key={token.id}><div className={`token-icon ${activeTab === 'mcp' ? 'is-mcp' : ''}`}><Key size={20} /></div><div><strong>{token.name}</strong><code>{token.prefix}••••••••••••</code><span>{activeTab === 'mcp' ? '全部当前与未来邮箱' : `${token.scopes.map((scope) => scope.replace('messages:', '')).join(' · ')} · ${token.mailboxes.length} 个邮箱`}</span></div><div className="token-time"><small>到期时间</small><span>{new Intl.DateTimeFormat('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit' }).format(new Date(token.expiresAt))}</span></div><button className="revoke" onClick={() => void revoke(token.id)}>撤销</button></article>)}</div>
      {activeTab === 'api' ? <aside className="quickstart"><div className="quickstart-title"><AddressBook size={21} /><div><strong>快速调用</strong><span>指定邮箱读取最新 10 封邮件</span></div></div><label className="gateway-account-select"><span>API 使用的邮箱</span><AppSelect value={gatewayAccount} onValueChange={setGatewayAccount} options={accounts.map((account) => ({ value: account.email, label: `${providerLabel[account.provider]} · ${account.displayName} · ${account.email}` }))} /></label><pre><code><span className="code-muted">curl</span> {endpoint || 'http://127.0.0.1/gateway/v1'}/mailboxes/{gatewayAccount || 'user@example.com'}/messages?limit=10 \\{`\n`}  -H <span className="code-string">&quot;Authorization: Bearer imail_xxx&quot;</span></code></pre><div className="security-note"><WarningCircle size={18} /><p><strong>仅监听本机回环地址</strong><span>网关请求和响应均不会暴露 iMail 内部邮箱 ID。</span></p></div>{available && activeEnabled && <a href={`${baseUrl}/gateway/docs`} target="_blank" rel="noreferrer">打开轻量 API Console <ArrowRight size={15} /></a>}</aside> : <aside className="quickstart mcp-quickstart"><div className="quickstart-title"><Code size={21} /><div><strong>接入前准备</strong><span>授权码与 API Token 不互通</span></div></div><ol><li>启用本机或远程 Rust HTTP 服务的 MCP 接入。</li><li>创建并立即复制 <code>imail_mcp_</code> 授权码。</li><li>复制配置并让 Agent 连接。</li><li>任务完成后关闭接入或撤销授权码。</li></ol><div className="security-note"><WarningCircle size={18} /><p><strong>完整控制权限</strong><span>可管理账户、邮件和草稿，仅签发给可信 Agent。</span></p></div></aside>}
    </div>
    {activeTab === 'mcp' && available && activeEnabled && <McpIntegrationGuide endpoint={`${baseUrl}/mcp`} setNotice={setNotice} />}
  </section>;
}
