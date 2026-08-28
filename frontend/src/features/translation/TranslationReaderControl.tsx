import { useCallback, useEffect, useRef, useState } from 'preact/compat';
import { AppButton } from '../../components/AppButton';
import { AppSelect } from '../../components/form-controls';
import { Globe, X } from '../../components/icons';
import { describeDesktopLogValue, desktopLog } from '../../services';
import { translationSettingsApi } from './translation-api';
import { detectEdgeSourceLanguage, translateDocumentWithEdge, type EdgeTranslationProgress } from './edge-local-translator';
import type { TranslationArtifact, TranslationDisplayMode, TranslationPreparation, TranslationPresentation, TranslationSettings } from './types';

const languages = [
  { value: 'zh-Hans', label: '简体中文' },
  { value: 'zh-Hant', label: '繁體中文' },
  { value: 'en', label: 'English' },
  { value: 'ja', label: '日本語' },
  { value: 'ko', label: '한국어' },
  { value: 'fr', label: 'Français' },
  { value: 'de', label: 'Deutsch' },
];

export function TranslationReaderControl({ messageId, hasHtml, displayMode, onDisplayModeChange, onPresentationChange, onClose }: {
  messageId: string;
  hasHtml: boolean;
  displayMode: TranslationDisplayMode;
  onDisplayModeChange: (mode: TranslationDisplayMode) => void;
  onPresentationChange: (presentation?: TranslationPresentation) => void;
  onClose: () => void;
}) {
  const [settings, setSettings] = useState<TranslationSettings>();
  const [profileId, setProfileId] = useState('');
  const [targetLanguage, setTargetLanguage] = useState('zh-Hans');
  const [artifact, setArtifact] = useState<TranslationArtifact>();
  const [preparation, setPreparation] = useState<TranslationPreparation>();
  const [pendingEdge, setPendingEdge] = useState<{ preparation: TranslationPreparation; sourceLanguage: string }>();
  const [preparing, setPreparing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');
  const abortRef = useRef<AbortController>();
  const requestGenerationRef = useRef(0);

  const publishArtifact = useCallback((nextPreparation: TranslationPreparation, nextArtifact: TranslationArtifact) => {
    setPreparation(nextPreparation);
    setArtifact(nextArtifact);
    onDisplayModeChange('bilingual');
    onPresentationChange({ document: nextPreparation.document, artifact: nextArtifact, targetLanguage: nextArtifact.key.targetLanguage, busy: false });
  }, [onDisplayModeChange, onPresentationChange]);

  useEffect(() => {
    let cancelled = false;
    setArtifact(undefined);
    setMessage('');
    onPresentationChange(undefined);
    void translationSettingsApi.read().then((value) => {
      if (cancelled) return;
      setSettings(value);
      setTargetLanguage(value.preferences.defaultTargetLanguage || 'zh-Hans');
      const ready = value.profiles.filter(({ status }) => !['disabled', 'needsCredential', 'needsConsent'].includes(status));
      setProfileId(value.environment.defaultProfileId && ready.some(({ profile }) => profile.id === value.environment.defaultProfileId)
        ? value.environment.defaultProfileId
        : ready[0]?.profile.id ?? '');
    }).catch((reason) => {
      if (!cancelled) setMessage(reason instanceof Error ? reason.message : '读取翻译服务失败');
    });
    return () => { cancelled = true; requestGenerationRef.current += 1; abortRef.current?.abort(); };
  }, [messageId, onPresentationChange]);

  const readyProfiles = settings?.profiles.filter(({ status }) => !['disabled', 'needsCredential', 'needsConsent'].includes(status)) ?? [];

  useEffect(() => {
    if (!profileId) return;
    let cancelled = false;
    const controller = new AbortController();
    requestGenerationRef.current += 1;
    abortRef.current?.abort();
    setPreparing(true);
    setPreparation(undefined);
    setPendingEdge(undefined);
    setArtifact(undefined);
    setMessage('');
    onPresentationChange(undefined);
    void translationSettingsApi.prepareMessage(messageId, { profileId, targetLanguage }, controller.signal).then((result) => {
      if (cancelled) return;
      setPreparation(result);
      if (result.cached) publishArtifact(result, result.cached);
    }).catch((reason) => {
      if (!cancelled) setMessage(reason instanceof Error ? reason.message : '无法准备邮件翻译');
    }).finally(() => {
      if (!cancelled) setPreparing(false);
    });
    return () => { cancelled = true; controller.abort(); };
  }, [messageId, onPresentationChange, profileId, publishArtifact, targetLanguage]);

  async function prepare() {
    if (!profileId || !preparation) return;
    abortRef.current?.abort();
    const controller = new AbortController();
    const requestGeneration = requestGenerationRef.current + 1;
    requestGenerationRef.current = requestGeneration;
    abortRef.current = controller;
    setBusy(true);
    setMessage('');
    setArtifact(undefined);
    onDisplayModeChange('bilingual');
    onPresentationChange({ document: preparation.document, targetLanguage, busy: true });
    try {
      if (pendingEdge) {
        await completeEdgeTranslation(pendingEdge.preparation, pendingEdge.sourceLanguage, controller, requestGeneration);
      } else if (preparation.cached) {
        if (!requestIsCurrent(controller, requestGeneration, requestGenerationRef.current)) return;
        publishArtifact(preparation, preparation.cached);
      } else if (preparation.profile.profile.provider.type === 'edge-local') {
        const onProgress = (progress: EdgeTranslationProgress) => setMessage(progressMessage(progress));
        const sourceLanguage = await detectEdgeSourceLanguage(preparation.document, { signal: controller.signal, onProgress });
        if (!requestIsCurrent(controller, requestGeneration, requestGenerationRef.current)) return;
        const precise = await translationSettingsApi.prepareMessage(messageId, { profileId, sourceLanguage, targetLanguage }, controller.signal);
        if (!requestIsCurrent(controller, requestGeneration, requestGenerationRef.current)) return;
        if (precise.cached) {
          publishArtifact(precise, precise.cached);
          setMessage('');
          return;
        }
        setPendingEdge({ preparation: precise, sourceLanguage });
        setMessage(`已识别为${languageLabel(sourceLanguage)}，正在启动本地翻译…`);
        try {
          await completeEdgeTranslation(precise, sourceLanguage, controller, requestGeneration);
        } catch (reason) {
          if (requiresFreshUserActivation(reason)) {
            setMessage(`已识别为${languageLabel(sourceLanguage)}。首次加载模型需要再次点击“继续翻译”。`);
            onPresentationChange(undefined);
            return;
          }
          throw reason;
        }
      } else if (['deepl', 'google-cloud', 'azure-translator', 'bing-web'].includes(preparation.profile.profile.provider.type)) {
        setMessage(`正在通过${preparation.profile.profile.displayName}翻译…`);
        const nextArtifact = await translationSettingsApi.executeMessage(messageId, { profileId, targetLanguage }, controller.signal);
        if (!requestIsCurrent(controller, requestGeneration, requestGenerationRef.current)) return;
        publishArtifact(preparation, nextArtifact);
        setMessage('');
      } else {
        setMessage('正文已准备完成；所选翻译服务的执行器尚未接入。');
        onPresentationChange(undefined);
      }
    } catch (reason) {
      if (!controller.signal.aborted) {
        const edgeProfile = preparation?.profile.profile.provider.type === 'edge-local' || Boolean(pendingEdge);
        if (edgeProfile) {
          void desktopLog('error', 'translation.edge_local_failed', [
            edgeRuntimeSummary(),
            `target=${targetLanguage}`,
            describeDesktopLogValue(reason),
          ].join(' '));
        }
        setMessage(edgeProfile ? edgeFailureMessage(reason) : reason instanceof Error ? reason.message : '无法准备邮件翻译');
        onPresentationChange(undefined);
      }
    } finally {
      if (abortRef.current === controller) {
        abortRef.current = undefined;
        setBusy(false);
      }
    }
  }

  async function completeEdgeTranslation(edgePreparation: TranslationPreparation, sourceLanguage: string, controller: AbortController, requestGeneration: number) {
    const onProgress = (progress: EdgeTranslationProgress) => setMessage(progressMessage(progress));
    const segments = await translateDocumentWithEdge(edgePreparation.document, sourceLanguage, targetLanguage, { signal: controller.signal, onProgress });
    if (!requestIsCurrent(controller, requestGeneration, requestGenerationRef.current)) return;
    const nextArtifact = await translationSettingsApi.completeMessage(messageId, { profileId, sourceLanguage, targetLanguage, segments }, controller.signal);
    if (!requestIsCurrent(controller, requestGeneration, requestGenerationRef.current)) return;
    publishArtifact(edgePreparation, nextArtifact);
    setPendingEdge(undefined);
    setMessage('');
  }

  function close() {
    requestGenerationRef.current += 1;
    abortRef.current?.abort();
    onPresentationChange(undefined);
    onClose();
  }

  return <section className="mail-translation-control" aria-label="邮件翻译">
    <header><span className="mail-translation-heading"><Globe size={18} /><strong>翻译</strong></span>{readyProfiles.length > 0 && <div className="mail-translation-fields">
      <AppSelect aria-label="翻译服务" value={profileId} options={readyProfiles.map(({ profile }) => ({ value: profile.id, label: profile.displayName }))} onValueChange={setProfileId} />
      <AppSelect aria-label="目标语言" value={targetLanguage} options={languages} onValueChange={setTargetLanguage} />
      <AppButton appearance="primary" disabled={busy || preparing || !profileId || !preparation || Boolean(artifact)} onClick={() => void prepare()}>{preparing ? '准备中…' : busy ? '处理中…' : artifact ? '已翻译' : pendingEdge ? '继续翻译' : '翻译'}</AppButton>
    </div>}<button className="mail-translation-close" type="button" title="关闭翻译" aria-label="关闭翻译" onClick={close}><X size={17} /></button></header>
    {readyProfiles.length === 0 && <p>暂无可用翻译服务，请先在“设置 → 邮件翻译”中完成配置。</p>}
    {message && <p className="mail-translation-message">{message}</p>}
    {artifact && <div className="mail-translation-viewbar"><span>阅读方式</span><div role="group" aria-label="译文显示方式">
      <button type="button" className={displayMode === 'bilingual' ? 'is-active' : ''} aria-pressed={displayMode === 'bilingual'} onClick={() => onDisplayModeChange('bilingual')}>双语</button>
      <button type="button" className={displayMode === 'translation' ? 'is-active' : ''} aria-pressed={displayMode === 'translation'} onClick={() => onDisplayModeChange('translation')}>仅译文</button>
      <button type="button" className={displayMode === 'original' ? 'is-active' : ''} aria-pressed={displayMode === 'original'} onClick={() => onDisplayModeChange('original')}>{hasHtml ? '原始排版' : '仅原文'}</button>
    </div></div>}
  </section>;
}

function requestIsCurrent(controller: AbortController, requestGeneration: number, currentGeneration: number) {
  return !controller.signal.aborted && requestGeneration === currentGeneration;
}

function languageLabel(language: string) {
  const labels: Record<string, string> = {
    en: '英语',
    fr: '法语',
    de: '德语',
    ja: '日语',
    ko: '韩语',
    zh: '中文',
    'zh-hans': '简体中文',
    'zh-hant': '繁体中文',
  };
  return labels[language.toLowerCase()] ?? language;
}

function progressMessage(progress: EdgeTranslationProgress) {
  if (progress.phase === 'detecting') return '正在识别原文语言…';
  if (progress.phase === 'downloading') return progress.progress === undefined
    ? '正在下载 Edge 本地翻译模型…'
    : `正在下载 Edge 本地翻译模型… ${Math.round(progress.progress * 100)}%`;
  return `正在本地翻译… ${progress.completed}/${progress.total}`;
}

function requiresFreshUserActivation(reason: unknown) {
  if (!(reason instanceof Error)) return false;
  return reason.name === 'NotAllowedError' || /user activation|user gesture|not allowed/i.test(reason.message);
}

function edgeFailureMessage(reason: unknown) {
  if (requiresFreshUserActivation(reason)) return 'Edge 未允许启动本地模型，请重新点击“继续翻译”。';
  if (reason instanceof Error && /download|network/i.test(`${reason.name} ${reason.message}`)) return 'Edge 本地翻译模型下载失败，请检查网络后重试。';
  return reason instanceof Error ? `Edge 本地翻译失败：${reason.message}` : 'Edge 本地翻译失败，请重试。';
}

function edgeRuntimeSummary() {
  const runtime = globalThis as typeof globalThis & { Translator?: unknown; LanguageDetector?: unknown };
  return `translator=${Boolean(runtime.Translator)} detector=${Boolean(runtime.LanguageDetector)} secure=${globalThis.isSecureContext} activation=${globalThis.navigator?.userActivation?.isActive ?? false}`;
}
