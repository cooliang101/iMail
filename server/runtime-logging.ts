const maxRuntimeLogCharacters = 8_000;
const emailPattern = /[a-z0-9._%+\-]+@[a-z0-9.\-]+\.[a-z]{2,}/gi;
const bearerPattern = /\bbearer\s+[a-z0-9._~+/\-]+=*/gi;
const sensitiveHeaderPattern = /(authorization|cookie|set-cookie)(\s*:\s*)[^\n]+/gi;
const sensitiveFieldPattern = /(["']?(?:authorization|cookie|password|passwd|secret|token|encryptedsecret|client_secret|access_token|refresh_token)["']?)(\s*[=:]\s*)(?:"[^"]*"|'[^']*'|[^\s,;&}]+)/gi;
const sensitiveQueryPattern = /([?&](?:code|state|token|access_token|refresh_token|client_secret)=)[^&\s]+/gi;

export function sanitizeRuntimeLogMessage(input: string) {
  const redacted = input.replaceAll('\0', '').replaceAll('\r', '')
    .replace(emailPattern, '<email>')
    .replace(sensitiveHeaderPattern, '$1$2<redacted>')
    .replace(bearerPattern, 'Bearer <redacted>')
    .replace(sensitiveFieldPattern, '$1$2<redacted>')
    .replace(sensitiveQueryPattern, '$1<redacted>');
  return redacted.length > maxRuntimeLogCharacters ? `${redacted.slice(0, maxRuntimeLogCharacters)}…<truncated>` : redacted;
}

export function describeRuntimeError(value: unknown) {
  try {
    if (value instanceof Error) return sanitizeRuntimeLogMessage(value.stack || `${value.name}: ${value.message}`);
    if (typeof value === 'string') return sanitizeRuntimeLogMessage(value);
    if (value === null) return 'null';
    if (value === undefined) return 'undefined';
    return `[${typeof value === 'object' ? value.constructor?.name || 'object' : typeof value}]`;
  } catch {
    return '[unavailable]';
  }
}

export function runtimeLog(level: 'INFO' | 'WARN' | 'ERROR', event: string, message: string) {
  const line = `[${new Date().toISOString()}][${level}][${event}] ${sanitizeRuntimeLogMessage(message)}\n`;
  (level === 'INFO' ? process.stdout : process.stderr).write(line);
}

let runtimeErrorLoggingInstalled = false;

export function installRuntimeErrorLogging(component: string) {
  if (runtimeErrorLoggingInstalled) return;
  runtimeErrorLoggingInstalled = true;
  process.once('uncaughtException', (error, origin) => {
    runtimeLog('ERROR', `${component}.uncaught_exception`, `origin=${origin}\n${describeRuntimeError(error)}`);
    process.exit(1);
  });
  process.once('unhandledRejection', (reason) => {
    runtimeLog('ERROR', `${component}.unhandled_rejection`, describeRuntimeError(reason));
    process.exit(1);
  });
  process.on('warning', (warning) => runtimeLog('WARN', `${component}.warning`, describeRuntimeError(warning)));
}
