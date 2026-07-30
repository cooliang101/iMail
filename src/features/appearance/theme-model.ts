import type { AppThemeId } from '../../app-model';

export const defaultThemeId: AppThemeId = 'mint-fresh';
export const themeStorageKey = 'imail.theme.v1';

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
    name: '霓虹终端',
    eyebrow: '科技风',
    description: '冰川白与深海墨色，配合青色信号光、细网格和更利落的几何边角。',
    colors: ['#071f29', '#0a8d9c', '#d9f6f7', '#f4fbfc'],
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
];

export function normalizeThemeId(value: unknown): AppThemeId {
  if (value === 'imail-light') return 'mint-fresh';
  return themeOptions.some((theme) => theme.id === value) ? value as AppThemeId : defaultThemeId;
}
