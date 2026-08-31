import { useEffect, useRef, useState, type FormEvent } from 'preact/compat';
import { AppCheckbox, AppInput, AppSelect } from '../../components/form-controls';
import { Overlay } from '../../components/shared';
import { AppButton } from '../../components/AppButton';
import { X } from '../../components/icons';
import type { Account } from '../../types';
import type { SearchFilters, SmartFolder } from '../../app-model';
import { localDateInput, validateFilters } from './search-model';
import './search.css';

export function AdvancedSearchPanel({ initial, folder, accounts, labels, onClose, onApply, onSave, onDelete }: {
  initial: SearchFilters; folder?: SmartFolder; accounts: Account[]; labels: string[];
  onClose: () => void; onApply: (filters: SearchFilters, folderId?: string) => Promise<boolean>;
  onSave: (name: string, filters: SearchFilters, id?: string) => Promise<SmartFolder>;
  onDelete: (id: string) => Promise<void>;
}) {
  const [filters, setFilters] = useState(initial);
  const [name, setName] = useState(folder?.name ?? '');
  const [savedId, setSavedId] = useState(folder?.id);
  const [saving, setSaving] = useState(Boolean(folder));
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const feedbackRef = useRef<HTMLDivElement>(null);
  useEffect(() => { if (error || confirmDelete) feedbackRef.current?.scrollIntoView({ block: 'nearest' }); }, [error, confirmDelete]);
  const patch = (update: Partial<SearchFilters>) => setFilters((current) => ({ ...current, ...update }));
  const toggle = (field: 'accountIds' | 'labels', value: string, checked: boolean) => patch({ [field]: checked ? [...(filters[field] ?? []), value] : (filters[field] ?? []).filter((item) => item !== value) });
  async function submit(event: FormEvent) {
    event.preventDefault();
    const invalid = validateFilters(filters);
    if (invalid) { setError(invalid); return; }
    if (saving && !name.trim()) { setError('请输入智能文件夹名称'); return; }
    setBusy(true); setError('');
    try {
      const saved = saving ? await onSave(name.trim(), filters, savedId) : undefined;
      if (saved) setSavedId(saved.id);
      if (await onApply(filters, saved?.id)) onClose();
      else setError('当前草稿未能保存，请先处理写信中的保存错误。查询条件已保留。');
    } catch (cause) { setError(cause instanceof Error ? cause.message : '搜索保存失败'); }
    finally { setBusy(false); }
  }
  return <Overlay dialogClassName="advanced-search-dialog" onClose={() => { if (!busy) onClose(); }}><section aria-labelledby="advanced-search-title" className="advanced-search-panel">
    <header><h2 id="advanced-search-title">{folder ? '编辑智能文件夹' : '高级搜索'}</h2><button type="button" aria-label="关闭高级搜索" disabled={busy} onClick={onClose}><X size={20} /></button></header>
    <form onSubmit={submit}>
      <div className="advanced-search-body app-scrollbar">
      <fieldset disabled={busy} className="search-fields">
        <div className="search-field-grid">
          {([['q', '关键词', '主题、摘要和参与者'], ['subject', '主题包含', '按字面匹配'], ['body', '正文包含', '中英文正文子串'], ['sender', '发件人', '完整邮箱地址'], ['recipient', '收件人 / 抄送人', '完整邮箱地址']] as const).map(([key,label,placeholder]) => <label key={key}><span>{label}</span><AppInput aria-label={label} value={filters[key] ?? ''} placeholder={placeholder} maxLength={key === 'sender' || key === 'recipient' ? 320 : 200} onChange={(event) => patch({ [key]: event.currentTarget.value || undefined })} /></label>)}
          <label><span>邮件文件夹</span><AppSelect aria-label="搜索邮件文件夹" value={filters.mailboxRole ?? ''} options={[{value:'',label:'所有文件夹'}, ...[['inbox','收件箱'],['sent','已发送'],['archive','归档'],['drafts','草稿'],['trash','已删除'],['junk','垃圾邮件'],['custom','自定义文件夹']].map(([value,label]) => ({value,label}))]} onValueChange={(value) => patch({mailboxRole:value || undefined,mailbox:undefined,mailboxName:undefined})} /></label>
          <label><span>开始时间（含）</span><AppInput aria-label="搜索开始时间" type="datetime-local" value={localDateInput(filters.since)} onChange={(event) => patch({since:event.currentTarget.value ? new Date(event.currentTarget.value).toISOString() : undefined})} /></label>
          <label><span>结束时间（不含）</span><AppInput aria-label="搜索结束时间" type="datetime-local" value={localDateInput(filters.before)} onChange={(event) => patch({before:event.currentTarget.value ? new Date(event.currentTarget.value).toISOString() : undefined})} /></label>
          {([['unread','阅读状态','未读','已读'],['flagged','星标','已加星标','未加星标'],['hasAttachments','附件','有附件','无附件'],['snoozed','稍后处理','仅稍后处理','排除稍后处理']] as const).map(([key,label,yes,no]) => <label key={key}><span>{label}</span><AppSelect aria-label={`搜索${label}`} value={filters[key] == null ? '' : String(filters[key])} options={[{value:'',label:'不限'},{value:'true',label:yes},{value:'false',label:no}]} onValueChange={(value) => patch({[key]:value === '' ? undefined : value === 'true'})} /></label>)}
        </div>
        {(filters.group || filters.mailbox || filters.mailboxName) && <div className="search-scope-note">当前范围：{[filters.group,filters.mailboxName ?? filters.mailbox].filter(Boolean).join(' / ')}<button type="button" onClick={() => patch({group:undefined,mailbox:undefined,mailboxName:undefined})}>取消范围限制</button></div>}
        <details><summary>邮箱账户 · {(filters.accountIds?.length ?? 0) ? `已选 ${filters.accountIds!.length}` : '全部'}</summary><div className="search-checkbox-list">{accounts.map((account) => <label key={account.id}><AppCheckbox checked={filters.accountIds?.includes(account.id) ?? false} onChange={(event) => toggle('accountIds',account.id,event.currentTarget.checked)} /><span>{account.displayName}<small>{account.email}</small></span></label>)}{filters.accountIds?.filter((id) => !accounts.some((account) => account.id === id)).map((id) => <label key={id}><AppCheckbox checked onChange={() => toggle('accountIds',id,false)} /><span>已移除的账户<small>{id}</small></span></label>)}</div></details>
        <details><summary>标签 · {(filters.labels?.length ?? 0) ? `同时包含 ${filters.labels!.length} 个` : '不限'}</summary><div className="search-checkbox-list">{[...new Set([...labels,...(filters.labels ?? [])])].map((label) => <label key={label}><AppCheckbox checked={filters.labels?.includes(label) ?? false} onChange={(event) => toggle('labels',label,event.currentTarget.checked)} /><span>{label}</span></label>)}{labels.length === 0 && !filters.labels?.length && <p>还没有邮件标签</p>}</div></details>
        <p className="search-help">条件之间同时满足；账户之间任选其一。正文仅搜索本实例已缓存的内容，不包含附件。时间使用当前设备时区，结果按邮件时间从新到旧排列。</p>
        {saving ? <label className="search-folder-name"><span>智能文件夹名称</span><AppInput aria-label="智能文件夹名称" value={name} onChange={(event) => setName(event.currentTarget.value)} maxLength={80} required /></label> : <button type="button" className="search-save-link" onClick={() => setSaving(true)}>保存为智能文件夹</button>}
      </fieldset>
      <div ref={feedbackRef}>
      {error && <p role="alert" className="inline-error">{error}</p>}
      {confirmDelete && folder && <div className="search-delete-confirm"><p>删除“{folder.name}”？只删除查询条件，不删除邮件。</p><button type="button" disabled={busy} onClick={async () => { setBusy(true); setError(''); try { await onDelete(folder.id); onClose(); } catch (cause) { setError(cause instanceof Error ? cause.message : '删除失败'); } finally { setBusy(false); } }}>确认删除智能文件夹</button><button type="button" disabled={busy} onClick={() => setConfirmDelete(false)}>取消</button></div>}
      </div>
      </div>
      <footer>{folder ? <button type="button" disabled={busy} onClick={() => setConfirmDelete(true)}>删除</button> : <button type="button" disabled={busy} onClick={() => { setFilters({}); setError(''); }}>清空条件</button>}<AppButton appearance="primary" type="submit" disabled={busy}>{busy ? '处理中…' : saving ? '保存并查看' : '搜索'}</AppButton></footer>
    </form>
  </section></Overlay>;
}
