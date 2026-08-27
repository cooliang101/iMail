import { api } from '../../services';
import type { TranslatedSegment, TranslationArtifact, TranslationCredentialKind, TranslationPreparation, TranslationProviderProfileInput, TranslationSettings } from './types';

export const translationSettingsApi = {
  read: () => api<TranslationSettings>('/api/translation-settings'),
  update: (settings: Pick<TranslationSettings, 'preferences' | 'environment'>) => api<TranslationSettings>('/api/translation-settings', {
    method: 'PUT',
    body: JSON.stringify(settings),
  }),
  upsertProfile: (profileId: string, input: TranslationProviderProfileInput) => api<TranslationSettings>(`/api/translation-profiles/${encodeURIComponent(profileId)}`, {
    method: 'PUT',
    body: JSON.stringify(input),
  }),
  updateCredential: (profileId: string, kind: TranslationCredentialKind, secret: string) => api<TranslationSettings>(`/api/translation-profiles/${encodeURIComponent(profileId)}/credential`, {
    method: 'PUT',
    body: JSON.stringify({ kind, secret }),
  }),
  clearCredential: (profileId: string) => api<TranslationSettings>(`/api/translation-profiles/${encodeURIComponent(profileId)}/credential`, { method: 'DELETE' }),
  acceptConsent: (profileId: string) => api<TranslationSettings>(`/api/translation-profiles/${encodeURIComponent(profileId)}/consent`, { method: 'POST' }),
  revokeConsent: (profileId: string) => api<TranslationSettings>(`/api/translation-profiles/${encodeURIComponent(profileId)}/consent`, { method: 'DELETE' }),
  deleteProfile: (profileId: string) => api<TranslationSettings>(`/api/translation-profiles/${encodeURIComponent(profileId)}`, { method: 'DELETE' }),
  prepareMessage: (messageId: string, input: { profileId: string; sourceLanguage?: string; targetLanguage: string }) => api<TranslationPreparation>(`/api/messages/${encodeURIComponent(messageId)}/translations/prepare`, {
    method: 'POST',
    body: JSON.stringify(input),
  }),
  completeMessage: (messageId: string, input: { profileId: string; sourceLanguage?: string; targetLanguage: string; segments: TranslatedSegment[] }) => api<TranslationArtifact>(`/api/messages/${encodeURIComponent(messageId)}/translations/complete`, {
    method: 'POST',
    body: JSON.stringify(input),
  }),
  executeMessage: (messageId: string, input: { profileId: string; sourceLanguage?: string; targetLanguage: string }) => api<TranslationArtifact>(`/api/messages/${encodeURIComponent(messageId)}/translations/run`, {
    method: 'POST',
    body: JSON.stringify(input),
  }),
  clearCache: () => api<{ cleared: number }>('/api/translation-cache', { method: 'DELETE' }),
};
