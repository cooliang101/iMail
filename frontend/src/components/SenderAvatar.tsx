import { useEffect, useRef, useState, type CSSProperties } from 'preact/compat';
import type { ContactLogo } from '../types';
import { desktopReadBinary } from '../desktop-http';
import { isTauriRuntime } from '../platform/tauri-runtime';

const desktopLogoSources = new Map<string, Promise<string>>();
const desktopLogoReadQueue: Array<() => void> = [];
const maxConcurrentDesktopLogoReads = 6;
let activeDesktopLogoReads = 0;

function runQueuedDesktopLogoReads() {
  while (activeDesktopLogoReads < maxConcurrentDesktopLogoReads) {
    const next = desktopLogoReadQueue.shift();
    if (!next) return;
    activeDesktopLogoReads += 1;
    next();
  }
}

function queueDesktopLogoRead<T>(read: () => Promise<T>) {
  return new Promise<T>((resolve, reject) => {
    desktopLogoReadQueue.push(() => {
      void read().then(resolve, reject).finally(() => {
        activeDesktopLogoReads -= 1;
        runQueuedDesktopLogoReads();
      });
    });
    runQueuedDesktopLogoReads();
  });
}

export function resolveSenderLogoSource(
  logoUrl: string,
  desktop = isTauriRuntime(),
  readBinary = desktopReadBinary,
) {
  if (!desktop) return Promise.resolve<string | undefined>(logoUrl);
  const cached = desktopLogoSources.get(logoUrl);
  if (cached) return cached;
  const source = queueDesktopLogoRead(() => readBinary(logoUrl)).then((bytes) => {
      const copy = new Uint8Array(bytes.byteLength);
      copy.set(bytes);
      return URL.createObjectURL(new Blob([copy.buffer]));
    });
  desktopLogoSources.set(logoUrl, source);
  void source.catch(() => {
    if (desktopLogoSources.get(logoUrl) === source) desktopLogoSources.delete(logoUrl);
  });
  return source;
}

export function initials(value: string) {
  const parts = value.trim().split(/\s+/);
  return (parts.length > 1 ? parts.map((part) => part[0]).join('') : value.slice(0, 2)).toUpperCase();
}

export function SenderAvatar({ logo, name, color, large = false }: { logo?: ContactLogo; name: string; color: string; large?: boolean }) {
  const avatarRef = useRef<HTMLSpanElement>(null);
  const [failed, setFailed] = useState(false);
  const logoUrl = logo?.url;
  const [source, setSource] = useState<string>();
  useEffect(() => {
    let active = true;
    let retryTimer: ReturnType<typeof setTimeout> | undefined;
    let observer: IntersectionObserver | undefined;
    setFailed(false);
    setSource(undefined);
    if (!logoUrl) return () => { active = false; };

    const load = (retry = false) => {
      void resolveSenderLogoSource(logoUrl).then((resolved) => {
        if (active) setSource(resolved);
      }).catch((error: unknown) => {
        const message = error instanceof Error ? error.message : String(error);
        if (!active || retry || message.includes('404')) return;
        retryTimer = setTimeout(() => load(true), 1500);
      });
    };
    const element = avatarRef.current;
    if (element && typeof IntersectionObserver !== 'undefined') {
      observer = new IntersectionObserver((entries) => {
        if (!entries.some((entry) => entry.isIntersecting)) return;
        observer?.disconnect();
        load();
      }, { rootMargin: '240px' });
      observer.observe(element);
    } else {
      load();
    }
    return () => {
      active = false;
      observer?.disconnect();
      if (retryTimer) clearTimeout(retryTimer);
    };
  }, [logoUrl]);
  return <span ref={avatarRef} className={`sender-avatar ${large ? 'large' : ''}`} style={{ '--avatar-color': color } as CSSProperties}>
    {initials(name)}
    {source && !failed && <img src={source} alt="" loading="lazy" onError={() => setFailed(true)} />}
  </span>;
}
