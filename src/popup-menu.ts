import { showContextSheet } from './context-sheet';
import { clamp, getSurface, glass, mix, Spring, type GlassSurface } from 'glassy-liquid';
import { toolbarMenuGlass } from './toolbar-menu-glass';
import { ChevronDown, ChevronRight } from 'lucide';
import { createLucideIcon } from './vendor/markra/shared/lucide';

export type MenuItem = { text: string; icon?: SVGElement; enabled?: boolean; destructive?: boolean; checked?: boolean; shortcut?: string; nativeRole?: 'Cut' | 'Copy' | 'Paste'; needsUserActivation?: boolean; run?: () => void | Promise<void>; items?: MenuItem[] };

const actionIcons: Record<string, string> = {
  分享: 'M12 15V3m-4 4 4-4 4 4M8 9H5v12h14V9h-3',
  移动: 'M3 6h7l2 2h9v12H3z',
  删除: 'M3 6h18M9 6V3h6v3M6 6l1 15h10l1-15m-8 4v7m4-7v7',
  置顶: 'M8 3h8l-1 6 3 4v2H6v-2l3-4zm4 12v7',
  取消置顶: 'M4 4l16 16M9 3h7l-1 6 3 4M6 13v2h7m-1 0v7',
  '锁定笔记…': 'M7 10V7a5 5 0 0 1 10 0v3M5 10h14v11H5z',
  复制笔记: 'M8 16H3V3h13v5M8 8h13v13H8zM15 11v7m-4-3h7',
  新建笔记: 'M12 4H4v16h16v-8m-10 2 1-4 8-8 3 3-8 8z',
  新建文件夹: 'M3 6h7l2 2h9v12H3zm9 4v7m-4-3h8',
  重命名文件夹: 'm4 20 1-5L17 3l4 4L9 19z',
  剪切: 'M6 6l12 12M6 18l12-12M8 6a2 2 0 1 1-4 0 2 2 0 1 1 4 0Zm0 12a2 2 0 1 1-4 0 2 2 0 1 1 4 0Z',
  复制: 'M8 16H3V3h13v5M8 8h13v13H8z',
  粘贴: 'M9 4H5v17h14V4h-4M9 3h6v4H9z',
  在下方插入行: 'M12 4v16m-5-5 5 5 5-5',
  在上方插入行: 'M12 20V4m-5 5 5-5 5 5',
  在右侧插入列: 'M4 12h16m-5-5 5 5-5 5',
  在左侧插入列: 'M20 12H4m5-5-5 5 5 5',
};
for (const label of ['删除行', '删除列', '删除表格']) actionIcons[label] = actionIcons.删除;

let native: import('@tauri-apps/api/menu').Menu | undefined;
let fallback: HTMLElement | undefined, generation = 0;
let fallbackAnchor: HTMLButtonElement | undefined;
let toolbarBackdrop: HTMLElement | undefined;
let closeToolbarMenu: ((remove: () => void) => void) | undefined;
let finishClosingToolbarMenu: (() => void) | undefined;
let closeGlassyMenu: (() => void) | undefined;
const glassyToolbarPreview = new URLSearchParams(location.search).get('menu') === 'glassy';
export const popupMenuOpen = () => Boolean(fallback);

