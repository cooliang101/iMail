import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import path from 'node:path';
import process from 'node:process';
import { describe, expect, it } from 'vitest';

describe('Node/Rust security contract', () => {
  it('keeps the deterministic Node vector synchronized with the shared fixture', () => {
    const generated = JSON.parse(execFileSync(process.execPath, ['scripts/generate-security-contract.mjs'], {
      cwd: process.cwd(), encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
    }));
    const fixture = JSON.parse(readFileSync(path.resolve('fixtures/security-v1.json'), 'utf8'));
    expect(generated).toEqual(fixture);
  });
});
