import { useState, type CSSProperties } from 'preact/compat';
import { Check, Palette } from '../../components/icons';
import type { AppPreferences } from '../../app-model';
import { themeOptions } from '../appearance';
import { SettingsLinkRow, SettingsPanelHeading } from '../../components/settings-navigation';
import { CustomThemeEditor } from './CustomThemeEditor';

export function AppearancePanel({ preferences, onChange }: { preferences: AppPreferences; onChange: (value: AppPreferences) => void }) {
  const [customEditorOpen, setCustomEditorOpen] = useState(false);

  if (customEditorOpen) return <section className="settings-feature-panel">
    <SettingsPanelHeading title="自定义主题" ancestors={['主题']} onBack={() => setCustomEditorOpen(false)} />
    <div className="settings-panel-body settings-detail-body"><CustomThemeEditor preferences={preferences} onChange={onChange} /></div>
  </section>;

  return <section className="settings-feature-panel">
    <SettingsPanelHeading title="主题" />
    <div className="settings-panel-body">
      <div className="theme-choice-grid" role="radiogroup" aria-label="应用主题">
        {themeOptions.map((theme) => {
          const selected = preferences.theme === theme.id;
          const colors = theme.id === 'custom'
            ? [preferences.customTheme.rail, preferences.customTheme.accent, preferences.customTheme.accentSubtle, preferences.customTheme.surface]
            : theme.colors;
          const previewStyle = theme.id === 'custom' ? {
            '--preview-rail': preferences.customTheme.rail, '--preview-accent': preferences.customTheme.accent,
            '--preview-soft': preferences.customTheme.accentSubtle, '--preview-surface': preferences.customTheme.surface,
          } as CSSProperties : undefined;
          return <button
            type="button"
            role="radio"
            aria-checked={selected}
            className={selected ? 'is-selected' : ''}
            key={theme.id}
            onClick={() => onChange({ ...preferences, theme: theme.id })}
          >
            <span className={`theme-preview theme-preview-${theme.id}`} style={previewStyle} aria-hidden="true">
              <i className="theme-preview-rail"><Palette size={15} weight="fill" /></i>
              <i className="theme-preview-nav"><b /><b /><b /></i>
              <i className="theme-preview-mail"><b /><b /><b /></i>
              <i className="theme-preview-reader"><b /><b /><b /><em /></i>
            </span>
            <span className="theme-choice-copy">
              <small>{theme.eyebrow}</small>
              <strong>{theme.name}</strong>
              <span>{theme.description}</span>
              <i className="theme-swatches" aria-hidden="true">{colors.map((color, index) => <b key={`${color}-${index}`} style={{ '--swatch': color } as CSSProperties} />)}</i>
            </span>
            <i className="theme-choice-check">{selected && <Check size={15} weight="bold" />}</i>
          </button>;
        })}
      </div>
      <div className="settings-link-list settings-link-list-spaced">
        <SettingsLinkRow icon={<Palette size={20} />} title="自定义主题" detail="单独编辑配色、圆角、阴影和界面密度。" value={preferences.theme === 'custom' ? '已启用' : '未启用'} onClick={() => { if (preferences.theme !== 'custom') onChange({ ...preferences, theme: 'custom' }); setCustomEditorOpen(true); }} />
      </div>
    </div>
  </section>;
}
