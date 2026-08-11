import { readFile } from 'node:fs/promises';
import { describe, expect, it } from 'vitest';

describe('Rust container definition', () => {
  it('builds the pinned MSRV binary and runs without Node as a non-root explicit HTTP bridge', async () => {
    const dockerfile = await readFile(new URL('../Dockerfile', import.meta.url), 'utf8');
    expect(dockerfile).toContain('FROM rust:1.77.2-bookworm AS rust-build');
    expect(dockerfile).toContain('CARGO_BUILD_JOBS=1');
    expect(dockerfile).toContain('NODE_OPTIONS=--max-old-space-size=768');
    expect(dockerfile).toContain('cargo build --locked --release');
    expect(dockerfile).toContain('FROM debian:bookworm-slim AS runtime');
    expect(dockerfile).not.toMatch(/FROM node:[^\n]+ AS runtime/);
    expect(dockerfile).toContain('USER 10001:10001');
    expect(dockerfile).toContain('IMAIL_SYNC_WORKER=true');
    expect(dockerfile).toContain('imail-storage-sqlite --bin imail-maintenance');
    expect(dockerfile).toContain('/release/imail-maintenance ./imail-maintenance');
    expect(dockerfile).toContain('STOPSIGNAL SIGTERM');
    expect(dockerfile).toContain('HEALTHCHECK');
    expect(dockerfile).toContain('"/app/imail-server", "--http", "--host", "0.0.0.0"');
  });

  it('defines a linux/amd64, read-only-root, graceful-SIGTERM smoke test', async () => {
    const smoke = await readFile(new URL('./smoke-rust-container.mjs', import.meta.url), 'utf8');
    expect(smoke).toContain("'--platform', 'linux/amd64'");
    expect(smoke).toContain("'--file', 'Dockerfile', '--tag', rustImage");
    expect(smoke).toContain("docker(['buildx', 'build', '--quiet', '--load', ...args])");
    expect(smoke).toContain("'--read-only'");
    expect(smoke).toContain("'--tmpfs', '/tmp:rw,noexec,nosuid,size=64m'");
    expect(smoke).toContain('source=${dataVolume},destination=/data');
    expect(smoke).toContain('source=${backupVolume},destination=/backups');
    expect(smoke).toContain("imageInspection.Architecture !== 'amd64'");
    expect(smoke).toContain("status === 'healthy'");
    expect(smoke).toContain('Rust 种子容器初始化注册失败');
    expect(smoke).toContain('Rust 未能读取重启前的登录数据');
    expect(smoke).toContain('Rust 未能读取重启前的偏好');
    expect(smoke).toContain('Rust 未能读取重启前的外部访问设置');
    expect(smoke).toContain('Rust 容器重启后实例身份变化');
    expect(smoke).toContain('secondInfo.instanceId !== firstInfo.instanceId');
    expect(smoke).toContain('authStatus.setupRequired !== false');
    expect(smoke).toContain("'stop', '--time', '10'");
    expect(smoke).toContain('stopped gracefully');
    expect(smoke).toContain("'/app/imail-maintenance', 'upgrade-preflight'");
    expect(smoke).toContain('integrityManifestVerified');
    expect(smoke).toContain("if (!duplicateRestoreRejected)");
    expect(smoke).toContain('rustVolumePersistence: true');
    expect(smoke).toContain("'volume', 'rm', '--force', dataVolume");
    expect(smoke).toContain("'--distribution', options.wslDistro");
    expect(smoke).toContain("'--cd', process.cwd(), '--exec', 'docker'");
    expect(smoke).toContain("dockerTransport: options.wslDistro ? `wsl:${options.wslDistro}` : 'native'");
    expect(smoke).toContain("writeFile(path.resolve(options.report), serialized, { flag: 'wx' })");
  });

  it('runs the Rust container gate before publishing the official Rust image', async () => {
    const workflow = await readFile(new URL('../.github/workflows/deployment-release.yml', import.meta.url), 'utf8');
    const candidateGate = workflow.indexOf('npm --prefix frontend run test:rust-container');
    const productionPublish = workflow.indexOf('docker/build-push-action@');
    expect(candidateGate).toBeGreaterThan(0);
    expect(productionPublish).toBeGreaterThan(candidateGate);
    expect(workflow).toContain('file: ./Dockerfile\n');
    const dockerfile = await readFile(new URL('../Dockerfile', import.meta.url), 'utf8');
    expect(dockerfile).toContain('FROM debian:bookworm-slim AS runtime');
    expect(dockerfile).not.toMatch(/FROM node:[^\n]+ AS runtime/);
  });
});
