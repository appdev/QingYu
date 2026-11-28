import { formatLabel } from './ui-language';
import { showContextSheet } from './context-sheet';
import { ChevronDown, ChevronRight } from 'lucide';
import { createLucideIcon } from './vendor/markra/shared/lucide';
import { mountWebGlass, usesGlisse } from './web-glass-mount';
import { shortcutText, showLiquidMenu } from './liquid-menu';

/** 一个菜单项。`text` 为空字符串时表示分隔线，前后的项分成两组。 */
export type MenuItem = {
  /** 显示的文字，也是读屏名称和首字母跳转的依据。 */
  text: string;
  /** 行首图标。尺寸由菜单决定，调用方只给图形。 */
  icon?: SVGElement;
  /** 设为 false 时灰显且不能选。 */
  enabled?: boolean;
  /** 删除这类不可撤销的操作，显示为红色。 */
  destructive?: boolean;
  /** 单选或开关项：给 true / false 都会按勾选项朗读，true 时显示勾号。同一组里每项都要给。 */
  checked?: boolean;
  /** 快捷键文字，如「⇧⌘P」。只在桌面宽度显示，桌面 App 的系统菜单会注册成真正的快捷键。 */
  shortcut?: string;
  /** 桌面 App 系统菜单里改用系统自带的剪切、拷贝、粘贴。 */
  nativeRole?: 'Cut' | 'Copy' | 'Paste';
  /** 需要用户手势才能执行（选文件、读写剪贴板）。带这类项的菜单不走系统菜单。 */
  needsUserActivation?: boolean;
  /** 选中后执行。菜单先关闭、焦点先恢复，再调用它；抛错或返回失败的 Promise 会交给 onError。 */
  run?: () => void | Promise<void>;
  /** 子菜单。有子菜单的项本身不执行 run。 */
  items?: MenuItem[];
};

