import { describe, expect, it } from 'vitest';
import { localPortConflictMessage, suggestedLocalServicePort } from './LocalPortRecovery';

describe('local port recovery', () => {
  it('recognizes daemon port conflicts without treating unrelated startup errors as conflicts', () => {
    expect(localPortConflictMessage(new Error('本地端口 8787 已被未知或不兼容服务占用，请选择其他端口'))).toBe(true);
    expect(localPortConflictMessage(new Error('本地服务启动超时，请查看服务日志'))).toBe(false);
  });

  it('suggests the next port and wraps at the upper boundary', () => {
    expect(suggestedLocalServicePort(8787)).toBe(8788);
    expect(suggestedLocalServicePort(65535)).toBe(8787);
  });
});
