import { createDrag, type Drag } from '@fluidkit/core';

export function bindVerticalPull(handle: HTMLElement, options: {
  canStart?: (event: PointerEvent) => boolean;
  move: (offset: number) => void;
  finish: (offset: number | null) => void;
}) {
  if (typeof ResizeObserver !== 'function' || typeof window.matchMedia !== 'function') return () => {};
  const events = new AbortController();
  const { signal } = events;
  let pointer: number | undefined;
  let released = false;
  let drag: Drag;
  handle.addEventListener('pointerdown', event => {
    if (!event.isPrimary || event.button !== 0 || pointer !== undefined) return;
    drag.disabled = options.canStart?.(event) === false;
    if (drag.disabled) return;
    drag.snap(0, { instant: true });
    pointer = event.pointerId;
    released = false;
  }, { signal });
  for (const type of ['pointerup', 'pointercancel'] as const) {
    window.addEventListener(type, event => {
      if (event.pointerId !== pointer) return;
      released = type === 'pointerup';
      pointer = undefined;
    }, { capture: true, signal });
  }
  handle.addEventListener('lostpointercapture', event => {
    if (event.pointerId !== pointer) return;
    pointer = undefined;
    if (drag.dragging) options.finish(null);
    drag.snap(0, { instant: true });
  }, { signal });
  // The panel owns what a pull changes: translate, backdrop opacity or sheet height.
  drag = createDrag({
    element: document.createElement('span'), handle, axis: 'y', snaps: [0],
    bounds: { y: [-Infinity, Infinity] },
    onMove({ y }) { if (drag?.dragging) options.move(y); },
    onEnd() { options.finish(released ? drag.position.y : null); },
  });
  return () => { events.abort(); drag.destroy(); };
}
