import { describe, expect, it } from 'vitest';
import { defaultAppPreferences, loadAppPreferences, preferencesStorageKey } from './settings-model';

describe('settings model', () => {
  it('returns defaults when no preferences were saved', () => {
    expect(loadAppPreferences({ getItem: () => null })).toEqual(defaultAppPreferences);
  });

  it('merges older partial preferences with current defaults', () => {
    const storage = { getItem: (key: string) => key === preferencesStorageKey ? JSON.stringify({ defaultMessageView: 'rendered', notificationKinds: { error: false } }) : null };
    expect(loadAppPreferences(storage)).toEqual({ ...defaultAppPreferences, defaultMessageView: 'rendered', notificationKinds: { unread: true, snooze: true, error: false } });
  });
});
