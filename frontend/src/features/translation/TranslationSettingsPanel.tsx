import { useEffect, useMemo, useState } from 'preact/compat';
import { AppButton } from '../../components/AppButton';
import { AppInput, AppSelect, AppSwitch, AppTextarea } from '../../components/form-controls';
import { Globe, Key, Plus, Trash } from '../../components/icons';
import { SettingsLinkRow, SettingsPanelHeading } from '../../components/settings-navigation';
import type { Notice } from '../../app-model';
import { isTauriRuntime } from '../../platform/tauri-runtime';
import { translationSettingsApi } from './translation-api';
import { supportsEdgeLocalTranslation } from './edge-local-translator';
import type {
  TranslationCredentialKind,
  TranslationExecutionTarget,
  TranslationProviderConfiguration,
  TranslationProviderDescriptor,
  TranslationProviderKind,
  TranslationProviderProfileInput,
  TranslationProviderProfileView,
  TranslationSettings,
} from './types';
import './translation-settings.css';

const targetLanguages = [
  { value: '', label: '跟随 iMail 语言' },
  { value: 'zh-Hans', label: '简体中文' },
  { value: 'zh-Hant', label: '繁體中文' },
  { value: 'en', label: 'English' },
  { value: 'ja', label: '日本語' },
  { value: 'ko', label: '한국어' },
  { value: 'fr', label: 'Français' },
  { value: 'de', label: 'Deutsch' },
];

const statusLabels: Record<TranslationProviderProfileView['status'], string> = {
  configured: '已配置',
  disabled: '已停用',
  needsCredential: '需要凭据',
  needsConsent: '待确认隐私',
  runtimeCheckRequired: '等待运行检测',
  experimental: '实验服务',
};

type EditorState = {
  descriptor: TranslationProviderDescriptor;
  profile?: TranslationProviderProfileView;
};

