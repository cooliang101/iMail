import { z } from 'zod';

const booleanQuery = z.enum(['true', 'false']).transform((value) => value === 'true');

export const gatewayMessageQuerySchema = z.object({
  mailbox: z.string().email().optional(),
  limit: z.coerce.number().int().min(1).max(100).default(25),
  cursor: z.string().min(1).max(1000).optional(),
  mailboxRole: z.enum(['inbox', 'sent', 'archive', 'trash', 'custom']).optional(),
  unread: booleanQuery.optional(),
  since: z.string().datetime({ offset: true }).optional(),
  before: z.string().datetime({ offset: true }).optional(),
  q: z.string().trim().max(200).optional(),
}).strict().refine((value) => !value.since || !value.before || value.since < value.before, {
  message: 'since 必须早于 before', path: ['since'],
});

export const gatewaySendSchema = z.object({
  mailbox: z.string().email(),
  to: z.array(z.string().email()).min(1).max(100),
  cc: z.array(z.string().email()).max(100).optional(),
  subject: z.string().trim().min(1).max(500),
  text: z.string().min(1).max(2_000_000),
  html: z.string().max(2_000_000).optional(),
}).strict();

export type GatewayMessageQuery = z.infer<typeof gatewayMessageQuerySchema>;
