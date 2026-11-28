// @vitest-environment jsdom
import './fixtures/chinese-ui';
import { beforeEach, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { preferences } from '../src/preferences';
import { mountSettings } from '../src/settings-view';
import { openSettings } from '../src/open-settings';
import type { SyncClient } from '../src/sync-session';
import { version } from '../package.json';

vi.hoisted(() => { vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {} })); });
const platform = vi.hoisted(() => ({ native: false }));
vi.mock('../src/preferences', async importOriginal => ({ ...await importOriginal<typeof import('../src/preferences')>(), mobile: true, get native() { return platform.native; } }));
vi.mock('@tauri-apps/api/core', async importOriginal => ({ ...await importOriginal<typeof import('@tauri-apps/api/core')>(), invoke: vi.fn() }));
beforeEach(() => {
  localStorage.clear(); preferences.update({}, true); document.body.replaceChildren(); history.replaceState(null, '');
  Object.defineProperties(HTMLDialogElement.prototype, {
    showModal: { configurable: true, value(this: HTMLDialogElement) { this.open = true; } },
    close: { configurable: true, value(this: HTMLDialogElement) { this.open = false; this.dispatchEvent(new Event('close')); } },
  });
});
const client: SyncClient = {
  load: async () => ({ endpoint: '', bucket: '', region: 'us-east-1', prefix: 'test', accessKeyId: '', secretAccessKey: '' }),
  save: vi.fn(), test: vi.fn(), flush: vi.fn(), run: vi.fn(),
  subscribe: fn => { fn({ busy: false, message: '', error: false }); return () => {}; },
};

it('changes language from the native mobile settings menu and stays on General', async () => {
  platform.native = true;
  const html = document.documentElement;
  const applyMobileLayout = () => html.setAttribute('data-mobile', '');
  window.addEventListener('preferences-applied', applyMobileLayout);
  applyMobileLayout();
  vi.mocked(invoke).mockResolvedValue(null);
  try {
    await openSettings(client, 'general');
    const root = document.querySelector<HTMLDialogElement>('.settings-inline')!;
    const trigger = root.querySelector<HTMLButtonElement>('#settings-select-language')!;
    trigger.click();
    const options = [...root.querySelectorAll<HTMLButtonElement>('[role=menuitemradio]')];
    options.find(option => option.textContent === '日本語')!.click();
    await vi.waitFor(() => expect(root.querySelector('.settings-select-popup')).toBeNull());
    expect(root.querySelector('.settings-detail-title')?.textContent).toBe('一般');
    expect(document.documentElement.lang).toBe('ja');
    expect(trigger.ariaLabel).toBe('言語：日本語');
    expect(history.state.settingsSection).toBe('general');
    root.querySelector<HTMLButtonElement>('.settings-detail .settings-back')!.click();
    await vi.waitFor(() => expect(root.hasAttribute('data-settings-detail')).toBe(false));
    root.querySelector<HTMLButtonElement>('.settings-done')!.click();
    await vi.waitFor(() => expect(document.querySelector('.settings-inline')).toBeNull());
  } finally {
    window.removeEventListener('preferences-applied', applyMobileLayout);
    html.removeAttribute('data-mobile');
    platform.native = false;
  }
});

it('requests Android local-network access only after a user selects the S3 authorization', async () => {
  platform.native = true;
  const userAgent = vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue('Android');
  const ipc = vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === 'sync_local_network_access') return Boolean((args as { request: boolean }).request);
    return null;
  });
  const root = document.body.appendChild(document.createElement('div'));
  const view = mountSettings(root, client, vi.fn());
  try {
    await view.loaded;
    const button = root.querySelector<HTMLButtonElement>('[aria-label="授权局域网访问"]')!;
    expect(button.closest('.settings-sync-form')).not.toBeNull();
    expect(ipc.mock.calls.filter(([command]) => command === 'sync_local_network_access')).toEqual([
      ['sync_local_network_access', { request: false }],
    ]);
    button.click();
    await vi.waitFor(() => expect(button.textContent).toBe('已授权'));
    expect(button.disabled).toBe(true);
    expect(ipc).toHaveBeenCalledWith('sync_local_network_access', { request: true });
  } finally {
    view.destroy();
    platform.native = false;
    userAgent.mockRestore();
    ipc.mockReset();
  }
});

