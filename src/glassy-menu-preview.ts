import 'glassy-liquid/components/menu';
import type { Spring } from 'glassy-liquid';
import type { GlassMenuElement } from 'glassy-liquid/components/menu';
import type { MenuItem } from './popup-menu';
import { toolbarMenuGlass } from './toolbar-menu-glass';

let finishClosingMenu: (() => void) | undefined;

// Try the same toolbar menu on every platform with ?menu=glassy.
export function createGlassyMenu(
  anchor: HTMLButtonElement,
  entries: MenuItem[],
  label: string,
  keepEditorFocus: boolean,
  run: (item: MenuItem) => void,
  dismiss: () => void,
) {
  finishClosingMenu?.();
  const menu = document.createElement('glass-menu') as GlassMenuElement;
  // glassy-liquid 0.1.0 sets its private spring immediately before emitting toggle.
  const spring = Reflect.get(menu, 'progress') as Pick<Spring, 'configure'> | undefined;
  menu.className = 'toolbar-glassy-menu';
  menu.setAttribute('placement', 'bottom-end');
  menu.glass = toolbarMenuGlass;

  const origin = document.createElement('span');
  const group = anchor.closest('.actions')?.getBoundingClientRect();
  const bounds = group && group.width < 150 ? group : anchor.getBoundingClientRect();
  origin.slot = 'trigger';
  origin.ariaHidden = 'true';
  Object.assign(origin.style, {
    position: 'fixed', visibility: 'hidden', left: `${bounds.left}px`, top: `${bounds.top}px`,
    width: `${bounds.width}px`, height: `${bounds.height}px`,
  });
  menu.append(origin);
  document.body.append(menu);

  const panel = menu.shadowRoot!.querySelector<HTMLElement>('[part="panel"]')!;
  const listeners = new AbortController();
  let switching = false;
  let select: (() => void)[] = [];

  function show(items: MenuItem[], title: string, back?: () => void) {
    switching = true;
    menu.hide();
    panel.ariaLabel = title;
    select = [];
    const rows: HTMLElement[] = [];
    function add(item: MenuItem, action: () => void) {
      const row = document.createElement('glass-menu-item');
      const title = document.createElement('span');
      title.textContent = item.text;
      row.append(title);
      row.setAttribute('value', String(select.push(action) - 1));
      row.toggleAttribute('disabled', item.enabled === false);
      row.toggleAttribute('destructive', Boolean(item.destructive));
      row.toggleAttribute('checkable', item.checked !== undefined);
      row.toggleAttribute('checked', item.checked === true);
      if (item.icon) {
        const icon = item.icon.cloneNode(true) as SVGElement;
        icon.classList.add('context-action-icon');
        title.prepend(icon);
      }
      if (item.items) {
        const arrow = document.createElement('span');
        arrow.slot = 'shortcut';
        arrow.textContent = '›';
        row.append(arrow);
      }
      rows.push(row);
    }
    if (back) {
      add({ text: '‹ 返回' }, back);
      rows.push(document.createElement('glass-menu-separator'));
    }
    for (const item of items) {
      if (!item.text) rows.push(document.createElement('glass-menu-separator'));
      else add(item, () => {
        if (item.items) show(item.items, item.text, () => show(items, title, back));
        else run(item);
      });
    }
    menu.replaceChildren(origin, ...rows);
    menu.show(!keepEditorFocus);
    switching = false;
  }

  menu.addEventListener('select', event => {
    event.preventDefault();
    select[Number((event as CustomEvent<{ value: string }>).detail.value)]?.();
  });
  menu.addEventListener('toggle', event => {
    if (!(event instanceof CustomEvent) || typeof event.detail?.open !== 'boolean') return;
    const { open } = event.detail;
    spring?.configure(open ? { duration: 0.24, bounce: 0.2 } : { duration: 0.17, bounce: 0 });
    if (!switching && !open) dismiss();
  });
  if (keepEditorFocus) document.addEventListener('pointerdown', event => {
    if (menu.open) event.preventDefault();
  }, { capture: true, signal: listeners.signal });

  return {
    menu,
    open: () => show(entries, label),
    close() {
      listeners.abort();
      menu.inert = true;
      if (panel.classList.contains('open')) {
        const observer = new MutationObserver(() => {
          if (!panel.classList.contains('open')) finish();
        });
        const finish = () => {
          observer.disconnect();
          menu.remove();
          finishClosingMenu = undefined;
        };
        finishClosingMenu = finish;
        observer.observe(panel, { attributes: true, attributeFilter: ['class'] });
        menu.hide();
      } else menu.remove();
    },
  };
}