const actionIcons: Record<string, string> = {
  分享: 'M12 15V3m-4 4 4-4 4 4M8 9H5v12h14V9h-3',
  移动: 'M3 6h7l2 2h9v12H3z',
  删除: 'M3 6h18M9 6V3h6v3M6 6l1 15h10l1-15m-8 4v7m4-7v7',
  置顶: 'M8 3h8l-1 6 3 4v2H6v-2l3-4zm4 12v7',
  取消置顶: 'M4 4l16 16M9 3h7l-1 6 3 4M6 13v2h7m-1 0v7',
  复制笔记: 'M8 16H3V3h13v5M8 8h13v13H8zM15 11v7m-4-3h7',
  '版本历史…': 'M3 12a9 9 0 1 0 2.6-6.4L3 8m0-5v5h5m4-1v5l3 2',
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
let closeFallback: (() => void) | undefined;
let toolbarBackdrop: HTMLElement | undefined;
let finishClosingToolbarMenu: (() => void) | undefined;
// Web 和非 Apple 的 App 用移植的 Liquid Glass 菜单，开合和点外关闭由它自己处理。
let liquidMenu: ReturnType<typeof showLiquidMenu> | undefined;
let liquidAnchor: HTMLButtonElement | undefined;
export const popupMenuOpen = () => Boolean(fallback || liquidMenu);

export function backPopupMenu() {
  if (liquidMenu) { liquidMenu.back(); return true; }
  if (closeFallback) { closeFallback(); return true; }
  return false;
}

const menuGlass = new WeakMap<HTMLElement, () => void>();
function removeMenu(menu: HTMLElement) {
  menuGlass.get(menu)?.();
  for (const child of menu.querySelectorAll<HTMLElement>('[role=menu]')) menuGlass.get(child)?.();
  menu.remove();
}

function closeToolbarMenu(menu: HTMLElement) {
  menu.inert = true;
  menu.hidden = true;
  const animations = menu.getAnimations?.() ?? [];
  if (!animations.length) { removeMenu(menu); return; }
  const finish = () => {
    if (finishClosingToolbarMenu !== finish) return;
    finishClosingToolbarMenu = undefined;
    removeMenu(menu);
  };
  finishClosingToolbarMenu = finish;
  void Promise.allSettled(animations.map(animation => animation.finished)).then(finish);
}

export function dismissPopupMenu() {
  const menu = fallback, anchor = fallbackAnchor;
  fallback = fallbackAnchor = closeFallback = undefined; generation++;
  const liquid = liquidMenu;
  liquidMenu = undefined;
  liquid?.close();
  liquidAnchor?.setAttribute('aria-expanded', 'false');
  liquidAnchor = undefined;
  toolbarBackdrop?.remove();
  toolbarBackdrop = undefined;
  anchor?.setAttribute('aria-expanded', 'false');
  if (!menu) return;
  if (anchor) closeToolbarMenu(menu);
  else removeMenu(menu);
}
document.addEventListener('pointerdown', event => {
  if (event.target !== toolbarBackdrop) return;
  // Complete the tap on the backdrop before dismissing the menu.
  event.preventDefault();
  event.stopImmediatePropagation();
}, true);
document.addEventListener('pointerdown', event => {
  if (liquidMenu) return;
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

/**
 * 弹出位置和收尾。样式、尺寸、平台（系统菜单还是网页菜单）、动画、点外关闭和键盘操作都由菜单内部处理，调用方不用管。
 * `anchor` 和 `at` 二选一：从按钮弹出给 `anchor`，右键或没有按钮时给 `at`。
 */
export type MenuOptions = {
  /** 菜单的读屏名称，默认「操作菜单」。 */
  label?: string;
  /** 从这个按钮弹出。打开期间按钮的 aria-expanded 为 true，再点一次按钮会收起菜单。 */
  anchor?: HTMLButtonElement;
  /** 桌面应用菜单在按钮下方打开，子菜单在父项旁边展开。 */
  presentation?: 'dropdown';
  /** 是否由键盘打开：是则焦点落在首个可用项，否则落在菜单本身。不传时按按钮是否带键盘焦点判断。 */
  keyboard?: boolean;
  /** 没有按钮时的弹出位置（视口坐标），如右键菜单。 */
  at?: { x: number; y: number };
  /** 手机长按时显示在菜单上方的预览卡片。带预览的菜单用长按预览布局。 */
  preview?: HTMLElement;
  /** 预览卡片对齐的元素，通常是被长按的那一行。 */
  previewAnchor?: HTMLElement;
  /** 长按预览布局里，开头这几项排成一行快捷操作。 */
  quickActions?: number;
  /** run 失败时的提示，默认打到控制台。 */
  onError?: (message: string) => void;
  /** 菜单关闭后把焦点还给谁，默认还给 anchor。 */
  restoreFocus?: () => void;
};

type NativeMenuItem ={ id: number; text: string; enabled: boolean; destructive: boolean; checked: boolean | null; children?: NativeMenuItem[][] };

/** 系统菜单打开失败时返回 false，由调用方退回网页菜单。 */
async function showMobileMenu(anchor: HTMLButtonElement, items: MenuItem[], run: (item: MenuItem) => void) {
  // 每项编一个号传给原生端，选中后按编号找回原来的菜单项；分隔线把菜单切成几组。
  const leaves = new Map<number, MenuItem>();
  let next = 1;
  const sections = (entries: MenuItem[]) => entries.reduce<NativeMenuItem[][]>((groups, item) => {
    if (!item.text) { groups.push([]); return groups; }
    const id = next++;
    if (!item.items) leaves.set(id, item);
    groups[groups.length - 1].push({
      id, text: item.text, enabled: item.enabled !== false, destructive: Boolean(item.destructive), checked: item.checked ?? null,
      ...(item.items ? { children: sections(item.items) } : {}),
    });
    return groups;
  }, [[]]).filter(group => group.length);
  const viewport = window.visualViewport, scale = viewport?.scale ?? 1;
  const box = anchor.getBoundingClientRect();
  const frame = [(box.left - (viewport?.offsetLeft ?? 0)) * scale, (box.top - (viewport?.offsetTop ?? 0)) * scale, box.width * scale, box.height * scale];
  anchor.setAttribute('aria-expanded', 'true');
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    const result = await invoke<{ id?: number } | null>('show_native_menu', { menu: { frame, sections: sections(items) } });
    const item = result?.id === undefined ? undefined : leaves.get(result.id);
    if (item) run(item);
    return true;
  } catch {
    return false;
  } finally {
    anchor.setAttribute('aria-expanded', 'false');
  }
}

function pathIcon(path: string) {
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('fill', 'none');
  svg.setAttribute('stroke', 'currentColor');
  svg.setAttribute('stroke-width', '1.7');
  svg.setAttribute('stroke-linejoin', 'round');
  svg.setAttribute('stroke-linecap', 'round');
  const shape = document.createElementNS('http://www.w3.org/2000/svg', 'path');
  shape.setAttribute('d', path);
  svg.append(shape);
  return svg;
}

/**
 * 所有弹出菜单的唯一入口。调用方只给菜单项和弹出位置。返回的 Promise 在网页菜单打开后、系统菜单关闭后结束，不要拿它判断菜单是否已关；要知道是否还开着用 popupMenuOpen()。
 * 分端：macOS 和 iOS App 用系统菜单，Web、Android、Windows、Linux 用 Liquid Glass 菜单，带预览的长按菜单和键盘上方快捷栏的菜单用网页菜单。
 */
export function showMenu(entries: MenuItem[] | Promise<MenuItem[]>, options: MenuOptions = {}) {
  const { anchor, at, label = '操作菜单' } = options;
  // 菜单开着时再点一次按钮就收起。
  if (anchor?.getAttribute('aria-expanded') === 'true') { dismissPopupMenu(); return Promise.resolve(); }
  anchor?.setAttribute('aria-haspopup', 'menu');
  const box = anchor?.getBoundingClientRect();
  const x = at?.x ?? box?.left ?? 0, y = at?.y ?? box?.bottom ?? 0;
  const restoreFocus = options.restoreFocus ?? (() => anchor?.focus({ preventScroll: true }));
  const keyboard = options.keyboard ?? Boolean(anchor?.matches(':focus-visible') && !document.documentElement.hasAttribute('data-pointer-focus'));
  const toolbar = anchor && {
    anchor,
    dropdown: options.presentation === 'dropdown',
    // 键盘上方的快捷栏贴着屏幕底边，菜单只能往上开。
    above: Boolean(anchor.closest('#mobile-tools')),
  };
  return openMenu(entries, x, y, options.onError ?? console.warn, restoreFocus, label, keyboard, options.preview, options.quickActions, toolbar, options.previewAnchor);
}

async function openMenu(entries: MenuItem[] | Promise<MenuItem[]>, x: number, y: number, error: (message: string) => void, restoreFocus: () => void, label: string, keyboard: boolean, preview?: HTMLElement, quickActions = 0, toolbar?: { anchor: HTMLButtonElement; above: boolean; dropdown: boolean }, previewAnchor?: HTMLElement) {
  dismissPopupMenu(); const request = generation;
  const anchor = toolbar?.anchor;
  const root = document.documentElement;
  const glass = usesGlisse(root);
  const desktop = !root.hasAttribute('data-mobile');
  const desktopToolbar = anchor && desktop;
  const dropdown = desktop && toolbar?.dropdown;
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
  function browserMenu(entries: MenuItem[], left: number, top: number, container: HTMLElement, trigger?: HTMLButtonElement, inline?: { item: MenuItem; width: number }) {
    const menu = document.createElement('div'); menu.className = 'note-context-menu';
    menu.role = 'menu'; menu.ariaLabel = trigger?.ariaLabel ?? label;
    menu.tabIndex = -1;
    const content = anchor ? document.createElement('div') : menu;
    if (anchor) {
      menu.classList.add('toolbar-context-menu', 'fk-transition', 'fk-opacity');
      if (dropdown) menu.classList.add('desktop-dropdown-menu');
      else if (glass) menu.classList.add('web-glass-frost');
      else menu.classList.add('lb', 'qingyu-glass');
      menu.classList.toggle('desktop-toolbar-menu', Boolean(desktopToolbar));
      content.className = 'toolbar-menu-items';
      content.role = 'presentation';
      menu.append(content);
    }
    if (anchor && !trigger) menu.dataset.toolbarMenuRoot = '';
    if (inline && trigger) {
      menu.classList.add('toolbar-submenu');
      menu.style.width = `${inline.width}px`;
      const header = document.createElement('button');
      header.type = 'button'; header.className = 'toolbar-submenu-header'; header.ariaLabel = formatLabel('收起{action}', { action: inline.item.text });
      const title = document.createElement('span'); title.textContent = inline.item.text;
      const icon = trigger.querySelector('.context-action-icon')?.cloneNode(true);
      if (icon) header.append(icon);
      header.append(title, createLucideIcon(document, ChevronDown, 'context-submenu-chevron'));
      header.onclick = () => {
        removeMenu(menu);
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
      const submenu = menu.querySelector<HTMLElement>(':scope > [role=menu]');
      if (submenu) removeMenu(submenu);
      menu.querySelector('[aria-expanded=true]')?.setAttribute('aria-expanded', 'false');
    }
    // 子菜单盖在上面时，点原菜单的空处或其他项只是收回子菜单。
    if (anchor && !desktop) content.addEventListener('pointerdown', event => {
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
      const key = document.createElement('kbd'); key.textContent = item.items ? '' : item.checked ? '✓' : anchor && root.hasAttribute('data-mobile') ? '' : dropdown ? shortcutText(item.shortcut ?? '') : item.shortcut ?? '';
      key.ariaHidden = 'true';
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
          const inline = anchor && !desktop;
          const row = button.getBoundingClientRect();
          const submenu = inline
            ? browserMenu(item.items!, box.left - 8, row.top - 8, menu, button, { item, width: box.width + 8 })
            : browserMenu(item.items!, box.right - 4, row.top, menu, button);
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
      if (dropdown && event.key.length === 1 && event.key !== ' ' && !event.ctrlKey && !event.metaKey && !event.altKey) {
        const key = event.key.toLocaleLowerCase();
        const match = [...buttons.slice(index + 1), ...buttons.slice(0, index + 1)]
          .find(button => button.ariaLabel?.toLocaleLowerCase().startsWith(key));
        if (match) { event.preventDefault(); match.focus(); }
      }
      if (event.key === 'Escape' || event.key === 'ArrowLeft' && trigger) {
        event.preventDefault(); event.stopPropagation();
        if (trigger) { removeMenu(menu); trigger.setAttribute('aria-expanded', 'false'); trigger.focus(); }
        else close();
      }
    };
    container.append(menu);
    // 长按预览菜单不挂玻璃层：Android WebView 上它只剩一层灰色模糊，菜单和遮罩糊成一片。
    if (glass && !dropdown && !preview) menuGlass.set(menu, mountWebGlass(menu, { tint: 82, blur: 14 }));
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
        left = Math.min(box.left - (dropdown ? 0 : 8), innerWidth - menu.offsetWidth - 8);
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
    const parent = anchor && trigger ? container.getBoundingClientRect() : undefined;
    menu.style.left = `${Math.max(4, Math.min(left, innerWidth - menu.offsetWidth - 4)) - (parent?.left ?? 0)}px`;
    menu.style.top = `${Math.max(4, Math.min(top, bottom - menu.offsetHeight - 4)) - (parent?.top ?? 0)}px`;
    return menu;
  }
  // 长按带预览的菜单和键盘上方快捷栏的菜单仍用下面的网页菜单。
  if (glass && !preview && !toolbar?.above && !dropdown) {
    const restore = () => { dismissPopupMenu(); restoreFocus(); };
    liquidMenu = showLiquidMenu({
      items, label, anchor, at: anchor ? undefined : { x, y }, keyboard,
      iconFor: item => item.icon ?? ((label === '表格操作' || label === '行选项' || label === '列选项') && actionIcons[item.text] ? pathIcon(actionIcons[item.text]) : undefined),
      select: item => { restore(); void Promise.resolve().then(item.run).catch(reason => error(reason instanceof Error ? reason.message : String(reason))); },
      dismiss: restore,
    });
    liquidAnchor = anchor;
    anchor?.setAttribute('aria-expanded', 'true');
    return;
  }
  // 桌面 App 的右键和按钮下拉都用系统菜单；带预览或需要用户激活（选文件、剪贴板）的菜单留在网页里。
  const desktopApp = !glass && !root.hasAttribute('data-mobile');
  if (desktopApp && !preview && !dropdown && !items.some(item => item.needsUserActivation)) {
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
      native = created;
      if (!anchor) { await native.popup(new LogicalPosition(x, y)); return; }
      // 从按钮左下角往下 4 px 弹出。菜单开着时按钮保持展开状态，工具栏高亮跟着它。
      const box = anchor.getBoundingClientRect();
      anchor.setAttribute('aria-expanded', 'true');
      try { await native.popup(new LogicalPosition(box.left, box.bottom + 4)); }
      finally { anchor.setAttribute('aria-expanded', 'false'); }
      return;
    } catch { if (request !== generation) return; }
  }
  // iOS 的按钮下拉使用 UIMenu。键盘上方的菜单留在页面内，以保留编辑器焦点。
  const mobileApp = !glass && root.hasAttribute('data-mobile');
  if (mobileApp && anchor && !toolbar?.above && !preview && !items.some(item => item.needsUserActivation)) {
    if (await showMobileMenu(anchor, items, run)) return;
    if (request !== generation) return;
  }
  finishClosingToolbarMenu?.();
  if (preview) {
    fallback = showContextSheet(preview, { label, anchor: previewAnchor, close, mountMenu: dialog => { browserMenu(items, x, y, dialog); } });
  } else fallback = browserMenu(items, x, y, document.body);
  closeFallback = close;
  fallbackAnchor = anchor;
  addToolbarBackdrop();
  anchor?.setAttribute('aria-expanded', 'true');
  const pointerOpenedToolbarMenu = toolbar && !keyboard;
  const first = pointerOpenedToolbarMenu ? fallback : fallback.querySelector<HTMLButtonElement>('button:not(:disabled)') ?? fallback.querySelector<HTMLElement>('[role=menu]') ?? fallback;
  if (!keepEditorFocus) first.focus({ preventScroll: true });
}
