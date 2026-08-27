import { useEffect, useRef, useState } from 'preact/compat';
import { AppButton } from '../../components/AppButton';
import { AppSelect } from '../../components/form-controls';
import { Globe, X } from '../../components/icons';
import { translationSettingsApi } from './translation-api';
import { detectEdgeSourceLanguage, translateDocumentWithEdge, type EdgeTranslationProgress } from './edge-local-translator';
import type { TranslationArtifact, TranslationPreparation, TranslationSettings } from './types';

const languages = [
  { value: 'zh-Hans', label: '简体中文' },
  { value: 'zh-Hant', label: '繁體中文' },
  { value: 'en', label: 'English' },
  { value: 'ja', label: '日本語' },
  { value: 'ko', label: '한국어' },
  { value: 'fr', label: 'Français' },
  { value: 'de', label: 'Deutsch' },
];

export function TranslationReaderControl({ messageId, onClose }: { messageId: string; onClose: () => void }) {
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

  useEffect(() => {
    let cancelled = false;
    setArtifact(undefined);
    setMessage('');
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
    return () => { cancelled = true; abortRef.current?.abort(); };
  }, [messageId]);

  const readyProfiles = settings?.profiles.filter(({ status }) => !['disabled', 'needsCredential', 'needsConsent'].includes(status)) ?? [];

  useEffect(() => {
    if (!profileId) return;
    let cancelled = false;
    abortRef.current?.abort();
    setPreparing(true);
    setPreparation(undefined);
    setPendingEdge(undefined);
    setArtifact(undefined);
    setMessage('');
    void translationSettingsApi.prepareMessage(messageId, { profileId, targetLanguage }).then((result) => {
      if (cancelled) return;
      setPreparation(result);
      if (result.cached) setArtifact(result.cached);
    }).catch((reason) => {
      if (!cancelled) setMessage(reason instanceof Error ? reason.message : '无法准备邮件翻译');
    }).finally(() => {
      if (!cancelled) setPreparing(false);
    });
    return () => { cancelled = true; };
  }, [messageId, profileId, targetLanguage]);

  async function prepare() {
    if (!profileId || !preparation) return;
    abortRef.current?.abort();
    const controller = new AbortController();
    abortRef.current = controller;
    setBusy(true);
    setMessage('');
    setArtifact(undefined);
    try {
      if (pendingEdge) {
        const onProgress = (progress: EdgeTranslationProgress) => setMessage(progressMessage(progress));
        const segments = await translateDocumentWithEdge(pendingEdge.preparation.document, pendingEdge.sourceLanguage, targetLanguage, { signal: controller.signal, onProgress });
        setArtifact(await translationSettingsApi.completeMessage(messageId, { profileId, sourceLanguage: pendingEdge.sourceLanguage, targetLanguage, segments }));
        setPendingEdge(undefined);
        setMessage('');
      } else if (preparation.cached) {
        setArtifact(preparation.cached);
      } else if (preparation.profile.profile.provider.type === 'edge-local') {
        const onProgress = (progress: EdgeTranslationProgress) => setMessage(progressMessage(progress));
        const sourceLanguage = await detectEdgeSourceLanguage(preparation.document, { signal: controller.signal, onProgress });
        const precise = await translationSettingsApi.prepareMessage(messageId, { profileId, sourceLanguage, targetLanguage });
        if (precise.cached) {
          setArtifact(precise.cached);
          setMessage('');
          return;
        }
        setPendingEdge({ preparation: precise, sourceLanguage });
        setMessage(`已识别为${languageLabel(sourceLanguage)}，点击“开始翻译”使用本地模型。`);
      } else if (['deepl', 'google-cloud', 'azure-translator', 'bing-web'].includes(preparation.profile.profile.provider.type)) {
        setMessage(`正在通过${preparation.profile.profile.displayName}翻译…`);
        setArtifact(await translationSettingsApi.executeMessage(messageId, { profileId, targetLanguage }));
        setMessage('');
      } else {
        setMessage('正文已准备完成；所选翻译服务的执行器尚未接入。');
      }
    } catch (reason) {
      if (!controller.signal.aborted) setMessage(reason instanceof Error ? reason.message : '无法准备邮件翻译');
    } finally {
      if (abortRef.current === controller) {
        abortRef.current = undefined;
        setBusy(false);
      }
    }
  }

  function close() {
    abortRef.current?.abort();
    onClose();
  }

  return <section className="mail-translation-control" aria-label="邮件翻译">
    <header><span><Globe size={18} /><strong>邮件翻译</strong></span><button type="button" title="关闭翻译" aria-label="关闭翻译" onClick={close}><X size={17} /></button></header>
    {readyProfiles.length > 0 ? <div className="mail-translation-fields">
      <AppSelect aria-label="翻译服务" value={profileId} options={readyProfiles.map(({ profile }) => ({ value: profile.id, label: profile.displayName }))} onValueChange={setProfileId} />
      <AppSelect aria-label="目标语言" value={targetLanguage} options={languages} onValueChange={setTargetLanguage} />
      <AppButton appearance="primary" disabled={busy || preparing || !profileId || !preparation || Boolean(artifact)} onClick={() => void prepare()}>{preparing ? '准备中…' : busy ? '处理中…' : artifact ? '已翻译' : pendingEdge ? '开始翻译' : '翻译'}</AppButton>
    </div> : <p>暂无可用翻译服务，请先在“设置 → 邮件翻译”中完成配置。</p>}
    {message && <p className="mail-translation-message">{message}</p>}
    {artifact && <div className="mail-translated-body" lang={artifact.key.targetLanguage}>{artifact.segments.map((segment) => <p key={segment.id}>{segment.text}</p>)}</div>}
  </section>;
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
