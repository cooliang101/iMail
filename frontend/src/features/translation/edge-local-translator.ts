import type { TranslatedSegment, TranslationDocument } from './types';

type EdgeAiAvailability = 'unavailable' | 'downloadable' | 'downloading' | 'available';
type DownloadProgress = { loaded: number; total: number };
type DownloadMonitor = { addEventListener: (type: 'downloadprogress', listener: (event: DownloadProgress) => void) => void };
type MonitorOptions = { monitor?: (monitor: DownloadMonitor) => void; signal?: AbortSignal };

type EdgeTranslatorSession = {
  translate: (text: string, options?: { signal?: AbortSignal }) => Promise<string>;
  destroy: () => void;
};

type EdgeTranslatorFactory = {
  availability: (options: { sourceLanguage: string; targetLanguage: string }) => Promise<EdgeAiAvailability>;
  create: (options: { sourceLanguage: string; targetLanguage: string } & MonitorOptions) => Promise<EdgeTranslatorSession>;
};

type LanguageDetectionResult = { detectedLanguage: string; confidence: number };
type EdgeLanguageDetectorSession = {
  detect: (text: string) => Promise<LanguageDetectionResult[]>;
  destroy: () => void;
};

type EdgeLanguageDetectorFactory = {
  availability: () => Promise<EdgeAiAvailability>;
  create: (options?: MonitorOptions) => Promise<EdgeLanguageDetectorSession>;
};

export type EdgeLocalTranslationRuntime = {
  translator?: EdgeTranslatorFactory;
  languageDetector?: EdgeLanguageDetectorFactory;
};

export type EdgeTranslationProgress =
  | { phase: 'detecting' }
  | { phase: 'downloading'; progress?: number }
  | { phase: 'translating'; completed: number; total: number };

export class EdgeLocalTranslationError extends Error {
  constructor(public readonly code: 'unsupported' | 'language-undetermined' | 'pair-unavailable' | 'same-language', message: string) {
    super(message);
    this.name = 'EdgeLocalTranslationError';
  }
}

export function edgeLocalTranslationRuntime(candidate: typeof globalThis = globalThis): EdgeLocalTranslationRuntime {
  const builtIns = candidate as typeof globalThis & {
    Translator?: EdgeTranslatorFactory;
    LanguageDetector?: EdgeLanguageDetectorFactory;
  };
  return { translator: builtIns.Translator, languageDetector: builtIns.LanguageDetector };
}

export function supportsEdgeLocalTranslation(runtime = edgeLocalTranslationRuntime()) {
  return Boolean(runtime.translator && runtime.languageDetector);
}

export async function detectEdgeSourceLanguage(
  document: TranslationDocument,
  options: { runtime?: EdgeLocalTranslationRuntime; signal?: AbortSignal; onProgress?: (progress: EdgeTranslationProgress) => void } = {},
) {
  const runtime = options.runtime ?? edgeLocalTranslationRuntime();
  const factory = runtime.languageDetector;
  if (!factory) throw unsupported();
  options.onProgress?.({ phase: 'detecting' });
  const availability = await factory.availability();
  if (availability === 'unavailable') throw unsupported();
  const session = await factory.create(modelOptions(options.signal, options.onProgress));
  try {
    const sample = document.segments.map(({ text }) => text).join('\n\n').slice(0, 12_000);
    const result = (await session.detect(sample)).find(({ detectedLanguage }) => detectedLanguage !== 'und');
    if (!result || result.confidence <= 0) {
      throw new EdgeLocalTranslationError('language-undetermined', '无法识别原文语言，暂时不能使用 Edge 本地翻译');
    }
    return normalizeEdgeLanguage(result.detectedLanguage);
  } finally {
    session.destroy();
  }
}

export async function translateDocumentWithEdge(
  document: TranslationDocument,
  sourceLanguage: string,
  targetLanguage: string,
  options: { runtime?: EdgeLocalTranslationRuntime; signal?: AbortSignal; onProgress?: (progress: EdgeTranslationProgress) => void } = {},
): Promise<TranslatedSegment[]> {
  const runtime = options.runtime ?? edgeLocalTranslationRuntime();
  const factory = runtime.translator;
  if (!factory) throw unsupported();
  const source = normalizeEdgeLanguage(sourceLanguage);
  const target = normalizeEdgeLanguage(targetLanguage);
  if (source === target) {
    throw new EdgeLocalTranslationError('same-language', '原文已经是所选目标语言');
  }
  const availability = await factory.availability({ sourceLanguage: source, targetLanguage: target });
  if (availability === 'unavailable') {
    throw new EdgeLocalTranslationError('pair-unavailable', 'Edge 本地模型不支持这个语言组合');
  }
  const session = await factory.create({
    sourceLanguage: source,
    targetLanguage: target,
    ...modelOptions(options.signal, options.onProgress),
  });
  try {
    const translated: TranslatedSegment[] = [];
    for (const [index, segment] of document.segments.entries()) {
      options.onProgress?.({ phase: 'translating', completed: index, total: document.segments.length });
      translated.push({ id: segment.id, text: await session.translate(segment.text, { signal: options.signal }) });
    }
    options.onProgress?.({ phase: 'translating', completed: document.segments.length, total: document.segments.length });
    return translated;
  } finally {
    session.destroy();
  }
}

function modelOptions(signal?: AbortSignal, onProgress?: (progress: EdgeTranslationProgress) => void): MonitorOptions {
  return {
    signal,
    monitor(monitor) {
      monitor.addEventListener('downloadprogress', ({ loaded, total }) => {
        onProgress?.({ phase: 'downloading', progress: total > 0 ? loaded / total : undefined });
      });
    },
  };
}

function normalizeEdgeLanguage(language: string) {
  return language.toLowerCase() === 'zh-hans' ? 'zh' : language;
}

function unsupported() {
  return new EdgeLocalTranslationError(
    'unsupported',
    '当前 WebView2 不支持 Edge 本地翻译，请更新 Microsoft Edge WebView2 Runtime',
  );
}
