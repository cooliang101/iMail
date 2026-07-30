import { useEffect, useState, type CSSProperties } from 'react';
import { Check, ClipboardText, Code, WarningCircle } from '@phosphor-icons/react';
import type { AppPreferences, CustomThemeDefinition } from '../../app-model';
import { AppInput, AppSelect, AppTextarea } from '../../components/form-controls';
import { parseCustomThemeJson } from '../appearance';
import customThemeGuide from '../../../docs/custom-theme.md?raw';

const colorFields: Array<{ key: keyof Pick<CustomThemeDefinition, 'canvas' | 'surface' | 'surfaceSubtle' | 'rail' | 'text' | 'textSecondary' | 'border' | 'accent' | 'accentSubtle'>; label: string }> = [
  { key: 'canvas', label: '画布' }, { key: 'surface', label: '主面板' }, { key: 'surfaceSubtle', label: '次级面板' },
  { key: 'rail', label: '账户轨' }, { key: 'text', label: '主文字' }, { key: 'textSecondary', label: '辅助文字' },
  { key: 'border', label: '边框' }, { key: 'accent', label: '强调色' }, { key: 'accentSubtle', label: '强调浅色' },
];

type Status = { kind: 'success' | 'error'; text: string } | null;

export function CustomThemeEditor({ preferences, onChange }: { preferences: AppPreferences; onChange: (value: AppPreferences) => void }) {
  const [draft, setDraft] = useState(preferences.customTheme);
  const [importValue, setImportValue] = useState('');
  const [guideOpen, setGuideOpen] = useState(false);
  const [status, setStatus] = useState<Status>(null);

  useEffect(() => { setDraft(preferences.customTheme); }, [preferences.customTheme]);

  function applyTheme(theme: CustomThemeDefinition) {
    setDraft(theme);
    onChange({ ...preferences, theme: 'custom', customTheme: theme });
    setStatus({ kind: 'success', text: '主题已校验并应用' });
  }

  function validateAndApply(theme: CustomThemeDefinition) {
    const parsed = parseCustomThemeJson(JSON.stringify(theme));
    if (!parsed.theme) { setStatus({ kind: 'error', text: parsed.error ?? '主题无效' }); return; }
    applyTheme(parsed.theme);
  }

  async function copyGuide() {
    try {
      await navigator.clipboard.writeText(customThemeGuide);
      setStatus({ kind: 'success', text: 'Markdown 规范已复制，可直接发送给 AI' });
    } catch { setStatus({ kind: 'error', text: '复制失败，请展开规范后手动复制' }); setGuideOpen(true); }
  }

  function importTheme() {
    const parsed = parseCustomThemeJson(importValue);
    if (!parsed.theme) { setStatus({ kind: 'error', text: parsed.error ?? '主题 JSON 无效' }); return; }
    applyTheme(parsed.theme);
  }

  return <section className="custom-theme-editor" aria-labelledby="custom-theme-title">
    <div className="custom-theme-editor-head">
      <div><small>安全令牌编辑器</small><h3 id="custom-theme-title">定义你的视觉语言</h3><p>修改后先校验再应用；颜色只接受六位十六进制值。</p></div>
      <div className="custom-theme-guide-actions">
        <button type="button" className="settings-secondary-action" onClick={() => setGuideOpen((value) => !value)}><Code size={16} />{guideOpen ? '收起规范' : '查看 AI 规范'}</button>
        <button type="button" className="settings-secondary-action" onClick={() => void copyGuide()}><ClipboardText size={16} />复制给 AI</button>
      </div>
    </div>

    <div className="custom-theme-fields">
      <label className="custom-theme-name"><span>主题名称</span><AppInput value={draft.name} maxLength={40} onChange={(_, data) => setDraft({ ...draft, name: data.value })} /></label>
      {colorFields.map((field) => <label key={field.key} className="custom-theme-color-field">
        <span>{field.label}</span><i style={{ '--custom-color': draft[field.key] } as CSSProperties} />
        <AppInput value={draft[field.key]} maxLength={7} onChange={(_, data) => setDraft({ ...draft, [field.key]: data.value })} />
      </label>)}
      <label><span>圆角</span><AppSelect value={draft.radius} onValueChange={(value) => setDraft({ ...draft, radius: value as CustomThemeDefinition['radius'] })} options={[{ value: 'compact', label: '紧凑' }, { value: 'balanced', label: '平衡' }, { value: 'rounded', label: '圆润' }]} /></label>
      <label><span>阴影</span><AppSelect value={draft.shadow} onValueChange={(value) => setDraft({ ...draft, shadow: value as CustomThemeDefinition['shadow'] })} options={[{ value: 'none', label: '无阴影' }, { value: 'soft', label: '柔和' }, { value: 'offset', label: '错位描边' }]} /></label>
      <label><span>字体气质</span><AppSelect value={draft.typography} onValueChange={(value) => setDraft({ ...draft, typography: value as CustomThemeDefinition['typography'] })} options={[{ value: 'system', label: '系统清晰' }, { value: 'technical', label: '技术感' }, { value: 'rounded', label: '柔和圆体' }]} /></label>
    </div>
    <div className="custom-theme-apply-row"><button type="button" className="settings-primary-action" onClick={() => validateAndApply(draft)}><Check size={16} weight="bold" />保存并应用</button></div>

    <div className="custom-theme-import">
      <div><strong>导入 AI 主题 JSON</strong><small>把 AI 按规范返回的纯 JSON 粘贴到这里。</small></div>
      <AppTextarea resize="vertical" rows={8} value={importValue} placeholder="{ &quot;name&quot;: &quot;...&quot;, ... }" onChange={(_, data) => setImportValue(data.value)} />
      <button type="button" className="settings-secondary-action" disabled={!importValue.trim()} onClick={importTheme}>校验并应用</button>
    </div>

    {guideOpen && <div className="custom-theme-guide"><div><Code size={17} /><strong>可复制给 AI 的 Markdown</strong></div><AppTextarea readOnly resize="vertical" rows={18} value={customThemeGuide} /></div>}
    {status && <p className={`custom-theme-status is-${status.kind}`}>{status.kind === 'success' ? <Check size={15} /> : <WarningCircle size={15} />} {status.text}</p>}
  </section>;
}
