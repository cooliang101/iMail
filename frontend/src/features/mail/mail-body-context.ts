export type MailBodyContextTarget =
  | { kind: 'selection'; text: string }
  | { kind: 'link'; href: string; image?: { src: string; alt: string } }
  | { kind: 'image'; src: string; alt: string }
  | { kind: 'body' };

export function resolveMailBodyContextTarget(root: HTMLElement, eventTarget: EventTarget | null, selection: Selection | null): MailBodyContextTarget {
  const selectedText = selection?.isCollapsed === false && selectionInside(root, selection) ? selection.toString() : '';
  if (selectedText.trim()) return { kind: 'selection', text: selectedText };

  const node = eventTarget as (Node & { parentElement?: Element | null }) | null;
  const element = node?.nodeType === 1 ? node as unknown as Element : node?.parentElement ?? null;
  const image = element?.closest<HTMLImageElement>('img[src]');
  const link = element?.closest<HTMLAnchorElement>('a[href]');
  if (link && root.contains(link)) return {
    kind: 'link',
    href: link.href,
    ...(image && link.contains(image) ? { image: { src: image.src, alt: image.alt } } : {}),
  };
  if (image && root.contains(image)) return { kind: 'image', src: image.src, alt: image.alt };
  return { kind: 'body' };
}

function selectionInside(root: HTMLElement, selection: Selection) {
  const anchor = selection.anchorNode;
  const focus = selection.focusNode;
  return Boolean(anchor && focus && root.contains(anchor) && root.contains(focus));
}

export function mailBodyUrlKind(value: string): 'web' | 'email' | 'phone' | null {
  try {
    const protocol = new URL(value).protocol.toLocaleLowerCase();
    if (protocol === 'http:' || protocol === 'https:') return 'web';
    if (protocol === 'mailto:') return 'email';
    if (protocol === 'tel:') return 'phone';
  } catch { /* Invalid sanitized content is treated as inert. */ }
  return null;
}

export function linkCopyValue(value: string, kind: 'web' | 'email' | 'phone') {
  if (kind === 'web') return value;
  try { return decodeURIComponent(new URL(value).pathname); }
  catch { return value; }
}

export function imageDownloadFilename(src: string, alt: string) {
  try {
    const candidate = new URL(src).pathname.split('/').pop();
    if (candidate && /\.(?:avif|bmp|gif|jpe?g|png|webp)$/i.test(candidate)) return safeDownloadName(decodeURIComponent(candidate), '邮件图片.png');
  } catch { /* Use the accessible label fallback. */ }
  return safeDownloadName(`${alt || '邮件图片'}.png`, '邮件图片.png');
}

export function displayedMessageFilename(subject: string) {
  return safeDownloadName(`${subject || '邮件正文'}.txt`, '邮件正文.txt');
}

function safeDownloadName(value: string, fallback: string) {
  const cleaned = value.trim().replace(/[<>:"/\\|?*\p{Cc}]/gu, '_').replace(/[. ]+$/g, '');
  const extension = cleaned.match(/\.[a-z\d]{1,8}$/i)?.[0] ?? '';
  const stem = cleaned.slice(0, extension ? -extension.length : undefined).slice(0, 100 - extension.length).replace(/[. ]+$/g, '');
  if (!stem) return fallback;
  const safeStem = /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:[. ]|$)/i.test(stem) ? `_${stem}` : stem;
  return `${safeStem}${extension}`;
}

export function canSaveImageDirectly(src: string, pageUrl: string) {
  if (/^data:image\/(?:avif|bmp|gif|jpeg|png|webp);base64,/i.test(src) || src.startsWith('blob:')) return true;
  try { return new URL(src).origin === new URL(pageUrl).origin; }
  catch { return false; }
}
