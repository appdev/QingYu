// @vitest-environment jsdom
import { beforeEach, expect, it, vi } from 'vitest';
import { preferences } from '../src/preferences';
import { mountSettings } from '../src/settings-view';
import { openSettings } from '../src/open-settings';
import type { SyncClient } from '../src/sync-session';

vi.hoisted(() => { vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {} })); });
vi.mock('../src/preferences', async importOriginal => ({ ...await importOriginal<typeof import('../src/preferences')>(), mobile: true, native: false }));
beforeEach(() => {
  localStorage.clear(); preferences.update({}, true); document.body.replaceChildren(); history.replaceState(null, '');
  Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value() { this.open = true; } });
});
const client: SyncClient = {
  load: async () => ({ endpoint: '', bucket: '', region: 'us-east-1', prefix: 'test', accessKeyId: '', secretAccessKey: '' }),
  save: vi.fn(), flush: vi.fn(), run: vi.fn(),
  subscribe: fn => { fn({ busy: false, message: '', error: false }); return () => {}; },
};

it('offers WYSIWYG as a mobile default and saves it without a desktop-only fallback', async () => {
  const root = document.createElement('div'); document.body.append(root);
  const view = mountSettings(root, client, vi.fn()); await view.loaded; view.select('editor');
  const select = root.querySelector<HTMLSelectElement>('[name=defaultMode]')!;
  expect([...select.options].find(option => option.value === 'rich')?.textContent).toBe('所见即所得');
  select.value = 'rich'; select.dispatchEvent(new Event('change', { bubbles: true }));
  await vi.waitFor(() => expect(preferences.value.defaultMode).toBe('rich'));
  view.destroy();
});

it('omits mobile typography and navigates only the three available tabs', async () => {
  const root = document.createElement('div'); document.body.append(root);
  const view = mountSettings(root, client, vi.fn()); await view.loaded;
  expect([...root.querySelectorAll('[role=tab]')].map(t => t.textContent)).toEqual(['通用', '编辑器', '同步']);
  expect(root.querySelector('#settings-panel-typography')).toBeNull();
  expect(root.querySelector('[name=editorFont]')).toBeNull();
  view.select('editor');
  root.querySelector('[data-section=editor]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
  expect(document.activeElement?.textContent).toBe('同步');
  root.querySelector('[data-section=sync]')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
  expect(document.activeElement?.textContent).toBe('通用');
  view.select('typography'); expect(root.querySelector('[aria-selected=true]')?.textContent).toBe('通用');
  view.destroy();
});

it('opens an anchored checked menu, saves a selection and keeps settings open on return', async () => {
  await openSettings(client);
  const root = document.querySelector<HTMLDialogElement>('.settings-inline')!;
  const appearance = root.querySelector<HTMLSelectElement>('[name=appearance]')!, trigger = root.querySelector<HTMLButtonElement>('#settings-select-appearance')!;
  expect(appearance.hidden).toBe(true);
  trigger.click();
  const popup = root.querySelector<HTMLDialogElement>('.settings-select-popup')!;
  expect(popup.open).toBe(true); expect(trigger.getAttribute('aria-expanded')).toBe('true');
  expect(popup.querySelector('[aria-checked=true]')?.textContent).toBe('跟随系统');
  const options = [...popup.querySelectorAll<HTMLButtonElement>('[role=menuitemradio]')];
  options.find(o => o.textContent === '深色')!.click();
  await vi.waitFor(() => expect(root.querySelector('.settings-select-popup')).toBeNull());
  expect(root.open).toBe(true); expect(trigger.textContent).toBe('深色');
  expect(document.activeElement).toBe(trigger);
  expect(preferences.value.appearance).toBe('dark');
  expect(document.documentElement.dataset.theme).toBe('dark');
  expect(JSON.parse(localStorage.getItem('miaoyan-editor.preferences.v1')!).appearance).toBe('dark');
  trigger.click(); history.back();
  await vi.waitFor(() => expect(root.querySelector('.settings-select-popup')).toBeNull());
  expect(root.open).toBe(true); expect(preferences.value.appearance).toBe('dark');
  root.querySelector<HTMLButtonElement>('.settings-back')!.click();
  await vi.waitFor(() => expect(document.querySelector('.settings-inline')).toBeNull());
});

it('dismisses outside clicks without changing preferences and supports keyboard choices', async () => {
  const root = document.createElement('div'); document.body.append(root);
  const view = mountSettings(root, client, vi.fn()); await view.loaded;
  const trigger = root.querySelector<HTMLButtonElement>('#settings-select-buttonDisplay')!;
  trigger.click(); const popup = root.querySelector<HTMLDialogElement>('.settings-select-popup')!;
  popup.dispatchEvent(new MouseEvent('click', { clientX: -1, clientY: -1, bubbles: true }));
  await vi.waitFor(() => expect(root.querySelector('.settings-select-popup')).toBeNull());
  expect(preferences.value.buttonDisplay).toBe('always');
  trigger.click(); const menu = root.querySelector<HTMLDialogElement>('.settings-select-popup')!;
  menu.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true }));
  expect(document.activeElement?.textContent).toBe('悬停时显示');
  menu.dispatchEvent(new Event('cancel', { cancelable: true }));
  await vi.waitFor(() => expect(root.querySelector('.settings-select-popup')).toBeNull());
  expect(preferences.value.buttonDisplay).toBe('always');
  view.destroy();
});
