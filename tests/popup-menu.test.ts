// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';

const native = vi.hoisted(() => ({ popup: vi.fn(), close: vi.fn(async () => undefined), created: vi.fn() }));
vi.mock('@tauri-apps/api/menu', () => ({
  Menu: { new: vi.fn(async (options: unknown) => { native.created(options); return { popup: native.popup, close: native.close }; }) },
}));

const mobile = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mobile.invoke }));

import { backPopupMenu, dismissPopupMenu, showMenu } from '../src/popup-menu';

afterEach(() => {
  dismissPopupMenu();
  document.body.replaceChildren();
  document.documentElement.removeAttribute('data-web');
  document.documentElement.removeAttribute('data-mobile');
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  vi.clearAllMocks();
});

const webMenuEnvironment = () => {
  document.documentElement.setAttribute('data-web', '');
};

function nativeApp(platform: 'MacIntel' | 'iPhone' | 'Linux armv8l' | 'Win32') {
  vi.stubGlobal('__TAURI_INTERNALS__', {});
  vi.spyOn(navigator, 'platform', 'get').mockReturnValue(platform);
}
function toolbarButton() {
  const button = document.body.appendChild(document.createElement('button'));
  button.ariaLabel = '更多';
  button.getBoundingClientRect = () => new DOMRect(120, 10, 32, 28);
  return button;
}

it('opens toolbar menus as native menus below the button in the desktop App', async () => {
  nativeApp('MacIntel');
  const anchor = toolbarButton();
  let expandedWhileOpen: string | null = null;
  native.popup.mockImplementation(async () => { expandedWhileOpen = anchor.getAttribute('aria-expanded'); });
  const run = vi.fn();

  await showMenu([{ text: '排序方式', items: [{ text: '标题', checked: true, run }] }], { anchor });

  expect(native.popup).toHaveBeenCalledOnce();
  expect(native.popup.mock.calls[0][0]).toMatchObject({ x: 120, y: 42 });
  expect(expandedWhileOpen).toBe('true');
  expect(anchor.getAttribute('aria-expanded')).toBe('false');
  expect(document.querySelector('[role=menu]')).toBeNull();
  expect(native.created).toHaveBeenCalledWith({ items: [expect.objectContaining({ text: '排序方式', items: [expect.objectContaining({ text: '标题', checked: true })] })] });
});

it('keeps menus that need user activation in the page', async () => {
  nativeApp('MacIntel');
  const anchor = toolbarButton();

  await showMenu([{ text: '添加文件…', needsUserActivation: true, run: vi.fn() }], { anchor });

  expect(native.popup).not.toHaveBeenCalled();
  expect(document.querySelector('.toolbar-context-menu')).not.toBeNull();
  expect(anchor.getAttribute('aria-expanded')).toBe('true');
});

it('opens Liquid Glass menus with separators and runs the chosen item in the web build', async () => {
  vi.stubGlobal('__TAURI_INTERNALS__', {});
  webMenuEnvironment();
  const anchor = toolbarButton();
  const pin = vi.fn(), trash = vi.fn();

  await showMenu([{ text: '置顶', run: pin }, { text: '' }, { text: '移到废纸篓', destructive: true, run: trash }], { anchor });

  expect(native.popup).not.toHaveBeenCalled();
  expect(document.querySelector('.toolbar-context-menu')).toBeNull();
  const menu = await vi.waitFor(() => {
    const found = document.querySelector<HTMLElement>('[role=menu]');
    expect(found).not.toBeNull();
    return found!;
  });
  const items = [...menu.querySelectorAll<HTMLElement>('[role=menuitem]')];
  expect(menu.closest('.qlg-menu-surface')?.querySelector('.qlg-menu.qlg-glass')).not.toBeNull();
  expect(items.map(item => item.textContent)).toEqual(['置顶', '移到废纸篓']);
  expect(items[1].hasAttribute('data-destructive')).toBe(true);
  expect(menu.querySelectorAll('[role=separator]')).toHaveLength(1);
  expect(anchor.getAttribute('aria-expanded')).toBe('true');

  // A pointerdown on a row must leave the menu open until the click.
  items[1].dispatchEvent(new Event('pointerdown', { bubbles: true }));
  await new Promise(resolve => setTimeout(resolve));
  expect(menu.isConnected).toBe(true);
  items[1].click();
  await vi.waitFor(() => expect(trash).toHaveBeenCalledOnce());
  expect(pin).not.toHaveBeenCalled();
  expect(anchor.getAttribute('aria-expanded')).toBe('false');
});

it('opens web submenus from the keyboard and restores focus through both levels', async () => {
  webMenuEnvironment();
  const anchor = toolbarButton();
  await showMenu([{ text: '停用', enabled: false }, { text: '排序', items: [{ text: '标题' }] }], { anchor, keyboard: true });
  const parent = document.activeElement as HTMLButtonElement;
  expect(parent.getAttribute('aria-label')).toBe('排序');
  const key = (value: string) => document.activeElement!.dispatchEvent(new KeyboardEvent('keydown', { key: value, bubbles: true, cancelable: true }));
  key('ArrowRight');
  expect(document.activeElement?.getAttribute('aria-label')).toBe('标题');
  key('Escape');
  expect(document.activeElement).toBe(parent);
  key('Escape');
  expect(document.activeElement).toBe(anchor);
  expect(anchor.getAttribute('aria-expanded')).toBe('false');
});

