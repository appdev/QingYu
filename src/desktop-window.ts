import { getCurrentWindow } from '@tauri-apps/api/window';
import { ChevronDown, Copy, Minus, Square, X } from 'lucide';
import { mobile, native } from './preferences';
import { showMenu, type MenuItem } from './popup-menu';
import { translatedLabel, type UiMessage, type WorkflowMessage } from './ui-language';
import { createLucideIcon } from './vendor/markra/shared/lucide';
import './desktop-window.css';

export const usesDesktopWindowBar = native && !mobile && !/Mac/.test(navigator.platform);

export function mountDesktopWindowBar({ menu, leading, trailing, report = console.warn }: {
  menu?: () => MenuItem[];
  leading?: HTMLElement[];
  trailing?: HTMLElement[];
  report?: (message: string) => void;
} = {}) {
  if (!usesDesktopWindowBar) return;
  const current = getCurrentWindow();
  const onError = (error: unknown) => report(String(error));
  const bar = document.createElement('header');
  bar.className = 'desktop-window-bar';
  const title = document.createElement('span');
  title.textContent = document.title;
  const titleKey = document.querySelector('title')?.dataset.i18n;
  if (titleKey) title.dataset.i18n = titleKey;
  const drag = document.createElement('div');
  drag.className = 'desktop-window-drag'; drag.dataset.tauriDragRegion = '';
  if (menu) {
    const button = document.createElement('button');
    button.type = 'button'; button.className = 'desktop-window-menu';
    button.append(title, createLucideIcon(document, ChevronDown, ''));
    button.setAttribute('aria-haspopup', 'menu');
    button.onclick = () => { void showMenu(menu(), { anchor: button, label: title.textContent, presentation: 'dropdown', onError }); };
    bar.append(button);
  } else {
    title.dataset.tauriDragRegion = '';
    drag.append(title);
  }
  function addTools(controls?: HTMLElement[]) {
    if (!controls?.length) return;
    const tools = document.createElement('div');
    tools.className = 'desktop-window-tools';
    tools.append(...controls);
    bar.append(tools);
  }
  addTools(leading);
  bar.append(drag);
  addTools(trailing);

  function control(label: UiMessage | WorkflowMessage, icon: typeof X, action: () => Promise<void>) {
    const button = document.createElement('button');
    button.type = 'button'; button.className = 'desktop-window-control';
    button.dataset.i18nTitle = button.dataset.i18nAriaLabel = label;
    button.title = button.ariaLabel = translatedLabel(label);
    button.append(createLucideIcon(document, icon, ''));
    button.onclick = () => { void action().catch(onError); };
    return button;
  }
  const close = control('menu.closeWindow', X, () => current.close());
  close.classList.add('desktop-window-close');
  const caption = document.createElement('div');
  caption.className = 'desktop-window-caption';
  caption.append(close);
  bar.append(caption);
  document.body.prepend(bar);
  void Promise.all([current.isMinimizable(), current.isMaximizable()]).then(async ([canMinimize, canMaximize]) => {
    if (canMinimize) close.before(control('最小化', Minus, () => current.minimize()));
    if (!canMaximize) return;
    const maximize = control('menu.maximizeWindow', Square, () => current.toggleMaximize());
    close.before(maximize);
    async function update() {
      const pressed = String(await current.isMaximized());
      if (pressed === maximize.getAttribute('aria-pressed')) return;
      maximize.setAttribute('aria-pressed', pressed);
      maximize.replaceChildren(createLucideIcon(document, pressed === 'true' ? Copy : Square, ''));
    }
    await update();
    const unlisten = await current.onResized(() => { void update().catch(onError); });
    window.addEventListener('pagehide', unlisten, { once: true });
  }).catch(onError);
}
