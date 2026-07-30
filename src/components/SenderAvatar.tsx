import { useEffect, useState, type CSSProperties } from 'react';
import type { ContactLogo } from '../types';

export function initials(value: string) {
  const parts = value.trim().split(/\s+/);
  return (parts.length > 1 ? parts.map((part) => part[0]).join('') : value.slice(0, 2)).toUpperCase();
}

export function SenderAvatar({ logo, name, color, large = false }: { logo?: ContactLogo; name: string; color: string; large?: boolean }) {
  const [failed, setFailed] = useState(false);
  const logoUrl = logo?.url;
  useEffect(() => { setFailed(false); }, [logoUrl]);
  return <span className={`sender-avatar ${large ? 'large' : ''}`} style={{ '--avatar-color': color } as CSSProperties}>
    {initials(name)}
    {logoUrl && !failed && <img src={logoUrl} alt="" loading="lazy" onError={() => setFailed(true)} />}
  </span>;
}
