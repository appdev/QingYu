import postcss from 'postcss';
import { readFileSync } from 'node:fs';
import { expect, it } from 'vitest';

// jsdom 不算层叠样式：直接看样式表，桌面标题栏的前进后退和工具栏按钮用同一套效果。
const rules = new Map<string, Record<string, string>>();
postcss.parse(readFileSync('src/sidebar.css', 'utf8')).walkRules(rule => {
  for (const selector of rule.selectors) {
    const declarations = rules.get(selector) ?? {};
    rule.walkDecls(declaration => { declarations[declaration.prop] = declaration.value; });
    rules.set(selector, declarations);
  }
});
const button = '#app-shell:not([data-mobile-page]) .note-navigation .icon-button';

it('draws back and forward like the toolbar buttons: ink icons, the same grey circle on press, faded when unavailable', () => {
  expect(rules.get(button)).toMatchObject({ color: 'var(--ink)' });
  expect(rules.get(`${button}:hover`)).toMatchObject({ background: 'transparent' });
  expect(rules.get(`${button}:disabled`)).toMatchObject({ color: 'var(--muted)', opacity: '.55' });
  // 按下的灰圆和玻璃工具栏按钮是同一条规则，内缩和填色也取同一组变量。
  expect(rules.get(`${button}::before`)).toEqual(rules.get('.glass-toolbar .icon-button::before'));
  expect(rules.get(`${button}::before`)).toMatchObject({ inset: 'var(--glass-lens-inset)', 'border-radius': '50%', background: 'var(--glass-lens-fill)' });
  expect(rules.get(`${button}:not(:disabled):active::before`)).toMatchObject({ opacity: '1' });
  const lens = (selector: string) => { const { '--glass-lens-inset': inset, '--glass-lens-fill': fill } = rules.get(selector)!; return { inset, fill }; };
  expect(lens('.note-navigation')).toEqual(lens('.glass-toolbar'));
  expect(lens(':root[data-theme=dark] .note-navigation').fill).toBe(lens(':root[data-theme=dark] .glass-toolbar').fill);
  // 新建按钮变灰时用的是同一个灰度。
  expect(rules.get('#app-shell:not([data-mobile-page]) .notes-navigation .icon-button:disabled')).toMatchObject({ color: 'var(--muted)', opacity: '.55' });
});
