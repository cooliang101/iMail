import { beforeEach, describe, expect, it } from 'vitest';
import { loadRememberedUsers } from './remembered-users';

describe('remembered users', () => {
  const values = new Map<string, string>();
  const storage = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => { values.set(key, value); },
  };
  beforeEach(() => values.clear());

  it('rejects valid JSON with the wrong shape', () => {
    storage.setItem('imail.remembered-app-users', 'null');
    expect(loadRememberedUsers(storage)).toEqual([]);
    storage.setItem('imail.remembered-app-users', '{}');
    expect(loadRememberedUsers(storage)).toEqual([]);
  });

  it('keeps only complete user entries', () => {
    storage.setItem('imail.remembered-app-users', JSON.stringify([
      { login: 'alice', displayName: 'Alice' },
      null,
      { login: '', displayName: 'Empty' },
      { login: 'missing-name' },
    ]));
    expect(loadRememberedUsers(storage)).toEqual([{ login: 'alice', displayName: 'Alice' }]);
  });
});