it('omits the former source editor controls on mobile', async () => {
  const root = document.createElement('div'); document.body.append(root);
  const view = mountSettings(root, client, vi.fn()); await view.loaded; view.select('general');
  expect(root.querySelector('#settings-panel-general [name=previewWidth]')).not.toBeNull();
  expect(root.querySelector('[name=defaultMode], [name=lineWrap]')).toBeNull();
  expect(root.querySelector<HTMLSelectElement>('[name=assetDownload]')!.value).toBe('on-demand');
  view.destroy();
});

it('opens settings at the category list and returns to it without changing the library route', async () => {
  const libraryRoute = { qingyuNavigation: { page: 'folders', folderId: null, trash: false } };
  history.replaceState(libraryRoute, '');
  const trigger = document.createElement('button'); document.body.append(trigger); trigger.focus();
  await openSettings(client);
  const root = document.querySelector<HTMLDialogElement>('.settings-inline')!;
  const home = root.querySelector<HTMLElement>('.settings-home .settings-pages')!;
  const general = root.querySelector<HTMLButtonElement>('[data-section=general]')!;
  expect(document.activeElement).toBe(home.querySelector('h1'));
  expect([...root.querySelectorAll<HTMLElement>('.settings-categories button')].map(row => row.dataset.section)).toEqual(['account', 'general', 'appearance', 'today', 'storage', 'help']);
  expect([...root.querySelectorAll('.settings-category-group > button')].map(row => row.textContent)).toEqual(['通用', '外观']);
  expect(root.querySelector('[role=tablist]')).toBeNull();
  expect(root.querySelector('#settings-panel-typography')).toBeNull();
  expect(root.querySelector('[name=editorFont]')).toBeNull();
  expect(root.hasAttribute('data-settings-detail')).toBe(false);

  home.scrollTop = 60;
  general.click();
  expect(history.state.settingsSection).toBe('general');
  expect(root.querySelector('.settings-detail-title')?.textContent).toBe('通用');
  expect(root.querySelector<HTMLElement>('#settings-panel-general')?.hidden).toBe(false);
  expect(root.querySelector('#settings-panel-editor')).toBeNull();
  root.querySelector<HTMLButtonElement>('.settings-detail .settings-back')!.click();
  await vi.waitFor(() => expect(root.hasAttribute('data-settings-detail')).toBe(false));
  expect(home.scrollTop).toBe(60);
  expect(document.activeElement).toBe(general);

  root.querySelector<HTMLButtonElement>('.settings-done')!.click();
  await vi.waitFor(() => expect(document.querySelector('.settings-inline')).toBeNull());
  expect(history.state).toEqual(libraryRoute);
  expect(document.activeElement).toBe(trigger);
});

it('discards a stale settings entry after reload so the new sheet can close', async () => {
  const route = { qingyuNavigation: { page: 'folders', folderId: null, trash: false } };
  history.replaceState({ ...route, settings: true, settingsSection: 'encryption', settingsDepth: 2 }, '');
  await openSettings(client);
  document.querySelector<HTMLButtonElement>('.settings-done')!.click();
  await vi.waitFor(() => expect(document.querySelector('.settings-inline')).toBeNull());
  expect(history.state).toEqual(route);
});

it('returns through the settings home when opened from the sync shortcut', async () => {
  await openSettings(client, 'sync');
  const root = document.querySelector<HTMLDialogElement>('.settings-inline')!;
  expect(root.querySelector('.settings-detail-title')?.textContent).toBe('存储与同步');
  expect(history.state.settingsSection).toBe('sync');
  expect(root.querySelector<HTMLElement>('#settings-panel-storage')?.hidden).toBe(false);
  expect(root.querySelector('.settings-footer')).toBeNull();
  expect(root.querySelector('.settings-sync-now')?.textContent).toBe('立即同步');
  const heading = root.querySelector<HTMLElement>('#settings-sync-heading')!;
  heading.scrollIntoView = vi.fn();
  await openSettings(client, 'sync');
  expect(heading.scrollIntoView).toHaveBeenCalledWith({ block: 'start' });
  history.back();
  await vi.waitFor(() => expect(root.hasAttribute('data-settings-detail')).toBe(false));
  expect(root.open).toBe(true);
  expect(document.activeElement).toBe(root.querySelector('[data-section=storage]'));
  history.back();
  await vi.waitFor(() => expect(document.querySelector('.settings-inline')).toBeNull());
});

