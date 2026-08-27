import { describe, expect, it } from 'vitest';
import { defaultAppPreferences, gatewayPreferencesPayload, loadAppPreferences, mergeGatewayPreferences, preferencesStorageKey, preferencesStorageKeyFor } from './settings-model';

describe('settings model', () => {
  it('returns defaults when no preferences were saved', () => {
    expect(loadAppPreferences({ getItem: () => null })).toEqual(defaultAppPreferences);
  });

  it('loads a supported interface language and rejects unknown locales', () => {
    expect(loadAppPreferences({ getItem: () => JSON.stringify({ language: 'en-US' }) }).language).toBe('en-US');
    expect(loadAppPreferences({ getItem: () => JSON.stringify({ language: 'fr-FR' }) }).language).toBe('zh-CN');
  });

  it('merges older partial preferences with current defaults', () => {
    const storage = { getItem: (key: string) => key === preferencesStorageKey ? JSON.stringify({ defaultMessageView: 'rendered', notificationKinds: { error: false } }) : null };
    expect(loadAppPreferences(storage)).toEqual({ ...defaultAppPreferences, defaultMessageView: 'rendered', notificationKinds: { unread: true, snooze: true, error: false } });
  });

  it('loads a user-scoped local cache without reading another account cache', () => {
    const firstKey = preferencesStorageKeyFor('first-user');
    const storage = { getItem: (key: string) => key === firstKey ? JSON.stringify({ startupView: 'starred' }) : null };
    expect(loadAppPreferences(storage, firstKey).startupView).toBe('starred');
    expect(loadAppPreferences(storage, preferencesStorageKeyFor('second-user')).startupView).toBe('inbox');
  });

  it('loads supported themes and replaces unknown legacy values', () => {
    const selected = { getItem: () => JSON.stringify({ theme: 'soft-neubrutalism' }) };
    const legacy = { getItem: () => JSON.stringify({ theme: 'imail-light' }) };
    expect(loadAppPreferences(selected).theme).toBe('soft-neubrutalism');
    expect(loadAppPreferences(legacy).theme).toBe('mint-fresh');
  });

  it('sends and restores safe custom appearance data through the HTTP gateway', () => {
    const custom = { ...defaultAppPreferences, theme: 'custom' as const, customTheme: { ...defaultAppPreferences.customTheme, accent: '#123456' } };
    const payload = gatewayPreferencesPayload(custom);
    expect(payload).toMatchObject({ theme: 'custom', customTheme: { accent: '#123456' } });
    const remote = { ...defaultAppPreferences, theme: 'custom' as const, customTheme: { ...defaultAppPreferences.customTheme, accent: '#654321' } };
    expect(mergeGatewayPreferences(custom, remote)).toMatchObject({ theme: 'custom', customTheme: { accent: '#654321' }, startupView: remote.startupView });
  });

  it('preserves an existing local custom theme while an older server record is migrated', () => {
    const local = { ...defaultAppPreferences, theme: 'custom' as const, customTheme: { ...defaultAppPreferences.customTheme, accent: '#123456' } };
    expect(mergeGatewayPreferences(local, defaultAppPreferences)).toMatchObject({ theme: 'custom', customTheme: { accent: '#123456' } });
  });
});
