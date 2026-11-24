// @vitest-environment jsdom
import './fixtures/chinese-ui';
import { beforeEach, expect, it, vi } from 'vitest';
import { mountSettings } from '../src/settings-view';
import { libraryLocation } from '../src/library-settings';
import { preferences } from '../src/preferences';
import { workflowText } from '../src/ui-language';
import { invoke } from '@tauri-apps/api/core';
import type { SyncClient } from '../src/sync-session';
import { s3Defaults } from '../src/sync-store';
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
beforeEach(() => {
  Object.defineProperties(HTMLDialogElement.prototype, {
    showModal: { configurable: true, value(this: HTMLDialogElement) { this.open = true; } },
    close: { configurable: true, value(this: HTMLDialogElement, result = '') { this.returnValue = result; this.open = false; this.dispatchEvent(new Event('close')); } },
  });
});

it('sets, unlocks and resets the sync password from Storage & Sync', async () => {
  const profile = { ...config, endpoint: 'https://s3.invalid', bucket: 'test', accessKeyId: 'test', secretAccessKey: 'test', mode: 's3' as const };
  const password = vi.fn<NonNullable<SyncClient['password']>>();
  const client: SyncClient = { load: async () => profile, save: vi.fn(), test: vi.fn(), flush: vi.fn(), run: vi.fn(), password,
    subscribe(listener) { listener({ busy: false, error: false, message: '需要输入数据仓库密钥', passwordRequired: {} }); return () => {}; } };
  const root = document.body.appendChild(document.createElement('div'));
  const view = mountSettings(root, client, vi.fn());
  try {
    view.select('storage');
    await view.loaded;
    expect(root.querySelector('.settings-sync-status[aria-live="polite"]')?.textContent).toBe('需要输入数据仓库密钥');
    const button = [...root.querySelectorAll('button')].find(button => button.textContent === '管理密钥')!;
    expect(button.disabled).toBe(false);
    expect(button.closest('.settings-row')?.textContent).toContain('数据仓库用于加密保存数据快照。同步时需要解锁密钥。');
    for (const [state, title] of [['new', '设置数据仓库密钥'], ['locked', '输入数据仓库密钥'], ['unlocked', '修改或重设数据仓库密钥']] as const) {
      const info = { state, etag: state === 'new' ? null : '"etag"' };
      password.mockResolvedValueOnce(info).mockResolvedValueOnce({ state: 'unlocked', etag: null });
      button.click();
      await vi.waitFor(() => expect(root.querySelector('.settings-password-form h2')?.textContent).toBe(title));
      const form = root.querySelector<HTMLFormElement>('.settings-password-form')!;
      const [entry, confirm] = form.querySelectorAll('input');
      expect(confirm!.closest('label')!.hidden).toBe(state === 'locked');
      expect(entry!.hasAttribute('maxlength')).toBe(false);
      expect(confirm!.hasAttribute('maxlength')).toBe(false);
      entry!.value = 'a';
      if (state !== 'locked') {
        form.dispatchEvent(new Event('submit', { cancelable: true }));
        expect(form.querySelector('[role=alert]')?.textContent).toBe('两次输入的密钥不一致');
        confirm!.value = entry!.value;
      }
      form.dispatchEvent(new Event('submit', { cancelable: true }));
      await vi.waitFor(() => expect(root.querySelector('.settings-password-form')).toBeNull());
      expect(password).toHaveBeenLastCalledWith(expect.objectContaining({ prefix: profile.prefix }), undefined, { ...info, password: 'a' });
    }
  } finally { view.destroy(); root.remove(); }
});

