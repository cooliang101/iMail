import type { CSSProperties } from 'preact/compat';
import { Check, Palette } from '../../components/icons';
import type { AppPreferences } from '../../app-model';
import { themeOptions } from '../appearance';
import { PanelHeading } from './PanelHeading';
import { CustomThemeEditor } from './CustomThemeEditor';

export function AppearancePanel({ preferences, onChange }: { preferences: AppPreferences; onChange: (value: AppPreferences) => void }) {
  return <section className="settings-feature-panel">
    <PanelHeading eyebrow="界面个性" title="主题" description="选择一套完整视觉语言并立即应用；内置主题和安全自定义主题都会同步到服务端。" />
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
      {preferences.theme === 'custom' && <CustomThemeEditor preferences={preferences} onChange={onChange} />}
    </div>
  </section>;
}
