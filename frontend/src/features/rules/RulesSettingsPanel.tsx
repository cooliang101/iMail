import { useEffect, useRef, useState } from 'preact/compat';
import type { MailRule, MailRuleInput, RulePreview, RuleRun } from '../../app-model';
import type { Account } from '../../types';
import { AppButton } from '../../components/AppButton';
import { AppSwitch } from '../../components/form-controls';
import { SettingsLinkRow, SettingsPanelHeading } from '../../components/settings-navigation';
import { Clock, Plus, SlidersHorizontal } from '../../components/icons';
import { mailRulesService } from '../../services/mail-rules';
import { RuleEditor } from './RuleEditor';
import { ruleInput, validateRule } from './rule-model';
import './rules.css';

const statuses: Record<RuleRun['status'], string> = { pending: '等待执行', running: '执行中', succeeded: '已完成', failed: '执行失败', needsReview: '需要核对', cancelled: '已取消' };
type Page = { kind: 'list' } | { kind: 'runs' } | { kind: 'edit'; saved?: MailRule };

export function RulesSettingsPanel({ accounts, onReload }: { accounts: Account[]; onReload: () => Promise<void> }) {
  const [rules, setRules] = useState<MailRule[]>([]);
  const [runs, setRuns] = useState<RuleRun[]>([]);
  const [page, setPage] = useState<Page>({ kind: 'list' });
  const [draft, setDraft] = useState<MailRuleInput>(() => ruleInput());
  const [preview, setPreview] = useState<RulePreview | null>(null);
  const [confirmation, setConfirmation] = useState<'apply' | 'delete' | null>(null);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');
  const alive = useRef(true);
  const feedback = useRef<HTMLDivElement>(null);
  async function refresh() {
    const [list, history] = await Promise.all([mailRulesService.list(), mailRulesService.runs()]);
    if (alive.current) { setRules(list.rules); setRuns(history.runs); }
  }
  useEffect(() => {
    alive.current = true;
    void refresh().catch(reason => { if (alive.current) setError(String(reason.message ?? reason)); }).finally(() => { if (alive.current) setLoading(false); });
    return () => { alive.current = false; };
  }, []);
  useEffect(() => { if (error || notice || confirmation) feedback.current?.scrollIntoView({ block: 'nearest' }); }, [error, notice, confirmation]);
  function open(next: Page) {
    if (busy) return;
    setPage(next); setPreview(null); setConfirmation(null); setError(''); setNotice('');
    if (next.kind === 'edit') setDraft(ruleInput(next.saved));
  }
  async function perform(action: () => Promise<void>) {
    if (busy) return;
    setBusy(true); setError(''); setNotice('');
    try { await action(); } catch (reason) { if (alive.current) setError(reason instanceof Error ? reason.message : '邮件规则操作失败'); }
    finally { if (alive.current) setBusy(false); }
  }
  const dirty = page.kind === 'edit' && (!page.saved || JSON.stringify(ruleInput(page.saved)) !== JSON.stringify(draft));
  const title = page.kind === 'list' ? '邮件规则' : page.kind === 'runs' ? '执行记录' : page.saved?.name ?? '新建规则';
  function check() { const invalid = validateRule(draft); if (invalid) { setError(invalid); return false; } return true; }
  return <section className="settings-feature-panel mail-rules-panel">
    <SettingsPanelHeading title={title} ancestors={page.kind === 'list' ? [] : ['邮件规则']} onBack={page.kind === 'list' || busy ? undefined : () => open({ kind: 'list' })} />
    <div className="settings-panel-body app-scrollbar" aria-busy={busy || loading}>
      {loading ? <div className="rule-loading" role="status">正在加载规则…</div> : page.kind === 'list' ? <>
        <div className="settings-link-list">
          <SettingsLinkRow icon={<Plus size={18} />} title="新建规则" onClick={() => open({ kind: 'edit' })} disabled={busy || rules.length >= 100} />
          <SettingsLinkRow icon={<Clock size={18} />} title="执行记录" onClick={() => open({ kind: 'runs' })} disabled={busy} />
        </div>
        <div className="rule-list">{rules.map(rule => <div className="rule-list-item" key={rule.id}>
          <SettingsLinkRow icon={<SlidersHorizontal size={18} />} title={rule.name} value={`优先级 ${rule.priority}`} onClick={() => open({ kind: 'edit', saved: rule })} disabled={busy} />
          <AppSwitch aria-label={`启用规则 ${rule.name}`} checked={rule.enabled} disabled={busy} onChange={(_, data) => void perform(async () => { await mailRulesService.setEnabled(rule.id, data.checked); await refresh(); })} />
        </div>)}</div>
        {rules.length === 0 && <div className="rule-empty"><SlidersHorizontal size={28} /><h3>让重复整理自动完成</h3><p>新建规则，按发件人、主题或附件自动添加标签、静音或归档。</p></div>}
      </> : page.kind === 'runs' ? <>
        <div className="rule-actions"><AppButton disabled={busy} onClick={() => void perform(refresh)}>刷新记录</AppButton><small>最近 200 条</small></div>
        {runs.length === 0 && <p className="rule-empty">还没有执行记录。新收件匹配规则或确认处理历史邮件后，记录会显示在这里。</p>}
        <ol className="rule-run-list">{runs.map(run => <li key={run.id}>
          <div><strong>{run.ruleName}</strong><span className={`rule-status is-${run.status}`}>{statuses[run.status]}</span></div>
          <small>{accounts.find(a => a.id === run.accountId)?.email ?? '邮箱已移除'} · {run.source === 'manual' ? '手动执行' : '新收件'} · {run.completedActions}/{run.totalActions} 个动作</small>
          <time dateTime={run.updatedAt}>{new Date(run.updatedAt).toLocaleString()}</time>
          {run.errorCode && <p className="rule-hint">{run.status === 'needsReview' ? '归档结果不确定，请先检查邮箱；为避免重复移动，不会自动重试。' : `错误：${run.errorCode}`}</p>}
          {run.status === 'failed' && <AppButton disabled={busy} onClick={() => void perform(async () => { await mailRulesService.retry(run.id); await refresh(); })}>重试未完成动作</AppButton>}
        </li>)}</ol>
      </> : <form onSubmit={event => { event.preventDefault(); if (check()) void perform(async () => { const result = await mailRulesService.save(draft, page.saved?.id); setPage({ kind: 'edit', saved: result.rule }); setDraft(ruleInput(result.rule)); setPreview(null); setNotice('规则已保存；已有邮件不会自动执行。'); await refresh(); }); }}>
        <RuleEditor value={draft} accounts={accounts} disabled={busy || confirmation !== null} onChange={value => { setDraft(value); setPreview(null); setConfirmation(null); setError(''); setNotice(''); }} />
        <div className="rule-actions">
          <AppButton type="submit" appearance="primary" disabled={busy || !dirty || confirmation !== null}>保存规则</AppButton>
          <AppButton disabled={busy || confirmation !== null} onClick={() => { if (check()) void perform(async () => { setPreview(await mailRulesService.preview(draft, dirty ? undefined : page.saved?.id)); }); }}>预览匹配</AppButton>
          {page.saved && <AppButton appearance="subtle" className="rule-danger" disabled={busy || confirmation !== null} onClick={() => { setPreview(null); setConfirmation('delete'); }}>删除规则</AppButton>}
        </div>
        <p className="rule-hint">自动处理仅作用于完成首次同步后发现的新收件，服务需要保持运行。历史邮件必须先保存、预览，再单独确认执行。</p>
        {preview && <section className="rule-preview" aria-label="规则匹配预览">
          <h3>匹配 {preview.total} 封 · 可执行 {preview.eligible} 封</h3>
          <p className="rule-hint">预览不修改邮件。已经执行过当前版本的邮件不会重复执行；下方最多展示 50 封。</p>
          <ul>{preview.messages.map(message => <li key={message.id}><strong>{message.subject || '（无主题）'}</strong><small>{accounts.find(a => a.id === message.accountId)?.email}</small></li>)}</ul>
          {preview.token && preview.eligible > 0 ? <AppButton disabled={busy || confirmation !== null} onClick={() => setConfirmation('apply')}>处理这 {preview.eligible} 封历史邮件…</AppButton> : dirty && <p className="rule-hint">保存规则后重新预览，才能处理历史邮件。</p>}
        </section>}
      </form>}
      <div ref={feedback}>
        {confirmation && <section className="rule-confirm" role="alert">
          <h3>{confirmation === 'delete' ? '删除这条规则？' : `确认处理 ${preview?.eligible ?? 0} 封历史邮件？`}</h3>
          <p>{confirmation === 'delete' ? '尚未开始的任务将取消，已执行的标签、静音或远程动作不会撤销。' : '将执行当前规则的全部动作，可能改变邮箱服务器中的已读、星标或归档位置。预览过期或邮件变化后需要重新预览。'}</p>
          <div className="rule-actions"><AppButton disabled={busy} onClick={() => setConfirmation(null)}>取消</AppButton><AppButton appearance="primary" disabled={busy} onClick={() => void perform(async () => {
            if (confirmation === 'delete' && page.kind === 'edit' && page.saved) { await mailRulesService.remove(page.saved.id); setPage({ kind: 'list' }); }
            if (confirmation === 'apply' && preview?.token) { const result = await mailRulesService.apply(preview.token); setNotice(`已提交 ${result.queued} 条规则任务，可在执行记录中查看进度。`); setPreview(null); await onReload(); }
            setConfirmation(null); await refresh();
          })}>{confirmation === 'delete' ? '确认删除' : '确认执行'}</AppButton></div>
        </section>}
        {error && <p role="alert" className="inline-error">{error}</p>}
        {error && page.kind === 'list' && <AppButton disabled={busy} onClick={() => void perform(refresh)}>重新加载</AppButton>}
        {notice && <p role="status" className="rule-notice">{notice}</p>}
      </div>
    </div>
  </section>;
}