function animateToolbarMenu(menu: HTMLElement, anchor: HTMLButtonElement, surface: GlassSurface) {
  finishClosingToolbarMenu?.();
  if (typeof matchMedia !== 'function' || matchMedia('(prefers-reduced-motion: reduce)').matches) return;
  const content = menu.querySelector<HTMLElement>('.toolbar-menu-items')!;
  const mobileToolbar = anchor.closest<HTMLElement>('#app-shell[data-mobile-page]:not([data-mobile-wide]) .actions');
  const panel = menu.getBoundingClientRect();
  const button = anchor.getBoundingClientRect();
  if (!panel.width || !panel.height || !button.width || !button.height) return;
  const { left, top } = menu.style;
  const panelRadius = surface.metrics.radius;
  const buttonRadius = Math.min(button.width, button.height) / 2;
  const progress = new Spring();
  const face = document.createElement('span');
  face.className = 'toolbar-menu-trigger-face';
  face.ariaHidden = 'true';
  face.style.width = `${button.width}px`;
  face.style.height = `${button.height}px`;
  const icon = anchor.querySelector('svg');
  if (icon) {
    const clone = icon.cloneNode(true) as SVGElement;
    const bounds = icon.getBoundingClientRect();
    clone.style.width = `${bounds.width}px`;
    clone.style.height = `${bounds.height}px`;
    face.append(clone);
  }
  let frame = 0;

  function paint() {
    const value = Math.max(0, progress.value);
    const width = mix(button.width, panel.width, value);
    const height = mix(button.height, panel.height, value);
    const radius = mix(buttonRadius, panelRadius, Math.min(1, value));
    menu.style.left = `${mix(button.right, panel.right, value) - width}px`;
    menu.style.top = `${mix(button.top, panel.top, value)}px`;
    menu.style.width = `${width}px`;
    menu.style.height = `${height}px`;
    menu.style.borderRadius = `${radius}px`;
    surface.setGeometry({ width, height }, radius);
    const glassOpacity = mobileToolbar ? clamp(value / .3, 0, 1) : 1;
    if (mobileToolbar) {
      mobileToolbar.style.setProperty('--toolbar-return-opacity', String(1 - glassOpacity));
      menu.style.setProperty('--toolbar-morph-opacity', `${glassOpacity * 100}%`);
      surface.setOpacity(glassOpacity);
    }
    face.style.opacity = String(clamp(1 - value * 3.2, 0, 1) * glassOpacity);
    content.style.opacity = String(clamp((value - .35) / .5, 0, 1));
  }

  function animate(target: 0 | 1, done: () => void) {
    cancelAnimationFrame(frame);
    menu.dataset.toolbarMorph = '';
    content.style.width = `${panel.width}px`;
    content.style.maxHeight = `${panel.height}px`;
    menu.append(face);
    const opening = target === 1;
    progress.to(target, opening ? { duration: .30, bounce: .25 } : { duration: .23, bounce: 0 });
    let last = performance.now();
    const deadline = last + (opening ? 380 : 300);
    const tick = (now: number) => {
      progress.step((now - last) / 1000);
      last = now;
      if (progress.settled || now >= deadline) {
        progress.set(target);
        done();
      } else {
        paint();
        frame = requestAnimationFrame(tick);
      }
    };
    paint();
    frame = requestAnimationFrame(tick);
  }

  animate(1, () => {
    face.remove();
    delete menu.dataset.toolbarMorph;
    mobileToolbar?.style.removeProperty('--toolbar-return-opacity');
    menu.style.removeProperty('--toolbar-morph-opacity');
    surface.setOpacity(1);
    Object.assign(menu.style, { left, top, width: '', height: '', borderRadius: '' });
    Object.assign(content.style, { width: '', maxHeight: '', opacity: '' });
    surface.setGeometry(null);
  });
  return (remove: () => void) => {
    finishClosingToolbarMenu = () => {
      cancelAnimationFrame(frame);
      finishClosingToolbarMenu = undefined;
      mobileToolbar?.style.removeProperty('--toolbar-return-opacity');
      remove();
    };
    animate(0, finishClosingToolbarMenu);
  };
}

export function dismissPopupMenu() {
  const menu = fallback, anchor = fallbackAnchor;
  const closePreview = closeGlassyMenu;
  const closeToolbar = closeToolbarMenu;
  closeGlassyMenu = undefined;
  closeToolbarMenu = undefined;
  fallback = fallbackAnchor = undefined; generation++;
  toolbarBackdrop?.remove();
  toolbarBackdrop = undefined;
  anchor?.setAttribute('aria-expanded', 'false');
  if (closePreview) { closePreview(); return; }
  if (!menu) return;
  const remove = () => {
    for (const submenu of menu.querySelectorAll<HTMLElement>('.note-context-menu')) getSurface(submenu)?.destroy();
    getSurface(menu)?.destroy(); menu.remove();
  };
  if (!closeToolbar) { remove(); return; }
  menu.inert = true;
  menu.style.pointerEvents = 'none';
  closeToolbar(remove);
}
document.addEventListener('pointerdown', event => {
  if (event.target !== toolbarBackdrop) return;
  // Complete the tap on the backdrop before either menu can dismiss on pointerdown.
  event.preventDefault();
  event.stopImmediatePropagation();
}, true);
document.addEventListener('pointerdown', event => {
  const target = event.target as Node;
  if (fallback?.contains(target) || fallbackAnchor?.contains(target)) return;
  if (fallbackAnchor && (document.activeElement as HTMLElement | null)?.isContentEditable) event.preventDefault();
  dismissPopupMenu();
});
document.addEventListener('keydown', event => {
  if (event.key !== 'Escape' || !fallbackAnchor || fallback?.contains(event.target as Node)) return;
  event.preventDefault(); event.stopPropagation(); dismissPopupMenu();
}, true);
window.addEventListener('resize', dismissPopupMenu);

