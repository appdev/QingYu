// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';

const platform = vi.hoisted(() => ({ native: true, mobile: false }));
const handle = vi.hoisted(() => ({
  close: vi.fn(async () => {}),
  minimize: vi.fn(async () => {}),
  toggleMaximize: vi.fn(async () => {}),
  isMinimizable: vi.fn(async () => true),
  isMaximizable: vi.fn(async () => true),
  isMaximized: vi.fn(async () => false),
  onResized: vi.fn(async (_update: () => void) => () => {}),
}));
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => handle }));
vi.mock('../src/preferences', () => platform);
const showMenu = vi.hoisted(() => vi.fn());
vi.mock('../src/popup-menu', () => ({ showMenu }));

afterEach(() => {
  window.dispatchEvent(new Event('pagehide'));
  document.body.replaceChildren();
  document.title = '';
  vi.restoreAllMocks();
  vi.resetModules();
  vi.clearAllMocks();
});

it('uses the application name as the menu trigger and moves existing actions into the bar', async () => {
  vi.spyOn(navigator, 'platform', 'get').mockReturnValue('Win32');
  document.title = '轻羽';
  const today = document.body.appendChild(document.createElement('button'));
  const search = document.body.appendChild(document.createElement('button'));
  const { mountDesktopWindowBar } = await import('../src/desktop-window');
  const items = [{ text: '设置' }];
  mountDesktopWindowBar({ menu: () => items, leading: [today], trailing: [search] });
  await vi.waitFor(() => expect(handle.onResized).toHaveBeenCalledOnce());

  const trigger = document.querySelector<HTMLButtonElement>('.desktop-window-menu')!;
  expect(trigger.textContent).toBe('轻羽');
  expect(today.parentElement).toBe(trigger.nextElementSibling);
  expect(search.parentElement?.nextElementSibling?.className).toBe('desktop-window-caption');
  trigger.click();
  expect(showMenu).toHaveBeenCalledWith(items, expect.objectContaining({ anchor: trigger, presentation: 'dropdown' }));
});

it('uses the app bar for Windows and routes window buttons through Tauri', async () => {
  vi.spyOn(navigator, 'platform', 'get').mockReturnValue('Win32');
  const { mountDesktopWindowBar } = await import('../src/desktop-window');
  mountDesktopWindowBar();
  await vi.waitFor(() => expect(handle.onResized).toHaveBeenCalledOnce());

  const bar = document.querySelector('.desktop-window-bar')!;
  const [minimize, maximize, close] = bar.querySelectorAll('button');
  expect(bar.querySelector('[data-tauri-drag-region]')).not.toBeNull();
  minimize.click();
  maximize.click();
  close.click();
  expect(handle.minimize).toHaveBeenCalledOnce();
  expect(handle.toggleMaximize).toHaveBeenCalledOnce();
  expect(handle.close).toHaveBeenCalledOnce();
  expect(maximize.getAttribute('aria-pressed')).toBe('false');
  handle.isMaximized.mockResolvedValueOnce(true);
  handle.onResized.mock.calls[0][0]();
  await vi.waitFor(() => expect(maximize.getAttribute('aria-pressed')).toBe('true'));
});

it('does not add desktop controls to a macOS app window', async () => {
  vi.spyOn(navigator, 'platform', 'get').mockReturnValue('MacIntel');
  const { mountDesktopWindowBar } = await import('../src/desktop-window');
  mountDesktopWindowBar();
  expect(document.querySelector('.desktop-window-bar')).toBeNull();
});
