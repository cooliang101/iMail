import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';

export type ContextMenuItem = { id: string; label: string; icon?: ReactNode; shortcut?: string; danger?: boolean; disabled?: boolean; separatorBefore?: boolean; onSelect: () => void };

export function ContextMenu({ x, y, label, items, onClose }: { x: number; y: number; label: string; items: ContextMenuItem[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement | null>(null);
  const [position, setPosition] = useState({ x, y });
  useLayoutEffect(() => {
    const menu = ref.current; if (!menu) return;
    const bounds = menu.getBoundingClientRect();
    setPosition({ x: Math.max(8, Math.min(x, window.innerWidth - bounds.width - 8)), y: Math.max(8, Math.min(y, window.innerHeight - bounds.height - 8)) });
  }, [x, y, items.length]);
  useEffect(() => {
    const close = () => onClose();
    const keydown = (event: KeyboardEvent) => { if (event.key === 'Escape') close(); };
    window.addEventListener('pointerdown', close); window.addEventListener('blur', close); window.addEventListener('resize', close); window.addEventListener('keydown', keydown);
    return () => { window.removeEventListener('pointerdown', close); window.removeEventListener('blur', close); window.removeEventListener('resize', close); window.removeEventListener('keydown', keydown); };
  }, [onClose]);
  return <div ref={ref} className="context-menu" role="menu" aria-label={label} style={{ left: position.x, top: position.y }} onPointerDown={(event) => event.stopPropagation()}>
    <div className="context-menu-title">{label}</div>{items.map((item) => <button key={item.id} type="button" role="menuitem" className={`${item.danger ? 'danger' : ''} ${item.separatorBefore ? 'separator' : ''}`.trim()} disabled={item.disabled} onClick={() => { item.onSelect(); onClose(); }}>{item.icon && <i>{item.icon}</i>}<span>{item.label}</span>{item.shortcut && <kbd>{item.shortcut}</kbd>}</button>)}
  </div>;
}
