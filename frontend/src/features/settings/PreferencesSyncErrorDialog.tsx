import { CloudSlash, X } from '@phosphor-icons/react';
import { Overlay } from '../../components/Overlay';
import '../../styles/dialogs.css';
import '../../styles/settings.css';

export function PreferencesSyncErrorDialog({ message, onClose }: { message: string; onClose: () => void }) {
  return <Overlay onClose={onClose} dialogClassName="preferences-sync-error-shell">
    <div className="preferences-sync-error-modal">
      <header>
        <i aria-hidden="true"><CloudSlash size={24} weight="duotone" /></i>
        <div>
          <span>设置同步</span>
          <h2>服务端同步失败</h2>
        </div>
        <button type="button" onClick={onClose} aria-label="关闭"><X size={18} /></button>
      </header>
      <p role="alert">{message}</p>
      <small>本机设置已保留，应用可以继续使用；后续修改设置时会再次尝试同步。</small>
      <footer>
        <button type="button" onClick={onClose}>知道了</button>
      </footer>
    </div>
  </Overlay>;
}