it('starts and stops the desktop MCP service and copies its current authenticated config', async () => {
  const token = 'synthetic-token';
  const ipc = vi.mocked(invoke), implementation = ipc.getMockImplementation()!;
  let running = false, startFails = true, port = 49151;
  ipc.mockImplementation(async (command, args) => {
    if (command !== 'local_mcp') return implementation(command, args);
    const { enabled } = args as { enabled?: boolean };
    if (enabled && startFails) { startFails = false; throw '无法启动 MCP 服务'; }
    if (enabled) port++;
    running = enabled ?? running;
    return (running ? { url: `http://127.0.0.1:${port}/mcp`, token } : null) as never;
  });
  const writeText = vi.fn().mockResolvedValue(undefined);
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
  const client: SyncClient = { load: async () => config, save: vi.fn(), test: vi.fn(), flush: vi.fn(), run: vi.fn(), subscribe: () => () => {} };
  const root = document.body.appendChild(document.createElement('div'));
  const view = mountSettings(root, client, vi.fn());
  try {
    view.select('ai');
    const page = root.querySelector('#settings-panel-ai')!;
    const toggle = page.querySelector('button')!;
    const reveal = page.querySelector<HTMLButtonElement>('#settings-mcp-reveal')!;
    const copy = page.querySelector<HTMLButtonElement>('#settings-mcp-copy')!;
    const copyAi = page.querySelector<HTMLButtonElement>('#settings-mcp-copy-ai')!;
    const clientChoice = page.querySelector<HTMLSelectElement>('#settings-mcp-client')!;
    const preview = page.querySelector<HTMLTextAreaElement>('[aria-label="配置预览"]')!;
    const fallback = page.querySelector<HTMLTextAreaElement>('#settings-mcp-fallback')!;
    function choose(name: string) {
      clientChoice.value = name;
      clientChoice.dispatchEvent(new Event('change'));
    }
    await vi.waitFor(() => expect(toggle.disabled).toBe(false));
    expect(reveal.disabled).toBe(true);
    expect(copy.disabled).toBe(true);
    expect(copyAi.disabled).toBe(true);
    toggle.click();
    await vi.waitFor(() => expect(page.textContent).toContain('无法启动 MCP 服务'));
    expect(copy.disabled).toBe(true);
    toggle.click();
    await vi.waitFor(() => expect(toggle.textContent).toBe('停止服务'));
    expect(page.textContent).toContain('运行中 · http://127.0.0.1:49152/mcp');
    expect(page.textContent).toContain('点击「复制配置」获取含完整令牌的 TOML 配置');
    expect(preview.value).toContain('http://127.0.0.1:49152/mcp');
    expect(preview.value).not.toContain(token);
    copy.click();
    expect(writeText).toHaveBeenLastCalledWith(`[mcp_servers.qingyu]\nurl = "http://127.0.0.1:49152/mcp"\nhttp_headers = { Authorization = "Bearer ${token}" }`);
    reveal.click();
    expect(reveal.textContent).toBe('隐藏令牌');
    expect(preview.value).toBe(writeText.mock.lastCall![0]);
    reveal.click();
    expect(preview.value).not.toContain(token);
    reveal.click();
    choose('Claude Code');
    expect(JSON.parse(preview.value).mcpServers.qingyu).toEqual({
      type: 'http', url: 'http://127.0.0.1:49152/mcp', headers: { Authorization: 'Bearer <令牌已隐藏>' },
    });
    copy.click();
    const claudeConfig = writeText.mock.lastCall![0];
    expect(JSON.parse(claudeConfig).mcpServers.qingyu.headers.Authorization).toBe(`Bearer ${token}`);
    copyAi.click();
    expect(writeText.mock.lastCall![0]).toEqual(expect.stringContaining('~/.claude.json'));
    expect(writeText.mock.lastCall![0]).toEqual(expect.stringContaining(claudeConfig));
    choose('Cursor');
    writeText.mockRejectedValueOnce(new Error('Clipboard denied'));
    copy.click();
    await vi.waitFor(() => expect(fallback.hidden).toBe(false));
    expect(JSON.parse(fallback.value).mcpServers.qingyu).toEqual({
      url: 'http://127.0.0.1:49152/mcp', headers: { Authorization: `Bearer ${token}` },
    });
    toggle.click();
    await vi.waitFor(() => expect(page.textContent).toContain('已停止'));
    expect(copy.disabled).toBe(true);
    expect(copyAi.disabled).toBe(true);
    expect(fallback.value).toBe('');
    expect(preview.value).toBe('');
    toggle.click();
    await vi.waitFor(() => expect(page.textContent).toContain('运行中 · http://127.0.0.1:49153/mcp'));
    expect(preview.value).not.toContain(token);
    copy.click();
    expect(writeText).toHaveBeenLastCalledWith(expect.stringContaining('http://127.0.0.1:49153/mcp'));
  } finally {
    view.destroy(); root.remove(); ipc.mockImplementation(implementation);
    Reflect.deleteProperty(navigator, 'clipboard');
  }
});

