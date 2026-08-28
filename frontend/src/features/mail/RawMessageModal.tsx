import { Overlay } from '../../components/Overlay';
import { File, X } from '../../components/icons';
import type { Message } from '../../types';

export function rawMessageSource(message: Pick<Message, 'html' | 'text'>) {
  return message.html !== undefined ? message.html : message.text ?? '';
}

export function RawMessageModal({ message, onClose }: { message: Message; onClose: () => void }) {
  const source = rawMessageSource(message);
  const format = message.html !== undefined ? 'HTML 源码' : '纯文本正文';

  return <Overlay onClose={onClose} wide dialogClassName="raw-message-modal">
    <div className="raw-message-dialog">
      <header className="raw-message-header">
        <span className="raw-message-icon"><File size={20} /></span>
        <span><strong>原始邮件正文</strong><small>{format} · {source.length.toLocaleString('zh-CN')} 个字符</small></span>
        <button type="button" aria-label="关闭原始邮件正文" title="关闭" onClick={onClose}><X size={19} /></button>
      </header>
      <p className="raw-message-safety">仅以纯文本显示本地保存的未清洗正文，不会渲染标签、执行脚本或加载邮件中的任何资源。</p>
      <pre className="raw-message-source" aria-label="未清洗的原始邮件正文">{source || '（原始邮件正文为空）'}</pre>
    </div>
  </Overlay>;
}
