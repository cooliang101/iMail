import { createLightTheme, type BrandVariants, type Theme } from '@fluentui/react-components';

/** Keep this Fluent scale aligned with the brand primitives in theme.css. */
export const imailBrand: BrandVariants = {
  10: '#061b17', 20: '#0b2b24', 30: '#0d4035', 40: '#105646',
  50: '#126c58', 60: '#14826a', 70: '#168f78', 80: '#2aa089',
  90: '#45b09b', 100: '#64c0ad', 110: '#83cfbe', 120: '#a2ddcf',
  130: '#c0e9df', 140: '#d9f2ec', 150: '#edf8f5', 160: '#f8fcfb',
};

export const imailFontFamily = "'Segoe UI Variable Text', 'Segoe UI Variable', 'Segoe UI', 'Microsoft YaHei UI', 'Microsoft YaHei', sans-serif";

export const imailTheme: Theme = {
  ...createLightTheme(imailBrand),
  fontFamilyBase: imailFontFamily,
};
