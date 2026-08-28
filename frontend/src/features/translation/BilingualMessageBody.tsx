import type { TranslationDisplayMode, TranslationPresentation } from './types';

export function BilingualMessageBody({ presentation, mode, hasHtml = false }: {
  presentation: TranslationPresentation;
  mode: Exclude<TranslationDisplayMode, 'original'>;
  hasHtml?: boolean;
}) {
  const translatedById = new Map(presentation.artifact?.segments.map((segment) => [segment.id, segment.text]));
  const sourceLanguage = presentation.artifact?.key.sourceLanguage ?? undefined;

  return <div className={`mail-bilingual-body is-${mode}`} aria-label={mode === 'bilingual' ? '双语邮件正文' : '邮件译文'} aria-busy={presentation.busy}>
    {hasHtml && <div className="mail-bilingual-layout-note"><span>译文使用清爽排版，图片与复杂格式保留在原始邮件中。</span></div>}
    {presentation.document.segments.map((source) => {
      const translated = translatedById.get(source.id);
      return <section className={`mail-bilingual-segment is-${source.kind}`} key={source.id}>
        {mode === 'bilingual' && <p className="mail-bilingual-source" lang={sourceLanguage}>{source.text}</p>}
        {translated
          ? <p className="mail-bilingual-translation" lang={presentation.targetLanguage}>{translated}</p>
          : <div className="mail-bilingual-pending" role="status" aria-label="正在翻译此段"><span /><span /></div>}
      </section>;
    })}
    {presentation.document.omittedQuotedText && <p className="mail-bilingual-omitted">引用历史未翻译，可切换到原文查看完整邮件。</p>}
  </div>;
}
