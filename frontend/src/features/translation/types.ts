export type TranslationProviderKind = 'edge-local' | 'deepl' | 'google-cloud' | 'azure-translator' | 'bing-web';
export type TranslationExecutionTarget = 'web-view' | 'local-service' | 'remote-service';
export type TranslationCredentialKind = 'deepl-api-key' | 'google-api-key' | 'google-service-account' | 'azure-api-key';
export type TranslationProviderStatus = 'configured' | 'disabled' | 'needsCredential' | 'needsConsent' | 'runtimeCheckRequired' | 'experimental';

export type TranslationProviderConfiguration =
  | { type: 'edge-local' }
  | { type: 'deepl'; plan: 'free' | 'pro' }
  | { type: 'google-cloud'; projectId: string; location?: string }
  | { type: 'azure-translator'; endpoint: string; region?: string }
  | { type: 'bing-web'; market?: string };

export type TranslationProviderDescriptor = {
  kind: TranslationProviderKind;
  displayName: string;
  executionTargets: TranslationExecutionTarget[];
  credentialKinds: TranslationCredentialKind[];
  sendsContentOffDevice: boolean;
  experimental: boolean;
  providerRevision: string;
  disclosureRevision: string;
};

export type TranslationProviderProfile = {
  id: string;
  displayName: string;
  executionTarget: TranslationExecutionTarget;
  provider: TranslationProviderConfiguration;
  credential?: { id: string; kind: TranslationCredentialKind } | null;
  enabled: boolean;
};

export type TranslationProviderProfileView = {
  profile: TranslationProviderProfile;
  status: TranslationProviderStatus;
  consent?: { profileId: string; disclosureRevision: string; acceptedAt: string } | null;
};

export type TranslationSettings = {
  preferences: {
    defaultTargetLanguage?: string | null;
    autoTranslate: boolean;
    cacheTranslations: boolean;
  };
  environment: { defaultProfileId?: string | null };
  registry: TranslationProviderDescriptor[];
  profiles: TranslationProviderProfileView[];
};

export type TranslationProviderProfileInput = {
  displayName: string;
  executionTarget: TranslationExecutionTarget;
  provider: TranslationProviderConfiguration;
  enabled: boolean;
};

export type TranslationSegment = { id: string; kind: 'paragraph' | 'list-item'; text: string };
export type TranslatedSegment = { id: string; text: string };
export type TranslationDocument = {
  messageId: string;
  bodyHash: string;
  segmentVersion: number;
  omittedQuotedText: boolean;
  segments: TranslationSegment[];
};
export type TranslationArtifact = {
  key: {
    userId: string;
    messageId: string;
    bodyHash: string;
    sourceLanguage?: string | null;
    targetLanguage: string;
    profileId: string;
    providerRevision: string;
    segmentVersion: number;
  };
  segments: TranslatedSegment[];
  createdAt: string;
  updatedAt: string;
};
export type TranslationPreparation = {
  document: TranslationDocument;
  profile: TranslationProviderProfileView;
  providerRevision: string;
  cacheKey: TranslationArtifact['key'];
  cached?: TranslationArtifact | null;
};

export type TranslationDisplayMode = 'bilingual' | 'translation' | 'original';

export type TranslationPresentation = {
  document: TranslationDocument;
  artifact?: TranslationArtifact;
  targetLanguage: string;
  busy: boolean;
};
