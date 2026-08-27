import { useEffect, useState } from 'preact/compat';
import { ArrowRight, CheckCircle, Code, Info, WarningCircle } from '../../components/icons';
import { usePlatform } from '../../platform/runtime';
import { api } from '../../services';
import type { ServiceInfo } from '../../types';
import { SettingsPanelHeading } from '../../components/settings-navigation';

const PROJECT_URL = 'https://github.com/cooliang101/imail';

export function AboutPanel() {
  const platform = usePlatform();
  const [service, setService] = useState<ServiceInfo>();
  const [serviceError, setServiceError] = useState('');
  const [linkError, setLinkError] = useState('');

  useEffect(() => {
    let cancelled = false;
    void api<ServiceInfo>('/api/system/info').then((value) => {
      if (!cancelled) setService(value);
    }).catch(() => {
      if (!cancelled) setServiceError('暂时无法读取服务版本');
    });
    return () => { cancelled = true; };
  }, []);

  async function openProject() {
    setLinkError('');
    try { await platform.openExternal(PROJECT_URL); }
    catch { setLinkError('无法打开浏览器，请复制下方项目地址'); }
  }

  return <section className="settings-feature-panel">
    <SettingsPanelHeading title="关于 iMail" />
    <div className="settings-panel-body about-panel-body">
      <section className="about-hero">
        <img src="/brand/imail-app-icon.png" alt="iMail" />
        <div><span>LOCAL-FIRST MAIL CLIENT</span><h3>iMail</h3><p>多邮箱统一收件箱与本地开发邮件网关。</p></div>
        <strong>v{__APP_VERSION__}</strong>
      </section>

      <dl className="settings-facts about-facts">
        <div><dt>客户端版本</dt><dd>{__APP_VERSION__}</dd></div>
        <div><dt>服务版本</dt><dd>{service?.version ?? (serviceError || '读取中…')}</dd></div>
        <div><dt>运行模式</dt><dd>{platform.kind === 'tauri' ? 'Windows 桌面端' : 'Web 客户端'}</dd></div>
        <div><dt>服务协议</dt><dd>{service ? `v${service.protocolVersion}` : '—'}</dd></div>
      </dl>

      <section className="about-project-card">
        <span className="about-project-icon"><Code size={23} /></span>
        <div><strong>GitHub 项目主页</strong><p>查看源代码、使用说明、问题反馈与后续版本。</p><code>{PROJECT_URL}</code></div>
        <button type="button" onClick={() => void openProject()}>打开 GitHub<ArrowRight size={16} /></button>
      </section>
      {linkError && <p className="about-link-error" role="alert"><WarningCircle size={16} />{linkError}</p>}

      <p className="about-status"><CheckCircle size={17} weight="fill" /><span>当前安装版本 <strong>{__APP_VERSION__}</strong></span><Info size={16} /><span>更新检查功能尚未启用，可从 GitHub 项目页关注新版本。</span></p>
    </div>
  </section>;
}
