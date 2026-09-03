import { Code, Copy, DownloadSimple, Eye, Globe, Image, LinkSimple } from '../../components/icons';
import { ContextMenu, type ContextMenuItem } from '../../components/ContextMenu';
import type { MessageBodyView } from '../../app-model';
import type { PlatformRuntime } from '../../platform/types';
import { canSaveImageDirectly, imageDownloadFilename, linkCopyValue, mailBodyUrlKind, type MailBodyContextTarget } from './mail-body-context';

export function MessageBodyContextMenu({ x, y, target, platform, bodyView, hasHtml, translationActive, onClose, onCopy, onDownloadRaw, onSaveBody, onTranslate, onToggleBodyView, onOpen, onShare, onSaveImage }: {
  x: number;
  y: number;
  target: MailBodyContextTarget;
  platform: PlatformRuntime;
  bodyView: MessageBodyView;
  hasHtml: boolean;
  translationActive: boolean;
  onClose: () => void;
  onCopy: (value: string) => void;
  onDownloadRaw: () => void;
  onSaveBody: () => void;
  onTranslate: () => void;
  onToggleBodyView: () => void;
  onOpen: (url: string) => void;
  onShare: (url: string) => void;
  onSaveImage: (url: string, filename: string) => void;
}) {
  let label = '邮件正文';
  let items: ContextMenuItem[] = [];

  if (target.kind === 'selection') {
    label = '所选文本';
    items = [{ id: 'copy-selection', label: '复制', icon: <Copy size={17} />, onSelect: () => onCopy(target.text) }];
  } else if (target.kind === 'link') {
    const kind = mailBodyUrlKind(target.href);
    if (!kind) return null;
    label = kind === 'email' ? '邮箱链接' : kind === 'phone' ? '电话链接' : '网页链接';
    const openLabel = kind === 'email' ? '写邮件' : kind === 'phone' ? '拨打电话' : platform.kind === 'web' ? '在新窗口打开' : '在默认浏览器打开';
    items = [
      { id: 'open-link', label: openLabel, icon: <LinkSimple size={17} />, onSelect: () => onOpen(target.href) },
      { id: 'copy-link', label: kind === 'web' ? '复制链接' : kind === 'email' ? '复制邮箱地址' : '复制电话号码', icon: <Copy size={17} />, onSelect: () => onCopy(linkCopyValue(target.href, kind)) },
      ...(kind === 'web' && platform.share ? [{ id: 'share-link', label: '分享链接', icon: <LinkSimple size={17} />, separatorBefore: true, onSelect: () => onShare(target.href) }] : []),
      ...(target.image ? imageItems(target.image, platform, onOpen, onCopy, onSaveImage, true) : []),
    ];
  } else if (target.kind === 'image') {
    label = '邮件图片';
    items = imageItems(target, platform, onOpen, onCopy, onSaveImage, false);
  } else {
    items = [
      { id: 'copy-body', label: '复制当前显示内容', icon: <Copy size={17} />, onSelect: () => onCopy('') },
      { id: 'download-source', label: '下载原始邮件（.eml）', icon: <DownloadSimple size={17} />, onSelect: onDownloadRaw },
      { id: 'save-body', label: '保存当前显示内容（.txt）', icon: <DownloadSimple size={17} />, onSelect: onSaveBody },
      { id: 'translate', label: '翻译邮件…', icon: <Globe size={17} />, separatorBefore: true, onSelect: onTranslate },
      ...(hasHtml && !translationActive ? [{ id: 'toggle-render', label: bodyView === 'source' ? '切换到原始样式' : '切换到纯文本阅读', icon: bodyView === 'source' ? <Eye size={17} /> : <Code size={17} />, onSelect: onToggleBodyView }] : []),
    ];
  }

  return <ContextMenu x={x} y={y} label={label} items={items} onClose={onClose} />;
}

function imageItems(image: { src: string; alt: string }, platform: PlatformRuntime, onOpen: (url: string) => void, onCopy: (value: string) => void, onSaveImage: (url: string, filename: string) => void, separated: boolean): ContextMenuItem[] {
  const kind = mailBodyUrlKind(image.src);
  const canSave = kind === 'web' || (typeof window !== 'undefined' && canSaveImageDirectly(image.src, window.location.href));
  return [
    ...(kind === 'web' ? [{ id: 'open-image', label: platform.kind === 'web' ? '在新窗口打开图片' : '在默认浏览器打开图片', icon: <Image size={17} />, separatorBefore: separated, onSelect: () => onOpen(image.src) }] : []),
    { id: 'copy-image-url', label: '复制图片地址', icon: <Copy size={17} />, separatorBefore: separated && kind !== 'web', onSelect: () => onCopy(image.src) },
    ...(canSave ? [{ id: 'save-image', label: '保存图片', icon: <DownloadSimple size={17} />, onSelect: () => onSaveImage(image.src, imageDownloadFilename(image.src, image.alt)) }] : []),
  ];
}
