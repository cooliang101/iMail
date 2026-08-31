import { useState } from 'preact/compat';
import type { Account } from '../../types';
import type { AccountSignature, AppPreferences, ComposeTemplate } from '../../app-model';
import { AppButton } from '../../components/AppButton';
import { AppCheckbox, AppInput, AppTextarea } from '../../components/form-controls';
import { SettingsLinkRow, SettingsPanelHeading } from '../../components/settings-navigation';
import { Envelope, File, Plus } from '../../components/icons';

type Detail = { kind: 'signature'; value: AccountSignature } | { kind: 'template'; value: ComposeTemplate };

export function CompositionSettingsPanel({ accounts, preferences, onChange }: {
  accounts: Account[]; preferences: AppPreferences; onChange: (value: AppPreferences) => void;
}) {
  const composition = preferences.composition ?? { signatures: [], templates: [] };
  const [detail, setDetail] = useState<Detail | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [error, setError] = useState('');
  const open = (next: Detail | null) => { setDetail(next); setConfirmDelete(false); setError(''); };
  const save = () => {
    if (!detail) return;
    if (detail.kind === 'signature') {
      if (new TextEncoder().encode(detail.value.text).length > 16000) { setError('签名不能超过 16 KB'); return; }
      onChange({ ...preferences, composition: { ...composition, signatures: [...composition.signatures.filter(item => item.accountId !== detail.value.accountId), detail.value] } });
    } else {
      if (!detail.value.name.trim()) { setError('请填写模板名称'); return; }
      if (new TextEncoder().encode(detail.value.name).length > 200 || new TextEncoder().encode(detail.value.text).length > 64000 || new TextEncoder().encode(detail.value.subject).length > 1000) { setError('模板名称、主题或正文过长'); return; }
      onChange({ ...preferences, composition: { ...composition, templates: [...composition.templates.filter(item => item.id !== detail.value.id), { ...detail.value, name: detail.value.name.trim() }] } });
    }
    open(null);
  };
  const remove = () => {
    if (!detail) return;
    onChange({ ...preferences, composition: detail.kind === 'signature'
      ? { ...composition, signatures: composition.signatures.filter(item => item.accountId !== detail.value.accountId) }
      : { ...composition, templates: composition.templates.filter(item => item.id !== detail.value.id) } });
    open(null);
  };
  const title = detail?.kind === 'signature'
    ? accounts.find(account => account.id === detail.value.accountId)?.email ?? '账户签名'
    : detail ? '编辑模板' : '写信';
  return <section className="settings-feature-panel">
    <SettingsPanelHeading title={title} ancestors={detail ? ['写信'] : []} onBack={detail ? () => open(null) : undefined} />
    <div className="settings-panel-body">
      {!detail ? <>
        <section className="account-detail-section"><h3>账户签名</h3><div className="settings-link-list">
          {accounts.map(account => <SettingsLinkRow key={account.id} icon={<Envelope size={18} />} title={account.email} value={composition.signatures.some(item => item.accountId === account.id && item.text) ? '已配置' : '未配置'} onClick={() => open({ kind: 'signature', value: composition.signatures.find(item => item.accountId === account.id) ?? { accountId: account.id, text: '', newMessages: true, replies: true } })} />)}
          {accounts.length === 0 && <p>接入邮箱后可配置账户签名。</p>}
        </div></section>
        <section className="account-detail-section"><h3>模板与快捷短语</h3><div className="settings-link-list">
          {composition.templates.map(template => <SettingsLinkRow key={template.id} icon={<File size={18} />} title={template.name} onClick={() => open({ kind: 'template', value: { ...template } })} />)}
          <SettingsLinkRow icon={<Plus size={18} />} title="新建模板" disabled={composition.templates.length >= 100} onClick={() => open({ kind: 'template', value: { id: crypto.randomUUID(), name: '', subject: '', text: '' } })} />
        </div></section>
      </> : <form className="account-inline-editor" onSubmit={event => { event.preventDefault(); save(); }}>
        {confirmDelete ? <div role="alert"><p>确认删除此{detail.kind === 'signature' ? '签名配置' : '模板'}？已保存的草稿不会改变。</p><AppButton onClick={() => setConfirmDelete(false)}>取消</AppButton><AppButton onClick={remove}>确认删除</AppButton></div> : <>
          {detail.kind === 'template' && <>
            <label>模板名称<AppInput aria-label="模板名称" value={detail.value.name} maxLength={200} onChange={event => setDetail({ ...detail, value: { ...detail.value, name: event.currentTarget.value } })} /></label>
            <label>主题（仅在写信主题为空时填入）<AppInput aria-label="模板主题" value={detail.value.subject} maxLength={1000} onChange={event => setDetail({ ...detail, value: { ...detail.value, subject: event.currentTarget.value } })} /></label>
          </>}
          <label>{detail.kind === 'signature' ? '签名正文' : '模板正文'}<AppTextarea aria-label={detail.kind === 'signature' ? '签名正文' : '模板正文'} rows={8} value={detail.value.text} onChange={event => { const text = event.currentTarget.value; setDetail(detail.kind === 'signature' ? { ...detail, value: { ...detail.value, text } } : { ...detail, value: { ...detail.value, text } }); }} /></label>
          <small>按纯文本保存，插入后可在写信编辑器中继续排版。</small>
          {detail.kind === 'signature' && <>
            <AppCheckbox label="新邮件与转发自动插入" checked={detail.value.newMessages} onChange={(_, data) => setDetail({ ...detail, value: { ...detail.value, newMessages: data.checked } })} />
            <AppCheckbox label="回复自动插入" checked={detail.value.replies} onChange={(_, data) => setDetail({ ...detail, value: { ...detail.value, replies: data.checked } })} />
          </>}
          {error && <p className="inline-error" role="alert">{error}</p>}
          <div><AppButton type="submit" appearance="primary">保存</AppButton><AppButton onClick={() => open(null)}>取消</AppButton><AppButton onClick={() => setConfirmDelete(true)}>删除</AppButton></div>
        </>}
      </form>}
    </div>
  </section>;
}
