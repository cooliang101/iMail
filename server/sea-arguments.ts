export type PackagedServiceArguments = {
  syncWorker: boolean;
  dataDir?: string;
  host?: string;
  port?: string;
  daemonControlFile?: string;
};

export function parsePackagedServiceArguments(args: string[]): PackagedServiceArguments {
  const result: PackagedServiceArguments = { syncWorker: false };
  for (let index = 0; index < args.length; index += 1) {
    const value = args[index];
    if (value === '--sync-worker') { result.syncWorker = true; continue; }
    const next = args[index + 1];
    if (!next || next.startsWith('--')) throw new Error(`服务启动参数 ${value} 缺少值`);
    if (value === '--data-dir') result.dataDir = next;
    else if (value === '--host') result.host = next;
    else if (value === '--port') result.port = next;
    else if (value === '--daemon-control-file') result.daemonControlFile = next;
    else throw new Error(`不支持的服务启动参数：${value}`);
    index += 1;
  }
  if (result.port && (!/^\d+$/.test(result.port) || Number(result.port) < 1 || Number(result.port) > 65_535)) throw new Error('服务端口无效');
  return result;
}

export function applyPackagedServiceArguments(input: PackagedServiceArguments) {
  process.env.IMAIL_PACKAGED_SERVICE = 'true';
  if (input.dataDir) process.env.IMAIL_DATA_DIR = input.dataDir;
  if (input.host) process.env.HOST = input.host;
  if (input.port) process.env.PORT = input.port;
  if (input.daemonControlFile) process.env.IMAIL_DAEMON_CONTROL_FILE = input.daemonControlFile;
}