it('keeps the local account card and returns to it from the S3 form', async () => {
  await openSettings(client);
  const root = document.querySelector<HTMLDialogElement>('.settings-inline')!;
  root.querySelector<HTMLButtonElement>('#settings-tab-account')!.click();
  expect(root.querySelector('.settings-detail-title')?.textContent).toBe('账户');
  root.querySelector<HTMLButtonElement>('#settings-panel-account [aria-label="存储与同步"]')!.click();
  expect(history.state.settingsDepth).toBe(2);
  expect(root.querySelector<HTMLElement>('#settings-panel-storage')?.hidden).toBe(false);
  root.querySelector<HTMLButtonElement>('.settings-detail .settings-back')!.click();
  await vi.waitFor(() => expect(history.state.settingsSection).toBe('account'));
  expect(root.querySelector<HTMLElement>('#settings-panel-account')?.hidden).toBe(false);
  history.go(-2);
  await vi.waitFor(() => expect(document.querySelector('.settings-inline')).toBeNull());
});

it('dismisses the whole settings sheet from the nested S3 form', async () => {
  const route = { qingyuNavigation: { page: 'folders', folderId: null, trash: false } };
  history.replaceState(route, ''); await openSettings(client, 'account');
  const root = document.querySelector<HTMLDialogElement>('.settings-inline')!;
  root.querySelector<HTMLButtonElement>('#settings-panel-account [aria-label="存储与同步"]')!.click();
  expect(history.state.settingsSection).toBe('sync'); expect(history.state.settingsDepth).toBe(2);
  expect(root.matches('.feature-panel-dialog[data-feature-animated]')).toBe(true);
  const cancel = new Event('cancel', { cancelable: true });
  root.dispatchEvent(cancel);
  expect(cancel.defaultPrevented).toBe(true);
  await vi.waitFor(() => expect(history.state.settingsSection).toBe('account'));
  expect(root.open).toBe(true);
  root.querySelector<HTMLButtonElement>('#settings-panel-account [aria-label="存储与同步"]')!.click();
  expect(history.state.settingsDepth).toBe(2);
  vi.spyOn(root, 'getBoundingClientRect').mockReturnValue(new DOMRect(10, 200, 380, 600));
  root.dispatchEvent(new MouseEvent('click', { bubbles: true, clientX: 200, clientY: 100 }));
  await vi.waitFor(() => expect(document.querySelector('.settings-inline')).toBeNull()); expect(history.state).toEqual(route);
});

it('dismisses the sheet from its backdrop without leaving a settings history entry behind', async () => {
  const libraryRoute = { qingyuNavigation: { page: 'folders', folderId: null, trash: false } };
  history.replaceState(libraryRoute, '');
  await openSettings(client, 'sync');
  const root = document.querySelector<HTMLDialogElement>('.settings-inline')!;
  vi.spyOn(root, 'getBoundingClientRect').mockReturnValue(new DOMRect(10, 200, 380, 600));
  root.dispatchEvent(new MouseEvent('click', { bubbles: true, clientX: 200, clientY: 400 }));
  expect(root.open).toBe(true);
  root.dispatchEvent(new MouseEvent('click', { bubbles: true, clientX: 200, clientY: 100 }));
  await vi.waitFor(() => expect(document.querySelector('.settings-inline')).toBeNull());
  expect(history.state).toEqual(libraryRoute);
});