export type MenuOptions = {
  label?: string;
  /** 从按钮弹出：统一用玻璃面板、从按钮展开的动画，桌面和手机的位置由这里决定。 */
  anchor?: HTMLButtonElement;
  /** 不传时按按钮是否带键盘焦点判断；键盘打开时焦点落在首个可用项。 */
  keyboard?: boolean;
  /** 右键之类没有按钮的菜单在这里弹出，桌面 App 用系统菜单。 */
  at?: { x: number; y: number };
  /** 手机长按时菜单上方的预览卡片。 */
  preview?: HTMLElement;
  previewAnchor?: HTMLElement;
  quickActions?: number;
  onError?: (message: string) => void;
  restoreFocus?: () => void;
};

// 所有弹出菜单的入口。调用方只给菜单项和触发位置，样式、定位、开合和焦点都在这里处理。
export function showMenu(entries: MenuItem[] | Promise<MenuItem[]>, options: MenuOptions = {}) {
  const { anchor, at, label = '操作菜单' } = options;
  // 菜单开着时再点一次按钮就收起。
  if (anchor?.getAttribute('aria-expanded') === 'true') { dismissPopupMenu(); return Promise.resolve(); }
  anchor?.setAttribute('aria-haspopup', 'menu');
  const box = anchor?.getBoundingClientRect();
  const x = at?.x ?? box?.left ?? 0, y = at?.y ?? box?.bottom ?? 0;
  const restoreFocus = options.restoreFocus ?? (() => anchor?.focus({ preventScroll: true }));
  const toolbar = anchor && {
    anchor, keyboard: options.keyboard ?? anchor.matches(':focus-visible'),
    // 键盘上方的快捷栏贴着屏幕底边，菜单只能往上开。
    above: Boolean(anchor.closest('#mobile-tools')),
  };
  return openMenu(entries, x, y, options.onError ?? console.warn, restoreFocus, label, options.preview, options.quickActions, toolbar, options.previewAnchor);
}

