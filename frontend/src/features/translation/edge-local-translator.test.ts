import { describe, expect, it, vi } from 'vitest';
import {
  detectEdgeSourceLanguage,
  supportsEdgeLocalTranslation,
  translateDocumentWithEdge,
  type EdgeLocalTranslationRuntime,
} from './edge-local-translator';
import type { TranslationDocument } from './types';

const document: TranslationDocument = {
  messageId: 'message-1',
  bodyHash: 'body-1',
  segmentVersion: 1,
  omittedQuotedText: true,
  segments: [
    { id: 's-one', kind: 'paragraph', text: 'Hello' },
    { id: 's-two', kind: 'paragraph', text: 'World' },
  ],
};

describe('Edge local translation adapter', () => {
  it('detects the source language and translates stable segments in order', async () => {
    const destroyDetector = vi.fn();
    const destroyTranslator = vi.fn();
    const translate = vi.fn(async (text: string) => `译文:${text}`);
    const runtime: EdgeLocalTranslationRuntime = {
      languageDetector: {
        availability: async () => 'available',
        create: async () => ({ detect: async () => [{ detectedLanguage: 'en', confidence: 0.98 }], destroy: destroyDetector }),
      },
      translator: {
        availability: async () => 'available',
        create: async () => ({ translate, destroy: destroyTranslator }),
      },
    };
    expect(supportsEdgeLocalTranslation(runtime)).toBe(true);
    const source = await detectEdgeSourceLanguage(document, { runtime });
    expect(source).toBe('en');
    await expect(translateDocumentWithEdge(document, source, 'zh-Hans', { runtime })).resolves.toEqual([
      { id: 's-one', text: '译文:Hello' },
      { id: 's-two', text: '译文:World' },
    ]);
    expect(translate).toHaveBeenCalledTimes(2);
    expect(destroyDetector).toHaveBeenCalledOnce();
    expect(destroyTranslator).toHaveBeenCalledOnce();
  });

  it('reports unsupported runtimes without attempting a network fallback', async () => {
    expect(supportsEdgeLocalTranslation({})).toBe(false);
    await expect(detectEdgeSourceLanguage(document, { runtime: {} })).rejects.toMatchObject({ code: 'unsupported' });
  });
});
