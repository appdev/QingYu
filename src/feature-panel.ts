import { translatedLabel } from './ui-language';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { native } from './preferences';
import { showWindowWhenPainted } from './window-ready';
import { bindVerticalPull } from './panel-pull';
import { mountDesktopWindowBar } from './desktop-window';
import './feature-panel.css';

function usesFeatureWindow() {
  return native && !document.documentElement.hasAttribute('data-mobile');
}

type MountPanel = (root: HTMLElement) => () => void;
type DialogContent = {
  dispose: () => void;
  back?: () => void;
  dismiss?: () => void;
  focus?: HTMLElement | null;
};

export function mountFeaturePanel(host: HTMLElement, title: string, close: () => void, mount: MountPanel) {
  const panel = document.createElement('section'); panel.className = 'feature-panel';
  const header = document.createElement('header'), heading = document.createElement('h2');
  const done = document.createElement('button');
  done.type = 'button';
  // 手机上和设置的「完成」一样是浮起胶囊；浏览器里的居中弹窗仍用次要按钮。
  done.className = document.documentElement.hasAttribute('data-mobile') ? 'button floating-button' : 'button secondary-button button-regular button-touch button-pill';
  done.textContent = translatedLabel('done'); done.onclick = close;
  heading.textContent = title; header.append(done, heading);
  const content = document.createElement('div'); content.className = 'feature-panel-content';
  panel.append(header, content); host.append(panel);
  const dispose = mount(content);
  return () => { dispose(); panel.remove(); };
}

export async function openFeaturePanel(options: {
  title: string;
  mount: (dialog: HTMLDialogElement, close: () => void) => DialogContent;
  restoreFocus?: () => void;
  openWindow?: () => Promise<void>;
  className?: string;
  animated?: boolean;
  dismissOnBackdrop?: boolean;
  dragHandle?: string;
}) {
  if (options.openWindow && usesFeatureWindow()) { await options.openWindow(); return; }
  const previous = document.activeElement as HTMLElement | null;
  const dialog = document.createElement('dialog');
  dialog.className = 'feature-panel-dialog';
  if (options.className) dialog.classList.add(options.className);
  if (options.animated) dialog.dataset.featureAnimated = '';
  dialog.ariaLabel = options.title;
  dialog.addEventListener('keydown', event => event.stopPropagation());
  document.body.append(dialog);
  let content: DialogContent;
  let disposeDrag: (() => void) | undefined;
  function close() {
    if (dialog.inert) return;
    dialog.inert = true;
    disposeDrag?.();
    function remove() {
      content.dispose();
      dialog.remove();
      if (options.restoreFocus) options.restoreFocus(); else previous?.focus({ preventScroll: true });
    }
    if (options.animated) {
      dialog.dataset.featureClosing = '';
      const animations = dialog.getAnimations?.() ?? [];
      if (animations.length) {
        void Promise.allSettled(animations.map(animation => animation.finished)).then(remove);
        return;
      }
    }
    remove();
  }
  try { content = options.mount(dialog, close); }
  catch (error) { dialog.remove(); throw error; }
  const dismiss = content.dismiss ?? close;
  dialog.addEventListener('close', close, { once: true });
  dialog.addEventListener('cancel', event => { event.preventDefault(); (content.back ?? close)(); });
  if (options.dismissOnBackdrop !== false) dialog.addEventListener('click', event => {
    if (event.target !== dialog) return;
    const rect = dialog.getBoundingClientRect();
    if (event.clientX < rect.left || event.clientX >= rect.right || event.clientY < rect.top || event.clientY >= rect.bottom) dismiss();
  });
  if (options.dragHandle) disposeDrag = bindPanelDrag(dialog, options.dragHandle, dismiss);
  dialog.showModal();
  content.focus?.focus({ preventScroll: true });
}

function bindPanelDrag(root: HTMLDialogElement, handle: string, dismiss: () => void) {
  const disposers = [...root.querySelectorAll<HTMLElement>(handle)].map(header => {
    let height = 0;
    return bindVerticalPull(header, {
      canStart(event) {
        if ((event.target as Element).closest('button') || root.inert || root.hasAttribute('data-feature-dragging')) return false;
        if (root.getAnimations?.().some(animation => animation.playState === 'running')) return false;
        height = root.getBoundingClientRect().height;
        return height > 0;
      },
      move(offset) {
        const y = Math.min(height, Math.max(0, offset));
        root.dataset.featureDragging = '';
        root.style.setProperty('--feature-sheet-transform', `translateY(${y}px)`);
        root.style.setProperty('--feature-backdrop-opacity', String(1 - y / height));
      },
      finish(offset) {
        root.removeAttribute('data-feature-dragging');
        if (offset !== null && offset >= Math.min(120, height / 4)) { dismiss(); return; }
        root.style.removeProperty('--feature-sheet-transform');
        root.style.removeProperty('--feature-backdrop-opacity');
      },
    });
  });
  return () => disposers.forEach(dispose => dispose());
}

export async function mountFeatureWindow(root: HTMLElement, mount: (close: () => void) => (() => void) | Promise<() => void>) {
  const windowHandle = getCurrentWindow();
  const close = () => { void windowHandle.close(); };
  root.classList.add('feature-window-root');
  mountDesktopWindowBar();
  document.addEventListener('keydown', event => {
    if (event.isComposing) return;
    if (event.key === 'Escape' || event.key.toLowerCase() === 'w' && (event.metaKey || event.ctrlKey)) {
      event.preventDefault(); close();
    }
  });
  try {
    const dispose = await mount(close);
    window.addEventListener('pagehide', dispose, { once: true });
  } finally { showWindowWhenPainted(); }
}
