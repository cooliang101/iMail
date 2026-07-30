import type { CSSProperties } from 'react';
import type { BrandVariants, Theme } from '@fluentui/react-components';
import { createLightTheme } from '@fluentui/react-components';
import type { CustomThemeDefinition } from '../../app-model';
import { imailFontFamily } from '../../theme';

function rgb(hex: string) {
  return [Number.parseInt(hex.slice(1, 3), 16), Number.parseInt(hex.slice(3, 5), 16), Number.parseInt(hex.slice(5, 7), 16)] as const;
}

function hex(values: readonly number[]) {
  return `#${values.map((value) => Math.round(Math.max(0, Math.min(255, value))).toString(16).padStart(2, '0')).join('')}`;
}

export function mixHex(first: string, second: string, secondWeight: number) {
  const a = rgb(first);
  const b = rgb(second);
  return hex(a.map((value, index) => value * (1 - secondWeight) + b[index] * secondWeight));
}

export function contrastText(background: string) {
  const [red, green, blue] = rgb(background).map((value) => value / 255);
  const luminance = [red, green, blue].map((value) => value <= .03928 ? value / 12.92 : ((value + .055) / 1.055) ** 2.4)
    .reduce((sum, value, index) => sum + value * [.2126, .7152, .0722][index], 0);
  return luminance > .46 ? '#17201e' : '#ffffff';
}

export function customBrandVariants(accent: string): BrandVariants {
  return {
    10: mixHex(accent, '#000000', .82), 20: mixHex(accent, '#000000', .69), 30: mixHex(accent, '#000000', .56),
    40: mixHex(accent, '#000000', .43), 50: mixHex(accent, '#000000', .3), 60: mixHex(accent, '#000000', .16),
    70: accent, 80: mixHex(accent, '#ffffff', .1), 90: mixHex(accent, '#ffffff', .22), 100: mixHex(accent, '#ffffff', .34),
    110: mixHex(accent, '#ffffff', .46), 120: mixHex(accent, '#ffffff', .57), 130: mixHex(accent, '#ffffff', .67),
    140: mixHex(accent, '#ffffff', .77), 150: mixHex(accent, '#ffffff', .87), 160: mixHex(accent, '#ffffff', .94),
  };
}

export function createCustomFluentTheme(customTheme: CustomThemeDefinition): Theme {
  return { ...createLightTheme(customBrandVariants(customTheme.accent)), fontFamilyBase: imailFontFamily };
}

const radiusScale = {
  compact: ['0.1875rem', '0.3125rem', '0.4375rem', '0.625rem', '0.75rem'],
  balanced: ['0.25rem', '0.4375rem', '0.625rem', '0.875rem', '1rem'],
  rounded: ['0.375rem', '0.625rem', '0.875rem', '1.25rem', '1.5rem'],
} as const;

