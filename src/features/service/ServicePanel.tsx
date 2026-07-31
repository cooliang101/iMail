import { HardDrives } from '@phosphor-icons/react';
import { configuredServiceUrl } from '../../service-config';
import { ServiceAddressEditor } from './ServiceAddressEditor';

export function ServicePanel() {
  const address = configuredServiceUrl() || `${window.location.origin}（同源）`;
  return <div className="settings-panel service-settings-panel">
    <header className="settings-panel-heading"><div><span>客户端连接</span><h2>iMail 服务</h2><p>此客户端不承载邮件服务，只连接到独立部署的 iMail 服务端。</p></div></header>
    <section className="service-endpoint-card"><HardDrives size={28} weight="duotone" /><div><small>当前服务地址</small><strong>{address}</strong><p>更换地址后客户端会重新载入，并在新服务上重新验证登录状态。</p></div></section>
    <ServiceAddressEditor />
  </div>;
}
