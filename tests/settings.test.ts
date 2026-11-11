// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { defaults, fontCSS, isDark, parsePreferences, preferences, preferencesKey, PreferenceStore, previewPreferences, settingsShortcut } from '../src/preferences';
import { mountSettings } from '../src/settings-view';
import { openSettings } from '../src/open-settings';
import type { SyncClient, SyncStatus } from '../src/sync-session';

vi.hoisted(() => { vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {} })); });
beforeEach(() => { localStorage.clear(); preferences.update({}, true); document.body.replaceChildren(); });
describe('preferences persistence', () => {
  it('keeps old defaults, validates stored values and discards unknown fields', () => {
    expect(parsePreferences({ version: 1 })).toEqual(defaults);
    expect(parsePreferences({ ...defaults, accessKeyId: 'must-not-save' })).not.toHaveProperty('accessKeyId');
    for (const change of [{ appearance: 'wrong' }, { previewWidth: -1 }, { editorFontSize: 100 }, { editorFont: 'a\nbody' }, { lineWrap: 'false' }, { version: 2 }]) expect(() => parsePreferences({ ...defaults, ...change })).toThrow();
  });
  it('saves before notifying and keeps the current values on storage failure', () => {
    const setItem = vi.fn(() => { throw new Error('full'); });
    const store = new PreferenceStore({ getItem: () => null, setItem }); const listener = vi.fn(); store.subscribe(listener);
    expect(() => store.update({ editorFontSize: 20 })).toThrow('full'); expect(store.value).toEqual(defaults); expect(listener).not.toHaveBeenCalled();
  });
  it('protects corrupt preferences until explicit reset, and reopens persisted settings', () => {
    localStorage.setItem(preferencesKey, 'broken'); const store = new PreferenceStore(localStorage);
    expect(store.error).not.toBe(''); expect(() => store.update({ appearance: 'dark' })).toThrow();
    store.update({}, true); store.update({ appearance: 'dark', previewFont: 'My Font', previewFontSize: 20 });
    expect(new PreferenceStore(localStorage).value).toEqual(store.value);
  });
  it('applies the same appearance and typography values to preview and editor without touching notes', () => {
    localStorage.setItem('note', 'original'); preferences.update({ appearance: 'dark', previewFont: 'Menlo', previewFontSize: 20, codeFont: 'text', editorFontSize: 18 });
    expect(isDark()).toBe(true); expect(document.documentElement.dataset.theme).toBe('dark');
    expect(previewPreferences()).toMatchObject({ dark: true, font: fontCSS('Menlo'), codeFont: fontCSS('Menlo'), size: 20 });
    expect(document.documentElement.style.getPropertyValue('--editor-size')).toBe('18px');
    expect(localStorage.getItem('note')).toBe('original'); preferences.update({ appearance: 'light' }); expect(isDark()).toBe(false);
    expect(fontCSS('a";color:red')).toContain('"a\\";color:red"');
  });
});
it('uses platform settings shortcuts without hijacking composition or modified commands', () => {
  expect(settingsShortcut(new KeyboardEvent('keydown', { key: ',', metaKey: true }), true)).toBe(true);
  expect(settingsShortcut(new KeyboardEvent('keydown', { key: ',', ctrlKey: true }), false)).toBe(true);
  for (const extra of [{ altKey: true }, { shiftKey: true }, { isComposing: true }, { ctrlKey: true }]) expect(settingsShortcut(new KeyboardEvent('keydown', { key: ',', metaKey: true, ...extra }), true)).toBe(false);
});
it('keeps inline settings modal, contains editor shortcuts, and preserves a sync editing lock on return', async () => {
  const shell = document.createElement('div'); shell.id = 'app-shell'; shell.inert = true; document.body.append(shell);
  const show = vi.fn(function(this: HTMLDialogElement) { this.open = true; }); Object.defineProperty(HTMLDialogElement.prototype, 'showModal', { configurable: true, value: show });
  const back = vi.spyOn(history, 'back').mockImplementation(() => {});
  const config = { endpoint: '', bucket: '', region: 'us-east-1', prefix: 'test', accessKeyId: '', secretAccessKey: '' };
  const client: SyncClient = { load: async () => config, save: vi.fn().mockResolvedValue(undefined), flush: vi.fn().mockResolvedValue(undefined), run: vi.fn(), subscribe: fn => { fn({ busy: false, message: '', error: false }); return () => {}; } };
  await openSettings(client); const dialog = document.querySelector<HTMLDialogElement>('.settings-inline')!;
  expect(show).toHaveBeenCalledOnce(); expect(dialog.open).toBe(true);
  const shortcut = vi.fn(); document.addEventListener('keydown', shortcut);
  dialog.dispatchEvent(new KeyboardEvent('keydown', { key: '3', metaKey: true, bubbles: true })); expect(shortcut).not.toHaveBeenCalled(); document.removeEventListener('keydown', shortcut);
  dialog.dispatchEvent(new Event('cancel', { cancelable: true })); expect(back).toHaveBeenCalledOnce();
  window.dispatchEvent(new PopStateEvent('popstate')); expect(document.querySelector('.settings-inline')).toBeNull(); expect(shell.inert).toBe(true);
  back.mockRestore(); delete (HTMLDialogElement.prototype as Partial<HTMLDialogElement>).showModal;
});
it('switches tabs with arrows, saves controls immediately, and leaves note data alone on reset', async () => {
  const root = document.createElement('div'); document.body.append(root);
  const config = { endpoint: 'https://unit.invalid', bucket: 'test', region: 'us-east-1', prefix: 'test', accessKeyId: '', secretAccessKey: '' };
  let publish!: (status: SyncStatus) => void;
  const run = vi.fn().mockResolvedValue(undefined);
  const client: SyncClient = { load: async () => config, save: vi.fn().mockResolvedValue(undefined), flush: vi.fn().mockResolvedValue(undefined), run, subscribe: fn => { publish = fn; fn({ busy: false, message: '', error: false }); return () => {}; } };
  const close = vi.fn(), view = mountSettings(root, client, close); await view.loaded;
  const tab = root.querySelector<HTMLButtonElement>('[data-section="general"]')!;
  tab.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
  expect(root.querySelector('#settings-panel-editor')?.hasAttribute('hidden')).toBe(false);
  expect(document.activeElement?.textContent).toBe('编辑器');
  const width = root.querySelector<HTMLSelectElement>('[name="previewWidth"]')!; width.value = '800'; width.dispatchEvent(new Event('change'));
  expect(preferences.value.previewWidth).toBe(800);
  expect(root.querySelectorAll('[name="editorFontSize"]')).toHaveLength(1);
  const ids = [...root.querySelectorAll('[id]')].map(node => node.id); expect(new Set(ids).size).toBe(ids.length);
  root.querySelector('main')!.scrollTop = 200; view.select('sync'); expect(root.querySelector('main')!.scrollTop).toBe(0);
  expect(root.querySelector('#settings-panel-sync')?.getAttribute('aria-hidden')).toBe('false'); expect(root.querySelector('#settings-panel-editor')?.getAttribute('aria-hidden')).toBe('true');
  expect(root.querySelector<HTMLInputElement>('[name="endpoint"]')?.value).toBe(config.endpoint);
  publish({ busy: true, message: '正在连接', error: false }); expect(root.querySelector<HTMLInputElement>('[name="bucket"]')!.disabled).toBe(true);
  publish({ busy: false, message: '连接失败', error: true }); expect(root.querySelector<HTMLInputElement>('[name="bucket"]')!.disabled).toBe(false);
  localStorage.setItem('note', 'keep'); root.querySelector<HTMLButtonElement>('.settings-reset')!.click(); expect(localStorage.getItem('note')).toBe('keep'); expect(preferences.value).toEqual(defaults);
  view.destroy(); expect(root.children).toHaveLength(0);
});

