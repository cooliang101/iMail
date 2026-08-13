import { describe, expect, it } from 'vitest';
import { visibleTrayAccounts } from './tray-menu-model';

describe('visibleTrayAccounts', () => {
  it('shows the first five accounts by default', () => {
    expect(visibleTrayAccounts([1, 2, 3, 4, 5, 6, 7], false)).toEqual([1, 2, 3, 4, 5]);
  });

  it('shows every account after expansion', () => {
    expect(visibleTrayAccounts([1, 2, 3, 4, 5, 6], true)).toEqual([1, 2, 3, 4, 5, 6]);
  });
});