export function TranslationSettingsPanel({ setNotice }: { setNotice: (notice: Notice) => void }) {
  const [settings, setSettings] = useState<TranslationSettings>();
  const [editor, setEditor] = useState<EditorState>();
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const edgeRuntimeSupported = supportsEdgeLocalTranslation();

  useEffect(() => {
    let cancelled = false;
    void translationSettingsApi.read().then((value) => {
      if (!cancelled) setSettings(value);
    }).catch((reason) => {
      if (!cancelled) setError(reason instanceof Error ? reason.message : '读取翻译设置失败');
    });
    return () => { cancelled = true; };
  }, []);

  const profilesByKind = useMemo(() => {
    const grouped = new Map<TranslationProviderKind, TranslationProviderProfileView[]>();
    for (const profile of settings?.profiles ?? []) {
      const kind = profile.profile.provider.type;
      grouped.set(kind, [...(grouped.get(kind) ?? []), profile]);
    }
    return grouped;
  }, [settings]);

  async function updatePreferences(next: TranslationSettings) {
    setSettings(next);
    setBusy(true);
    setError('');
    try {
      setSettings(await translationSettingsApi.update({ preferences: next.preferences, environment: next.environment }));
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : '保存翻译设置失败');
      void translationSettingsApi.read().then(setSettings).catch(() => undefined);
    } finally {
      setBusy(false);
    }
  }

  async function clearTranslationCache() {
    setBusy(true);
    setError('');
    try {
      const result = await translationSettingsApi.clearCache();
      setNotice({ kind: 'success', text: result.cleared > 0 ? `已清除 ${result.cleared} 条译文缓存` : '当前没有译文缓存' });
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : '清除译文缓存失败');
    } finally {
      setBusy(false);
    }
  }

  if (!settings) return <section className="settings-feature-panel"><SettingsPanelHeading title="邮件翻译" /><div className="settings-panel-body"><p className={error ? 'translation-settings-error' : 'settings-note'}>{error || '正在读取翻译设置…'}</p></div></section>;
  if (editor) return <ProviderEditor settings={settings} editor={editor} busy={busy} error={error} onBack={() => { setEditor(undefined); setError(''); }} onSettings={setSettings} onBusy={setBusy} onError={setError} setNotice={setNotice} />;

  const profileOptions = [
    { value: '', label: '每次翻译时选择' },
    ...settings.profiles.filter(({ status }) => !['disabled', 'needsCredential', 'needsConsent'].includes(status)).map(({ profile }) => ({ value: profile.id, label: profile.displayName })),
  ];
  return <section className="settings-feature-panel translation-settings-panel"><SettingsPanelHeading title="邮件翻译" />
    <div className="settings-panel-body">
      {error && <p className="translation-settings-error">{error}</p>}
      <section className="settings-section" aria-label="翻译偏好">
        <div className="settings-row"><span><strong>默认翻译服务</strong></span><AppSelect value={settings.environment.defaultProfileId ?? ''} disabled={busy} options={profileOptions} onValueChange={(defaultProfileId) => void updatePreferences({ ...settings, environment: { defaultProfileId: defaultProfileId || undefined } })} /></div>
        <div className="settings-row"><span><strong>默认目标语言</strong></span><AppSelect value={settings.preferences.defaultTargetLanguage ?? ''} disabled={busy} options={targetLanguages} onValueChange={(defaultTargetLanguage) => void updatePreferences({ ...settings, preferences: { ...settings.preferences, defaultTargetLanguage: defaultTargetLanguage || undefined } })} /></div>
        <div className="settings-row"><span><strong>自动翻译外语邮件</strong></span><AppSwitch aria-label="自动翻译外语邮件" checked={settings.preferences.autoTranslate} disabled={busy} onChange={(_, data) => void updatePreferences({ ...settings, preferences: { ...settings.preferences, autoTranslate: data.checked } })} /></div>
        <div className="settings-row"><span><strong>缓存译文</strong></span><AppSwitch aria-label="缓存译文" checked={settings.preferences.cacheTranslations} disabled={busy} onChange={(_, data) => void updatePreferences({ ...settings, preferences: { ...settings.preferences, cacheTranslations: data.checked } })} /></div>
        <div className="settings-row"><span><strong>清除译文缓存</strong></span><AppButton appearance="secondary" disabled={busy} onClick={() => void clearTranslationCache()}>清除</AppButton></div>
      </section>
      <div className="translation-provider-heading"><strong>翻译服务</strong><small>凭据仅加密保存在当前服务</small></div>
      <div className="settings-link-list translation-provider-list">
        {settings.registry.map((descriptor) => {
          const profiles = profilesByKind.get(descriptor.kind) ?? [];
          if (profiles.length === 0) return <SettingsLinkRow key={descriptor.kind} icon={descriptor.experimental ? <Key size={19} /> : <Globe size={19} />} title={descriptor.displayName} value="未配置" onClick={() => setEditor({ descriptor })} />;
          return <div className="translation-provider-group" key={descriptor.kind}>
            {profiles.map((profile) => <SettingsLinkRow key={profile.profile.id} icon={descriptor.experimental ? <Key size={19} /> : <Globe size={19} />} title={profile.profile.displayName} value={descriptor.kind === 'edge-local' && profile.status === 'runtimeCheckRequired' ? (edgeRuntimeSupported ? '本机 API 可用' : '当前运行时不支持') : statusLabels[profile.status]} onClick={() => setEditor({ descriptor, profile })} />)}
            <SettingsLinkRow icon={<Plus size={18} />} title={`添加 ${descriptor.displayName} 配置`} value="新增" onClick={() => setEditor({ descriptor })} />
          </div>;
        })}
      </div>
    </div>
  </section>;
}

