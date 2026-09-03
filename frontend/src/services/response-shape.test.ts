import { describe, expect, it } from 'vitest';
import { responseArray, responseBoolean, responseNumber, responseObjectArray, responseOptionalString, responseStringArray } from './response-shape';

describe('API response shape guards', () => {
  it('accepts fields with the expected primitive shape', () => {
    const value = { items: [], total: 2, hasMore: false, cursor: null };
    expect(responseArray(value, 'items')).toEqual([]);
    expect(responseNumber(value, 'total')).toBe(2);
    expect(responseBoolean(value, 'hasMore')).toBe(false);
    expect(responseOptionalString(value, 'cursor')).toBeUndefined();
  });

  it('rejects null and missing collection fields', () => {
    expect(() => responseArray(null, 'items')).toThrow('数据格式不正确');
    expect(() => responseArray({ items: null }, 'items')).toThrow('缺少 items');
    expect(() => responseObjectArray({ items: [null] }, 'items')).toThrow('缺少 items');
    expect(() => responseStringArray({ items: ['ok', null] }, 'items')).toThrow('缺少 items');
  });
});
