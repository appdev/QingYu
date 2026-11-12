import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import type { Plugin } from 'postcss';
import type palette from '../src/palette.json';

export const palettePath = fileURLToPath(new URL('../src/palette.json', import.meta.url));

export function paletteCSS(colors: typeof palette) {
  return Object.entries(colors).map(([mode, tokens]) => {
    const selector = mode === 'light' ? ':root' : ':root[data-theme="dark"]';
    const arrow = `url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='8' height='12' viewBox='0 0 8 12'%3E%3Cpath d='m1 4 3-3 3 3M1 8l3 3 3-3' stroke='${encodeURIComponent(tokens.muted)}' stroke-width='1.3' fill='none' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E")`;
    const values = { ...tokens, 'select-arrow': arrow };
    return `${selector} {\n${Object.entries(values).map(([name, value]) => `  --${name}: ${value};`).join('\n')}\n}`;
  }).join('\n');
}

// PostCSS also visits theme.css when another stylesheet imports it.
export const palettePlugin: Plugin = {
  postcssPlugin: 'qingyu-palette',
  AtRule: {
    'qingyu-palette'(rule, { result }) {
      rule.replaceWith(paletteCSS(JSON.parse(readFileSync(palettePath, 'utf8'))));
      result.messages.push({ type: 'dependency', plugin: 'qingyu-palette', file: palettePath, parent: rule.source?.input.file });
    },
  },
};