export function customThemeCssVariables(theme: CustomThemeDefinition): CSSProperties {
  const brand = customBrandVariants(theme.accent);
  const radii = radiusScale[theme.radius];
  const raised = mixHex(theme.surface, '#ffffff', .55);
  const borderStrong = mixHex(theme.border, theme.text, .3);
  const tertiary = mixHex(theme.textSecondary, theme.surface, .25);
  const shadowColor = mixHex(theme.rail, theme.accent, .18);
  const shadows = theme.shadow === 'none'
    ? ['none', 'none', 'none']
    : theme.shadow === 'offset'
      ? [`3px 3px 0 ${theme.rail}`, `5px 5px 0 ${theme.rail}`, `8px 8px 0 ${theme.rail}`]
      : [`0 5px 18px ${shadowColor}16`, `0 16px 38px ${shadowColor}24`, `0 24px 70px ${shadowColor}42`];
  const displayFont = theme.typography === 'technical'
    ? "'Cascadia Code', 'Segoe UI Variable Display', 'Microsoft YaHei UI', sans-serif"
    : theme.typography === 'rounded'
      ? "'Arial Rounded MT Bold', 'Segoe UI Variable Display', 'Microsoft YaHei UI', sans-serif"
      : "'Segoe UI Variable Display', 'Segoe UI Variable', 'Segoe UI', 'Microsoft YaHei UI', sans-serif";
  const variables: Record<string, string> = {
    '--font-family-display': displayFont,
    '--color-canvas': theme.canvas, '--color-surface': theme.surface, '--color-surface-subtle': theme.surfaceSubtle,
    '--color-surface-raised': raised, '--color-surface-sunken': mixHex(theme.canvas, theme.rail, .035), '--color-rail': theme.rail,
    '--color-text': theme.text, '--color-text-strong': mixHex(theme.text, '#000000', .16), '--color-text-secondary': theme.textSecondary,
    '--color-text-tertiary': tertiary, '--color-text-inverse': contrastText(theme.rail),
    '--color-border': theme.border, '--color-border-strong': borderStrong,
    '--color-accent': theme.accent, '--color-accent-hover': mixHex(theme.accent, '#000000', .12), '--color-accent-pressed': mixHex(theme.accent, '#000000', .25),
    '--color-accent-subtle': theme.accentSubtle, '--color-accent-subtle-hover': mixHex(theme.accentSubtle, theme.accent, .08),
    '--color-focus': mixHex(theme.accent, '#ffffff', .12), '--color-focus-ring': `${theme.accent}24`,
    '--color-message-hover': mixHex(theme.surfaceSubtle, theme.accentSubtle, .28), '--color-message-selected': theme.accentSubtle,
    '--color-reader-surface': mixHex(theme.surface, theme.canvas, .12), '--color-reader-toolbar': theme.surfaceSubtle,
    '--color-mail-body': mixHex(theme.surface, theme.accentSubtle, .32), '--color-mail-body-border': theme.border,
    '--color-rail-text': contrastText(theme.rail), '--color-rail-muted': mixHex(contrastText(theme.rail), theme.rail, .42),
    '--color-rail-control': mixHex(theme.rail, contrastText(theme.rail), .1), '--color-rail-control-hover': mixHex(theme.rail, contrastText(theme.rail), .16),
    '--color-rail-control-active': theme.accentSubtle, '--color-rail-control-active-text': contrastText(theme.accentSubtle),
    '--color-rail-accent': theme.accent, '--color-rail-border': mixHex(theme.rail, theme.accent, .28),
    '--color-brand-logo-primary': contrastText(theme.rail), '--color-brand-logo-accent': theme.accent,
    '--shadow-rail-control': theme.shadow === 'none' ? 'none' : theme.shadow === 'offset' ? `3px 3px 0 ${theme.text}` : `0 7px 18px ${shadowColor}52`,
    '--color-overlay-scrim': `${theme.rail}66`, '--color-dialog-surface': mixHex(theme.surface, theme.canvas, .08),
    '--color-dialog-border': borderStrong, '--color-dialog-control': raised,
    '--color-dialog-control-hover': mixHex(theme.surfaceSubtle, theme.accentSubtle, .22),
    '--color-dialog-control-selected': theme.accentSubtle, '--color-dialog-control-selected-text': contrastText(theme.accentSubtle),
    '--color-dialog-callout': theme.accentSubtle, '--color-dialog-callout-text': mixHex(theme.accent, theme.text, .32),
    '--shadow-dialog': shadows[2], '--shadow-dialog-control': shadows[0],
    '--neutral-0': raised, '--neutral-10': theme.surface, '--neutral-20': theme.surfaceSubtle, '--neutral-30': theme.canvas,
    '--neutral-40': mixHex(theme.border, theme.surface, .35), '--neutral-50': theme.border, '--neutral-60': borderStrong,
    '--neutral-70': tertiary, '--neutral-80': theme.textSecondary, '--neutral-90': mixHex(theme.textSecondary, theme.text, .36),
    '--neutral-100': mixHex(theme.textSecondary, theme.text, .62), '--neutral-110': mixHex(theme.text, '#000000', .08), '--neutral-120': theme.text,
    '--radius-xs': radii[0], '--radius-sm': radii[1], '--radius-md': radii[2], '--radius-lg': radii[3], '--radius-xl': radii[4],
    '--shadow-sm': shadows[0], '--shadow-md': shadows[1], '--shadow-lg': shadows[2],
  };
  for (const [key, value] of Object.entries(brand)) variables[`--brand-${key}`] = value;
  return variables as CSSProperties;
}
