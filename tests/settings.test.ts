// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { changeNoteTextSize, defaults, fontCSS, isDark, noteTextSizeShortcut, parsePreferences, preferences, preferencesKey, PreferenceStore, previewPreferences, settingsShortcut } from '../src/preferences';
import { mountSettings } from '../src/settings-view';
import { openSettings } from '../src/open-settings';
import { libraryKey } from '../src/notebook';
import type { SyncClient, SyncStatus } from '../src/sync-session';
import { s3Defaults } from '../src/sync-store';

vi.hoisted(() => { vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {} })); });
beforeEach(() => { document.querySelector('meta[name="qingyu-web-auth"]')?.remove(); localStorage.clear(); preferences.update({}, true); document.body.replaceChildren(); });
it('server settings describe the mounted library and never load browser S3 credentials', async () => {
  const marker = document.createElement('meta'); marker.name = 'qingyu-web-auth'; marker.content = 'enabled'; document.head.append(marker);
  const root = document.createElement('div'); document.body.append(root);
  const client: SyncClient = { load: vi.fn(), save: vi.fn(), test: vi.fn(), flush: vi.fn(), run: vi.fn(), subscribe: vi.fn() };
  const view = mountSettings(root, client, vi.fn()); await view.loaded;
  expect(root.textContent).toContain('服务器挂载的笔记目录');
  expect(root.querySelector('[data-section="sync"]')).toBeNull(); expect(root.querySelector('.settings-sync-form')).toBeNull();
  expect(client.load).not.toHaveBeenCalled(); expect(client.subscribe).not.toHaveBeenCalled();
  view.select('sync');
  expect(root.querySelector('#settings-panel-storage')?.hasAttribute('hidden')).toBe(false);
  expect(root.querySelector('h1')?.textContent).toBe('存储');
  view.destroy(); marker.remove();
});
it('shows saved browser note sizes and omits settings that require a native host', async () => {
  const root = document.body.appendChild(document.createElement('div'));
  const save = (source: string) => localStorage.setItem(libraryKey, JSON.stringify({ notes: [{ source }, { source: '删', trashed: true }] }));
  save('中文');
  const view = mountSettings(root, undefined, vi.fn());
  try {
    view.select('storage');
    await Promise.resolve();
    expect(root.querySelector('output')?.textContent).toBe('9 B');
    expect(root.querySelector('.settings-usage-row[data-usage=notes] .settings-usage-value')?.textContent).toBe('6 B');
    expect(root.querySelector<HTMLElement>('.settings-storage-meter [data-usage=trash]')?.style.width).toBe('33.333333333333336%');
    expect(root.querySelector('.settings-sync-form, #settings-tab-ai, #settings-tab-account, [name=menuBar], [name=alwaysOnTop]')).toBeNull();
    save('新正文');
    window.dispatchEvent(new Event('focus'));
    await Promise.resolve();
    expect(root.querySelector('output')?.textContent).toBe('12 B');
  } finally { view.destroy(); }
});

