import type { CallToolResult } from '@modelcontextprotocol/server';
import { z } from 'zod';

export const attachmentSchema = z.object({
  id: z.string().min(1).max(100).optional(), filename: z.string().min(1).max(255),
  contentType: z.string().min(1).max(150), data: z.string().max(21_000_000),
});

export function output(data: Record<string, unknown>): CallToolResult {
  const safe = JSON.parse(JSON.stringify(data)) as Record<string, unknown>;
  return { content: [{ type: 'text', text: JSON.stringify(safe, null, 2) }], structuredContent: safe };
}
