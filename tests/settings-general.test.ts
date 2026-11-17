// @vitest-environment jsdom
import { expect, it, vi } from 'vitest';
import { mountSettings } from '../src/settings-view';
import { libraryLocation } from '../src/library-settings';
import { preferences } from '../src/preferences';
import { invoke } from '@tauri-apps/api/core';
import type { SyncClient } from '../src/sync-session';
vi.hoisted(() => { Object.assign(window, { __TAURI_INTERNALS__: {} }); vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {} })); });
vi.mock('../src/library-settings', async importOriginal => ({
  ...await importOriginal<typeof import('../src/library-settings')>(),
  libraryLocation: vi.fn(), libraryDailyChoices: vi.fn(), libraryStorageUsage: vi.fn().mockResolvedValue({ notes: 0, files: 0, trash: 0 }),
}));
vi.mock('@tauri-apps/api/path', () => ({ homeDir: vi.fn().mockResolvedValue('/Users/test') }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async (command: string) => {
  if (command === 'account_status') return null;
  if (command.startsWith('plugin:path|')) return '/Users/test';
  return { shortcut: 'Alt+Super+KeyM', active: true, recording: false, error: null, defaultShortcut: 'Alt+Super+KeyM' };
}) }));
const config = { endpoint: '', bucket: '', region: 'us-east-1', prefix: 'test', accessKeyId: '', secretAccessKey: '' };

it('starts and stops the desktop API and copies a self-contained authenticated MCP config', async () => {
  const connection = { url: 'http://127.0.0.1:8787/mcp', token: 'synthetic-token' };
  const ipc = vi.mocked(invoke), implementation = ipc.getMockImplementation()!;
  let running = false, portBusy = true;
  ipc.mockImplementation(async (command, args) => {
    if (command !== 'local_mcp') return implementation(command, args);
    const { enabled } = args as { enabled?: boolean };
    if (enabled && portBusy) { portBusy = false; throw '端口已被占用'; }
    running = enabled ?? running;
    return (running ? connection : null) as never;
  });
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
  const client: SyncClient = { load: async () => config, save: vi.fn(), flush: vi.fn(), run: vi.fn(), subscribe: () => () => {} };
  const root = document.body.appendChild(document.createElement('div'));
  const view = mountSettings(root, client, vi.fn());
  try {
    view.select('ai');
    const page = root.querySelector('#settings-panel-ai')!;
    const toggle = page.querySelector('button')!;
    const copy = page.querySelector<HTMLButtonElement>('#settings-mcp-copy')!;
    await vi.waitFor(() => expect(toggle.disabled).toBe(false));
    expect(copy.disabled).toBe(true);
    toggle.click();
    await vi.waitFor(() => expect(page.textContent).toContain('端口已被占用'));
    expect(copy.disabled).toBe(true);
    toggle.click();
    await vi.waitFor(() => expect(toggle.textContent).toBe('停止服务'));
    expect(page.textContent).toContain(connection.url);
    expect(page.textContent).not.toContain(connection.token);
    copy.click();
    expect(writeText).toHaveBeenCalledWith(`[mcp_servers.qingyu]\nurl = "${connection.url}"\nhttp_headers = { Authorization = "Bearer ${connection.token}" }`);
    toggle.click();
    await vi.waitFor(() => expect(page.textContent).toContain('已停止'));
    expect(copy.disabled).toBe(true);
  } finally {
    view.destroy(); root.remove(); ipc.mockImplementation(implementation);
    Reflect.deleteProperty(navigator, 'clipboard');
  }
});

it('shows the current notes directory, changes it immediately and keeps it on cancellation or error', async () => {
  vi.mocked(libraryLocation).mockResolvedValue('/Users/test/Documents/Notes');
  const client: SyncClient = { load: async () => config, save: vi.fn(), flush: vi.fn(), run: vi.fn(), subscribe: fn => { fn({ busy: false, message: '', error: false }); return () => {}; } };
  const root = document.createElement('div'); document.body.append(root); const view = mountSettings(root, client, vi.fn());
  const general = root.querySelector('#settings-panel-general')!;
  expect([...general.querySelectorAll<HTMLInputElement | HTMLSelectElement>('[name=buttonDisplay], [name=alwaysOnTop], [name=previewWidth]')].map(control => control.name))
    .toEqual(['buttonDisplay', 'alwaysOnTop', 'previewWidth']);
  await vi.waitFor(() => expect(root.textContent).toContain('~/Documents/Notes'));
  expect(root.textContent).toContain('唤起快捷键');
  const button = root.querySelector<HTMLButtonElement>('[aria-label="更改笔记位置"]')!;
  vi.mocked(libraryLocation).mockResolvedValueOnce('/Users/test/Documents/New'); button.click();
  await vi.waitFor(() => expect(root.textContent).toContain('~/Documents/New')); expect(libraryLocation).toHaveBeenLastCalledWith(true);
  vi.mocked(libraryLocation).mockResolvedValueOnce('/Users/test/Documents/New'); button.click();
  await vi.waitFor(() => expect(button.disabled).toBe(false)); expect(root.querySelector('.settings-message')?.textContent).toBe('');
  vi.mocked(libraryLocation).mockRejectedValueOnce(new Error('目录不可写')); button.click();
  await vi.waitFor(() => expect(root.querySelector('.settings-message')?.textContent).toBe('目录不可写'));
  expect(root.textContent).toContain('~/Documents/New'); view.destroy(); root.remove();
});

