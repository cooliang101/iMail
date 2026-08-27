import type { ComponentChildren } from 'preact';
import { ArrowLeft, ArrowRight } from './icons';

export function SettingsPanelHeading({ title, ancestors = [], onBack }: {
  title: string;
  ancestors?: string[];
  onBack?: () => void;
}) {
  return <header className={`settings-panel-heading${onBack ? ' has-back' : ''}`}>
    {onBack && <button type="button" className="settings-heading-back" aria-label={`返回${ancestors[ancestors.length - 1] || '上一级设置'}`} onClick={onBack}><ArrowLeft size={19} /></button>}
    <nav className="settings-heading-trail" aria-label="设置层级">
      {ancestors.map((label, index) => <span key={`${label}-${index}`}>{label}<i aria-hidden="true">/</i></span>)}
      <h2>{title}</h2>
    </nav>
  </header>;
}

export function SettingsLinkRow({ icon, title, detail, value, danger = false, disabled = false, disclosure = true, expanded, onClick }: {
  icon: ComponentChildren;
  title: string;
  detail?: string;
  value?: string;
  danger?: boolean;
  disabled?: boolean;
  disclosure?: boolean;
  expanded?: boolean;
  onClick: () => void;
}) {
  return <button type="button" className={`settings-link-row${danger ? ' is-danger' : ''}${disclosure ? '' : ' no-disclosure'}${expanded ? ' is-expanded' : ''}`} disabled={disabled} aria-expanded={expanded} onClick={onClick}>
    <i className="settings-link-icon">{icon}</i>
    <span><strong>{title}</strong>{detail && <small>{detail}</small>}</span>
    {value && <em>{value}</em>}
    {disclosure && <ArrowRight className="settings-link-arrow" size={17} />}
  </button>;
}
