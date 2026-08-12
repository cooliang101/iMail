import { useEffect, useMemo, useState } from 'react';
import { ArrowLeft, File, FileArchive, Image as ImageIcon, SpinnerGap, WarningCircle, X } from '@phosphor-icons/react';
import { Overlay } from '../../components/Overlay';
import type { MessageAttachment } from '../../types';
import { usePlatform } from '../../platform/runtime';
import {
  createPreview,
  deletePreview,
  formatAttachmentSize,
  previewContentPath,
  resolvePreviewSource,
  resolvePreviewText,
  type ArchiveEntry,
  type PreviewDescriptor,
  type PreviewKind,
  type PreviewSession,
} from './attachment-api';

type SelectedResource = {
  name: string;
  kind: PreviewKind;
  contentType: string;
  path: string;
  size: number;
};

export function AttachmentPreviewModal({ messageId, attachment, onClose }: {
  messageId: string;
  attachment: MessageAttachment;
  onClose: () => void;
}) {
  const [session, setSession] = useState<PreviewSession>();
  const [error, setError] = useState('');
  const [selected, setSelected] = useState<SelectedResource>();

  useEffect(() => {
    let active = true;
    let createdId = '';
    setSession(undefined);
    setSelected(undefined);
    setError('');
    void createPreview(messageId, attachment.index)
      .then((value) => {
        createdId = value.previewId;
        if (active) setSession(value);
        else void deletePreview(value.previewId).catch(() => undefined);
      })
      .catch((reason) => {
        if (active) setError(reason instanceof Error ? reason.message : '无法准备附件预览');
      });
    return () => {
      active = false;
      if (createdId) void deletePreview(createdId).catch(() => undefined);
    };
  }, [attachment.index, messageId]);

  const resource = selected ?? (session ? {
    name: session.descriptor.filename,
    kind: session.descriptor.kind,
    contentType: session.descriptor.contentType,
    path: previewContentPath(session.previewId),
    size: session.descriptor.size,
  } : undefined);

  return <Overlay onClose={onClose} wide dialogClassName="attachment-preview-modal">
    <header className="attachment-preview-header">
      <div>
        <span>{selected ? '压缩包内容' : '附件预览'}</span>
        <h2 title={resource?.name ?? attachment.filename}>{resource?.name ?? attachment.filename}</h2>
        <p>{resource ? `${formatAttachmentSize(resource.size)} · ${resourceKindLabel(resource.kind)}` : '正在从邮箱服务器安全读取附件'}</p>
      </div>
      <div className="attachment-preview-header-actions">
        {selected && <button type="button" title="返回压缩包" aria-label="返回压缩包" onClick={() => setSelected(undefined)}><ArrowLeft size={18} /></button>}
        <button type="button" title="关闭" aria-label="关闭附件预览" onClick={onClose}><X size={18} /></button>
      </div>
    </header>
    <div className="attachment-preview-stage">
      {!session && !error && <div className="attachment-preview-status"><SpinnerGap className="spin" size={34} /><strong>正在准备预览</strong><span>首次查看需要从邮箱服务器读取附件</span></div>}
      {error && <div className="attachment-preview-status error"><WarningCircle size={34} /><strong>无法查看这个附件</strong><span>{error}</span></div>}
      {session && resource && (resource.kind === 'archive' && !selected
        ? <ArchiveBrowser descriptor={session.descriptor} previewId={session.previewId} onSelect={setSelected} />
        : <ResourceViewer resource={resource} />)}
    </div>
  </Overlay>;
}

function ResourceViewer({ resource }: { resource: SelectedResource }) {
  const platform = usePlatform();
  const [source, setSource] = useState('');
  const [error, setError] = useState('');
  useEffect(() => {
    let active = true;
    let revoke: () => void = () => undefined;
    setSource(''); setError('');
    const loading = resource.kind === 'text'
      ? resolvePreviewText(resource.path, platform.kind === 'tauri').then((value) => ({ url: value, revoke: () => undefined }))
      : resolvePreviewSource(resource.path, resource.contentType, platform.kind === 'tauri');
    void loading
      .then((value) => { revoke = value.revoke; if (active) setSource(value.url); else revoke(); })
      .catch((reason) => { if (active) setError(reason instanceof Error ? reason.message : '附件内容读取失败'); });
    return () => { active = false; revoke(); };
  }, [platform.kind, resource.contentType, resource.path]);

  if (resource.kind === 'unsupported') return <PreviewUnsupported />;
  if (error) return <div className="attachment-preview-status error"><WarningCircle size={34} /><strong>附件内容读取失败</strong><span>{error}</span></div>;
  if (!source) return <div className="attachment-preview-status"><SpinnerGap className="spin" size={30} /><strong>正在加载内容</strong></div>;
  if (resource.kind === 'text') return <div className="attachment-text-viewer"><pre>{source}</pre></div>;
  if (resource.kind === 'image') return <div className="attachment-image-viewer"><img src={source} alt={resource.name} /></div>;
  if (resource.kind === 'pdf') return <object className="attachment-pdf-viewer" data={source} type="application/pdf"><PreviewUnsupported text="当前运行环境无法显示 PDF，请使用下载功能查看。" /></object>;
  if (resource.kind === 'video') return <div className="attachment-video-viewer"><video src={source} controls preload="metadata">当前运行环境无法播放该视频。</video></div>;
  return <PreviewUnsupported />;
}

function ArchiveBrowser({ descriptor, previewId, onSelect }: {
  descriptor: PreviewDescriptor;
  previewId: string;
  onSelect: (resource: SelectedResource) => void;
}) {
  const entries = useMemo(() => [...descriptor.archiveEntries].sort((left, right) => Number(right.directory) - Number(left.directory) || left.path.localeCompare(right.path, 'zh-CN')), [descriptor.archiveEntries]);
  return <div className="archive-browser">
    <div className="archive-browser-summary"><FileArchive size={21} /><span><strong>{entries.length} 个条目</strong><small>文件按需在 Rust 层解压，不会自动写入磁盘</small></span></div>
    <div className="archive-entry-list" role="list">
      {entries.map((entry) => <ArchiveEntryRow key={entry.id} entry={entry} onOpen={() => onSelect({ name: entry.name, kind: entry.kind, contentType: entry.contentType, path: previewContentPath(previewId, entry.id), size: entry.size })} />)}
    </div>
  </div>;
}

function ArchiveEntryRow({ entry, onOpen }: { entry: ArchiveEntry; onOpen: () => void }) {
  const previewable = !entry.directory && !entry.encrypted && entry.kind !== 'unsupported' && entry.kind !== 'archive';
  return <article className="archive-entry" role="listitem">
    <i>{entry.directory ? <FileArchive size={18} /> : entry.kind === 'image' ? <ImageIcon size={18} /> : <File size={18} />}</i>
    <span><strong title={entry.path}>{entry.path}</strong><small>{entry.directory ? '文件夹' : entry.encrypted ? '加密文件' : formatAttachmentSize(entry.size)}</small></span>
    {previewable && <button type="button" onClick={onOpen}>查看</button>}
  </article>;
}

function PreviewUnsupported({ text = '当前附件类型暂不支持应用内查看，请使用下载功能。' }: { text?: string }) {
  return <div className="attachment-preview-status"><File size={36} /><strong>暂无可用预览</strong><span>{text}</span></div>;
}

function resourceKindLabel(kind?: PreviewKind) {
  return ({ image: '图片', pdf: 'PDF 文档', video: '视频', archive: 'ZIP 压缩包', text: '纯文本文档', unsupported: '未知格式' } as Record<PreviewKind, string>)[kind ?? 'unsupported'];
}