it('recovers storage settings after an initial timeout without keeping an error on other pages', async () => {
  const timeout = new Error('主窗口未响应');
  vi.mocked(libraryLocation).mockRejectedValueOnce(timeout).mockResolvedValue('/Users/test/Documents/Ready');
  const load = vi.fn().mockRejectedValueOnce(timeout).mockResolvedValue({ ...config, mode: 's3' });
  const client: SyncClient = { load, save: vi.fn(), flush: vi.fn(), run: vi.fn(), subscribe: () => () => {} };
  const root = document.body.appendChild(document.createElement('div'));
  const view = mountSettings(root, client, vi.fn());
  try {
    await view.loaded;
    expect(root.querySelector('.settings-message')?.textContent).toBe('');
    expect(root.textContent).toContain('暂时无法读取笔记位置');
    const region = root.querySelector<HTMLInputElement>('input[name=region]')!;
    expect(region.disabled).toBe(true);
    view.select('storage');
    window.dispatchEvent(new Event('focus'));
    await vi.waitFor(() => {
      expect(root.textContent).toContain('~/Documents/Ready');
      expect(region.value).toBe(config.region);
      expect(region.disabled).toBe(false);
      expect(root.querySelector('.settings-sync-status')?.textContent).toBe('');
    });
    expect(load).toHaveBeenCalledTimes(2);
  } finally { view.destroy(); root.remove(); }
});

it('switches between cloud plan and S3 only after saving the active mode', async () => {
  vi.mocked(libraryLocation).mockResolvedValue('/Users/test/Documents/Notes');
  const profile = { ...config, endpoint: 'https://unit.invalid', accessKeyId: 'test-id', secretAccessKey: 'test-secret' };
  const save = vi.fn().mockResolvedValue(undefined);
  const client: SyncClient = { load: async () => profile, save, flush: vi.fn(), run: vi.fn(), subscribe: () => () => {} };
  const root = document.body.appendChild(document.createElement('div'));
  const view = mountSettings(root, client, vi.fn());
  try {
    await view.loaded; view.select('storage');
    const mode = root.querySelector<HTMLSelectElement>('[name=storageMode]')!;
    const cloud = root.querySelector<HTMLElement>('#settings-panel-storage .settings-client-account')!.parentElement!;
    const s3 = root.querySelector<HTMLFormElement>('.settings-sync-form')!;
    const field = s3.querySelector<HTMLInputElement>('[name=secretAccessKey]')!;
    expect(mode.value).toBe('s3');
    expect(cloud.hidden).toBe(true);
    function select(value: string) { mode.value = value; mode.dispatchEvent(new Event('change')); }
    save.mockRejectedValueOnce(new Error('保存失败'));
    select('cloud');
    await vi.waitFor(() => expect(mode.disabled).toBe(false));
    expect(mode.value).toBe('s3'); expect(cloud.hidden).toBe(true);
    select('cloud');
    await vi.waitFor(() => expect(cloud.hidden).toBe(false));
    expect(save).toHaveBeenLastCalledWith({ ...profile, mode: 'cloud' });
    expect(s3.closest('[hidden]')).not.toBeNull(); expect(field.disabled).toBe(true);
    s3.dispatchEvent(new Event('submit', { cancelable: true }));
    expect(client.run).not.toHaveBeenCalled();
    select('s3');
    await vi.waitFor(() => expect(field.disabled).toBe(false));
    expect(cloud.hidden).toBe(true); expect(field.value).toBe(profile.secretAccessKey);
  } finally { view.destroy(); root.remove(); }
});

it('can sync now with automatic cloud sync off and saves the toggle for the signed-in account', async () => {
  vi.mocked(libraryLocation).mockResolvedValue('/Users/test/Documents/Notes');
  const account = { profile: { id: 'fixture-account' }, subscription: { active: true, emailVerificationRequired: false, quotaBytes: 100 } };
  const ipc = vi.mocked(invoke), implementation = ipc.getMockImplementation()!;
  ipc.mockImplementation((command, args) => {
    if (command === 'account_status') return Promise.resolve(account as never);
    if (command === 'account_redemptions') return Promise.resolve([] as never);
    return implementation(command, args);
  });
  const run = vi.fn().mockResolvedValue(undefined);
  const client: SyncClient = { load: async () => ({ ...config, mode: 'cloud' }), save: vi.fn(), flush: vi.fn(), run, subscribe: () => () => {} };
  const root = document.body.appendChild(document.createElement('div'));
  const view = mountSettings(root, client, vi.fn());
  try {
    await view.loaded; view.select('storage');
    const manual = root.querySelector<HTMLButtonElement>('.settings-sync-now')!;
    const auto = root.querySelector<HTMLInputElement>('[name=automaticSync]')!;
    const interval = root.querySelector<HTMLSelectElement>('[name=syncInterval]')!;
    expect(auto.checked).toBe(false); expect(interval.value).toBe('3');
    await vi.waitFor(() => expect(manual.disabled).toBe(false));
    manual.click();
    await vi.waitFor(() => expect(run).toHaveBeenCalledWith(config, 'fixture-account'));
    auto.click();
    expect(preferences.value.autoSyncCloudAccount).toBe('fixture-account');
    interval.value = '5'; interval.dispatchEvent(new Event('change'));
    expect(preferences.value.syncInterval).toBe(5);
    auto.click(); expect(preferences.value.autoSyncCloudAccount).toBe('');
  } finally {
    view.destroy(); root.remove(); ipc.mockImplementation(implementation);
    preferences.update({ syncInterval: 3, autoSyncCloudAccount: '' });
  }
});
