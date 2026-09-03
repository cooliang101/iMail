import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'preact/compat';

export type ContextMenuItem = { id: string; label: string; icon?: ReactNode; shortcut?: string; danger?: boolean; disabled?: boolean; separatorBefore?: boolean; onSelect: () => void };

export function ContextMenu({ x, y, label, items, onClose }: { x: number; y: number; label: string; items: ContextMenuItem[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement | null>(null);
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const [position, setPosition] = useState({ x, y });
  useLayoutEffect(() => {
    const menu = ref.current; if (!menu) return;
    const bounds = menu.getBoundingClientRect();
    setPosition({ x: Math.max(8, Math.min(x, window.innerWidth - bounds.width - 8)), y: Math.max(8, Math.min(y, window.innerHeight - bounds.height - 8)) });
  }, [x, y, items.length]);
  useEffect(() => { ref.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus(); }, [x, y]);
  useEffect(() => {
    const close = () => onCloseRef.current();
    const keydown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' || event.key === 'Tab') { close(); return; }
      if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
      const buttons = Array.from(ref.current?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? []);
      if (!buttons.length) return;
      event.preventDefault();
      event.stopPropagation();
      const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 : event.key === 'ArrowDown' ? (current + 1) % buttons.length : current < 0 ? buttons.length - 1 : (current - 1 + buttons.length) % buttons.length;
      buttons[next].focus();
    };
    window.addEventListener('pointerdown', close); window.addEventListener('blur', close); window.addEventListener('resize', close); window.addEventListener('keydown', keydown);
    return () => { window.removeEventListener('pointerdown', close); window.removeEventListener('blur', close); window.removeEventListener('resize', close); window.removeEventListener('keydown', keydown); };
  }, []);
  return <div ref={ref} className="context-menu" role="menu" aria-label={label} style={{ left: position.x, top: position.y }} onPointerDown={(event) => event.stopPropagation()}>
    <div className="context-menu-title">{label}</div>{items.map((item) => <button key={item.id} type="button" role="menuitem" className={`${item.danger ? 'danger' : ''} ${item.separatorBefore ? 'separator' : ''}`.trim()} disabled={item.disabled} onClick={() => { item.onSelect(); onClose(); }}>{item.icon && <i>{item.icon}</i>}<span>{item.label}</span>{item.shortcut && <kbd>{item.shortcut}</kbd>}</button>)}
  </div>;
}
