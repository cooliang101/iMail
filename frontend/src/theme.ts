import { createLightTheme, type BrandVariants, type Theme } from '@fluentui/react-components';
import type { AppThemeId } from './app-model';

/** Keep these Fluent scales aligned with the brand primitives in theme.css. */
type BuiltInThemeId = Exclude<AppThemeId, 'custom'>;

const brands: Record<BuiltInThemeId, BrandVariants> = {
  'mint-fresh': {
    10: '#061b17', 20: '#0b2b24', 30: '#0d4035', 40: '#105646',
    50: '#126c58', 60: '#14826a', 70: '#168f78', 80: '#2aa089',
    90: '#45b09b', 100: '#64c0ad', 110: '#83cfbe', 120: '#a2ddcf',
    130: '#c0e9df', 140: '#d9f2ec', 150: '#edf8f5', 160: '#f8fcfb',
  },
  tech: {
    10: '#211006', 20: '#351909', 30: '#4b230c', 40: '#632e0e',
    50: '#7e3a11', 60: '#9b4715', 70: '#bd571b', 80: '#d16a28',
    90: '#df8143', 100: '#e89a62', 110: '#efb181', 120: '#f4c6a1',
    130: '#f7d9bf', 140: '#fae8d8', 150: '#fcf3eb', 160: '#fefaf7',
  },
  'business-blue': {
    10: '#06172d', 20: '#0a2444', 30: '#0c345f', 40: '#0d467c',
    50: '#0e5999', 60: '#126db5', 70: '#1d7fc6', 80: '#368fd1',
    90: '#559fda', 100: '#73afe2', 110: '#91bfe9', 120: '#aecff0',
    130: '#c9def5', 140: '#dfebf9', 150: '#eff5fc', 160: '#f8fbfe',
  },
  'soft-neubrutalism': {
    10: '#201421', 20: '#342036', 30: '#4b2e4c', 40: '#633d65',
    50: '#7d4d7f', 60: '#985e9a', 70: '#aa70ab', 80: '#b982ba',
    90: '#c794c8', 100: '#d3a8d3', 110: '#debade', 120: '#e7cce7',
    130: '#efddef', 140: '#f5eaf5', 150: '#faf4fa', 160: '#fdfafd',
  },
};

export const imailFontFamily = "'Segoe UI Variable Text', 'Segoe UI Variable', 'Segoe UI', 'Microsoft YaHei UI', 'Microsoft YaHei', sans-serif";

export const appThemes = Object.fromEntries(Object.entries(brands).map(([id, brand]) => [id, {
  ...createLightTheme(brand),
  fontFamilyBase: imailFontFamily,
}])) as Record<BuiltInThemeId, Theme>;
