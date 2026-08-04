import { describe, expect, it } from 'vitest';
import { authorizationExportPasswordError, CLEAR_USER_DATA_CONFIRMATION, clearUserDataReady } from './privacy-actions';

describe('privacy actions', () => {
  it('requires the current password and exact second-stage deletion phrase', () => {
    expect(clearUserDataReady('password-123', CLEAR_USER_DATA_CONFIRMATION)).toBe(true);
    expect(clearUserDataReady('short', CLEAR_USER_DATA_CONFIRMATION)).toBe(false);
    expect(clearUserDataReady('password-123', '清除数据')).toBe(false);
  });

  it('requires a distinct, confirmed export file password', () => {
    expect(authorizationExportPasswordError('short', 'short')).toContain('12');
    expect(authorizationExportPasswordError('export-password-123', 'export-password-456')).toContain('不一致');
    expect(authorizationExportPasswordError('export-password-123', 'export-password-123')).toBe('');
  });
});
