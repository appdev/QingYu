import { attachPress, clamp, glass, supportsBackdropSvg } from 'glassy-liquid';

export function glassToggle(input: HTMLInputElement) {
  const track = document.createElement('span');
  track.className = 'glass-toggle';
  const thumb = document.createElement('span');
  thumb.className = 'glass-toggle-thumb';
  thumb.ariaHidden = 'true';
  track.append(input, thumb);
  let drag: { pointerId: number; start: number; travel: number; checked: boolean } | undefined;
  let dragged = false;

  input.addEventListener('pointerdown', event => {
    if (input.disabled || event.button !== 0 || !event.isPrimary) return;
    drag = { pointerId: event.pointerId, start: event.clientX, travel: track.clientWidth - thumb.offsetWidth - 4, checked: input.checked };
    dragged = false;
    track.dataset.pressed = '';
    input.setPointerCapture(event.pointerId);
  });
  function dragPosition(pointer: NonNullable<typeof drag>, x: number) {
    return clamp((pointer.checked ? 1 : 0) + (x - pointer.start) / Math.max(1, pointer.travel), 0, 1);
  }
  input.addEventListener('pointermove', event => {
    if (!drag || event.pointerId !== drag.pointerId) return;
    if (!dragged && Math.abs(event.clientX - drag.start) < 5) return;
    dragged = true;
    track.toggleAttribute('data-dragging', true);
    track.style.setProperty('--glass-toggle-x', `${dragPosition(drag, event.clientX) * drag.travel}px`);
  });
  function stopDrag() {
    drag = undefined;
    delete track.dataset.pressed;
    delete track.dataset.dragging;
    track.style.removeProperty('--glass-toggle-x');
  }
  input.addEventListener('pointerup', event => {
    if (!drag || event.pointerId !== drag.pointerId) return;
    const checked = dragPosition(drag, event.clientX) >= .5;
    stopDrag();
    if (!dragged || checked === input.checked) return;
    input.checked = checked;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.dispatchEvent(new Event('change', { bubbles: true }));
  });
  input.addEventListener('lostpointercapture', event => {
    if (drag?.pointerId !== event.pointerId) return;
    stopDrag();
    dragged = false;
  });
  // A drag commits on release. Cancel the native click's second toggle.
  input.addEventListener('click', event => {
    if (dragged && event.detail > 0) event.preventDefault();
    dragged = false;
  });
  return track;
}

export function mountGlassButton(button: HTMLButtonElement) {
  button.classList.add('glass-button');
  const surface = glass(button, { radius: 'capsule', blur: 8, fill: 'var(--glass-control-fill)', shadow: false });
  attachPress(button, surface, { grow: -.04, stretch: false });
}

export function mountGlassToolbar(toolbar: HTMLElement) {
  toolbar.classList.add('glass-toolbar');
  const surface = glass(toolbar, {
    radius: 'capsule', blur: supportsBackdropSvg() ? 8 : 16, specular: 0,
    fill: 'var(--glass-control-fill)', shadow: false,
  });
  const lens = document.createElement('span');
  lens.className = 'glass-toolbar-lens';
  lens.ariaHidden = 'true';
  lens.hidden = true;
  toolbar.append(lens);
  const buttons = [...toolbar.querySelectorAll<HTMLButtonElement>('button')];
  for (const button of buttons) attachPress(button, surface, { grow: -.04, stretch: false, glow: false });

  function updateLens() {
    const active = buttons.find(button => button.id !== 'sidebar-button' && button.ariaExpanded === 'true'
      && !button.disabled && toolbar.contains(button) && button.getClientRects().length);
    const appearing = lens.hidden;
    lens.toggleAttribute('hidden', !active);
    if (!active) return;
    // 从隐藏到出现时直接落在按钮上；只有在两个按钮之间切换时才滑过去。
    if (appearing) lens.style.transition = 'none';
    // 和 Pinto 一样画成按钮中间的一个圆，比按钮四周各小一圈。
    const inset = parseFloat(getComputedStyle(toolbar).getPropertyValue('--glass-lens-inset')) || 0;
    const size = Math.max(0, Math.min(active.offsetWidth, active.offsetHeight) - inset * 2);
    Object.assign(lens.style, {
      width: `${size}px`, height: `${size}px`,
      translate: `${active.offsetLeft + (active.offsetWidth - size) / 2}px ${active.offsetTop + (active.offsetHeight - size) / 2}px`,
    });
    if (appearing) {
      void lens.offsetWidth;
      lens.style.removeProperty('transition');
    }
  }
  new MutationObserver(updateLens).observe(toolbar, { subtree: true, attributes: true, attributeFilter: ['aria-expanded', 'hidden', 'disabled'] });
  if (typeof ResizeObserver === 'function') new ResizeObserver(updateLens).observe(toolbar);
  toolbar.addEventListener('keydown', event => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    const visible = buttons.filter(button => toolbar.contains(button) && !button.disabled && button.getClientRects().length);
    const index = visible.indexOf(event.target as HTMLButtonElement);
    if (index < 0 || !visible.length) return;
    event.preventDefault();
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? visible.length - 1
      : (index + (event.key === 'ArrowLeft' ? -1 : 1) + visible.length) % visible.length;
    visible[next].focus();
  });
}
