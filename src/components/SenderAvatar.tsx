import { useEffect, useState, type CSSProperties } from 'react';
import type { ContactLogo } from '../types';
import { desktopReadBinary } from '../desktop-http';
import { isTauriRuntime } from '../platform/tauri-runtime';

const desktopLogoSources = new Map<string, Promise<string | undefined>>();

export function resolveSenderLogoSource(
  logoUrl: string,
  desktop = isTauriRuntime(),
  readBinary = desktopReadBinary,
) {
  if (!desktop) return Promise.resolve<string | undefined>(logoUrl);
  const cached = desktopLogoSources.get(logoUrl);
  if (cached) return cached;
  const source = readBinary(logoUrl)
    .then((bytes) => {
      const copy = new Uint8Array(bytes.byteLength);
      copy.set(bytes);
      return URL.createObjectURL(new Blob([copy.buffer]));
    })
    .catch(() => undefined);
  desktopLogoSources.set(logoUrl, source);
  return source;
}

export function initials(value: string) {
  const parts = value.trim().split(/\s+/);
  return (parts.length > 1 ? parts.map((part) => part[0]).join('') : value.slice(0, 2)).toUpperCase();
}

export function SenderAvatar({ logo, name, color, large = false }: { logo?: ContactLogo; name: string; color: string; large?: boolean }) {
  const [failed, setFailed] = useState(false);
  const logoUrl = logo?.url;
  const [source, setSource] = useState<string>();
  useEffect(() => {
    let active = true;
    setFailed(false);
    setSource(undefined);
    if (logoUrl) void resolveSenderLogoSource(logoUrl).then((resolved) => { if (active) setSource(resolved); });
    return () => { active = false; };
  }, [logoUrl]);
  return <span className={`sender-avatar ${large ? 'large' : ''}`} style={{ '--avatar-color': color } as CSSProperties}>
    {initials(name)}
    {source && !failed && <img src={source} alt="" loading="lazy" onError={() => setFailed(true)} />}
  </span>;
}
