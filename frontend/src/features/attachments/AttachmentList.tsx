import { useState } from 'react';
import { DownloadSimple, Eye, File } from '@phosphor-icons/react';
import type { MessageAttachment } from '../../types';
import { usePlatform } from '../../platform/runtime';
import { absoluteServiceUrl } from '../../service-config';
import { AttachmentPreviewModal } from './AttachmentPreviewModal';
import { canPreviewAttachment, formatAttachmentSize } from './attachment-api';

export function AttachmentList({ messageId, attachments }: { messageId: string; attachments: MessageAttachment[] }) {
  const platform = usePlatform();
  const [selected, setSelected] = useState<MessageAttachment>();
  return <>
    <section className="attachments">
      <p>{attachments.length} 个附件 · 支持安全查看图片、PDF、视频、纯文本和 ZIP 压缩包</p>
      <div className="attachment-list">
        {attachments.map((attachment, listIndex) => {
          const path = `/api/messages/${encodeURIComponent(messageId)}/attachments/${attachment.index ?? listIndex}`;
          const url = platform.kind === 'web' ? absoluteServiceUrl(path) : path;
          return <article className="attachment-card" key={`${attachment.filename}-${listIndex}`}>
            <File size={23} weight="duotone" />
            <span><strong title={attachment.filename}>{attachment.filename}</strong><small>{formatAttachmentSize(attachment.size)}</small></span>
            <div>
              {canPreviewAttachment(attachment) && <button type="button" onClick={() => setSelected(attachment)}><Eye size={15} />查看</button>}
              {platform.kind === 'tauri'
                ? <button type="button" onClick={() => void platform.saveDownload({ url, filename: attachment.filename })}><DownloadSimple size={15} />下载</button>
                : <a href={url} download={attachment.filename}><DownloadSimple size={15} />下载</a>}
            </div>
          </article>;
        })}
      </div>
    </section>
    {selected && <AttachmentPreviewModal messageId={messageId} attachment={selected} onClose={() => setSelected(undefined)} />}
  </>;
}
