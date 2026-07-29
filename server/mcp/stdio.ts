import { serveStdio } from '@modelcontextprotocol/server/stdio';
import { createMailMcpServer } from './server.js';
import { authenticateToken } from '../tokens.js';

const code = process.env.IMAIL_MCP_AUTH_CODE;
const token = await authenticateToken(code, 'mcp:full');
if (!token) {
  process.stderr.write('iMail MCP: IMAIL_MCP_AUTH_CODE 无效、已过期或缺少 mcp:full 权限。\n');
  process.exitCode = 1;
} else {
  serveStdio(() => createMailMcpServer(), { onerror: (error) => process.stderr.write(`iMail MCP: ${error.message}\n`) });
}
