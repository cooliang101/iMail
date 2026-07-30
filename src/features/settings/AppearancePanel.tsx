import { Check, Palette } from '@phosphor-icons/react';
import type { AppPreferences } from '../../app-model';
import { themeOptions } from '../appearance';
import { PanelHeading } from './PanelHeading';

export function AppearancePanel({ preferences, onChange }: { preferences: AppPreferences; onChange: (value: AppPreferences) => void }) {
  return <section className="settings-feature-panel">
    <PanelHeading eyebrow="界面个性" title="主题" description="选择一套完整的视觉语言。切换会立即应用，并同步到当前 iMail 用户。" />
    <div className="settings-panel-body">
      <div className="theme-choice-grid" role="radiogroup" aria-label="应用主题">
        {themeOptions.map((theme) => {
          const selected = preferences.theme === theme.id;
          return <button
            type="button"
            role="radio"
            aria-checked={selected}
            className={selected ? 'is-selected' : ''}
            key={theme.id}
            onClick={() => onChange({ ...preferences, theme: theme.id })}
          >
            <span className={`theme-preview theme-preview-${theme.id}`} aria-hidden="true">
              <i className="theme-preview-rail"><Palette size={15} weight="fill" /></i>
              <i className="theme-preview-nav"><b /><b /><b /></i>
              <i className="theme-preview-mail"><b /><b /><b /></i>
              <i className="theme-preview-reader"><b /><b /><b /><em /></i>
            </span>
            <span className="theme-choice-copy">
              <small>{theme.eyebrow}</small>
              <strong>{theme.name}</strong>
              <span>{theme.description}</span>
              <i className="theme-swatches" aria-hidden="true">{theme.colors.map((color) => <b key={color} style={{ '--swatch': color } as React.CSSProperties} />)}</i>
            </span>
            <i className="theme-choice-check">{selected && <Check size={15} weight="bold" />}</i>
          </button>;
        })}
      </div>
    </div>
  </section>;
}
