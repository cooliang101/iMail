import { describe, expect, it } from 'vitest';
import { defaultAppPreferences, loadAppPreferences, preferencesStorageKey, preferencesStorageKeyFor } from './settings-model';

describe('settings model', () => {
  it('returns defaults when no preferences were saved', () => {
    expect(loadAppPreferences({ getItem: () => null })).toEqual(defaultAppPreferences);
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
});
