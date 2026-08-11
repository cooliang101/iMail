import { readFile, readdir } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const METHODS = ['get', 'post', 'put', 'patch', 'delete'];

function routeKey(method, route) {
  return `${method.toUpperCase()} ${route}`;
}

function balancedRouteCalls(source) {
  const calls = [];
  let cursor = 0;
  while ((cursor = source.indexOf('.route(', cursor)) >= 0) {
    const start = source.indexOf('(', cursor);
    let depth = 0;
    let quote;
    let escaped = false;
    for (let index = start; index < source.length; index += 1) {
      const character = source[index];
      if (quote) {
        if (escaped) escaped = false;
        else if (character === '\\') escaped = true;
        else if (character === quote) quote = undefined;
        continue;
      }
      if (character === '"' || character === "'") quote = character;
      else if (character === '(') depth += 1;
      else if (character === ')') {
        depth -= 1;
        if (depth === 0) {
          calls.push(source.slice(start + 1, index));
          cursor = index + 1;
          break;
        }
      }
    }
    if (cursor <= start) throw new Error('unbalanced Rust Router::route call');
  }
  return calls;
}

async function rustApplicationRoutes(root) {
  const sourceDirectory = path.join(root, 'rust', 'crates', 'imail-http', 'src');
  const files = (await readdir(sourceDirectory))
    .filter((name) => name.endsWith('.rs') && !['gateway.rs', 'mcp.rs', 'web_client.rs'].includes(name));
  const routes = new Set();
  for (const name of files) {
    let source = await readFile(path.join(sourceDirectory, name), 'utf8');
    if (name === 'lib.rs') source = source.split('#[cfg(test)]')[0];
    for (const call of balancedRouteCalls(source)) {
      const route = call.match(/^\s*"([^"]+)"/)?.[1];
      if (!route?.startsWith('/api/') || route.includes('*')) continue;
      for (const method of METHODS) {
        if (new RegExp(`(?:\\b|\\.)${method}\\s*\\(`).test(call)) {
          routes.add(routeKey(method, route));
        }
      }
    }
  }
  return [...routes].sort();
}

export async function applicationHttpRoutes(root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')) {
  return rustApplicationRoutes(root);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const routes = await applicationHttpRoutes();
  process.stdout.write(`${JSON.stringify(routes, null, 2)}\n`);
}
