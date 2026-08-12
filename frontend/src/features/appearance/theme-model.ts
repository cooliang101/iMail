import type { AppThemeId, CustomThemeDefinition } from '../../app-model';

export const defaultThemeId: AppThemeId = 'mint-fresh';
export const themeStorageKey = 'imail.theme.v1';
export const customThemeStorageKey = 'imail.custom-theme.v1';
export const hexColorPattern = /^#[0-9a-fA-F]{6}$/;

export const defaultCustomTheme: CustomThemeDefinition = {
  name: '我的主题',
  canvas: '#e9edf4',
  surface: '#fbfcff',
  surfaceSubtle: '#f2f5fa',
  rail: '#20283a',
  text: '#202536',
  textSecondary: '#677086',
  border: '#d4dae6',
  accent: '#d06f52',
  accentSubtle: '#f8e8e2',
  radius: 'balanced',
  shadow: 'soft',
  typography: 'system',
};

export const themeOptions: Array<{
  id: AppThemeId;
  name: string;
  eyebrow: string;
  description: string;
  colors: [string, string, string, string];
}> = [
  {
    id: 'mint-fresh',
    name: '薄荷清新',
    eyebrow: 'iMail 经典',
    description: '保留原有的冷灰绿、深松石账户轨与低饱和薄荷强调色，熟悉、轻盈且耐读。',
    colors: ['#10231f', '#168f78', '#e2f2ee', '#fbfdfc'],
  },
  {
    id: 'tech',
    name: '琥珀终端',
    eyebrow: '科技风',
    description: '冷峻石墨与工业灰，配合琥珀信号灯、细网格和更利落的终端几何。',
    colors: ['#111a20', '#bd571b', '#f4e3d6', '#eef2f4'],
  },
  {
    id: 'business-blue',
    name: '深海蓝图',
    eyebrow: '蓝色商业风',
    description: '稳健的海军蓝、清晰的蓝色操作层级与克制阴影，适合长时间办公。',
    colors: ['#102d55', '#1d7fc6', '#e8f2fb', '#ffffff'],
  },
  {
    id: 'soft-neubrutalism',
    name: '柔和撞色',
    eyebrow: 'Soft Neubrutalism',
    description: '奶油底色、柔和粉彩、深色描边和轻微错位阴影，醒目但不刺眼。',
    colors: ['#34303a', '#c891c8', '#a9ddcf', '#fff4d6'],
  },
  {
    id: 'custom',
    name: '自定义主题',
    eyebrow: '你的配色',
    description: '编辑颜色、圆角、阴影和字体风格，也可以导入 AI 按规范生成的主题 JSON。',
    colors: [defaultCustomTheme.rail, defaultCustomTheme.accent, defaultCustomTheme.accentSubtle, defaultCustomTheme.surface],
  },
];

export function normalizeThemeId(value: unknown): AppThemeId {
  if (value === 'imail-light') return 'mint-fresh';
  return themeOptions.some((theme) => theme.id === value) ? value as AppThemeId : defaultThemeId;
}

const radiusValues = ['compact', 'balanced', 'rounded'] as const;
const shadowValues = ['none', 'soft', 'offset'] as const;
const typographyValues = ['system', 'technical', 'rounded'] as const;
const colorKeys = ['canvas', 'surface', 'surfaceSubtle', 'rail', 'text', 'textSecondary', 'border', 'accent', 'accentSubtle'] as const;

export function normalizeCustomTheme(value: unknown): CustomThemeDefinition {
  if (!value || typeof value !== 'object') return { ...defaultCustomTheme };
  const source = value as Partial<CustomThemeDefinition>;
  const result = { ...defaultCustomTheme };
  if (typeof source.name === 'string' && source.name.trim()) result.name = source.name.trim().slice(0, 40);
  for (const key of colorKeys) if (typeof source[key] === 'string' && hexColorPattern.test(source[key])) result[key] = source[key].toLowerCase();
  if (radiusValues.includes(source.radius as typeof radiusValues[number])) result.radius = source.radius as CustomThemeDefinition['radius'];
  if (shadowValues.includes(source.shadow as typeof shadowValues[number])) result.shadow = source.shadow as CustomThemeDefinition['shadow'];
  if (typographyValues.includes(source.typography as typeof typographyValues[number])) result.typography = source.typography as CustomThemeDefinition['typography'];
  return result;
}

export function parseCustomThemeJson(value: string): { theme?: CustomThemeDefinition; error?: string } {
  try {
    const source = JSON.parse(value) as Record<string, unknown>;
    if (!source || typeof source !== 'object' || Array.isArray(source)) return { error: '主题 JSON 必须是一个对象' };
    const missing = colorKeys.filter((key) => !hexColorPattern.test(String(source[key] ?? '')));
    if (missing.length > 0) return { error: `以下颜色缺失或不是 #RRGGBB：${missing.join('、')}` };
    if (typeof source.name !== 'string' || !source.name.trim()) return { error: '主题名称不能为空' };
    if (!radiusValues.includes(source.radius as typeof radiusValues[number])) return { error: 'radius 必须是 compact、balanced 或 rounded' };
    if (!shadowValues.includes(source.shadow as typeof shadowValues[number])) return { error: 'shadow 必须是 none、soft 或 offset' };
    if (!typographyValues.includes(source.typography as typeof typographyValues[number])) return { error: 'typography 必须是 system、technical 或 rounded' };
    return { theme: normalizeCustomTheme(source) };
  } catch {
    return { error: '无法解析 JSON，请检查引号、逗号和括号' };
  }
}
