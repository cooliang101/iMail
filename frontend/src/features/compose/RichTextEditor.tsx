import { useEffect, useRef, useState, type ChangeEvent, type KeyboardEvent } from 'preact/compat';
import { Editor } from '@tiptap/core';
import StarterKit from '@tiptap/starter-kit';
import Image from '@tiptap/extension-image';
import Link from '@tiptap/extension-link';
import Placeholder from '@tiptap/extension-placeholder';
import TextAlign from '@tiptap/extension-text-align';
import { ArrowClockwise, ArrowCounterClockwise, ImageSquare, LinkSimple, ListBullets, ListNumbers, Paperclip, Quotes, TextAlignCenter, TextAlignLeft, TextB, TextHTwo, TextItalic, TextStrikethrough, TextUnderline } from '../../components/icons';
import { selectRichTextToolbarState, type RichTextToolbarState } from './rich-text-toolbar-state';

function fileAsDataUrl(file: File) {
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = () => reject(new Error(`无法读取 ${file.name}`));
    reader.readAsDataURL(file);
  });
}

export function RichTextEditor({ initialHtml, onChange, onAddAttachments, onError }: {
  initialHtml: string;
  onChange: (html: string, text: string) => void;
  onAddAttachments: (files: File[]) => void;
  onError: (message: string) => void;
}) {
  const [linkOpen, setLinkOpen] = useState(false);
  const [linkValue, setLinkValue] = useState('');
  const [editor, setEditor] = useState<Editor | null>(null);
  const [state, setState] = useState<RichTextToolbarState | null>(null);
  const initialHtmlRef = useRef(initialHtml);
  const editorContainerRef = useRef<HTMLDivElement | null>(null);
  const imageInputRef = useRef<HTMLInputElement | null>(null);
  const attachmentInputRef = useRef<HTMLInputElement | null>(null);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  useEffect(() => {
    const element = editorContainerRef.current;
    if (!element) return;

    const currentEditor = new Editor({
      element,
      extensions: [
        StarterKit.configure({ link: false }),
        Link.configure({ openOnClick: false, defaultProtocol: 'https' }),
        Image.configure({ allowBase64: true }),
        TextAlign.configure({ types: ['heading', 'paragraph'] }),
        Placeholder.configure({ placeholder: '写下邮件内容…' }),
      ],
      content: initialHtmlRef.current,
      editorProps: { attributes: { class: 'composer-editor-content', 'aria-label': '邮件正文' } },
      onUpdate: ({ editor: current }) => onChangeRef.current(current.getHTML(), current.getText({ blockSeparator: '\n' })),
    });
    const updateToolbarState = () => setState(selectRichTextToolbarState({ editor: currentEditor }));

    currentEditor.on('transaction', updateToolbarState);
    setEditor(currentEditor);
    updateToolbarState();

    return () => {
      currentEditor.off('transaction', updateToolbarState);
      currentEditor.destroy();
    };
    // The editor owns its initial document. Draft switches remount this component.
  }, []);

  async function addInlineImage(event: ChangeEvent<HTMLInputElement>) {
    const file = event.currentTarget.files?.[0]; event.currentTarget.value = '';
    if (!file || !editor) return;
    if (file.size > 3 * 1024 * 1024) { onError('正文图片不能超过 3 MB'); return; }
    try { editor.chain().focus().setImage({ src: await fileAsDataUrl(file), alt: file.name }).run(); }
    catch (error) { onError(error instanceof Error ? error.message : '图片读取失败'); }
  }

  function applyLink() {
    if (!editor) return;
    const value = linkValue.trim();
    if (!value) editor.chain().focus().unsetLink().run();
    else editor.chain().focus().extendMarkRange('link').setLink({ href: /^\w+:/.test(value) ? value : `https://${value}` }).run();
    setLinkOpen(false); setLinkValue('');
  }

  const command = (action: () => void) => { action(); };
  return <section className="composer-editor">
    <div className="composer-toolbar" aria-label="正文格式工具栏">
      <div className="composer-tool-group">
        <button type="button" className={state?.bold ? 'active' : ''} title="粗体" aria-label="粗体" onClick={() => command(() => editor?.chain().focus().toggleBold().run())}><TextB size={17} /></button>
        <button type="button" className={state?.italic ? 'active' : ''} title="斜体" aria-label="斜体" onClick={() => command(() => editor?.chain().focus().toggleItalic().run())}><TextItalic size={17} /></button>
        <button type="button" className={state?.underline ? 'active' : ''} title="下划线" aria-label="下划线" onClick={() => command(() => editor?.chain().focus().toggleUnderline().run())}><TextUnderline size={17} /></button>
        <button type="button" className={state?.strike ? 'active' : ''} title="删除线" aria-label="删除线" onClick={() => command(() => editor?.chain().focus().toggleStrike().run())}><TextStrikethrough size={17} /></button>
      </div>
      <div className="composer-tool-group">
        <button type="button" className={state?.heading ? 'active' : ''} title="二级标题" aria-label="二级标题" onClick={() => command(() => editor?.chain().focus().toggleHeading({ level: 2 }).run())}><TextHTwo size={17} /></button>
        <button type="button" className={state?.bullet ? 'active' : ''} title="项目符号" aria-label="项目符号列表" onClick={() => command(() => editor?.chain().focus().toggleBulletList().run())}><ListBullets size={17} /></button>
        <button type="button" className={state?.ordered ? 'active' : ''} title="编号列表" aria-label="编号列表" onClick={() => command(() => editor?.chain().focus().toggleOrderedList().run())}><ListNumbers size={17} /></button>
        <button type="button" className={state?.quote ? 'active' : ''} title="引用" aria-label="引用" onClick={() => command(() => editor?.chain().focus().toggleBlockquote().run())}><Quotes size={17} /></button>
      </div>
      <div className="composer-tool-group">
        <button type="button" className={!state?.alignCenter ? 'active' : ''} title="左对齐" aria-label="左对齐" onClick={() => command(() => editor?.chain().focus().setTextAlign('left').run())}><TextAlignLeft size={17} /></button>
        <button type="button" className={state?.alignCenter ? 'active' : ''} title="居中" aria-label="居中" onClick={() => command(() => editor?.chain().focus().setTextAlign('center').run())}><TextAlignCenter size={17} /></button>
        <button type="button" className={state?.link ? 'active' : ''} title="链接" aria-label="插入链接" onClick={() => { setLinkValue(editor?.getAttributes('link').href ?? ''); setLinkOpen((current) => !current); }}><LinkSimple size={17} /></button>
        <button type="button" title="正文图片" aria-label="插入正文图片" onClick={() => imageInputRef.current?.click()}><ImageSquare size={17} /></button>
        <button type="button" title="添加附件" aria-label="添加邮件附件" onClick={() => attachmentInputRef.current?.click()}><Paperclip size={17} /></button>
      </div>
      <div className="composer-tool-group composer-history">
        <button type="button" title="撤销" aria-label="撤销" disabled={!state?.canUndo} onClick={() => command(() => editor?.chain().focus().undo().run())}><ArrowCounterClockwise size={17} /></button>
        <button type="button" title="重做" aria-label="重做" disabled={!state?.canRedo} onClick={() => command(() => editor?.chain().focus().redo().run())}><ArrowClockwise size={17} /></button>
      </div>
      <input ref={imageInputRef} className="sr-only" type="file" accept="image/*" onChange={(event) => void addInlineImage(event)} />
      <input ref={attachmentInputRef} className="sr-only" type="file" multiple onChange={(event) => { onAddAttachments(Array.from(event.currentTarget.files ?? [])); event.currentTarget.value = ''; }} />
      {linkOpen && <div className="composer-link-popover"><input autoFocus autoComplete="off" data-form-type="other" data-lpignore="true" data-1p-ignore="true" value={linkValue} onChange={(event) => setLinkValue(event.currentTarget.value)} onKeyDown={(event: KeyboardEvent<HTMLInputElement>) => { if (event.key === 'Enter') { event.preventDefault(); applyLink(); } }} placeholder="https://example.com" aria-label="链接地址" /><button type="button" onClick={applyLink}>应用</button></div>}
    </div>
    <div ref={editorContainerRef} />
  </section>;
}
