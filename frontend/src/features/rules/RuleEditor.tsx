import type { MailRuleInput, RuleAction, RuleCondition } from '../../app-model';
import type { Account } from '../../types';
import { AppInput, AppSelect, AppCheckbox, AppSwitch } from '../../components/form-controls';
import { AppButton } from '../../components/AppButton';
import { Plus, X } from '../../components/icons';
import { actionOptions, conditionOptions, mailboxOptions, newAction, newCondition } from './rule-model';

export function RuleEditor({ value, accounts, onChange, disabled }: { value: MailRuleInput; accounts: Account[]; onChange: (value: MailRuleInput) => void; disabled: boolean }) {
  const patch = (change: Partial<MailRuleInput>) => onChange({ ...value, ...change });
  const condition = (index: number, next: RuleCondition) => patch({ conditions: value.conditions.map((c, i) => i === index ? next : c) });
  const action = (index: number, next: RuleAction) => {
    const actions = value.actions.map((a, i) => i === index ? next : a);
    patch({ actions: [...actions.filter(a => a.type !== 'archive'), ...actions.filter(a => a.type === 'archive')] });
  };
  const booleanOptions = [{ value: 'true', label: '是' }, { value: 'false', label: '否' }];
  return <fieldset className="rule-editor" disabled={disabled}>
    <div className="rule-editor-basics">
      <label>规则名称<AppInput aria-label="规则名称" value={value.name} maxLength={80} onChange={e => patch({ name: e.currentTarget.value })} /></label>
      <label>优先级<AppInput aria-label="规则优先级" type="number" min={0} max={10000} step={1} value={value.priority} onChange={e => patch({ priority: Number(e.currentTarget.value) })} /></label>
    </div>
    <AppSwitch label="自动处理新收件" checked={value.enabled} onChange={(_, data) => patch({ enabled: data.checked })} />
    <section className="rule-editor-section"><h3>适用邮箱</h3>
      <AppCheckbox label="全部邮箱（含以后新增的邮箱）" checked={value.accountIds.length === 0} onChange={(_, data) => patch({ accountIds: data.checked ? [] : accounts.map(a => a.id) })} disabled={accounts.length === 0} />
      <div className="rule-account-list">{accounts.map(account => <AppCheckbox key={account.id} label={account.email} checked={value.accountIds.length === 0 || value.accountIds.includes(account.id)} onChange={(_, data) => {
        const selected = value.accountIds.length ? value.accountIds : accounts.map(a => a.id);
        const next = data.checked ? [...new Set([...selected, account.id])] : selected.filter(id => id !== account.id);
        // Keep at least one specific account; empty means all, not none.
        if (next.length) patch({ accountIds: next });
      }} />)}</div>
    </section>
    <section className="rule-editor-section"><div className="rule-section-heading"><h3>匹配条件</h3><AppSelect aria-label="匹配方式" value={value.matchMode} options={[{ value: 'all', label: '满足全部条件' }, { value: 'any', label: '满足任一条件' }]} onValueChange={v => patch({ matchMode: v as 'all' | 'any' })} disabled={disabled} /></div>
      <div className="rule-clause-list">{value.conditions.map((item, index) => <div className="rule-clause" key={index}>
        <AppSelect aria-label={`条件 ${index + 1}`} value={item.field} options={conditionOptions} onValueChange={v => condition(index, newCondition(v))} disabled={disabled} />
        {typeof item.value === 'boolean' ? <AppSelect aria-label={`条件值 ${index + 1}`} value={String(item.value)} options={booleanOptions} onValueChange={v => condition(index, { ...item, value: v === 'true' })} disabled={disabled} />
          : item.field === 'mailboxRole' ? <AppSelect aria-label={`条件值 ${index + 1}`} value={item.value} options={mailboxOptions} onValueChange={v => condition(index, { ...item, value: v })} disabled={disabled} />
          : <AppInput aria-label={`条件值 ${index + 1}`} value={item.value} placeholder={item.field === 'senderDomain' ? 'github.com' : item.field === 'sender' || item.field === 'recipient' ? 'name@example.com' : '输入匹配内容'} maxLength={320} onChange={e => condition(index, { ...item, value: e.currentTarget.value })} />}
        <AppButton appearance="subtle" icon={<X size={16} />} aria-label={`删除条件 ${index + 1}`} disabled={value.conditions.length === 1} onClick={() => patch({ conditions: value.conditions.filter((_, i) => i !== index) })} />
      </div>)}</div>
      <AppButton appearance="subtle" icon={<Plus size={16} />} disabled={value.conditions.length >= 20} onClick={() => patch({ conditions: [...value.conditions, newCondition('subjectContains')] })}>添加条件</AppButton>
    </section>
    <section className="rule-editor-section"><h3>执行动作</h3>
      <div className="rule-clause-list">{value.actions.map((item, index) => <div className="rule-clause" key={index}>
        <AppSelect aria-label={`动作 ${index + 1}`} value={item.type} options={actionOptions.map(option => ({ ...option, disabled: option.value === 'archive' && item.type !== 'archive' && value.actions.some(a => a.type === 'archive') }))} onValueChange={v => action(index, newAction(v))} disabled={disabled} />
        {'value' in item ? typeof item.value === 'boolean' ? <AppSelect aria-label={`动作值 ${index + 1}`} value={String(item.value)} options={item.type === 'markRead' ? [{ value: 'true', label: '已读' }, { value: 'false', label: '未读' }] : [{ value: 'true', label: '开启' }, { value: 'false', label: '关闭' }]} onValueChange={v => action(index, { ...item, value: v === 'true' })} disabled={disabled} />
          : <AppInput aria-label={`动作值 ${index + 1}`} value={item.value} maxLength={80} placeholder="标签名称" onChange={e => action(index, { ...item, value: e.currentTarget.value })} />
          : <span className="rule-hint">远程移动，最后执行</span>}
        <AppButton appearance="subtle" icon={<X size={16} />} aria-label={`删除动作 ${index + 1}`} disabled={value.actions.length === 1} onClick={() => patch({ actions: value.actions.filter((_, i) => i !== index) })} />
      </div>)}</div>
      <AppButton appearance="subtle" icon={<Plus size={16} />} disabled={value.actions.length >= 20} onClick={() => patch({ actions: [...value.actions.filter(a => a.type !== 'archive'), newAction('addLabel'), ...value.actions.filter(a => a.type === 'archive')] })}>添加动作</AppButton>
    </section>
    <AppCheckbox label="匹配后停止处理后续规则" checked={value.stopProcessing} onChange={(_, data) => patch({ stopProcessing: data.checked })} />
    <p className="rule-hint">优先级数字越小越先执行。同一封邮件按原始内容匹配；冲突动作由后执行的规则覆盖。标签与静音仅保存在 iMail，已读、星标和归档会同步到邮箱服务器。</p>
  </fieldset>;
}
