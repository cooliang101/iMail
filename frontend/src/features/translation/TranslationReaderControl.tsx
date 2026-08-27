import { useEffect, useState } from 'preact/compat';
import { AppButton } from '../../components/AppButton';
import { AppSelect } from '../../components/form-controls';
import { Globe, X } from '../../components/icons';
import { translationSettingsApi } from './translation-api';
import type { TranslationArtifact, TranslationSettings } from './types';

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
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState('');

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
    return () => { cancelled = true; };
  }, [messageId]);

  const readyProfiles = settings?.profiles.filter(({ status }) => !['disabled', 'needsCredential', 'needsConsent'].includes(status)) ?? [];

  async function prepare() {
    if (!profileId) return;
    setBusy(true);
    setMessage('');
    setArtifact(undefined);
    try {
      const result = await translationSettingsApi.prepareMessage(messageId, { profileId, targetLanguage });
      if (result.cached) {
        setArtifact(result.cached);
      } else {
        setMessage('正文已准备完成；所选翻译服务的执行器尚未接入。');
      }
    } catch (reason) {
      setMessage(reason instanceof Error ? reason.message : '无法准备邮件翻译');
    } finally {
      setBusy(false);
    }
  }

  return <section className="mail-translation-control" aria-label="邮件翻译">
    <header><span><Globe size={18} /><strong>邮件翻译</strong></span><button type="button" title="关闭翻译" aria-label="关闭翻译" onClick={onClose}><X size={17} /></button></header>
    {readyProfiles.length > 0 ? <div className="mail-translation-fields">
      <AppSelect aria-label="翻译服务" value={profileId} options={readyProfiles.map(({ profile }) => ({ value: profile.id, label: profile.displayName }))} onValueChange={setProfileId} />
      <AppSelect aria-label="目标语言" value={targetLanguage} options={languages} onValueChange={setTargetLanguage} />
      <AppButton appearance="primary" disabled={busy || !profileId} onClick={() => void prepare()}>{busy ? '处理中…' : '翻译'}</AppButton>
    </div> : <p>暂无可用翻译服务，请先在“设置 → 邮件翻译”中完成配置。</p>}
    {message && <p className="mail-translation-message">{message}</p>}
    {artifact && <div className="mail-translated-body" lang={artifact.key.targetLanguage}>{artifact.segments.map((segment) => <p key={segment.id}>{segment.text}</p>)}</div>}
  </section>;
}
