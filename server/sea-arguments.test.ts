import { describe, expect, it } from 'vitest';
import { parsePackagedServiceArguments } from './sea-arguments.js';

describe('packaged service arguments', () => {
  it('parses daemon paths and network settings without shell interpretation', () => {
    expect(parsePackagedServiceArguments([
      '--data-dir', 'C:\\Users\\Me\\iMail Data', '--host', '127.0.0.1', '--port', '18787', '--daemon-control-file', 'C:\\token',
    ])).toEqual({
      syncWorker: false, dataDir: 'C:\\Users\\Me\\iMail Data', host: '127.0.0.1', port: '18787', daemonControlFile: 'C:\\token',
    });
  });

  it('accepts the internal worker flag and rejects malformed input', () => {
    expect(parsePackagedServiceArguments(['--sync-worker'])).toEqual({ syncWorker: true });
    expect(() => parsePackagedServiceArguments(['--port', '70000'])).toThrow('端口');
    expect(() => parsePackagedServiceArguments(['--unknown', 'value'])).toThrow('不支持');
    expect(() => parsePackagedServiceArguments(['--data-dir'])).toThrow('缺少值');
  });
});
