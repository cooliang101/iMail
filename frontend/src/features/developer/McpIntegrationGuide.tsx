import { useState } from 'preact/compat';
import { BookOpen, Code, Copy, Terminal, WarningCircle } from '../../components/icons';
import type { Notice } from '../../app-model';
import mcpGuideMarkdown from '../../../../docs/mcp-integration.md?raw';

const toolGroups = [
  { name: '状态与账户', tools: 'imail_status · accounts_list · account_add_with_code · account_start_oauth · account_reconnect_oauth · account_update · account_update_authorization_code · account_test_connection · account_remove' },
  { name: '同步与邮件', tools: 'mailbox_sync · sync_policy_get · sync_policy_update · messages_list · message_get · message_update · message_move · message_send' },
  { name: '附件与草稿', tools: 'attachment_download · drafts_list · draft_get · draft_save · draft_delete' },
  { name: '整理', tools: 'labels_list · notifications_list' },
];

export function McpIntegrationGuide({ endpoint, setNotice }: { endpoint: string; setNotice: (notice: Notice) => void }) {
  const [rawVisible, setRawVisible] = useState(false);
  const httpConfig = JSON.stringify({ mcpServers: { imail: { url: endpoint, headers: { Authorization: 'Bearer imail_mcp_xxx' } } } }, null, 2);

  async function copy(value: string, label: string) {
    try { await navigator.clipboard.writeText(value); setNotice({ kind: 'success', text: `${label}已复制` }); }
    catch { setNotice({ kind: 'error', text: `${label}复制失败，请手动选择` }); }
  }

  return <section className="mcp-guide" aria-labelledby="mcp-guide-title">
    <header><div><span>Agent 接入文档</span><h2 id="mcp-guide-title">创建后即可连接</h2><p>复制下方配置，将刚创建的 <code>imail_mcp_</code> 授权码交给 MCP 客户端。工具发现与调用由客户端完成。</p></div><div className="mcp-guide-actions"><button aria-expanded={rawVisible} aria-controls="mcp-raw-markdown" onClick={() => setRawVisible((value) => !value)}><Code size={16} />{rawVisible ? '收起原文' : '查看原文'}</button><button onClick={() => void copy(mcpGuideMarkdown, '文档')}><Copy size={16} />复制</button></div></header>
    {rawVisible && <article className="mcp-raw-markdown" id="mcp-raw-markdown"><div><strong>docs/mcp-integration.md</strong><span>以下内容与仓库源文件保持一致</span></div><pre><code>{mcpGuideMarkdown}</code></pre></article>}
    <div className="mcp-guide-grid mcp-guide-grid-single">
      <article className="mcp-config-card"><div className="mcp-config-heading"><div><strong>Streamable HTTP</strong><span>标准 MCP 客户端配置</span></div><button onClick={() => void copy(httpConfig, 'HTTP 配置')}><Copy size={15} />复制配置</button></div><pre><code>{httpConfig}</code></pre><p>将示例中的 <code>imail_mcp_xxx</code> 替换为刚创建的完整授权码。</p></article>
    </div>
    <div className="mcp-reference-grid">
      <article className="mcp-tool-reference"><div className="mcp-section-title"><Terminal size={19} /><div><strong>可调用工具</strong><span>连接后由 Agent 自动发现</span></div></div>{toolGroups.map((group) => <div className="mcp-tool-row" key={group.name}><strong>{group.name}</strong><code>{group.tools}</code></div>)}</article>
      <aside className="mcp-agent-notes"><div className="mcp-section-title"><WarningCircle size={19} /><div><strong>Agent 调用约定</strong><span>减少误操作与重复发送</span></div></div><ol><li>先调用 <code>accounts_list</code>，按邮箱地址确认账户。</li><li>操作邮件前先调用 <code>messages_list</code> 或 <code>message_get</code>。</li><li>发送前确认发件邮箱、收件人、主题和正文。</li><li>移除账户、移动邮件或删除草稿前获得用户确认。</li></ol><p>MCP 响应不会返回邮箱密码、OAuth Token、授权码或加密字段。</p></aside>
    </div>
  </section>;
}
