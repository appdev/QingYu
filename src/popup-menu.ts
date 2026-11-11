export type MenuItem = { text: string; enabled?: boolean; shortcut?: string; run?: () => void | Promise<void>; items?: MenuItem[] };

let native: import('@tauri-apps/api/menu').Menu | undefined;
let fallback: HTMLElement | undefined, generation = 0;
export const popupMenuOpen = () => Boolean(fallback);
export function dismissPopupMenu() { fallback?.remove(); fallback = undefined; generation++; }
document.addEventListener('pointerdown', event => { if (!fallback?.contains(event.target as Node)) dismissPopupMenu(); });
window.addEventListener('resize', dismissPopupMenu);

export async function popupMenu(items: MenuItem[], x: number, y: number, error: (message: string) => void, restoreFocus: () => void, label = '操作菜单') {
  dismissPopupMenu(); const request = generation;
  const run = (item: MenuItem) => { dismissPopupMenu(); void Promise.resolve().then(item.run).catch(reason => error(reason instanceof Error ? reason.message : String(reason))); };
  function browserMenu(entries: MenuItem[], left: number, top: number, parent?: HTMLElement) {
    const menu = document.createElement('div'); menu.className = 'note-context-menu'; menu.role = 'menu'; menu.ariaLabel = parent ? '移动或导出' : label;
    for (const item of entries) {
      if (!item.text) { const divider = document.createElement('hr'); divider.role = 'separator'; menu.append(divider); continue; }
      const button = document.createElement('button'); button.role = 'menuitem'; button.ariaLabel = item.text; button.disabled = item.enabled === false;
      const label = document.createElement('span'); label.textContent = item.text;
      const key = document.createElement('kbd'); key.textContent = item.items ? '›' : item.shortcut ?? '';
      button.append(label, key); menu.append(button);
      if (item.items) {
        button.setAttribute('aria-haspopup', 'menu');
        const open = () => {
          menu.querySelector('.note-context-menu')?.remove(); const box = button.getBoundingClientRect();
          browserMenu(item.items!, box.right - 4, box.top, menu).querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
        };
        button.onclick = open;
        button.onkeydown = event => { if (event.key === 'ArrowRight') { event.preventDefault(); open(); } };
      } else button.onclick = () => run(item);
    }
    menu.onkeydown = event => {
      if ((event.target as HTMLElement).closest('[role=menu]') !== menu) return;
      const buttons = [...menu.querySelectorAll<HTMLButtonElement>(':scope > button:not(:disabled)')], index = buttons.indexOf(document.activeElement as HTMLButtonElement);
      if (['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) {
        event.preventDefault(); buttons[event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length]?.focus();
      }
      if (event.key === 'Escape') { event.stopPropagation(); dismissPopupMenu(); restoreFocus(); }
      if (event.key === 'ArrowLeft' && parent) { event.stopPropagation(); menu.remove(); parent.querySelector<HTMLButtonElement>('[aria-haspopup]')?.focus(); }
    };
    (parent ?? document.body).append(menu);
    menu.style.left = `${Math.max(4, Math.min(left, innerWidth - menu.offsetWidth - 4))}px`;
    menu.style.top = `${Math.max(4, Math.min(top, innerHeight - menu.offsetHeight - 4))}px`;
    return menu;
  }
  if ('__TAURI_INTERNALS__' in window) {
    try {
      const { Menu } = await import('@tauri-apps/api/menu'), { LogicalPosition } = await import('@tauri-apps/api/dpi');
      if (request !== generation) return;
      const previous = native; native = undefined; await previous?.close();
      const shortcuts: Record<string, string> = { '⌥⌘I': 'Alt+CmdOrCtrl+I', '⇧⌘P': 'Shift+CmdOrCtrl+P', '⌘D': 'CmdOrCtrl+D', '⌘R': 'CmdOrCtrl+R', '⌘⌫': 'CmdOrCtrl+Backspace', '⇧⌘N': 'Shift+CmdOrCtrl+N', '⇧F6': 'Shift+F6', '⌥⌘R': 'Alt+CmdOrCtrl+R', '⌘J': 'CmdOrCtrl+J', '⇧⌘⌫': 'Shift+CmdOrCtrl+Backspace' };
      const options = (entries: MenuItem[]): import('@tauri-apps/api/menu').MenuOptions['items'] => entries.map(item => !item.text ? { item: 'Separator' as const } : item.items ? { text: item.text, enabled: item.enabled, items: options(item.items) } : {
        text: item.text, enabled: item.enabled, accelerator: shortcuts[item.shortcut ?? ''], action: () => { setTimeout(() => run(item), 0); },
      });
      const created = await Menu.new({ items: options(items) });
      if (request !== generation) { await created.close(); return; }
      native = created; await native.popup(new LogicalPosition(x, y));
    } catch { error('操作菜单未能打开，请重试'); }
  } else {
    fallback = browserMenu(items, x, y); fallback.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();
  }
}
