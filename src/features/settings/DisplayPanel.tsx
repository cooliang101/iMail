import { Check, Database, Eye } from '@phosphor-icons/react';
import type { AppPreferences } from '../../app-model';
import { PanelHeading } from './PanelHeading';

export function DisplayPanel({ preferences, onChange }: { preferences: AppPreferences; onChange: (value: AppPreferences) => void }) {
  return <section className="settings-feature-panel"><PanelHeading eyebrow="阅读体验" title="邮件展示" description="选择每封邮件正文首次打开时的查看方式，仍可在邮件内随时切换。" />
    <div className="settings-panel-body"><div className="display-choice-grid" role="radiogroup" aria-label="默认邮件正文视图">
      <button type="button" role="radio" aria-checked={preferences.defaultMessageView === 'source'} className={preferences.defaultMessageView === 'source' ? 'is-selected' : ''} onClick={() => onChange({ ...preferences, defaultMessageView: 'source' })}><Database size={22} /><span><strong>原始内容</strong><small>移除 HTML 节点、样式、脚本和远程资源，只显示纯文本内容，防止通过邮件嗅探器追踪阅读行为。</small></span><i>{preferences.defaultMessageView === 'source' && <Check size={14} />}</i></button>
      <button type="button" role="radio" aria-checked={preferences.defaultMessageView === 'rendered'} className={preferences.defaultMessageView === 'rendered' ? 'is-selected' : ''} onClick={() => onChange({ ...preferences, defaultMessageView: 'rendered' })}><Eye size={22} /><span><strong>渲染邮件</strong><small>提取并安全清洗邮件正文，在阅读页内保留内联样式排版。</small></span><i>{preferences.defaultMessageView === 'rendered' && <Check size={14} />}</i></button>
    </div></div>
  </section>;
}