it('marks checked web menu items as a single-choice group and still runs them', async () => {
  webMenuEnvironment();
  const anchor = toolbarButton();
  const created = vi.fn(), title = vi.fn();

  await showMenu([{ text: '修改日期', checked: true, run: vi.fn() }, { text: '创建日期', checked: false, run: created }, { text: '标题', checked: false, run: title }], { anchor });

  const radios = await vi.waitFor(() => {
    const found = [...document.querySelectorAll<HTMLElement>('[role=menuitemradio]')];
    expect(found).toHaveLength(3);
    return found;
  });
  expect(radios.map(item => item.getAttribute('aria-checked'))).toEqual(['true', 'false', 'false']);
  radios[2].click();
  await vi.waitFor(() => expect(title).toHaveBeenCalledOnce());
  expect(created).not.toHaveBeenCalled();
});

it('opens toolbar menus as system menus in the mobile App and runs the chosen item', async () => {
  nativeApp('iPhone');
  document.documentElement.setAttribute('data-mobile', '');
  const anchor = toolbarButton();
  const pin = vi.fn(), title = vi.fn(), trash = vi.fn();
  let expandedWhileOpen: string | null = null;
  mobile.invoke.mockImplementation(async () => { expandedWhileOpen = anchor.getAttribute('aria-expanded'); return { id: 3 }; });

  await showMenu([
    { text: '置顶', run: pin },
    { text: '排序方式', items: [{ text: '标题', checked: true, run: title }] },
    { text: '' },
    { text: '移到废纸篓', destructive: true, run: trash },
  ], { anchor });

  expect(mobile.invoke).toHaveBeenCalledWith('show_native_menu', { menu: {
    frame: [120, 10, 32, 28],
    sections: [[
      { id: 1, text: '置顶', enabled: true, destructive: false, checked: null },
      { id: 2, text: '排序方式', enabled: true, destructive: false, checked: null, children: [[
        { id: 3, text: '标题', enabled: true, destructive: false, checked: true },
      ]] },
    ], [{ id: 4, text: '移到废纸篓', enabled: true, destructive: true, checked: null }]],
  } });
  expect(expandedWhileOpen).toBe('true');
  expect(anchor.getAttribute('aria-expanded')).toBe('false');
  await vi.waitFor(() => expect(title).toHaveBeenCalledOnce());
  expect(pin).not.toHaveBeenCalled();
  expect(document.querySelector('[role=menu]')).toBeNull();
});

it('falls back to the page menu when the mobile system menu is unavailable', async () => {
  nativeApp('iPhone');
  document.documentElement.setAttribute('data-mobile', '');
  const anchor = toolbarButton();
  mobile.invoke.mockRejectedValue(new Error('系统菜单不可用'));

  await showMenu([{ text: '标题', run: vi.fn() }], { anchor });

  expect(document.querySelector('.toolbar-context-menu')).not.toBeNull();
  expect(anchor.getAttribute('aria-expanded')).toBe('true');
});

it('keeps keyboard-bar menus in the page on mobile', async () => {
  nativeApp('iPhone');
  document.documentElement.setAttribute('data-mobile', '');
  const bar = document.body.appendChild(document.createElement('div'));
  bar.id = 'mobile-tools';
  const anchor = bar.appendChild(toolbarButton());

  await showMenu([{ text: '重做', run: vi.fn() }], { anchor });

  expect(mobile.invoke).not.toHaveBeenCalled();
  expect(document.querySelector('.toolbar-context-menu')).not.toBeNull();
});


it.each(['Win32', 'Linux armv8l'] as const)('uses the Liquid Glass menu instead of native menus on %s', async platform => {
  nativeApp(platform);
  document.documentElement.toggleAttribute('data-mobile', platform === 'Linux armv8l');
  const anchor = toolbarButton();
  await showMenu([{ text: '置顶' }], { anchor });
  expect(document.querySelector('.qlg-menu-surface .qlg-menu.qlg-glass')).not.toBeNull();
  expect(native.popup).not.toHaveBeenCalled();
  expect(mobile.invoke).not.toHaveBeenCalled();
});

it.each([402, 800, 1280])('uses Android back to close one menu level at a time at %d px', async width => {
  nativeApp('Linux armv8l');
  document.documentElement.setAttribute('data-mobile', '');
  vi.stubGlobal('innerWidth', width);
  const anchor = toolbarButton();
  const restoreFocus = vi.fn(() => anchor.focus());
  await showMenu([{ text: '排序方式', items: [{ text: '标题' }] }], { at: { x: 30, y: 90 }, keyboard: true, restoreFocus });
  const row = document.activeElement as HTMLButtonElement;
  expect(row.ariaLabel).toBe('排序方式');
  row.click();
  expect(document.activeElement?.getAttribute('aria-label')).toBe('标题');
  expect(backPopupMenu()).toBe(true);
  expect(document.activeElement).toBe(row);
  expect(restoreFocus).not.toHaveBeenCalled();
  expect(backPopupMenu()).toBe(true);
  expect(document.activeElement).toBe(anchor);
  expect(backPopupMenu()).toBe(false);
  expect(mobile.invoke).not.toHaveBeenCalled();
});
