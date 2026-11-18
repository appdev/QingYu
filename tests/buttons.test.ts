// @vitest-environment jsdom
import postcss from 'postcss';
import { readFileSync } from 'node:fs';
import { afterEach, expect, it, vi } from 'vitest';
import { alertActions, showAlert } from '../src/alert-dialog';

afterEach(() => { document.body.replaceChildren(); });

// 每个选择器最后一次声明的值（同一选择器可能分几条规则写）。
function rules(path: string) {
  const found = new Map<string, Record<string, string>>();
  postcss.parse(readFileSync(path, 'utf8')).walkRules(rule => {
    for (const selector of rule.selectors) {
      const declarations = found.get(selector) ?? {};
      rule.walkDecls(declaration => { declarations[declaration.prop] = declaration.value; });
      found.set(selector, declarations);
    }
  });
  return found;
}

it('draws the three Pinto button kinds in its three sizes, without borders or shadows', () => {
  const style = rules('src/style.css');
  const base = style.get(':is(.primary-button, .secondary-button, .quiet-button)')!;
  expect(base).toMatchObject({ height: '28px', 'border-radius': '10px', border: '0', font: '600 13px/1 var(--font)' });
  expect(base['box-shadow']).toBeUndefined();
  expect(style.get(':is(.primary-button, .secondary-button, .quiet-button):focus-visible')).toMatchObject({ outline: '2px solid var(--brand)', 'outline-offset': '2px' });
  expect(style.get(':is(.primary-button, .secondary-button, .quiet-button):not(:disabled):active')).toMatchObject({ transform: 'scale(.97)' });
  expect(style.get('.primary-button')).toMatchObject({ background: 'var(--primary-fill)', color: 'var(--primary-ink)' });
  expect(style.get('.secondary-button')).toMatchObject({ background: 'var(--sand)', color: 'var(--ink)' });
  expect(style.get('.quiet-button')).toMatchObject({ background: 'transparent', color: 'var(--muted)' });
  expect(style.get('.button-regular')).toMatchObject({ height: '40px', 'border-radius': '14px', 'font-size': '15px' });
  expect(style.get('.button-big')).toMatchObject({ height: '50px', 'border-radius': '14px', 'font-size': '17px' });
  // 系统提示框里是 28px 的胶囊，常规字重，默认按钮用强调色。
  expect(style.get('.alert-actions :is(.primary-button, .secondary-button)')).toMatchObject({ width: '100%', 'border-radius': '999px', 'font-weight': '400' });
  expect(style.get('.alert-actions .primary-button:not(:disabled)')).toMatchObject({ 'background-color': 'var(--alert-primary)' });
  expect(style.get('.alert-dialog')).toMatchObject({ width: 'min(260px, calc(100vw - 32px))', 'border-radius': '24px' });
});

it('keeps buttons from restyling themselves anywhere else', () => {
  for (const path of ['src/sidebar.css', 'src/settings.css', 'src/calendar.css', 'src/note-picture.css', 'src/menu-bar.css', 'src/rich-editor.css']) {
    postcss.parse(readFileSync(path, 'utf8')).walkRules(rule => {
      // 只看真正选中按钮的规则；:not(.primary-button) 这类是把按钮排除在外。
      if (!/\.(primary|secondary|quiet)-button/.test(rule.selector.replace(/:not\([^)]*\)/g, ''))) return;
      rule.walkDecls(declaration => {
        expect(['background', 'background-color', 'border', 'box-shadow', 'font-weight'], `${path}: ${rule.selector}`).not.toContain(declaration.prop);
      });
    });
  }
  const html = readFileSync('index.html', 'utf8');
  expect(html).toContain('<button id="cancel-folder" class="secondary-button"');
  expect(html).toContain('<button id="new-empty-note" class="secondary-button">');
});

it('opens an alert with the focus on itself, runs the default button on Return and leaves the rest secondary', () => {
  HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  const dialog = document.createElement('dialog');
  const confirm = document.createElement('button'), cancel = document.createElement('button');
  const run = vi.fn();
  confirm.onclick = run;
  dialog.append(alertActions(confirm, cancel));
  showAlert(dialog, confirm);
  expect(dialog.isConnected).toBe(true);
  expect(dialog.classList.contains('alert-dialog')).toBe(true);
  expect(document.activeElement).toBe(dialog);
  expect([...dialog.querySelector('.alert-actions')!.children].map(button => button.className)).toEqual(['primary-button', 'secondary-button']);
  dialog.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true, isComposing: true }));
  expect(run).not.toHaveBeenCalled();
  dialog.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
  expect(run).toHaveBeenCalledTimes(1);
  // Tab 到按钮上以后，回车交给按钮自己。
  cancel.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
  expect(run).toHaveBeenCalledTimes(1);
});
