import { createDrag, createIndicator, installPress } from '@fluidkit/core';
import { mountWebGlass, usesGlisse } from './web-glass-mount';
import './glass-toggle.css';

export function mountTabIndicator(container: HTMLElement) {
  const win = container.ownerDocument.defaultView;
  if (!win?.matchMedia || typeof ResizeObserver !== 'function') return;
  const thumb = container.ownerDocument.createElement('span');
  thumb.className = 'tab-indicator';
  thumb.ariaHidden = 'true';
  container.prepend(thumb);
  const unmount = usesGlisse(container) ? mountWebGlass(thumb, { tint: 88, blur: 3 }) : undefined;
  const indicator = createIndicator({
    container, indicator: thumb, items: 'button', trigger: 'press', focus: false,
    click: container.getAttribute('aria-orientation') !== 'vertical',
  });
  let pending: HTMLButtonElement | undefined;
  const shown = new ResizeObserver(() => { if (pending) select(pending); });
  function select(button: HTMLButtonElement) {
    if (!button.getClientRects().length) {
      pending = button;
      shown.observe(container);
      return;
    }
    pending = undefined;
    shown.disconnect();
    indicator.update();
    indicator.rest = button;
    if (!container.hasAttribute('data-tab-indicator')) {
      indicator.to(button, { instant: true });
      container.dataset.tabIndicator = '';
    }
  }
  return {
    select,
    destroy() { shown.disconnect(); indicator.destroy(); unmount?.(); thumb.remove(); delete container.dataset.tabIndicator; },
  };
}

export function glassToggle(input: HTMLInputElement, track: HTMLElement = document.createElement('label')) {
  // A label keeps native click and keyboard activation while Drag handles the hit area.
  track.className = 'glass-toggle';
  const thumb = document.createElement('span');
  const web = usesGlisse(input);
  thumb.className = web ? 'glass-toggle-thumb web-glass-knob' : 'glass-toggle-thumb lb qingyu-glass';
  thumb.ariaHidden = 'true';
  track.append(input, thumb);
  const unmount = web ? mountWebGlass(thumb, { tint: 94, blur: 2 }) : undefined;
  if (typeof ResizeObserver !== 'function' || typeof window.matchMedia !== 'function') {
    return { element: track, destroy() { unmount?.(); } };
  }

  const events = new AbortController();
  let gesture: AbortController | undefined;
  let released = false;
  track.addEventListener('pointerdown', event => {
    if (!event.isPrimary || event.button !== 0 || gesture) return;
    drag.disabled = input.disabled;
    if (input.disabled) return;
    drag.snap(Number(input.checked), { instant: true });
    released = false;
    gesture = new AbortController();
    const end = (endEvent: PointerEvent) => {
      if (endEvent.pointerId !== event.pointerId) return;
      released = endEvent.type === 'pointerup';
      gesture?.abort();
      gesture = undefined;
    };
    const options = { capture: true, signal: gesture.signal };
    window.addEventListener('pointerup', end, options);
    window.addEventListener('pointercancel', end, options);
  }, { signal: events.signal });

  const drag = createDrag({
    element: thumb, handle: track, axis: 'x', rubberband: 0, deceleration: .99,
    snaps: () => [0, Math.max(0, track.clientWidth - thumb.offsetWidth - 4)],
    onEnd(snap) {
      if (!released || input.disabled || snap === null || input.checked === (snap === 1)) return;
      input.checked = snap === 1;
      input.dispatchEvent(new Event('input', { bubbles: true }));
      input.dispatchEvent(new Event('change', { bubbles: true }));
    },
  });
  return { element: track, destroy() { gesture?.abort(); events.abort(); drag.destroy(); unmount?.(); } };
}

export function decorateGlisseToggle(input: HTMLInputElement) {
  if (!usesGlisse(input)) return;
  const track = document.createElement('span');
  input.before(track);
  return glassToggle(input, track);
}

function mountGlassSurface(element: HTMLElement) {
  const surface = document.createElement('span');
  const web = usesGlisse(element);
  surface.className = web ? 'glass-control-surface web-glass-frost' : 'glass-control-surface lb qingyu-glass';
  surface.ariaHidden = 'true';
  element.prepend(surface);
  if (web) mountWebGlass(surface, { tint: 62, blur: 8 });
  else installPress(element.ownerDocument);
}

export function mountGlassButton(button: HTMLButtonElement) {
  button.classList.add('glass-button');
  if (!usesGlisse(button)) button.classList.add('lb-swell');
  mountGlassSurface(button);
}

export function mountGlassToolbar(toolbar: HTMLElement) {
  toolbar.classList.add('glass-toolbar');
  mountGlassSurface(toolbar);
  const buttons = [...toolbar.querySelectorAll<HTMLButtonElement>('button')];
  const editor = toolbar.classList.contains('actions');
  if (!editor && !usesGlisse(toolbar)) for (const button of buttons) button.classList.add('lb-swell');
  const lens = document.createElement('span');
  lens.className = 'glass-toolbar-lens';
  toolbar.append(lens);
  lens.ariaHidden = 'true';
  const indicator = createIndicator({
    container: toolbar, indicator: lens, items: 'button:not(:disabled)',
    trigger: editor ? 'press' : 'manual', click: false, focus: false,
  });
  const visible = (button: HTMLButtonElement) => !button.disabled && toolbar.contains(button) && Boolean(button.getClientRects().length);
  function updateLens() {
    indicator.update();
    const active = buttons.find(button => button.id !== 'sidebar-button' && button.ariaExpanded === 'true' && visible(button));
    if (editor) {
      if (indicator.rest !== (active ?? null)) indicator.rest = active ?? null;
    } else if (active !== indicator.current) {
      indicator.to(active ?? null, { instant: !active || !indicator.current });
    }
  }
  new MutationObserver(updateLens).observe(toolbar, { subtree: true, attributes: true, attributeFilter: ['aria-expanded', 'hidden', 'disabled'] });
  updateLens();

  toolbar.addEventListener('keydown', event => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    const available = buttons.filter(visible);
    const index = available.indexOf(event.target as HTMLButtonElement);
    if (index < 0) return;
    event.preventDefault();
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? available.length - 1
      : (index + (event.key === 'ArrowLeft' ? -1 : 1) + available.length) % available.length;
    available[next].focus();
  });
}