it('tracks a header drag, resets on a short or cancelled pull, and dismisses on a long pull', async () => {
  vi.stubGlobal('ResizeObserver', class {
    observe() {}
    disconnect() {}
  });
  const libraryRoute = { qingyuNavigation: { page: 'folders', folderId: null, trash: false } };
  history.replaceState(libraryRoute, '');
  await openSettings(client, 'sync');
  const root = document.querySelector<HTMLDialogElement>('.settings-inline')!;
  const header = root.querySelector<HTMLElement>('.settings-detail .settings-header')!;
  vi.spyOn(root, 'getBoundingClientRect').mockReturnValue(new DOMRect(0, 200, 400, 600));
  const pointer = (type: string, clientY: number) => header.dispatchEvent(Object.assign(
    new MouseEvent(type, { bubbles: true, clientY }),
    { pointerId: 1, isPrimary: true },
  ));

  pointer('pointerdown', 220);
  pointer('pointermove', 300);
  expect(root.hasAttribute('data-feature-dragging')).toBe(true);
  expect(root.style.getPropertyValue('--feature-sheet-transform')).toBe('translateY(76px)');
  pointer('pointerup', 300);
  expect(root.style.getPropertyValue('--feature-sheet-transform')).toBe('');
  expect(root.open).toBe(true);
  expect(history.state.settingsSection).toBe('sync');

  pointer('pointerdown', 220);
  pointer('pointermove', 380);
  pointer('pointercancel', 380);
  expect(root.style.getPropertyValue('--feature-sheet-transform')).toBe('');
  expect(root.open).toBe(true);

  pointer('pointerdown', 220);
  pointer('pointermove', 380);
  pointer('pointerup', 380);
  expect(root.style.getPropertyValue('--feature-sheet-transform')).toBe('translateY(156px)');
  await vi.waitFor(() => expect(document.querySelector('.settings-inline')).toBeNull());
  expect(history.state).toEqual(libraryRoute);
});

it('opens an anchored checked menu, saves a selection and keeps settings open on return', async () => {
  await openSettings(client);
  const root = document.querySelector<HTMLDialogElement>('.settings-inline')!;
  root.querySelector<HTMLButtonElement>('[data-section=appearance]')!.click();
  const appearance = root.querySelector<HTMLSelectElement>('[name=previewFont]')!, trigger = root.querySelector<HTMLButtonElement>('#settings-select-previewFont')!;
  expect(appearance.hidden).toBe(true);
  trigger.click();
  const popup = root.querySelector<HTMLDialogElement>('.settings-select-popup')!;
  expect(popup.open).toBe(true); expect(trigger.getAttribute('aria-expanded')).toBe('true');
  expect(popup.querySelector('[aria-checked=true]')?.textContent).toBe('默认');
  const options = [...popup.querySelectorAll<HTMLButtonElement>('[role=menuitemradio]')];
  options.find(o => o.textContent === '衬线')!.click();
  await vi.waitFor(() => expect(root.querySelector('.settings-select-popup')).toBeNull());
  expect(root.open).toBe(true); expect(trigger.textContent).toBe('衬线');
  expect(history.state.settingsSection).toBe('appearance');
  expect(document.activeElement).toBe(trigger);
  expect(preferences.value.previewFont).toBe('ui-serif');
  expect(JSON.parse(localStorage.getItem('miaoyan-editor.preferences.v1')!).previewFont).toBe('ui-serif');
  trigger.click(); history.back();
  await vi.waitFor(() => expect(root.querySelector('.settings-select-popup')).toBeNull());
  expect(root.open).toBe(true); expect(preferences.value.previewFont).toBe('ui-serif');
  root.querySelector<HTMLButtonElement>('.settings-detail .settings-back')!.click();
  await vi.waitFor(() => expect(root.hasAttribute('data-settings-detail')).toBe(false));
  root.querySelector<HTMLButtonElement>('.settings-done')!.click();
  await vi.waitFor(() => expect(document.querySelector('.settings-inline')).toBeNull());
});

it('dismisses outside clicks without changing preferences and supports keyboard choices', async () => {
  const root = document.createElement('div'); document.body.append(root);
  const view = mountSettings(root, client, vi.fn()); await view.loaded;
  view.select('general');
  const trigger = root.querySelector<HTMLButtonElement>('#settings-select-previewWidth')!;
  trigger.click(); const popup = root.querySelector<HTMLDialogElement>('.settings-select-popup')!;
  popup.dispatchEvent(new MouseEvent('click', { clientX: -1, clientY: -1, bubbles: true }));
  await vi.waitFor(() => expect(root.querySelector('.settings-select-popup')).toBeNull());
  expect(preferences.value.previewWidth).toBe(1000);
  trigger.click(); const menu = root.querySelector<HTMLDialogElement>('.settings-select-popup')!;
  menu.dispatchEvent(new KeyboardEvent('keydown', { key: 'End', bubbles: true }));
  expect(document.activeElement?.textContent).toBe('铺满窗口');
  menu.dispatchEvent(new Event('cancel', { cancelable: true }));
  await vi.waitFor(() => expect(root.querySelector('.settings-select-popup')).toBeNull());
  expect(preferences.value.previewWidth).toBe(1000);
  view.destroy();
});

