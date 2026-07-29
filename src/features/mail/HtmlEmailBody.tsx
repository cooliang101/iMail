import { useCallback, useEffect, useRef, useState, type SyntheticEvent } from 'react';

export function HtmlEmailBody({ html, subject }: { html: string; subject: string }) {
  const [ready, setReady] = useState(false);
  const frameRef = useRef<HTMLIFrameElement>(null);
  const resizeObserverRef = useRef<ResizeObserver | null>(null);

  const resizeFrame = useCallback(() => {
    const frame = frameRef.current;
    const document = frame?.contentDocument;
    if (!frame || !document) return;
    const body = document.body;
    const bodyStyle = body ? document.defaultView?.getComputedStyle(body) : undefined;
    const marginBottom = Number.parseFloat(bodyStyle?.marginBottom ?? '0') || 0;
    const bodyBottom = body
      ? body.getBoundingClientRect().bottom - document.documentElement.getBoundingClientRect().top + marginBottom
      : document.documentElement.scrollHeight;
    const height = body ? Array.from(body.children).reduce((bottom, child) => {
      const style = document.defaultView?.getComputedStyle(child);
      const childMarginBottom = Number.parseFloat(style?.marginBottom ?? '0') || 0;
      return Math.max(bottom, child.getBoundingClientRect().bottom - document.documentElement.getBoundingClientRect().top + childMarginBottom);
    }, bodyBottom) : bodyBottom;
    const horizontalScrollbarSpace = document.documentElement.scrollWidth > document.documentElement.clientWidth ? 18 : 2;
    frame.style.height = `${Math.max(190, height + horizontalScrollbarSpace)}px`;
  }, []);

  const handleLoad = useCallback((event: SyntheticEvent<HTMLIFrameElement>) => {
    resizeObserverRef.current?.disconnect();
    resizeFrame();
    const document = event.currentTarget.contentDocument;
    if (!document) return;
    const observer = new ResizeObserver(resizeFrame);
    if (document.body) observer.observe(document.body);
    document.querySelectorAll('img').forEach((image) => image.addEventListener('load', resizeFrame, { once: true }));
    resizeObserverRef.current = observer;
    window.requestAnimationFrame(() => setReady(true));
  }, [resizeFrame]);

  useEffect(() => () => resizeObserverRef.current?.disconnect(), []);

  return <div className={`mail-html-stage ${ready ? 'is-ready' : ''}`} aria-busy={!ready}>
    {!ready && <div className="mail-html-placeholder" role="status" aria-label="正在排版邮件内容"><i /><i /><i /></div>}
    <iframe ref={frameRef} className="mail-html-frame" title={`邮件正文：${subject}`} sandbox="allow-same-origin allow-popups allow-popups-to-escape-sandbox" srcDoc={emailDocument(html)} onLoad={handleLoad} aria-hidden={!ready} tabIndex={ready ? 0 : -1} />
  </div>;
}

function emailDocument(html: string) {
  return `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta http-equiv="Content-Security-Policy" content="script-src 'none'; object-src 'none'; form-action 'none'"><base target="_blank"><style>html{height:auto!important;min-height:0!important;color-scheme:light}body{height:auto!important;min-height:0!important;margin:0;overflow-wrap:anywhere;color:#384944;background:#fff;font:14px/1.7 'Segoe UI Variable Text','Segoe UI',sans-serif}body>*{max-width:100%!important;box-sizing:border-box}img{max-width:100%!important;height:auto}table{max-width:100%!important}a{color:#187763}pre{white-space:pre-wrap}blockquote{margin-inline:0;padding-left:14px;border-left:3px solid #dce8e4;color:#63756f}</style></head><body>${html}</body></html>`;
}
