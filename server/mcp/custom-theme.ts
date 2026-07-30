import { z } from 'zod';
import { getMetadata, setMetadata } from '../store.js';

const metadataKey = 'mcp_custom_theme_v1';
const color = z.string().regex(/^#[0-9a-fA-F]{6}$/, '颜色必须是 #RRGGBB');

export const customThemeSchema = z.object({
  name: z.string().trim().min(1).max(40),
  canvas: color, surface: color, surfaceSubtle: color, rail: color,
  text: color, textSecondary: color, border: color, accent: color, accentSubtle: color,
  radius: z.enum(['compact', 'balanced', 'rounded']),
  shadow: z.enum(['none', 'soft', 'offset']),
  typography: z.enum(['system', 'technical', 'rounded']),
}).strict();

export type McpCustomTheme = z.infer<typeof customThemeSchema>;

export const defaultMcpCustomTheme: McpCustomTheme = {
  name: '我的主题', canvas: '#e9edf4', surface: '#fbfcff', surfaceSubtle: '#f2f5fa', rail: '#20283a',
  text: '#202536', textSecondary: '#677086', border: '#d4dae6', accent: '#d06f52', accentSubtle: '#f8e8e2',
  radius: 'balanced', shadow: 'soft', typography: 'system',
};

export async function readMcpCustomTheme(): Promise<McpCustomTheme> {
  const stored = await getMetadata(metadataKey);
  if (!stored) return structuredClone(defaultMcpCustomTheme);
  try { return customThemeSchema.parse(JSON.parse(stored)); }
  catch { return structuredClone(defaultMcpCustomTheme); }
}

export async function updateMcpCustomTheme(theme: McpCustomTheme): Promise<McpCustomTheme> {
  const next = customThemeSchema.parse(theme);
  await setMetadata(metadataKey, JSON.stringify(next));
  return next;
}
