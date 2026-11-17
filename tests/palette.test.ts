import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import postcss from 'postcss';
import { expect, it } from 'vitest';
import palette from '../src/palette.json';
import { palettePath, palettePlugin } from '../tools/palette';

it('compiles both appearances from one palette and tracks edits to the source', async () => {
  expect(Object.keys(palette.dark).sort()).toEqual(Object.keys(palette.light).sort());
  const result = await postcss([palettePlugin]).process('@qingyu-palette;', { from: 'src/theme.css' });
  expect(result.css).toContain(`--muted: ${palette.light.muted}`);
  expect(result.css).toContain(':root[data-theme="dark"]');
  expect(result.css).not.toContain('@qingyu-palette');
  expect(result.messages).toContainEqual(expect.objectContaining({ type: 'dependency', file: palettePath }));
});

it('uses the same colors for diagrams, mindmaps and document surfaces', () => {
  const changed = structuredClone(palette);
  changed.light.paper = '#FFFFEE'; changed.dark.brand = '#11CC88';
  changed.light.title = '#8855BB'; changed.dark['diagram-text'] = '#FAFAFA';
  const window: Record<string, any> = { QingYuPalette: changed };
  runInNewContext(readFileSync('public/miaoyan/js/theme-config.js', 'utf8'), { window });
  for (const dark of [false, true]) {
    const colors = changed[dark ? 'dark' : 'light'];
    const diagram = window.ThemeColors.getThemeColors(dark);
    expect(diagram.background).toBe(colors.paper);
    expect(diagram.codeBg).toBe(colors.code);
    expect(diagram.textColor).toBe(colors['diagram-text']);
    expect(window.ThemeColors.getMermaidConfig(dark).themeVariables.nodeBorder).toBe(colors['diagram-text']);
    expect(window.ThemeColors.getMarkmapColors(dark)).toEqual([colors.brand, colors.title, colors.link, colors.gold, colors.markup]);
  }
});

it('keeps color literals out of application styles and renderer adapters', () => {
  for (const path of [
    'src/theme.css', 'src/style.css', 'src/sidebar.css', 'src/settings.css', 'src/calendar.css', 'src/rich-editor.css',
    'prototypes/web-login/tokens.css', 'public/miaoyan/css/theme-light.css', 'public/miaoyan/css/theme-dark.css',
    'public/miaoyan/css/base.css', 'public/miaoyan/css/typography.css',
  ]) {
    postcss.parse(readFileSync(path, 'utf8')).walkDecls(declaration => {
      expect(declaration.value, `${path}: ${declaration.prop}`).not.toMatch(/#[\da-f]{3,8}\b|rgba?\(\s*\d/i);
    });
  }
});

it('keeps dark fallback highlighting on a dark surface with readable text', () => {
  expect(palette.dark['brand-hover']).not.toBe(palette.dark.brand);
  expect(palette.dark['brand-active']).not.toBe(palette.dark.brand);
  const luminance = (hex: string) => {
    const rgb = [1, 3, 5].map(index => parseInt(hex.slice(index, index + 2), 16) / 255)
      .map(value => value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4);
    return rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
  };
  expect(palette.dark['highlight-fallback-background']).toBe(palette.dark.code);
  for (const [role, color] of Object.entries(palette.dark)) {
    if (role === 'error' || role.startsWith('highlight-fallback-') && role !== 'highlight-fallback-background') {
      const foreground = luminance(color), background = luminance(role === 'error' ? palette.dark.paper : palette.dark.code);
      expect((Math.max(foreground, background) + 0.05) / (Math.min(foreground, background) + 0.05), role).toBeGreaterThanOrEqual(4.5);
    }
  }
});