function ProviderEditor({ settings, editor, busy, error, onBack, onSettings, onBusy, onError, setNotice }: {
  settings: TranslationSettings;
  editor: EditorState;
  busy: boolean;
  error: string;
  onBack: () => void;
  onSettings: (settings: TranslationSettings) => void;
  onBusy: (busy: boolean) => void;
  onError: (error: string) => void;
  setNotice: (notice: Notice) => void;
}) {
  const existing = editor.profile?.profile;
  const [displayName, setDisplayName] = useState(existing?.displayName ?? editor.descriptor.displayName);
  const [provider, setProvider] = useState<TranslationProviderConfiguration>(() => existing?.provider ?? defaultConfiguration(editor.descriptor.kind));
  const [credentialKind, setCredentialKind] = useState<TranslationCredentialKind>(existing?.credential?.kind ?? editor.descriptor.credentialKinds[0] ?? 'deepl-api-key');
  const [secret, setSecret] = useState('');
  const [enabled, setEnabled] = useState(existing?.enabled ?? !editor.descriptor.experimental);
  const [acceptsDisclosure, setAcceptsDisclosure] = useState(Boolean(editor.profile?.consent));
  const profileId = existing?.id ?? newProfileId(editor.descriptor.kind);
  const executionTarget = existing?.executionTarget ?? defaultExecutionTarget(editor.descriptor);

  async function save() {
    onBusy(true);
    onError('');
    try {
      const input: TranslationProviderProfileInput = { displayName, executionTarget, provider, enabled };
      let next = await translationSettingsApi.upsertProfile(profileId, input);
      if (secret.trim()) next = await translationSettingsApi.updateCredential(profileId, credentialKind, secret.trim());
      if (editor.descriptor.sendsContentOffDevice && acceptsDisclosure && !next.profiles.find(({ profile }) => profile.id === profileId)?.consent) {
        next = await translationSettingsApi.acceptConsent(profileId);
      }
      if (editor.descriptor.sendsContentOffDevice && !acceptsDisclosure && next.profiles.find(({ profile }) => profile.id === profileId)?.consent) {
        next = await translationSettingsApi.revokeConsent(profileId);
      }
      onSettings(next);
      setNotice({ kind: 'success', text: '翻译服务设置已保存' });
      onBack();
    } catch (reason) {
      onError(reason instanceof Error ? reason.message : '保存翻译服务失败');
    } finally {
      onBusy(false);
    }
  }

  async function remove() {
    if (!existing) return;
    onBusy(true);
    onError('');
    try {
      onSettings(await translationSettingsApi.deleteProfile(existing.id));
      setNotice({ kind: 'success', text: '翻译服务配置已移除' });
      onBack();
    } catch (reason) {
      onError(reason instanceof Error ? reason.message : '移除翻译服务失败');
    } finally {
      onBusy(false);
    }
  }

  async function clearCredential() {
    if (!existing?.credential) return;
    onBusy(true);
    onError('');
    try {
      const next = await translationSettingsApi.clearCredential(existing.id);
      onSettings(next);
      setNotice({ kind: 'success', text: '已删除翻译服务凭据' });
      onBack();
    } catch (reason) {
      onError(reason instanceof Error ? reason.message : '删除翻译服务凭据失败');
    } finally {
      onBusy(false);
    }
  }

  const configurationValid = provider.type !== 'google-cloud' || Boolean(provider.projectId.trim());
  const endpointValid = provider.type !== 'azure-translator' || /^https:\/\/\S+$/i.test(provider.endpoint.trim());

  return <section className="settings-feature-panel translation-settings-panel"><SettingsPanelHeading ancestors={['邮件翻译']} title={displayName || editor.descriptor.displayName} onBack={onBack} />
    <div className="settings-panel-body settings-detail-body translation-provider-editor">
      {error && <p className="translation-settings-error">{error}</p>}
      <div className="translation-provider-form">
        <label><span>配置名称</span><AppInput value={displayName} maxLength={80} autoComplete="off" onChange={(event) => setDisplayName(event.currentTarget.value)} /></label>
        {provider.type === 'deepl' && <label><span>API 套餐</span><AppSelect value={provider.plan} options={[{ value: 'free', label: 'DeepL API Free' }, { value: 'pro', label: 'DeepL API Pro' }]} onValueChange={(plan) => setProvider({ type: 'deepl', plan: plan as 'free' | 'pro' })} /></label>}
        {provider.type === 'google-cloud' && <><label><span>Google Cloud 项目 ID</span><AppInput value={provider.projectId} autoComplete="off" onChange={(event) => setProvider({ ...provider, projectId: event.currentTarget.value })} /></label><label><span>区域</span><AppInput value={provider.location ?? ''} placeholder="global" autoComplete="off" onChange={(event) => setProvider({ ...provider, location: event.currentTarget.value || undefined })} /></label></>}
        {provider.type === 'azure-translator' && <><label><span>服务地址</span><AppInput value={provider.endpoint} inputMode="url" autoComplete="off" onChange={(event) => setProvider({ ...provider, endpoint: event.currentTarget.value })} /></label><label><span>区域</span><AppInput value={provider.region ?? ''} autoComplete="off" onChange={(event) => setProvider({ ...provider, region: event.currentTarget.value || undefined })} /></label></>}
        {provider.type === 'bing-web' && <label><span>市场区域</span><AppInput value={provider.market ?? ''} placeholder="zh-CN" autoComplete="off" onChange={(event) => setProvider({ ...provider, market: event.currentTarget.value || undefined })} /></label>}
        {editor.descriptor.credentialKinds.length > 0 && <><label><span>凭据类型</span><AppSelect value={credentialKind} options={editor.descriptor.credentialKinds.map((kind) => ({ value: kind, label: credentialKindLabel(kind) }))} onValueChange={(kind) => { setCredentialKind(kind as TranslationCredentialKind); setSecret(''); }} /></label><label><span className="translation-credential-heading"><span>{existing?.credential ? '替换凭据（留空则不修改）' : '服务凭据'}</span>{existing?.credential && <button type="button" disabled={busy} onClick={() => void clearCredential()}>删除已存凭据</button>}</span>{credentialKind === 'google-service-account' ? <AppTextarea rows={7} value={secret} autoComplete="off" data-form-type="other" placeholder="粘贴完整的 Service Account JSON" onChange={(event) => setSecret(event.currentTarget.value)} /> : <AppInput type="password" value={secret} autoComplete="new-password" data-form-type="other" onChange={(event) => setSecret(event.currentTarget.value)} />}</label></>}
      </div>
      <div className="settings-section translation-provider-switches">
        <div className="settings-row"><span><strong>启用此服务</strong></span><AppSwitch aria-label="启用此翻译服务" checked={enabled} onChange={(_, data) => setEnabled(data.checked)} /></div>
        {editor.descriptor.sendsContentOffDevice && <div className="settings-row"><span><strong>允许发送邮件文本</strong><small>翻译时，清洗后的可见正文会发送给 {editor.descriptor.displayName}。</small></span><AppSwitch aria-label="允许发送邮件文本" checked={acceptsDisclosure} onChange={(_, data) => { setAcceptsDisclosure(data.checked); if (!data.checked) setEnabled(false); }} /></div>}
      </div>
      <div className="translation-provider-actions">
        {existing && <AppButton appearance="subtle" icon={<Trash size={16} />} disabled={busy} onClick={() => void remove()}>移除配置</AppButton>}
        <span />
        <AppButton appearance="secondary" disabled={busy} onClick={onBack}>取消</AppButton>
        <AppButton appearance="primary" icon={<Plus size={16} />} disabled={busy || !displayName.trim() || !configurationValid || !endpointValid || (editor.descriptor.sendsContentOffDevice && enabled && !acceptsDisclosure)} onClick={() => void save()}>{busy ? '保存中…' : '保存配置'}</AppButton>
      </div>
    </div>
  </section>;
}

