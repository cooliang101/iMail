import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { describe, expect, it } from 'vitest';
import { applicationHttpRoutes } from './application-http-routes.mjs';

describe('application HTTP route inventory', () => {
  it('keeps the Rust application control plane aligned with the checked-in contract', async () => {
    const root = path.resolve(import.meta.dirname, '..');
    const expected = JSON.parse(await readFile(path.join(root, 'contracts', 'application-http-routes.json'), 'utf8')) as string[];
    const actual = await applicationHttpRoutes(root);
    expect(actual).toEqual(expected);
  });
});