it('refreshes MCP, shortcut and theme labels in the mounted settings view', async () => {
  const ipc = vi.mocked(invoke), previousInvoke = ipc.getMockImplementation()!;
  const previousLanguage = preferences.value.language;
  ipc.mockImplementation(async (command, args) => {
    if (command === 'local_mcp') return { url: 'http://127.0.0.1:49152/mcp', token: 'synthetic-token' } as never;
    if (command === 'activation_shortcut_command') {
      return { shortcut: 'Alt+Space', active: true, recording: false, error: '快捷键组合无效', defaultShortcut: '' } as never;
    }
    return previousInvoke(command, args);
  });
  const root = document.body.appendChild(document.createElement('div'));
  const view = mountSettings(root, undefined, vi.fn());
  const text = (selector: string) => root.querySelector(selector)?.textContent?.trim();
  try {
    view.select('ai');
    await vi.waitFor(() => expect(text('#settings-mcp-heading')).toBe('客户端配置'));
    await vi.waitFor(() => expect(text('.settings-shortcut-status')).toBe('快捷键组合无效'));
    expect(text('[data-color-theme=forest] > span:last-child')).toBe('森林');
    preferences.update({ language: 'fr' });
    expect(text('#settings-mcp-heading')).toBe('Configuration du client');
    expect(text('#settings-mcp-copy')).toBe('Copier la configuration');
    expect(text('.settings-mcp-address')).toContain('http://127.0.0.1:49152/mcp');
    expect(text('[data-color-theme=forest] > span:last-child')).toBe('Forêt');
    expect(text('[name=previewFont] option[value=ui-serif]')).toBe('Avec empattements');
    expect(text('#settings-panel-shortcuts [data-i18n="重命名"]')).toBe('Renommer');
    expect(text('.settings-shortcut button')).toContain('Espace');
    expect(text('.settings-shortcut-status')).toBe(workflowText('快捷键组合无效', 'fr'));
  } finally {
    view.destroy(); root.remove(); preferences.update({ language: previousLanguage }); ipc.mockImplementation(previousInvoke);
  }
});

it('shows the current notes directory, changes it immediately and keeps it on cancellation or error', async () => {
  vi.mocked(libraryLocation).mockResolvedValue('/Users/test/Documents/Notes');
  const client: SyncClient = { load: async () => config, save: vi.fn(), test: vi.fn(), flush: vi.fn(), run: vi.fn(), subscribe: fn => { fn({ busy: false, message: '', error: false }); return () => {}; } };
  const root = document.createElement('div'); document.body.append(root); const view = mountSettings(root, client, vi.fn());
  const general = root.querySelector('#settings-panel-general')!;
  expect([...general.querySelectorAll<HTMLInputElement | HTMLSelectElement>('[name=buttonDisplay], [name=alwaysOnTop], [name=previewWidth]')].map(control => control.name))
    .toEqual(['buttonDisplay', 'alwaysOnTop', 'previewWidth']);
  await vi.waitFor(() => expect(root.textContent).toContain('~/Documents/Notes'));
  expect(root.textContent).toContain('唤起快捷键');
  const button = root.querySelector<HTMLButtonElement>('[aria-label="更改笔记位置"]')!;
  vi.mocked(libraryLocation).mockResolvedValueOnce('/Users/test/Documents/New'); button.click();
  await vi.waitFor(() => expect(root.textContent).toContain('~/Documents/New')); expect(libraryLocation).toHaveBeenLastCalledWith(true);
  expect(root.querySelector('.settings-message')?.textContent).toBe('');
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
  const client: SyncClient = { load, save: vi.fn(), test: vi.fn(), flush: vi.fn(), run: vi.fn(), subscribe: () => () => {} };
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
  const client: SyncClient = { load: async () => profile, save, test: vi.fn(), flush: vi.fn(), run: vi.fn(), subscribe: () => () => {} };
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
    function select(value: string) {
      mode.value = value; mode.dispatchEvent(new Event('change'));
      root.querySelector<HTMLButtonElement>('.settings-provider-confirmation [data-continue]')!.click();
    }
    mode.value = 'cloud'; mode.dispatchEvent(new Event('change'));
    const dialog = root.querySelector<HTMLDialogElement>('.settings-provider-confirmation')!;
    expect(dialog.open).toBe(true); expect(dialog.querySelector('h2')?.textContent).toBe('切换云存储提供商');
    expect(dialog.querySelector('p')?.textContent).toBe('切换同步提供商前，需要检查资源和历史快照的完整性，必要时会从当前云端下载缺失内容，可能需要较长时间。是否继续？');
    expect(save).not.toHaveBeenCalled();
    dialog.querySelector<HTMLButtonElement>('[data-cancel]')!.click();
    await vi.waitFor(() => expect(mode.disabled).toBe(false));
    expect(mode.value).toBe('s3'); expect(save).not.toHaveBeenCalled();
    save.mockRejectedValueOnce(new Error('保存失败'));
    select('cloud');
    await vi.waitFor(() => expect(mode.disabled).toBe(false));
    expect(mode.value).toBe('s3'); expect(cloud.hidden).toBe(true);
    select('cloud');
    await vi.waitFor(() => expect(cloud.hidden).toBe(false));
    expect(save).toHaveBeenLastCalledWith({ ...profile, ...s3Defaults, mode: 'cloud' });
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
  const client: SyncClient = { load: async () => ({ ...config, mode: 'cloud' }), save: vi.fn(), test: vi.fn(), flush: vi.fn(), run, subscribe: () => () => {} };
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
    await vi.waitFor(() => expect(run).toHaveBeenCalledWith({ ...config, ...s3Defaults }, 'fixture-account'));
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
