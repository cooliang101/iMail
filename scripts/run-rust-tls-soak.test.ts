import { describe, expect, it } from 'vitest';
import { parseArguments } from './run-rust-tls-soak.mjs';

describe('real TLS runtime soak launcher', () => {
  it('requires a bounded duration and a versioned report under the retained output root', () => {
    expect(parseArguments([
      '--duration-seconds', '60',
      '--max-growth-mib', '24',
      '--report', 'output/rust-migration-tests/r6-real-tls-soak-v1.json',
    ])).toMatchObject({ durationSeconds: 60, maximumGrowthMiB: 24 });
    expect(parseArguments([
      '60', '24', 'output/rust-migration-tests/r6-real-tls-soak-v1.json',
    ])).toMatchObject({ durationSeconds: 60, maximumGrowthMiB: 24 });
    expect(() => parseArguments([])).toThrow(/duration-seconds/);
    expect(() => parseArguments(['--duration-seconds', '29', '--report', 'output/rust-migration-tests/x.json'])).toThrow(/30\.\.86400/);
    expect(() => parseArguments(['--duration-seconds', '60', '--report', '.data/x.json'])).toThrow(/output\/rust-migration-tests/);
    expect(() => parseArguments(['--duration-seconds', '60', '--report', 'output/rust-migration-tests/x.txt'])).toThrow(/\.json/);
  });
});
