import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { CaretDown, CaretUp, EnvelopeSimple, PencilSimple, Power, Tray } from '@phosphor-icons/react';
import type { AppThemeId, CustomThemeDefinition } from '../../app-model';
import { ProviderIcon } from '../../components/provider-icons';
import { customThemeCssVariables } from '../appearance/theme-runtime';
import { defaultCustomTheme, normalizeCustomTheme, normalizeThemeId } from '../appearance/theme-model';
import { visibleTrayAccounts } from './tray-menu-model';
import './tray-menu.css';

type TrayAccount = {
  id: string;
  email: string;
  displayName: string;
  provider: 'outlook' | 'gmail' | 'qq' | 'yahoo' | 'hotmail' | 'icloud' | 'custom';
  color: string;
};
type TrayMenuData = { accounts: TrayAccount[]; themeId: AppThemeId; customTheme: CustomThemeDefinition };

const emptyData: TrayMenuData = { accounts: [], themeId: 'mint-fresh', customTheme: defaultCustomTheme };

async function invokeTray<T>(command: string, args?: Record<string, unknown>) {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(command, args);
}

function applyTheme(data: TrayMenuData) {
  const themeId = normalizeThemeId(data.themeId);
  const custom = normalizeCustomTheme(data.customTheme);
  document.documentElement.dataset.theme = themeId;
  document.documentElement.style.colorScheme = 'light';
  for (const [name, value] of Object.entries(customThemeCssVariables(custom))) {
    if (themeId === 'custom') document.documentElement.style.setProperty(name, String(value));
    else document.documentElement.style.removeProperty(name);
  }
}

export function TrayMenuApp() {
  const [data, setData] = useState<TrayMenuData>(emptyData);
  const [expanded, setExpanded] = useState(false);
  const panelRef = useRef<HTMLElement>(null);
  const visibleAccounts = visibleTrayAccounts(data.accounts, expanded);

  useEffect(() => {
    let closed = false;
    let unlisten: (() => void) | undefined;
    void invokeTray<TrayMenuData>('desktop_get_tray_menu').then((next) => {
      if (!closed) setData(next);
    });
    void import('@tauri-apps/api/event').then(async ({ listen }) => {
      unlisten = await listen<TrayMenuData>('desktop-tray-menu-updated', ({ payload }) => {
        if (!closed) setData(payload);
      });
      if (closed) unlisten();
    });
    return () => { closed = true; unlisten?.(); };
  }, []);

  useEffect(() => { applyTheme(data); }, [data]);
  useEffect(() => {
    const close = (event: KeyboardEvent) => {
      if (event.key === 'Escape') void invokeTray('desktop_tray_action', { action: 'hide' });
    };
    window.addEventListener('keydown', close);
    return () => window.removeEventListener('keydown', close);
  }, []);
  useLayoutEffect(() => {
    if (!panelRef.current) return;
    const height = Math.ceil(panelRef.current.getBoundingClientRect().height + 16);
    void invokeTray('desktop_resize_tray_menu', { height });
  }, [data.accounts.length, expanded]);

  const action = (name: string, accountId?: string) => {
    void invokeTray('desktop_tray_action', { action: name, accountId });
  };

  return <main className="tray-menu-shell">
    <section className="tray-menu-panel" ref={panelRef} aria-label="iMail 托盘菜单">
      <header className="tray-menu-header">
        <span className="tray-menu-brand"><EnvelopeSimple size={20} weight="fill" /></span>
        <span><strong>iMail</strong><small>邮件在后台保持同步</small></span>
      </header>
      <div className="tray-menu-actions">
        <button type="button" onClick={() => action('show')}><Tray size={18} weight="duotone" /><span>打开 iMail</span></button>
        <button type="button" onClick={() => action('compose')}><PencilSimple size={18} weight="duotone" /><span>写邮件</span></button>
      </div>
      <div className="tray-menu-divider" />
      <div className="tray-menu-section-title"><span>邮箱</span><small>{data.accounts.length}</small></div>
      <div className="tray-account-list">
        {visibleAccounts.map((account) => <button className="tray-account" type="button" key={account.id} onClick={() => action('account', account.id)}>
          <span className={`tray-provider provider-${account.provider}`} style={{ '--account-color': account.color } as React.CSSProperties}><ProviderIcon provider={account.provider} /></span>
          <span><strong>{account.displayName || account.email}</strong><small>{account.email}</small></span>
        </button>)}
        {data.accounts.length === 0 && <div className="tray-menu-empty">暂未添加邮箱</div>}
      </div>
      {data.accounts.length > 5 && <button className="tray-menu-more" type="button" onClick={() => setExpanded((value) => !value)}>
        {expanded ? <CaretUp size={15} /> : <CaretDown size={15} />}{expanded ? '收起邮箱' : `展示更多（${data.accounts.length - 5}）`}
      </button>}
      <div className="tray-menu-divider" />
      <button className="tray-menu-quit" type="button" onClick={() => action('quit')}><Power size={17} />退出 iMail</button>
    </section>
  </main>;
}
