import { describe, expect, it } from 'vitest';
import { embeddedDomainCall } from './mail/tauri';

describe('mail rules desktop routing', () => {
  it.each([
    ['/api/mail-rules', 'GET', undefined, 'mailRulesList'],
    ['/api/mail-rules', 'POST', {}, 'mailRuleCreate'],
    ['/api/mail-rules/rule-1', 'PUT', {}, 'mailRuleUpdate'],
    ['/api/mail-rules/rule-1', 'DELETE', undefined, 'mailRuleDelete'],
    ['/api/mail-rules/preview', 'POST', {}, 'mailRulePreview'],
    ['/api/mail-rules/apply', 'POST', { token: 'preview', confirmed: true }, 'mailRuleApply'],
    ['/api/mail-rule-runs', 'GET', undefined, 'mailRuleRuns'],
    ['/api/mail-rule-runs/run-1/retry', 'POST', undefined, 'mailRuleRetry'],
  ])('maps %s %s to the embedded control plane', (path, method, body, operation) => {
    expect(embeddedDomainCall(path as string, { method: method as string, body: body === undefined ? undefined : JSON.stringify(body) })?.operation).toBe(operation);
  });
});