async function openMenu(entries: MenuItem[] | Promise<MenuItem[]>, x: number, y: number, error: (message: string) => void, restoreFocus: () => void, label = '操作菜单', preview?: HTMLElement, quickActions = 0, toolbar?: { anchor: HTMLButtonElement; keyboard: boolean; above?: boolean }, previewAnchor?: HTMLElement) {
  dismissPopupMenu(); const request = generation;
  const anchor = toolbar?.anchor;
  const root = document.documentElement;
  const desktopToolbar = anchor && (root.hasAttribute('data-web') ? matchMedia('(min-width: 901px)').matches : !root.hasAttribute('data-mobile'));
  const keepEditorFocus = anchor && (document.activeElement as HTMLElement | null)?.isContentEditable;
  const items = Array.isArray(entries) ? entries : await entries;
  if (request !== generation) return;
  const close = () => { dismissPopupMenu(); if (!keepEditorFocus) restoreFocus(); };
  const run = (item: MenuItem) => { close(); void Promise.resolve().then(item.run).catch(reason => error(reason instanceof Error ? reason.message : String(reason))); };
  function addToolbarBackdrop() {
    if (!anchor || desktopToolbar) return;
    const backdrop = document.createElement('div');
    backdrop.className = 'toolbar-menu-backdrop';
    backdrop.ariaHidden = 'true';
    backdrop.onclick = close;
    document.body.append(backdrop);
    toolbarBackdrop = backdrop;
  }
  if (anchor && glassyToolbarPreview && !toolbar?.above) {
    const { createGlassyMenu } = await import('./glassy-menu-preview');
    if (request !== generation || !anchor.isConnected) return;
    const preview = createGlassyMenu(anchor, items, label, Boolean(keepEditorFocus), run, () => {
      if (fallback === preview.menu) dismissPopupMenu();
    });
    preview.menu.classList.toggle('desktop-toolbar-menu', Boolean(desktopToolbar));
    fallback = preview.menu;
    fallbackAnchor = anchor;
    closeGlassyMenu = preview.close;
    addToolbarBackdrop();
    anchor.setAttribute('aria-expanded', 'true');
    preview.open();
    return;
  }
  function browserMenu(entries: MenuItem[], left: number, top: number, container: HTMLElement, trigger?: HTMLButtonElement, inline?: { item: MenuItem; width: number }) {
    const menu = document.createElement('div'); menu.className = 'note-context-menu'; menu.role = 'menu'; menu.ariaLabel = trigger?.ariaLabel ?? label;
    menu.tabIndex = -1;
    const content = anchor ? document.createElement('div') : menu;
    if (anchor) {
      menu.classList.add('toolbar-context-menu');
      menu.classList.toggle('desktop-toolbar-menu', Boolean(desktopToolbar));
      if (!trigger) menu.dataset.toolbarMenuRoot = '';
      content.className = 'toolbar-menu-items';
      content.role = 'presentation';
      menu.append(content);
    }
    if (inline && trigger) {
      menu.classList.add('toolbar-submenu');
      menu.style.width = `${inline.width}px`;
      const header = document.createElement('button');
      header.type = 'button'; header.className = 'toolbar-submenu-header'; header.ariaLabel = `收起${inline.item.text}`;
      const title = document.createElement('span'); title.textContent = inline.item.text;
      const icon = trigger.querySelector('.context-action-icon')?.cloneNode(true);
      if (icon) header.append(icon);
      header.append(title, createLucideIcon(document, ChevronDown, 'context-submenu-chevron'));
      header.onclick = () => {
        getSurface(menu)?.destroy(); menu.remove();
        trigger.setAttribute('aria-expanded', 'false');
        if (!keepEditorFocus) trigger.focus({ preventScroll: true });
      };
      const divider = document.createElement('hr'); divider.role = 'separator';
      content.append(header, divider);
    }
    if (keepEditorFocus) menu.onpointerdown = event => {
      if ((event.target as Element).closest('button')) event.preventDefault();
    };
    if (!trigger && quickActions) menu.style.setProperty('--quick-actions', String(quickActions));
    function closeSubmenu() {
      const submenu = menu.querySelector<HTMLElement>(':scope > .note-context-menu');
      if (submenu) getSurface(submenu)?.destroy();
      submenu?.remove();
      menu.querySelector('[aria-expanded=true]')?.setAttribute('aria-expanded', 'false');
    }
    // 子菜单盖在上面时，点原菜单的空处或其他项只是收回子菜单。
    if (anchor && !desktopToolbar) content.addEventListener('pointerdown', event => {
      const submenu = menu.querySelector<HTMLElement>(':scope > .toolbar-submenu');
      if (!submenu || submenu.contains(event.target as Node)) return;
      event.preventDefault(); event.stopPropagation();
      submenu.querySelector<HTMLButtonElement>('.toolbar-submenu-header')?.click();
    }, true);
    for (const [index, item] of entries.entries()) {
      if (!item.text) { const divider = document.createElement('hr'); divider.role = 'separator'; content.append(divider); continue; }
      const button = document.createElement('button'); button.role = item.checked === undefined ? 'menuitem' : 'menuitemradio'; button.ariaLabel = item.text; button.disabled = item.enabled === false;
      if (item.checked !== undefined) button.setAttribute('aria-checked', String(item.checked));
      if (!trigger && index < quickActions) button.classList.add('context-quick-action');
      if (item.destructive) button.classList.add('destructive');
      const title = document.createElement('span'); title.textContent = item.text;
      const icon = actionIcons[item.text];
      if (item.icon) {
        item.icon.classList.add('context-action-icon');
        button.append(item.icon);
      } else if ((preview || label === '表格操作' || label === '行选项' || label === '列选项') && icon) button.innerHTML = `<svg class="context-action-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round" stroke-linecap="round" aria-hidden="true"><path d="${icon}"/></svg>`;
      const key = document.createElement('kbd'); key.textContent = item.items ? '' : item.checked ? '✓' : anchor && document.documentElement.hasAttribute('data-mobile') ? '' : item.shortcut ?? '';
      if (item.items) key.append(createLucideIcon(document, ChevronRight, 'context-submenu-chevron'));
      button.append(title, key); content.append(button);
      if (inline && item.checked !== undefined) button.classList.add('context-check-leading');
      if (item.items) {
        button.setAttribute('aria-haspopup', 'menu');
        button.setAttribute('aria-expanded', 'false');
        const open = () => {
          if (button.disabled) return;
          closeSubmenu(); const box = menu.getBoundingClientRect();
          button.setAttribute('aria-expanded', 'true');
          // 手机上和 iOS 的 UIMenu 一样，子菜单盖在原菜单上展开：顶行是加粗的父项和向下箭头，点它收回。
          const inline = anchor && !desktopToolbar;
          const row = button.getBoundingClientRect();
          const submenu = inline
            ? browserMenu(item.items!, box.left - 8, row.top - 8, menu, button, { item, width: box.width + 8 })
            : browserMenu(item.items!, box.right - 4, row.top, menu, button);
          // 工具栏菜单的二级菜单也用同样的玻璃面板，否则半透明底色下会透出后面的正文。
          if (anchor) {
            const radius = Number.parseFloat(getComputedStyle(submenu).borderTopLeftRadius) || 0;
            glass(submenu, { ...toolbarMenuGlass, radius, fill: 'var(--toolbar-menu-glass-fill)', shadow: false });
          }
          // 触摸展开时焦点放在子菜单本身，免得首行一出来就是高亮的；键盘展开仍落到首个可用项。
          if (keepEditorFocus) return;
          if (inline && !button.matches(':focus-visible')) submenu.focus({ preventScroll: true });
          else submenu.querySelector<HTMLButtonElement>(inline ? 'button:not(:disabled):not(.toolbar-submenu-header)' : 'button:not(:disabled)')?.focus();
        };
        button.onclick = open;
        button.onkeydown = event => { if (event.key === 'ArrowRight') { event.preventDefault(); open(); } };
      } else button.onclick = () => run(item);
    }
    menu.onkeydown = event => {
      if ((event.target as HTMLElement).closest('[role=menu]') !== menu) return;
      const buttons = [...content.querySelectorAll<HTMLButtonElement>(':scope > button:not(:disabled)')], index = buttons.indexOf(document.activeElement as HTMLButtonElement);
      const previous = index < 0 ? buttons.length - 1 : (index + buttons.length - 1) % buttons.length;
      const next = (index + 1) % buttons.length;
      if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
        event.preventDefault(); buttons[event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 : event.key === 'ArrowUp' ? previous : next]?.focus();
      }
      if (event.key === 'Tab') { event.preventDefault(); buttons[event.shiftKey ? previous : next]?.focus(); }
      if (event.key === 'Escape' || event.key === 'ArrowLeft' && trigger) {
        event.preventDefault(); event.stopPropagation();
        if (trigger) { getSurface(menu)?.destroy(); menu.remove(); trigger.setAttribute('aria-expanded', 'false'); trigger.focus(); }
        else close();
      }
    };
    container.append(menu);
    const bottom = anchor ? window.visualViewport?.height ?? innerHeight : innerHeight;
    if (anchor && !trigger) {
      if (toolbar?.above) {
        const box = anchor.getBoundingClientRect();
        const edge = Math.min(box.top, bottom);
        menu.style.maxHeight = `${Math.max(0, edge - 16)}px`;
        left = box.left + (box.width - menu.offsetWidth) / 2;
        top = edge - menu.offsetHeight - 8;
      } else if (desktopToolbar) {
        const box = anchor.getBoundingClientRect();
        left = Math.min(box.left - 8, innerWidth - menu.offsetWidth - 8);
        top = box.bottom + 4;
      } else {
        // 手机和平板：首行顶边对齐按钮所在工具栏的顶边。右半边的按钮贴右边缘 8 px，左半边的从按钮左边展开。
        const bar = anchor.closest('.actions') ?? anchor;
        const box = bar.getBoundingClientRect(), button = anchor.getBoundingClientRect();
        left = button.left + button.width / 2 > innerWidth / 2 ? innerWidth - menu.offsetWidth - 8 : button.left;
        top = Math.max(4, box.top - parseFloat(getComputedStyle(content).paddingTop));
      }
    }
    if (anchor && (!toolbar?.above || trigger)) menu.style.maxHeight = `${Math.max(0, bottom - top - 8)}px`;
    if (trigger && !inline && left + menu.offsetWidth > innerWidth - 4) left = container.getBoundingClientRect().left - menu.offsetWidth + 4;
    menu.style.left = `${Math.max(4, Math.min(left, innerWidth - menu.offsetWidth - 4))}px`;
    menu.style.top = `${Math.max(4, Math.min(top, bottom - menu.offsetHeight - 4))}px`;
    return menu;
  }
  if (!anchor && !preview && !items.some(item => item.needsUserActivation) && '__TAURI_INTERNALS__' in window && !document.documentElement.hasAttribute('data-mobile')) {
    try {
      const { Menu } = await import('@tauri-apps/api/menu'), { LogicalPosition } = await import('@tauri-apps/api/dpi');
      if (request !== generation) return;
      const previous = native; native = undefined; await previous?.close();
      const shortcuts: Record<string, string> = { '⌥⌘I': 'Alt+CmdOrCtrl+I', '⇧⌘P': 'Shift+CmdOrCtrl+P', '⌘D': 'CmdOrCtrl+D', '⌘R': 'CmdOrCtrl+R', '⌘⌫': 'CmdOrCtrl+Backspace', '⇧⌘N': 'Shift+CmdOrCtrl+N', '⇧F6': 'Shift+F6', '⌥⌘R': 'Alt+CmdOrCtrl+R', '⌘J': 'CmdOrCtrl+J', '⇧⌘⌫': 'Shift+CmdOrCtrl+Backspace' };
      const options = (entries: MenuItem[]): import('@tauri-apps/api/menu').MenuOptions['items'] => entries.map(item => !item.text ? { item: 'Separator' as const } : item.nativeRole ? { item: item.nativeRole, text: item.text } : item.items ? { text: item.text, enabled: item.enabled, items: options(item.items) } : {
        text: item.text, enabled: item.enabled, checked: item.checked, accelerator: shortcuts[item.shortcut ?? ''], action: () => { setTimeout(() => run(item), 0); },
      });
      const created = await Menu.new({ items: options(items) });
      if (request !== generation) { await created.close(); return; }
      native = created; await native.popup(new LogicalPosition(x, y)); return;
    } catch { if (request !== generation) return; }
  }
  if (preview) {
    fallback = showContextSheet(preview, { label, anchor: previewAnchor, close, mountMenu: dialog => { browserMenu(items, x, y, dialog); } });
  } else fallback = browserMenu(items, x, y, document.body);
  fallbackAnchor = anchor;
  addToolbarBackdrop();
  anchor?.setAttribute('aria-expanded', 'true');
  if (anchor) {
    const radius = Number.parseFloat(getComputedStyle(fallback).borderTopLeftRadius) || 0;
    const surface = glass(fallback, { ...toolbarMenuGlass, radius, fill: 'var(--toolbar-menu-glass-fill)', shadow: false });
    closeToolbarMenu = animateToolbarMenu(fallback, anchor, surface);
  }
  const pointerOpenedToolbarMenu = toolbar && !toolbar.keyboard;
  const first = pointerOpenedToolbarMenu ? fallback : fallback.querySelector<HTMLButtonElement>('button:not(:disabled)') ?? fallback.querySelector<HTMLElement>('[role=menu]') ?? fallback;
  if (!keepEditorFocus) first.focus({ preventScroll: true });
}
