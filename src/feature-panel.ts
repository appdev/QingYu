import { translatedLabel } from './ui-language';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { native } from './preferences';
import { showWindowWhenPainted } from './window-ready';
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
  openWindow: () => Promise<void>;
  className?: string;
  animated?: boolean;
  dismissOnBackdrop?: boolean;
  dragHandle?: string;
}) {
  if (usesFeatureWindow()) { await options.openWindow(); return; }
  const previous = document.activeElement as HTMLElement | null;
  const dialog = document.createElement('dialog');
  dialog.className = 'feature-panel-dialog';
  if (options.className) dialog.classList.add(options.className);
  if (options.animated) dialog.dataset.featureAnimated = '';
  dialog.ariaLabel = options.title;
  dialog.addEventListener('keydown', event => event.stopPropagation());
  document.body.append(dialog);
  let content: DialogContent;
  function close() {
    if (dialog.inert) return;
    dialog.inert = true;
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
  if (options.dragHandle) bindPanelDrag(dialog, options.dragHandle, dismiss);
  dialog.showModal();
  content.focus?.focus({ preventScroll: true });
}

function bindPanelDrag(root: HTMLDialogElement, handle: string, dismiss: () => void) {
  let drag: { pointerId: number; startY: number; height: number } | undefined;
  function move(clientY: number, startY: number, height: number) {
    const offset = Math.min(height, Math.max(0, clientY - startY));
    root.style.setProperty('--feature-sheet-transform', `translateY(${offset}px)`);
    root.style.setProperty('--feature-backdrop-opacity', String(1 - offset / height));
    return offset;
  }
  function endDrag(dismissed = false) {
    drag = undefined;
    root.removeAttribute('data-feature-dragging');
    if (dismissed) { dismiss(); return; }
    root.style.removeProperty('--feature-sheet-transform');
    root.style.removeProperty('--feature-backdrop-opacity');
  }
  root.addEventListener('pointerdown', event => {
    const target = event.target as HTMLElement;
    const header = target.closest<HTMLElement>(handle);
    if (!header || target.closest('button') || !event.isPrimary || event.button !== 0 || drag || root.inert) return;
    if (root.getAnimations?.().some(animation => animation.playState === 'running')) return;
    const height = root.getBoundingClientRect().height;
    if (!height) return;
    drag = { pointerId: event.pointerId, startY: event.clientY, height };
    root.dataset.featureDragging = '';
    header.setPointerCapture?.(event.pointerId);
  });
  root.addEventListener('pointermove', event => {
    if (drag?.pointerId === event.pointerId) move(event.clientY, drag.startY, drag.height);
  });
  root.addEventListener('pointerup', event => {
    if (drag?.pointerId !== event.pointerId) return;
    const offset = move(event.clientY, drag.startY, drag.height);
    endDrag(offset >= Math.min(120, drag.height / 4));
  });
  for (const type of ['pointercancel', 'lostpointercapture'] as const) root.addEventListener(type, event => {
    if (drag?.pointerId === event.pointerId) endDrag();
  });
}

export async function mountFeatureWindow(root: HTMLElement, mount: (close: () => void) => (() => void) | Promise<() => void>) {
  const windowHandle = getCurrentWindow();
  const close = () => { void windowHandle.close(); };
  root.classList.add('feature-window-root');
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