describe('preferences persistence', () => {
  it('loads folder choices and persists the daily options', async () => {
    const root = document.createElement('div'); document.body.append(root);
    const client: SyncClient = { load: vi.fn().mockResolvedValue({}), save: vi.fn(), test: vi.fn(), flush: vi.fn(), run: vi.fn(), subscribe: () => () => {} };
    const view = mountSettings(root, client, vi.fn(), undefined, () => ({ folders: [['project', '项目']] }));
    try {
      view.select('today');
      const folder = root.querySelector<HTMLSelectElement>('[name=todayFolder]')!;
      const title = root.querySelector<HTMLSelectElement>('[name=todayTitle]')!;
      await vi.waitFor(() => expect([...folder.options].map(item => item.text)).toEqual(['每日笔记', '项目']));
      expect(root.querySelector('[name=todayTemplate]')).toBeNull();
      for (const [element, value] of [[folder, 'project'], [title, 'iso']] as const) {
        element.value = value; element.dispatchEvent(new Event('change'));
      }
      root.querySelector<HTMLInputElement>('[name=todayOpenOnLaunch]')!.click();
      expect(new PreferenceStore(localStorage).value).toMatchObject({ todayFolder: 'project', todayTitle: 'iso', todayOpenOnLaunch: true });
    } finally { view.destroy(); root.remove(); }
  });
  it('validates stored values and discards unknown fields', () => {
    expect(parsePreferences({ version: 1 })).toEqual(defaults);
    expect(parsePreferences({ ...defaults, accessKeyId: 'must-not-save' })).not.toHaveProperty('accessKeyId');
    expect(parsePreferences({ version: 1, todayFolder: 'project', todayTemplate: 'old-note' })).toEqual({ ...defaults, todayFolder: 'project' });
    expect(parsePreferences({ version: 1, previewFont: 'ui-serif', codeFont: null, interfaceFont: 'Menlo' }))
      .toEqual({ ...defaults, previewFont: 'ui-serif' });
    for (const change of [{ appearance: 'wrong' }, { theme: 'unknown' }, { theme: ['paper'] }, { theme: '__proto__' }, { previewWidth: -1 }, { previewFontSize: 100 }, { previewFont: 'a\nbody' }, { version: 2 }]) expect(() => parsePreferences({ ...defaults, ...change })).toThrow();
  });
  it.each(['rich', 'edit', 'preview', 'split'])('drops legacy %s mode and its source settings', defaultMode => {
    localStorage.setItem(preferencesKey, JSON.stringify({ defaultMode, lineWrap: false, editorFontSize: 18, editorFont: 'Menlo', appearance: 'dark' }));
    const store = new PreferenceStore(localStorage);
    expect(store.error).toBe('');
    expect(store.value).toEqual({ ...defaults, appearance: 'dark' });
  });
  it('saves before notifying and keeps the current values on storage failure', () => {
    const setItem = vi.fn(() => { throw new Error('full'); });
    const store = new PreferenceStore({ getItem: () => null, setItem }); const listener = vi.fn(); store.subscribe(listener);
    expect(() => store.update({ previewFontSize: 20 })).toThrow('full'); expect(store.value).toEqual(defaults); expect(listener).not.toHaveBeenCalled();
  });
  it('protects corrupt preferences until explicit reset, and reopens persisted settings', () => {
    localStorage.setItem(preferencesKey, 'broken'); const store = new PreferenceStore(localStorage);
    expect(store.error).not.toBe(''); expect(() => store.update({ appearance: 'dark' })).toThrow();
    store.update({}, true); store.update({ appearance: 'dark', previewFont: 'My Font', previewFontSize: 20 });
    expect(new PreferenceStore(localStorage).value).toEqual(store.value);
  });
  it('applies the same appearance and typography values to preview and editor without touching notes', () => {
    localStorage.setItem('note', 'original'); preferences.update({ appearance: 'dark', previewFont: 'Menlo', previewFontSize: 20 });
    expect(isDark()).toBe(true); expect(document.documentElement.dataset.theme).toBe('dark');
    expect(previewPreferences()).toMatchObject({ dark: true, font: fontCSS('Menlo'), size: 20 });
    expect(document.documentElement.style.getPropertyValue('--preview-size')).toBe('20px');
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
  const client: SyncClient = { load: async () => config, save: vi.fn().mockResolvedValue(undefined), test: vi.fn(), flush: vi.fn().mockResolvedValue(undefined), run: vi.fn(), subscribe: fn => { fn({ busy: false, message: '', error: false }); return () => {}; } };
  await openSettings(client); const dialog = document.querySelector<HTMLDialogElement>('.settings-inline')!;
  expect(show).toHaveBeenCalledOnce(); expect(dialog.open).toBe(true);
  const shortcut = vi.fn(); document.addEventListener('keydown', shortcut);
  dialog.dispatchEvent(new KeyboardEvent('keydown', { key: '3', metaKey: true, bubbles: true })); expect(shortcut).not.toHaveBeenCalled(); document.removeEventListener('keydown', shortcut);
  dialog.dispatchEvent(new Event('cancel', { cancelable: true })); expect(back).toHaveBeenCalledOnce();
  window.dispatchEvent(new PopStateEvent('popstate')); expect(document.querySelector('.settings-inline')).toBeNull(); expect(shell.inert).toBe(true);
  back.mockRestore(); delete (HTMLDialogElement.prototype as Partial<HTMLDialogElement>).showModal;
});
it('moves legacy desktop note sizes to Pinto stops and steps only note text', () => {
  expect(parsePreferences({ previewFontSize: 16, previewFont: 'Menlo' })).toMatchObject({ previewFontSize: 17, previewFont: 'Menlo' });
  expect(parsePreferences({ previewFontSize: 28 }).previewFontSize).toBe(24);
  preferences.update({ previewFontSize: 16 });
  changeNoteTextSize('bigger'); expect(previewPreferences().size).toBe(20);
  changeNoteTextSize('smaller'); expect(previewPreferences().size).toBe(17);
  changeNoteTextSize('actual'); expect(previewPreferences().size).toBe(13);
  changeNoteTextSize('smaller'); expect(previewPreferences().size).toBe(13);
});

it('accepts the equals key WebKit reports for Command plus', () => {
  expect(noteTextSizeShortcut(new KeyboardEvent('keydown', { key: '=', metaKey: true, shiftKey: true }))).toBe('bigger');
  expect(noteTextSizeShortcut(new KeyboardEvent('keydown', { key: '+', metaKey: true, shiftKey: true }))).toBe('bigger');
  expect(noteTextSizeShortcut(new KeyboardEvent('keydown', { key: '-', metaKey: true }))).toBe('smaller');
  expect(noteTextSizeShortcut(new KeyboardEvent('keydown', { key: '0', metaKey: true }))).toBe('actual');
  expect(noteTextSizeShortcut(new KeyboardEvent('keydown', { key: '=', metaKey: true, isComposing: true }))).toBeUndefined();
});

it('switches tabs with arrows and saves controls without a settings footer', async () => {
  const root = document.createElement('div'); document.body.append(root);
  const config = { endpoint: 'https://unit.invalid', bucket: 'test', region: 'us-east-1', prefix: 'test', accessKeyId: '', secretAccessKey: '' };
  let publish!: (status: SyncStatus) => void;
  const run = vi.fn().mockResolvedValue(undefined);
  const client: SyncClient = { load: async () => config, save: vi.fn().mockResolvedValue(undefined), test: vi.fn(), flush: vi.fn().mockResolvedValue(undefined), run, subscribe: fn => { publish = fn; fn({ busy: false, message: '', error: false }); return () => {}; } };
  const close = vi.fn(), view = mountSettings(root, client, close); await view.loaded;
  const sidebar = root.querySelector('.settings-sidebar [role="tablist"]')!;
  expect(sidebar.getAttribute('aria-orientation')).toBe('vertical');
  const tab = sidebar.querySelector<HTMLButtonElement>('[data-section="general"]')!;
  tab.click();
  const sort = root.querySelector<HTMLInputElement>('#settings-panel-general [name="sortCheckedItems"]')!;
  expect(sort.disabled).toBe(false); expect(sort.checked).toBe(true);
  sort.click();
  expect(new PreferenceStore(localStorage).value.sortCheckedItems).toBe(false);
  tab.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
  expect(root.querySelector('#settings-panel-appearance')?.hasAttribute('hidden')).toBe(false);
  expect(root.querySelector('h1')?.textContent).toBe('外观');
  expect(document.activeElement?.textContent).toBe('外观');
  tab.click();
  const width = root.querySelector<HTMLSelectElement>('#settings-panel-general [name="previewWidth"]')!; width.value = '800'; width.dispatchEvent(new Event('change'));
  expect(preferences.value.previewWidth).toBe(800);
  expect(root.querySelector('[name=defaultMode], [name=lineWrap], [name=editorFontSize]')).toBeNull();
  const ids = [...root.querySelectorAll('[id]')].map(node => node.id); expect(new Set(ids).size).toBe(ids.length);
  const storageTab = sidebar.querySelector<HTMLButtonElement>('[data-section="storage"]')!;
  expect(storageTab.textContent).toBe('存储与同步');
  expect(sidebar.querySelector('[data-section="sync"]')).toBeNull();
  root.querySelector('main')!.scrollTop = 200; storageTab.click(); expect(root.querySelector('main')!.scrollTop).toBe(0);
  const storagePanel = root.querySelector('#settings-panel-storage')!;
  expect(storagePanel.querySelector('.settings-storage-meter')).not.toBeNull();
  expect(storagePanel.querySelector('.settings-sync-form')).not.toBeNull();
  expect(storagePanel.getAttribute('aria-hidden')).toBe('false');
  expect(root.querySelector('#settings-panel-sync')).toBeNull();
  const syncHeading = storagePanel.querySelector<HTMLElement>('#settings-sync-heading')!;
  syncHeading.scrollIntoView = vi.fn();
  view.select('sync');
  expect(syncHeading.scrollIntoView).toHaveBeenCalledWith({ block: 'start' });
  expect(storageTab.getAttribute('aria-selected')).toBe('true');
  expect(root.querySelector('h1')?.textContent).toBe('存储与同步');
  expect(root.querySelector('#settings-panel-editor, #settings-tab-editor')).toBeNull();
  expect(root.querySelector<HTMLInputElement>('[name="endpoint"]')?.value).toBe(config.endpoint);
  publish({ busy: true, message: '正在连接', error: false }); expect(root.querySelector<HTMLInputElement>('[name="bucket"]')!.disabled).toBe(true);
  publish({ busy: false, message: '连接失败', error: true }); expect(root.querySelector<HTMLInputElement>('[name="bucket"]')!.disabled).toBe(false);
  expect(root.querySelector('.settings-footer, .settings-reset')).toBeNull();
  expect(root.querySelectorAll('#settings-panel-general .settings-select-icon')).toHaveLength(2);
  view.destroy(); expect(root.children).toHaveLength(0);
});

it('autosaves S3 fields, removes the save button, and displays redacted sync errors beneath the sync button', async () => {
  const root = document.createElement('div'); document.body.append(root);
  const config = { endpoint: 'https://unit.invalid', bucket: 'test', region: 'us-east-1', prefix: 'test', accessKeyId: 'fake-id', secretAccessKey: 'old-secret' };
  const save = vi.fn().mockResolvedValue(undefined), run = vi.fn().mockRejectedValue(new Error('认证失败 new-secret fake-id'));
  const client: SyncClient = { load: async () => config, save, test: vi.fn(), flush: vi.fn(), run, subscribe: fn => { fn({ busy: false, message: '', error: false }); return () => {}; } };
  const view = mountSettings(root, client, vi.fn()); await view.loaded; view.select('sync');
  expect([...root.querySelectorAll('button')].some(button => button.textContent === '保存连接')).toBe(false);
  expect(root.textContent).not.toContain('仅在本次运行'); expect(root.textContent).not.toContain('不会保存到磁盘');
  const secret = root.querySelector<HTMLInputElement>('[name="secretAccessKey"]')!; secret.value = 'new-secret'; secret.dispatchEvent(new Event('input', { bubbles: true }));
  expect(save).toHaveBeenCalledWith({ ...config, ...s3Defaults, secretAccessKey: 'new-secret' }); expect(run).not.toHaveBeenCalled();
  const button = root.querySelector<HTMLButtonElement>('.settings-sync-now')!; button.click();
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
  const client: SyncClient = { load: () => new Promise((_, fail) => { reject = fail; }), save: vi.fn(), test: vi.fn(), flush: vi.fn(), run: vi.fn(), subscribe: fn => { fn({ busy: false, message: '', error: false }); return () => {}; } };
  const view = mountSettings(root, client, vi.fn());
  expect(root.querySelector<HTMLInputElement>('[name="secretAccessKey"]')!.disabled).toBe(true);
  reject(new Error('无法读取已保存的连接')); await view.loaded;
  expect(root.querySelector('.settings-sync-status')?.textContent).toContain('无法读取已保存的连接'); expect(client.save).not.toHaveBeenCalled();
  view.destroy();
});

it('starts unconfigured sync off and exposes advanced S3 fields and a copyable redacted test error', async () => {
  const root = document.body.appendChild(document.createElement('div'));
  const config = { endpoint: '', bucket: '', region: 'us-east-1', prefix: 'test', accessKeyId: '', secretAccessKey: '' };
  const test = vi.fn().mockRejectedValue(new Error('HTTP 403\n<Code>AccessDenied</Code> synthetic-secret'));
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
  const client: SyncClient = { load: async () => config, save: vi.fn().mockResolvedValue(undefined), test, flush: vi.fn(), run: vi.fn(), subscribe: () => () => {} };
  const view = mountSettings(root, client, vi.fn()); await view.loaded; view.select('storage');
  try {
    const auto = root.querySelector<HTMLInputElement>('[name=automaticSync]')!;
    expect(auto.checked).toBe(false); expect(auto.disabled).toBe(true);
    expect(root.querySelector<HTMLSelectElement>('[name=assetDownload]')!.value).toBe('all');
    for (const [name, value] of Object.entries({ endpoint: 'https://unit.invalid', bucket: 'test', accessKeyId: 'synthetic-id', secretAccessKey: 'synthetic-secret', timeout: '299', concurrency: '16', forcePathStyle: 'false', tlsVerify: 'false', assetDownload: 'on-demand' })) {
      const field = root.querySelector<HTMLInputElement | HTMLSelectElement>(`[name=${name}]`)!;
      field.value = value; field.dispatchEvent(new Event(field instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
    }
    root.querySelector<HTMLButtonElement>('.settings-sync-test')!.click();
    await vi.waitFor(() => expect(root.querySelector('.settings-sync-error')?.hasAttribute('hidden')).toBe(false));
    expect(test).toHaveBeenCalledWith({ ...config, endpoint: 'https://unit.invalid', bucket: 'test', accessKeyId: 'synthetic-id', secretAccessKey: 'synthetic-secret', timeout: 299, concurrency: 16, forcePathStyle: false, tlsVerify: false, assetDownload: 'on-demand' });
    expect(client.run).not.toHaveBeenCalled();
    root.querySelector<HTMLButtonElement>('.settings-sync-copy')!.click();
    await vi.waitFor(() => expect(writeText).toHaveBeenCalledWith('HTTP 403\n<Code>AccessDenied</Code> [已隐藏]'));
    expect(root.querySelector('.settings-sync-error pre')?.querySelector('code')).toBeNull();
  } finally { view.destroy(); root.remove(); Reflect.deleteProperty(navigator, 'clipboard'); }
});
