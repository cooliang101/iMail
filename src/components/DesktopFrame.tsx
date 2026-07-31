import { useEffect, useState, type ReactNode } from 'react';
import { Copy, Minus, Square, X } from '@phosphor-icons/react';
import { BrandLogo } from './brand-logo';
import { isTauriRuntime } from '../platform/tauri-runtime';

type DesktopWindow = Awaited<ReturnType<typeof import('@tauri-apps/api/window')['getCurrentWindow']>>;

function DesktopTitlebar() {
  const [maximized, setMaximized] = useState(false);
  const [appWindow, setAppWindow] = useState<DesktopWindow>();

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void import('@tauri-apps/api/window').then(async ({ getCurrentWindow }) => {
      const current = getCurrentWindow();
      if (!active) return;
      setAppWindow(current);
      const update = async () => { if (active) setMaximized(await current.isMaximized()); };
      await update();
      unlisten = await current.onResized(update);
    });
    return () => { active = false; unlisten?.(); };
  }, []);

  const run = (action: (window: DesktopWindow) => Promise<void>) => {
    if (appWindow) void action(appWindow).catch((error) => console.error('[window]', error));
  };

  return <header className="desktop-titlebar">
    <div className="desktop-titlebar-drag" data-tauri-drag-region onDoubleClick={() => run((window) => window.toggleMaximize())}>
      <BrandLogo className="desktop-titlebar-logo" />
      <span>iMail</span>
    </div>
    <nav className="desktop-window-controls" aria-label="窗口控制">
      <button type="button" aria-label="最小化" title="最小化" onClick={() => run((window) => window.minimize())}><Minus size={14} weight="bold" /></button>
      <button type="button" aria-label={maximized ? '还原窗口' : '最大化'} title={maximized ? '还原窗口' : '最大化'} onClick={() => run((window) => window.toggleMaximize())}>
        {maximized ? <Copy size={12} weight="bold" /> : <Square size={12} weight="bold" />}
      </button>
      <button type="button" className="desktop-window-close" aria-label="关闭" title="关闭" onClick={() => run((window) => window.close())}><X size={15} weight="bold" /></button>
    </nav>
  </header>;
}

export function DesktopFrame({ children }: { children: ReactNode }) {
  if (!isTauriRuntime()) return children;
  return <div className="desktop-frame">
    <DesktopTitlebar />
    <div className="desktop-content">{children}</div>
  </div>;
}