it('shows the bundled version and keeps disconnected and desktop-only controls out of mobile settings', () => {
  const root = document.body.appendChild(document.createElement('div'));
  const view = mountSettings(root, client, vi.fn());
  const general = root.querySelector('#settings-panel-general')!;
  expect(general.textContent).toContain(version);
  expect(root.querySelector('#settings-panel-help')?.textContent).toContain(version);
  expect(root.querySelector('#settings-panel-ai, #settings-panel-privacy, #settings-panel-shortcuts, [name=buttonDisplay]')).toBeNull();
  expect(general.querySelector('button:disabled')).toBeNull();
  view.destroy();
});

it.each([false, true])('opens GitHub Issues from help (native: %s)', native => {
  platform.native = native;
  const open = vi.spyOn(window, 'open').mockReturnValue(null);
  const ipc = vi.mocked(invoke).mockResolvedValue(null);
  const root = document.body.appendChild(document.createElement('div'));
  const view = mountSettings(root, undefined, vi.fn());
  try {
    view.select('help');
    const page = root.querySelector('#settings-panel-help')!;
    expect(page.querySelector('img')?.getAttribute('src')).toBe('/brand/qingyu-app.png');
    expect(page.textContent).toContain('轻羽 — 轻量的所见即所得 Markdown 编辑器。');
    for (const [label, path] of [['问题反馈', 'issues/new'], ['常见问题', 'issues']]) {
      const button = [...page.querySelectorAll('button')].find(item => item.querySelector('.settings-label')?.textContent === label)!;
      button.click();
      const url = `https://github.com/appdev/QingYu/${path}`;
      if (native) expect(ipc).toHaveBeenLastCalledWith('plugin:opener|open_url', { url });
      else expect(open).toHaveBeenLastCalledWith(url, '_blank', 'noopener,noreferrer');
    }
  } finally {
    view.destroy();
    platform.native = false;
    ipc.mockReset();
    open.mockRestore();
  }
});

const [major, minor] = version.split('.').map(Number);
const newerVersion = `${major}.${minor + 10}.0`;
it.each([
  [200, `v${newerVersion}`, `发现新版本 ${newerVersion}`, true],
  [200, `v${version}`, '已是最新版本', false],
  [200, 'v0.0.0', '已是最新版本', false],
  [200, 'unrecognized', '暂时无法检查更新', false],
  [404, undefined, '暂时无法检查更新', false],
] as const)('checks a release before showing the result (%s, %s)', async (status, tag, title, hasUpdate) => {
  const fetchRelease = vi.fn(async () => new Response(JSON.stringify({ tag_name: tag }), { status }));
  vi.stubGlobal('fetch', fetchRelease);
  const open = vi.spyOn(window, 'open').mockReturnValue(null);
  const root = document.body.appendChild(document.createElement('div'));
  const view = mountSettings(root, undefined, vi.fn());
  try {
    view.select('help');
    const button = root.querySelector<HTMLButtonElement>('#settings-panel-help button')!;
    button.click();
    expect(button.disabled).toBe(true);
    expect(button.textContent).toContain('正在检查');
    button.click();
    expect(fetchRelease).toHaveBeenCalledExactlyOnceWith('https://api.github.com/repos/appdev/QingYu/releases/latest', expect.anything());
    expect(open).not.toHaveBeenCalled();
    await vi.waitFor(() => expect(root.querySelector('dialog[open] h2')?.textContent).toBe(title));
    expect(button.disabled).toBe(false);
    const download = [...root.querySelectorAll<HTMLButtonElement>('dialog button')].find(item => item.textContent === '前往下载');
    expect(Boolean(download)).toBe(hasUpdate);
    if (download) {
      download.click();
      expect(open).toHaveBeenCalledExactlyOnceWith('https://github.com/appdev/QingYu/releases/latest', '_blank', 'noopener,noreferrer');
    }
  } finally {
    view.destroy();
    vi.unstubAllGlobals();
    open.mockRestore();
  }
});
