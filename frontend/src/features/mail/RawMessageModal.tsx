import { useEffect, useState } from 'preact/hooks';
import { Overlay } from '../../components/Overlay';
import { DownloadSimple, File, X } from '../../components/icons';
import { usePlatform } from '../../platform/runtime';
import { api } from '../../services';
import type { Message } from '../../types';
import { downloadRawMessage } from './raw-message-download';

type MessageSourceResponse = { available: boolean; size: number; sourceBase64?: string | null };
type SourceState =
  | { status: 'loading' }
  | { status: 'ready'; source: string; size: number }
  | { status: 'unavailable'; source: string }
  | { status: 'error'; message: string };

export function rawMessageSource(message: Pick<Message, 'html' | 'text'>) {
  return message.html !== undefined ? message.html : message.text ?? '';
}

export function decodeRawMessageSource(sourceBase64: string) {
  const binary = atob(sourceBase64);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return new TextDecoder().decode(bytes);
}

export function RawMessageModal({ message, onClose }: { message: Message; onClose: () => void }) {
  const platform = usePlatform();
  const messageId = message.id;
  const fallbackSource = rawMessageSource(message);
  const [state, setState] = useState<SourceState>({ status: 'loading' });
  const [downloadBusy, setDownloadBusy] = useState(false);
  const [downloadError, setDownloadError] = useState('');

  useEffect(() => {
    const controller = new AbortController();
    setState({ status: 'loading' });
    void api<MessageSourceResponse>(`/api/messages/${encodeURIComponent(messageId)}/source`, { signal: controller.signal })
      .then((result) => {
        if (!result.available || !result.sourceBase64) {
          setState({ status: 'unavailable', source: fallbackSource });
          return;
        }
        setState({ status: 'ready', source: decodeRawMessageSource(result.sourceBase64), size: result.size });
      })
      .catch((error: unknown) => {
        if (controller.signal.aborted) return;
        setState({ status: 'error', message: error instanceof Error ? error.message : '读取失败' });
      });
    return () => controller.abort();
  }, [fallbackSource, messageId]);

  const source = state.status === 'ready' || state.status === 'unavailable' ? state.source : '';
  const detail = state.status === 'ready'
    ? `完整 EML / RFC 822 · ${state.size.toLocaleString('zh-CN')} 字节`
    : state.status === 'unavailable'
      ? `历史邮件正文回退 · ${source.length.toLocaleString('zh-CN')} 个字符`
      : state.status === 'error' ? '读取失败' : '正在读取…';
  const download = async () => {
    setDownloadBusy(true);
    setDownloadError('');
    try { await downloadRawMessage(platform, message); }
    catch (reason) { setDownloadError(reason instanceof Error ? reason.message : '原始邮件下载失败'); }
    finally { setDownloadBusy(false); }
  };

  return <Overlay onClose={onClose} wide dialogClassName="raw-message-modal">
    <div className="raw-message-dialog">
      <header className="raw-message-header">
        <span className="raw-message-icon"><File size={20} /></span>
        <span><strong>原始邮件</strong><small>{detail}</small></span>
        <span className="raw-message-header-actions">
          <button className="raw-message-download-button" type="button" aria-label={downloadBusy ? '正在下载原始邮件' : '下载原始邮件'} title={downloadBusy ? '正在下载…' : '下载原始邮件'} disabled={state.status !== 'ready' || downloadBusy} onClick={() => void download()}><DownloadSimple size={18} /></button>
          <button type="button" aria-label="关闭原始邮件" title="关闭" onClick={onClose}><X size={19} /></button>
        </span>
      </header>
      <p className={`raw-message-safety${state.status === 'unavailable' ? ' is-warning' : ''}`}>
        {state.status === 'unavailable'
          ? '这封历史邮件同步时尚未保存完整 RFC 822，下面仅回退显示本地未清洗正文。后续新同步邮件会保存完整原始字节。'
          : '原始 EML 仅以纯文本显示；不会渲染 HTML、执行脚本、加载图片或访问邮件中的任何资源。'}
      </p>
      <div className="raw-message-download-error" role={downloadError ? 'alert' : undefined}>{downloadError}</div>
      <pre className="raw-message-source app-scrollbar" aria-label="原始邮件 RFC 822 文本">
        {state.status === 'loading' ? '正在读取原始邮件…' : state.status === 'error' ? `无法读取原始邮件：${state.message}` : source || '（原始邮件为空）'}
      </pre>
    </div>
  </Overlay>;
}
