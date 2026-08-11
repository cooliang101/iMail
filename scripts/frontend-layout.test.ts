import { access, readFile } from 'node:fs/promises';
import path from 'node:path';
import { describe, expect, it } from 'vitest';

const workspaceRoot = path.resolve(import.meta.dirname, '..');

async function exists(relativePath: string) {
  return access(path.join(workspaceRoot, relativePath)).then(() => true, () => false);
}

describe('frontend workspace layout', () => {
  it('keeps the complete npm and Vite project under frontend', async () => {
    for (const required of [
      'frontend/src',
      'frontend/public',
      'frontend/index.html',
      'frontend/package.json',
      'frontend/package-lock.json',
      'frontend/tsconfig.json',
      'frontend/tsconfig.app.json',
      'frontend/tsconfig.tools.json',
      'frontend/vite.config.ts',
    ]) {
      expect(await exists(required), required).toBe(true);
    }
    for (const forbidden of ['src', 'public', 'package.json', 'package-lock.json', 'node_modules', 'dist']) {
      expect(await exists(forbidden), forbidden).toBe(false);
    }
  });

  it('points Tauri, Docker and CI at the frontend workspace', async () => {
    const tauri = JSON.parse(await readFile(path.join(workspaceRoot, 'src-tauri', 'tauri.conf.json'), 'utf8'));
    expect(tauri.$schema).toBe('../frontend/node_modules/@tauri-apps/cli/config.schema.json');
    expect(tauri.build.frontendDist).toBe('../frontend/dist');
    expect(tauri.build.beforeBuildCommand).toBe('npm run build:web');

    const dockerfile = await readFile(path.join(workspaceRoot, 'Dockerfile'), 'utf8');
    expect(dockerfile).toContain('COPY frontend/package.json frontend/package-lock.json ./');
    expect(dockerfile).toContain('COPY --from=web-build /app/frontend/dist ./dist');

    const workflow = await readFile(
      path.join(workspaceRoot, '.github', 'workflows', 'deployment-release.yml'),
      'utf8',
    );
    expect(workflow).toContain('cache-dependency-path: frontend/package-lock.json');
    expect(workflow).toContain('npm ci --prefix frontend');
  });
});
