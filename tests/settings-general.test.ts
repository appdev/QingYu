// @vitest-environment jsdom
import { expect, it, vi } from 'vitest';
import { mountSettings } from '../src/settings-view';
import { libraryLocation } from '../src/library-settings';
import type { SyncClient } from '../src/sync-session';
vi.hoisted(() => { Object.assign(window, { __TAURI_INTERNALS__: {} }); vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {} })); });
vi.mock('../src/library-settings', () => ({ libraryLocation: vi.fn() }));
vi.mock('@tauri-apps/api/path', () => ({ homeDir: vi.fn().mockResolvedValue('/Users/test') }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async (command: string) => {
  if (command === 'system_fonts') return ['Menlo'];
  if (command.startsWith('plugin:path|')) return '/Users/test';
  return { shortcut: 'Alt+Super+KeyM', active: true, recording: false, error: null, defaultShortcut: 'Alt+Super+KeyM' };
}) }));
it('shows the current notes directory, changes it immediately and keeps it on cancellation or error', async () => {
  vi.mocked(libraryLocation).mockResolvedValue('/Users/test/Documents/Notes');
  const config = { endpoint: '', bucket: '', region: 'us-east-1', prefix: 'test', accessKeyId: '', secretAccessKey: '' };
  const client: SyncClient = { load: async () => config, save: vi.fn(), flush: vi.fn(), run: vi.fn(), subscribe: fn => { fn({ busy: false, message: '', error: false }); return () => {}; } };
  const root = document.createElement('div'); document.body.append(root); const view = mountSettings(root, client, vi.fn());
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
