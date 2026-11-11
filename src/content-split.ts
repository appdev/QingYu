// MiaoYan EditorContentSplitView: drag bounds differ from mode restoration.
export function constrainContentSplit(total: number, width: number) {
  return Math.max(200, Math.min(width, total - 200));
}

export function restoreContentSplit(total: number, ratio: number) {
  const width = ratio > 0 ? total * ratio : Math.min(total / 2, 600);
  return Math.min(1200, constrainContentSplit(total, width));
}

export function setupContentSplit(workspace: HTMLElement, measure: () => void, error: (message: string) => void) {
  const key = 'miaoyan-editor.content-split.v1';
  const handle = workspace.querySelector<HTMLElement>('#splitter')!;
  const pane = workspace.querySelector<HTMLElement>('#editor-pane')!;
  let ratio = 0, stored = '';
  try {
    stored = localStorage.getItem(key) ?? '';
    const saved: unknown = JSON.parse(stored || 'null');
    if (typeof saved === 'number' && Number.isFinite(saved) && saved > 0 && saved < 1) ratio = saved;
  } catch { /* First split uses the native default. */ }
  let entering = false, previousWidth = 0;
  let drag: { id: number; x: number; width: number; ratio: number } | undefined;
  const active = () => workspace.dataset.mode === 'split';
  const totalWidth = () => workspace.getBoundingClientRect().width;

  function save() {
    const next = JSON.stringify(ratio);
    if (next === stored) return;
    try { localStorage.setItem(key, next); stored = next; }
    catch { error('编辑与预览的分栏位置未能保存'); }
  }

  function render(width: number) {
    const total = totalWidth();
    const value = Math.round(width * devicePixelRatio) / devicePixelRatio;
    ratio = value / total;
    previousWidth = total;
    workspace.style.setProperty('--split', `${value}px`);
    handle.setAttribute('aria-valuemin', String(Math.min(200, value)));
    handle.setAttribute('aria-valuemax', String(Math.max(total - 200, value)));
    handle.setAttribute('aria-valuenow', String(value));
    measure();
    if (!drag) save();
  }

  function update() {
    // Release CSS can arrive after module initialization; wait for the real layout.
    if (!active() || getComputedStyle(workspace).display !== 'flex') return;
    const total = totalWidth();
    if (!total || !entering && total === previousWidth) return;
    const width = entering ? restoreContentSplit(total, ratio) : total * ratio;
    entering = false;
    // Native live window resizing scales proportionally; it does not reapply drag bounds.
    render(width);
  }

  function finish(cancel = false) {
    if (!drag) return;
    const previous = drag;
    drag = undefined;
    delete document.body.dataset.contentResizing;
    if (workspace.hasPointerCapture(previous.id)) workspace.releasePointerCapture(previous.id);
    if (cancel) { ratio = previous.ratio; if (active()) render(totalWidth() * ratio); }
    else save();
  }

  handle.onpointerdown = event => {
    if (event.button !== 0 || !active() || drag) return;
    event.preventDefault();
    drag = { id: event.pointerId, x: event.clientX, width: pane.getBoundingClientRect().width, ratio };
    document.body.dataset.contentResizing = 'true';
    workspace.setPointerCapture(event.pointerId);
  };
  workspace.addEventListener('pointermove', event => {
    if (drag?.id === event.pointerId) render(constrainContentSplit(totalWidth(), drag.width + event.clientX - drag.x));
  });
  workspace.addEventListener('pointerup', event => { if (drag?.id === event.pointerId) finish(); });
  workspace.addEventListener('pointercancel', event => { if (drag?.id === event.pointerId) finish(true); });
  workspace.addEventListener('lostpointercapture', event => { if (drag?.id === event.pointerId) finish(true); });
  window.addEventListener('blur', () => finish(true));
  document.addEventListener('keydown', event => { if (drag && event.key === 'Escape') { event.preventDefault(); finish(true); } });
  handle.onkeydown = event => {
    if (!active() || !['ArrowLeft', 'ArrowRight'].includes(event.key)) return;
    event.preventDefault();
    render(constrainContentSplit(totalWidth(), pane.getBoundingClientRect().width + totalWidth() * (event.key === 'ArrowLeft' ? -.02 : .02)));
  };
  new ResizeObserver(update).observe(workspace);
  return { modeChanged() { finish(true); entering = active(); previousWidth = 0; update(); } };
}