function defaultConfiguration(kind: TranslationProviderKind): TranslationProviderConfiguration {
  switch (kind) {
    case 'edge-local': return { type: 'edge-local' };
    case 'deepl': return { type: 'deepl', plan: 'free' };
    case 'google-cloud': return { type: 'google-cloud', projectId: '', location: 'global' };
    case 'azure-translator': return { type: 'azure-translator', endpoint: 'https://api.cognitive.microsofttranslator.com' };
    case 'bing-web': return { type: 'bing-web', market: 'zh-CN' };
  }
}

function defaultExecutionTarget(descriptor: TranslationProviderDescriptor): TranslationExecutionTarget {
  if (descriptor.kind === 'edge-local') return 'web-view';
  const preferred = isTauriRuntime() ? 'local-service' : 'remote-service';
  return descriptor.executionTargets.includes(preferred) ? preferred : descriptor.executionTargets[0];
}

function newProfileId(kind: TranslationProviderKind) {
  const suffix = globalThis.crypto?.randomUUID?.() ?? `${Date.now()}-${Math.random().toString(16).slice(2)}`;
  return `${kind}-${suffix}`;
}

function credentialKindLabel(kind: TranslationCredentialKind) {
  const labels: Record<TranslationCredentialKind, string> = {
    'deepl-api-key': 'DeepL API Key',
    'google-api-key': 'Google API Key',
    'google-service-account': 'Google Service Account JSON',
    'azure-api-key': 'Azure API Key',
  };
  return labels[kind];
}
