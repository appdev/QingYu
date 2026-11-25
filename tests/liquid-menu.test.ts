// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { MenuItem } from '../src/popup-menu';
import { shortcutText, showLiquidMenu, type LiquidMenuRequest } from '../src/liquid-menu';

beforeEach(() => {
  // 减少动态效果时弹簧直接落定，开合不用等动画。
  vi.stubGlobal('matchMedia', (query: string) => ({ matches: query.includes('reduce'), addEventListener() {}, removeEventListener() {} }));
  vi.stubGlobal('innerWidth', 1280);
});
afterEach(() => { document.body.replaceChildren(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });

function open(items: MenuItem[], extra: Partial<LiquidMenuRequest> = {}) {
  const anchor = document.body.appendChild(document.createElement('button'));
  anchor.getBoundingClientRect = () => new DOMRect(1200, 10, 32, 32);
  // 和 popup-menu 一样：选中或点外时由调用方关菜单。
  let handle: { close(): void } | undefined;
  const request: LiquidMenuRequest = {
    items, label: '列表选项', anchor, keyboard: false, iconFor: () => undefined,
    select: vi.fn(() => handle?.close()), dismiss: vi.fn(() => handle?.close()), ...extra,
  };
  const menu = handle = showLiquidMenu(request);
  return { request, menu, anchor };
}
const key = (key: string) => document.activeElement!.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
const menus = () => [...document.querySelectorAll<HTMLElement>('.qlg-menu-surface [role=menu]')];

it('draws groups, checks, shortcuts and destructive rows, and focuses the menu itself on pointer open', () => {
  vi.spyOn(navigator, 'platform', 'get').mockReturnValue('MacIntel');
  open([
    { text: '' },
    { text: '置顶', shortcut: '⇧⌘P' },
    { text: '' }, { text: '' },
    { text: '修改日期', checked: true },
    { text: '标题', checked: false },
    { text: '' },
    { text: '移到废纸篓', destructive: true },
    { text: '' },
  ]);
  const [menu] = menus();
  expect([...menu.children].map(node => node.role)).toEqual(['menuitem', 'separator', 'menuitemradio', 'menuitemradio', 'separator', 'menuitem']);
  expect(menu.querySelector('.qlg-menu__shortcut')?.textContent).toBe('⇧⌘P');
  expect([...menu.querySelectorAll('[role=menuitemradio]')].map(row => row.getAttribute('aria-checked'))).toEqual(['true', 'false']);
  expect(menu.querySelectorAll('.qlg-menu__check')).toHaveLength(1);
  expect(menu.querySelector('.qlg-menu__check svg')?.getAttribute('width')).toBe('16');
  expect(menu.lastElementChild!.hasAttribute('data-destructive')).toBe(true);
  expect(document.activeElement).toBe(menu);
  // 每次改形状都让背景模糊重新生成，修 Android WebView 停在水滴形状的裁剪。
  const panel = document.querySelector<HTMLElement>('.qlg-menu')!;
  expect(['', '12.001px']).toContain(panel.style.getPropertyValue('--qlg-glass-blur'));
});

it('runs the chosen item once from the keyboard, including the click the browser follows up with', () => {
  const pin = { text: '置顶' }, copy = { text: '复制笔记' };
  const { request } = open([pin, copy], { keyboard: true });
  expect(document.activeElement?.textContent).toBe('置顶');
  key('ArrowDown');
  expect(document.activeElement?.textContent).toBe('复制笔记');
  const row = document.activeElement as HTMLButtonElement;
  key('Enter');
  // close() 之后浏览器补发的键盘点击不能再执行一次。
  row.click();
  expect(request.select).toHaveBeenCalledOnce();
  expect(request.select).toHaveBeenCalledWith(copy);
});

it('opens a submenu over its row, folds it back, and runs items inside it', () => {
  const title = { text: '标题', checked: false };
  const { request } = open([
    { text: '排序方式', items: [{ text: '修改日期', checked: true }, title, { text: '' }, { text: '最新在前', checked: true }] },
    { text: '新建文件夹' },
  ], { keyboard: true });
  const row = document.activeElement as HTMLButtonElement;
  expect(row.getAttribute('aria-haspopup')).toBe('menu');

  key('ArrowRight');
  expect(menus()).toHaveLength(2);
  const child = menus()[1];
  expect(child.getAttribute('aria-label')).toBe('排序方式');
  expect(child.firstElementChild!.hasAttribute('data-header')).toBe(true);
  expect([...child.querySelectorAll('[role=separator]')]).toHaveLength(2);
  expect(row.getAttribute('aria-expanded')).toBe('true');
  expect(document.querySelector('.qlg-menu-surface')!.hasAttribute('data-covered')).toBe(true);
  expect(document.activeElement?.textContent).toBe('修改日期');

  key('Escape');
  expect(menus()).toHaveLength(1);
  expect(row.getAttribute('aria-expanded')).toBe('false');
  expect(document.activeElement).toBe(row);
  expect(request.dismiss).not.toHaveBeenCalled();

  key('Enter');
  key('ArrowDown');
  key('Enter');
  expect(request.select).toHaveBeenCalledWith(title);
  expect(document.querySelectorAll('.qlg-menu-surface')).toHaveLength(0);
});

it('closes a submenu first and the whole menu on the next Escape', () => {
  const { request } = open([{ text: '排序方式', items: [{ text: '修改日期' }] }], { keyboard: true });
  key('ArrowRight');
  key('Escape');
  expect(request.dismiss).not.toHaveBeenCalled();
  key('Escape');
  expect(request.dismiss).toHaveBeenCalledOnce();
  expect(document.querySelectorAll('.qlg-menu-surface')).toHaveLength(0);
});

it('collapses the submenu from its header row and jumps by typed letters', () => {
  open([
    { text: '排序方式', items: [{ text: '修改日期' }] },
    { text: '按日期分组' },
    { text: '新建文件夹' },
  ], { keyboard: true });
  key('x');
  expect(document.activeElement?.textContent).toBe('排序方式');
  key('新');
  expect(document.activeElement?.textContent).toBe('新建文件夹');
  key('Home');
  key('Enter');
  const header = menus()[1].querySelector<HTMLButtonElement>('[data-header]')!;
  header.focus();
  key('Enter');
  expect(menus()).toHaveLength(1);
});

it('removes every layer and restores the button once closed', () => {
  const { menu, anchor } = open([{ text: '排序方式', items: [{ text: '修改日期' }] }], { keyboard: true });
  key('ArrowRight');
  expect(document.querySelectorAll('.qlg-menu-surface')).toHaveLength(2);
  menu.close();
  expect(document.querySelectorAll('.qlg-menu-surface')).toHaveLength(0);
  expect(anchor.style.opacity).toBe('');
});

it('sizes the menu for phones, tablets and desktops', () => {
  // 360px 宽的手机按 402px 基准缩放：宽 250、行高 42、字号 17 都乘 360 / 402。
  vi.stubGlobal('innerWidth', 360);
  open([{ text: '置顶' }]);
  const phone = document.querySelector<HTMLElement>('.qlg-menu-surface')!;
  const k = 360 / 402;
  expect(Number.parseFloat(phone.style.width)).toBeCloseTo(250 * k, 3);
  expect(Number.parseFloat(phone.style.getPropertyValue('--qlg-menu-row'))).toBeCloseTo(42 * k, 3);
  expect(Number.parseFloat(phone.style.getPropertyValue('--qlg-menu-font'))).toBeCloseTo(17 * k, 3);
  document.body.replaceChildren();

  vi.stubGlobal('innerWidth', 800);
  open([{ text: '置顶' }]);
  const tablet = document.querySelector<HTMLElement>('.qlg-menu-surface')!;
  expect(tablet.style.width).toBe('236px');
  expect(tablet.style.getPropertyValue('--qlg-menu-row')).toBe('38px');
  expect(tablet.style.getPropertyValue('--qlg-menu-font')).toBe('16px');
  document.body.replaceChildren();

  vi.stubGlobal('innerWidth', 1280);
  open([{ text: '置顶' }]);
  const desktop = document.querySelector<HTMLElement>('.qlg-menu-surface')!;
  expect(desktop.style.width).toBe('218px');
  expect(desktop.style.getPropertyValue('--qlg-menu-row')).toBe('24px');
  expect(desktop.style.getPropertyValue('--qlg-menu-font')).toBe('13px');
});

it('spells shortcuts the Windows and Linux way off Apple platforms', () => {
  vi.spyOn(navigator, 'platform', 'get').mockReturnValue('Win32');
  expect(shortcutText('⇧⌘P')).toBe('Ctrl+Shift+P');
  expect(shortcutText('⌥⌘R')).toBe('Ctrl+Alt+R');
  expect(shortcutText('⇧⌘⌫')).toBe('Ctrl+Shift+Backspace');
  expect(shortcutText('⇧F6')).toBe('Shift+F6');
});

it('holds the desktop hover back until the mouse actually moves', () => {
  // 点开按钮的那一下记下鼠标的位置。
  document.dispatchEvent(new MouseEvent('pointerdown', { clientX: 1210, clientY: 20 }));
  open([{ text: '置顶' }, { text: '复制笔记' }]);
  const surface = document.querySelector<HTMLElement>('.qlg-menu-surface')!;
  expect(surface.hasAttribute('data-desktop')).toBe(true);
  expect(surface.hasAttribute('data-idle')).toBe(true);
  // jsdom 没有 PointerEvent，用 MouseEvent 补上 pointerType。
  const move = (x: number, y: number) => window.dispatchEvent(Object.assign(new MouseEvent('pointermove', { clientX: x, clientY: y }), { pointerType: 'mouse' }));
  // 菜单在静止的鼠标下长出来，浏览器补发的、离点开位置不到 2px 的 pointermove 不算数。
  move(1211, 20);
  expect(surface.hasAttribute('data-idle')).toBe(true);
  move(1210, 40);
  expect(surface.hasAttribute('data-idle')).toBe(false);
});

it.each([360, 800, 1280])('keeps touch interaction at %d px on mobile', width => {
  const root = document.documentElement;
  root.setAttribute('data-mobile', '');
  try {
    vi.stubGlobal('innerWidth', width);
    const { menu } = open([{ text: '置顶' }]);
    const surface = document.querySelector<HTMLElement>('.qlg-menu-surface')!;
    expect(surface.hasAttribute('data-desktop')).toBe(false);
    expect(surface.hasAttribute('data-idle')).toBe(false);
    expect(surface.hasAttribute('data-qlg-press')).toBe(true);
    menu.close();
  } finally {
    root.removeAttribute('data-mobile');
  }
});

it('uses the lite menu on weak devices and on devices that dropped frames before', () => {
  vi.stubGlobal('innerWidth', 360);
  vi.spyOn(navigator, 'hardwareConcurrency', 'get').mockReturnValue(4);
  open([{ text: '置顶' }]);
  const weak = document.querySelector<HTMLElement>('.qlg-menu-surface')!;
  expect(weak.hasAttribute('data-lite')).toBe(true);
  // 减少动态效果时直接落定，换上半透明的玻璃。
  expect(weak.hasAttribute('data-landed')).toBe(true);
  expect(weak.querySelector<HTMLElement>('.qlg-menu__content')!.style.filter).toBe('none');
  document.body.replaceChildren();

  vi.spyOn(navigator, 'hardwareConcurrency', 'get').mockReturnValue(8);
  localStorage.removeItem('qingyu.menu.lite');
  open([{ text: '置顶' }]);
  expect(document.querySelector('.qlg-menu-surface')!.hasAttribute('data-lite')).toBe(false);
  document.body.replaceChildren();

  localStorage.setItem('qingyu.menu.lite', '1');
  open([{ text: '置顶' }]);
  expect(document.querySelector('.qlg-menu-surface')!.hasAttribute('data-lite')).toBe(true);
  localStorage.removeItem('qingyu.menu.lite');
});
