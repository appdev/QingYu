import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import type { Plugin } from 'postcss';
import type palette from '../src/palette.json';

export const palettePath = fileURLToPath(new URL('../src/palette.json', import.meta.url));

export function paletteCSS(colors: typeof palette) {
  return Object.entries(colors).map(([mode, tokens]) => {
    const selector = mode === 'light' ? ':root' : ':root[data-theme="dark"]';
    return `${selector} {\n${Object.entries(tokens).map(([name, value]) => `  --${name}: ${value};`).join('\n')}\n}`;
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
