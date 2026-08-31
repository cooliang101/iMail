import { describe, expect, it } from 'vitest';
import { newAction, newCondition, ruleInput, validateRule } from './rule-model';

describe('mail rules', () => {
  const valid = () => ({ ...ruleInput(), name: '财务通知', conditions: [newCondition('hasAttachments')], actions: [newAction('mute')] });
  it('requires explicit non-empty conditions and validates numeric priority', () => {
    expect(validateRule(ruleInput())).toBeTruthy();
    expect(validateRule(valid())).toBeNull();
    expect(validateRule({ ...valid(), conditions: [] })).toBeTruthy();
    expect(validateRule({ ...valid(), priority: 0.5 })).toBeTruthy();
    expect(validateRule({ ...valid(), priority: -1 })).toBeTruthy();
  });
  it('keeps false values and requires archive to be last', () => {
    expect(validateRule({ ...valid(), conditions: [{ field: 'unread', value: false }], actions: [{ type: 'markRead', value: false }, { type: 'archive' }] })).toBeNull();
    expect(validateRule({ ...valid(), actions: [{ type: 'archive' }, { type: 'mute', value: true }] })).toBeTruthy();
  });
  it('rejects invalid addresses, controls and empty labels', () => {
    expect(validateRule({ ...valid(), conditions: [{ field: 'sender', value: 'not an email' }] })).toBeTruthy();
    expect(validateRule({ ...valid(), conditions: [{ field: 'senderDomain', value: '@github.com' }] })).toBeTruthy();
    expect(validateRule({ ...valid(), actions: [{ type: 'addLabel', value: '' }] })).toBeTruthy();
    expect(validateRule({ ...valid(), name: 'one\ntwo' })).toBeTruthy();
  });
  it('copies editable fields only and never mutates a saved rule', () => {
    const saved = { ...valid(), id: 'rule-1', revision: 2 };
    const copy = ruleInput(saved);
    copy.conditions[0].value = false;
    expect(saved.conditions[0].value).toBe(true);
    expect(copy).not.toHaveProperty('id');
    expect(copy).not.toHaveProperty('revision');
  });
});
