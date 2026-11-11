// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { createShortcutRecorder, shortcutFromEvent, shortcutLabel, type ShortcutStatus } from '../src/shortcut-recorder';
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
afterEach(() => { document.body.replaceChildren(); vi.resetAllMocks(); });

it('records physical keys with modifiers and formats platform labels', () => {
  expect(shortcutFromEvent(new KeyboardEvent('keydown', { code: 'KeyM', altKey: true, metaKey: true }))).toBe('Alt+Super+KeyM');
  expect(shortcutFromEvent(new KeyboardEvent('keydown', { code: 'Digit1', ctrlKey: true, shiftKey: true }))).toBe('Control+Shift+Digit1');
  for (const extra of [{}, { shiftKey: true }, { ctrlKey: true, isComposing: true }, { ctrlKey: true, repeat: true }, { ctrlKey: true, code: 'ControlLeft' }]) expect(shortcutFromEvent(new KeyboardEvent('keydown', { code: 'KeyA', ...extra }))).toBeNull();
  expect(shortcutLabel('Alt+Super+KeyM', true)).toBe('⌥⌘M');
  expect(shortcutLabel('Control+Alt+Digit1', false)).toBe('Ctrl+Alt+1');
});

it('suspends capture, cancels on Escape, persists a new key, and clears the binding', async () => {
  let current: ShortcutStatus = { shortcut: 'Alt+Super+KeyM', active: true, recording: false, error: null, defaultShortcut: 'Alt+Super+KeyM' };
  vi.mocked(invoke).mockImplementation(async (_, args) => {
    const { action, shortcut } = args as { action: string; shortcut?: string };
    if (action === 'record') current = { ...current, active: false, recording: true };
    if (action === 'cancel') current = { ...current, active: !!current.shortcut, recording: false };
    if (action === 'set') current = { ...current, shortcut: shortcut!, active: !!shortcut, recording: false };
    return current;
  });
  const error = vi.fn(), view = createShortcutRecorder(error); document.body.append(view.control); await view.loaded;
  const button = view.control.querySelector<HTMLButtonElement>('button')!;
  button.click(); await vi.waitFor(() => expect(view.control.dataset.recording).toBe('true'));
  button.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', code: 'Escape', bubbles: true }));
  await vi.waitFor(() => expect(current.active).toBe(true)); expect(current.shortcut).toBe('Alt+Super+KeyM');
  button.click(); await vi.waitFor(() => expect(current.recording).toBe(true));
  button.dispatchEvent(new KeyboardEvent('keydown', { key: 'j', code: 'KeyJ', ctrlKey: true, altKey: true, bubbles: true }));
  await vi.waitFor(() => expect(current.shortcut).toBe('Control+Alt+KeyJ')); expect(current.active).toBe(true);
  view.control.querySelector<HTMLButtonElement>('.shortcut-clear')!.click();
  await vi.waitFor(() => expect(current.shortcut).toBe('')); expect(current.active).toBe(false); expect(button.textContent).toBe('设置快捷键');
  view.destroy();
});

it('shows a registration failure while restoring the previously registered shortcut', async () => {
  const status: ShortcutStatus = { shortcut: 'Control+Alt+KeyJ', active: true, recording: false, error: null, defaultShortcut: 'Control+Alt+KeyM' };
  vi.mocked(invoke).mockImplementation(async (_, args) => {
    if ((args as { action: string }).action === 'set') throw new Error('快捷键已被其他应用占用');
    if ((args as { action: string }).action === 'record') return { ...status, recording: true, active: false };
    return status;
  });
  const error = vi.fn(), view = createShortcutRecorder(error); document.body.append(view.control); await view.loaded;
  const button = view.control.querySelector<HTMLButtonElement>('button')!; button.click();
  await vi.waitFor(() => expect(view.control.dataset.recording).toBe('true'));
  button.dispatchEvent(new KeyboardEvent('keydown', { key: 'k', code: 'KeyK', ctrlKey: true, altKey: true, bubbles: true }));
  await vi.waitFor(() => expect(error).toHaveBeenCalledWith('快捷键已被其他应用占用'));
  await vi.waitFor(() => expect(view.control.dataset.recording).toBe('false'));
  expect(button.textContent).toBe(shortcutLabel(status.shortcut)); view.destroy();
});