it('autosaves S3 fields, removes the save button, and displays redacted sync errors beneath the sync button', async () => {
  const root = document.createElement('div'); document.body.append(root);
  const config = { endpoint: 'https://unit.invalid', bucket: 'test', region: 'us-east-1', prefix: 'test', accessKeyId: 'fake-id', secretAccessKey: 'old-secret' };
  const save = vi.fn().mockResolvedValue(undefined), run = vi.fn().mockRejectedValue(new Error('认证失败 new-secret fake-id'));
  const client: SyncClient = { load: async () => config, save, flush: vi.fn(), run, subscribe: fn => { fn({ busy: false, message: '', error: false }); return () => {}; } };
  const view = mountSettings(root, client, vi.fn()); await view.loaded; view.select('sync');
  expect([...root.querySelectorAll('button')].some(button => button.textContent === '保存连接')).toBe(false);
  expect(root.textContent).not.toContain('仅在本次运行'); expect(root.textContent).not.toContain('不会保存到磁盘');
  const secret = root.querySelector<HTMLInputElement>('[name="secretAccessKey"]')!; secret.value = 'new-secret'; secret.dispatchEvent(new Event('input', { bubbles: true }));
  expect(save).toHaveBeenCalledWith({ ...config, secretAccessKey: 'new-secret' }); expect(run).not.toHaveBeenCalled();
  const button = root.querySelector<HTMLButtonElement>('button[type="submit"]')!; button.click();
  const status = root.querySelector<HTMLElement>('.settings-sync-status')!;
  await vi.waitFor(() => expect(status.textContent).toBe('认证失败 [已隐藏] [已隐藏]'));
  expect(status.dataset.error).toBe('true'); expect(button.compareDocumentPosition(status) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy(); expect(button.disabled).toBe(false);
  secret.value = ''; secret.dispatchEvent(new Event('input', { bubbles: true })); button.click();
  await vi.waitFor(() => expect(status.textContent).toBe('请填写Secret Access Key')); expect(run).toHaveBeenCalledTimes(1);
  save.mockRejectedValueOnce(new Error('系统凭据存储已锁定')); secret.value = 'retry-secret'; secret.dispatchEvent(new Event('input', { bubbles: true }));
  await vi.waitFor(() => expect(status.textContent).toContain('凭据存储已锁定'));
  view.destroy();
});

it('keeps S3 fields disabled until persisted credentials finish loading and surfaces load failures below the form', async () => {
  const root = document.createElement('div'); document.body.append(root);
  let reject!: (error: Error) => void;
  const client: SyncClient = { load: () => new Promise((_, fail) => { reject = fail; }), save: vi.fn(), flush: vi.fn(), run: vi.fn(), subscribe: fn => { fn({ busy: false, message: '', error: false }); return () => {}; } };
  const view = mountSettings(root, client, vi.fn());
  expect(root.querySelector<HTMLInputElement>('[name="secretAccessKey"]')!.disabled).toBe(true);
  reject(new Error('无法读取已保存的连接')); await view.loaded;
  expect(root.querySelector('.settings-sync-status')?.textContent).toContain('无法读取已保存的连接'); expect(client.save).not.toHaveBeenCalled();
  view.destroy();
});
