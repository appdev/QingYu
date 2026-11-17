// WebKit can pan the entire document to reveal a focused field, even with a fixed body.
export function keepMobileViewportAtTop() {
  if (window.scrollY && (window.visualViewport?.scale ?? 1) === 1) window.scrollTo(0, 0);
}

export function keepEditorFocusForTouchButtons(container: HTMLElement, accepts = (_button: HTMLButtonElement) => true) {
  let activeTouch: { id: number; button: HTMLButtonElement } | undefined;
  container.addEventListener('touchstart', event => {
    const button = (event.target as Element).closest<HTMLButtonElement>('button');
    if (!button || !accepts(button) || event.touches.length !== 1 || !event.cancelable) return;
    // WebKit can blur the editor before the synthetic click reaches the button.
    event.preventDefault();
    activeTouch = { id: event.changedTouches[0].identifier, button };
  }, { passive: false });
  container.addEventListener('touchend', event => {
    const touch = [...event.changedTouches].find(touch => touch.identifier === activeTouch?.id);
    if (!touch || !activeTouch) return;
    const { button } = activeTouch;
    activeTouch = undefined;
    if (button.contains(container.ownerDocument.elementFromPoint(touch.clientX, touch.clientY))) button.click();
  });
  container.addEventListener('touchcancel', () => { activeTouch = undefined; });
}

export function showEditorInputPanel(panel: HTMLDialogElement, input?: HTMLElement) {
  const doc = panel.ownerDocument;
  let target: HTMLElement | undefined;
  let inputMode: string | null;
  function releaseInput() {
    if (!target) return;
    if (inputMode === null) target.removeAttribute('inputmode');
    else target.setAttribute('inputmode', inputMode);
    if (doc.activeElement === target) target.blur();
  }
  function useInput(next?: HTMLElement) {
    if (next === target) return;
    releaseInput();
    target = next;
    if (!next) return;
    inputMode = next.getAttribute('inputmode');
    next.setAttribute('inputmode', 'none');
    // Re-enter the editor with its soft keyboard suppressed, keeping the editor's selection.
    next.blur();
    next.focus({ preventScroll: true });
  }
  panel.addEventListener('pointerdown', event => {
    if ((event.target as Element).closest('button')) event.preventDefault();
  });
  doc.body.append(panel);
  panel.open = true;
  useInput(input);
  return { useInput, close() { panel.remove(); releaseInput(); } };
}

// Native insets and the app toolbar already define the editor's visible box.
export function revealCaret(root?: HTMLElement) {
  if (!root || !root.contains(root.ownerDocument.activeElement)) return;
  const selection = root.ownerDocument.getSelection();
  if (!selection?.rangeCount || !root.contains(selection.anchorNode)) return;
  const range = selection.getRangeAt(0).cloneRange();
  range.collapse(false);
  const node = range.startContainer;
  const target = node.nodeType === Node.TEXT_NODE ? node.parentElement : node.childNodes[range.startOffset] ?? node;
  const caret = range.getClientRects()[0] ?? (target instanceof Element && target !== root ? target.getBoundingClientRect() : undefined);
  // Leave room for Android's native selection handle above the format bar.
  revealInScroller(root, caret, 32);
}

type VerticalBounds = Pick<DOMRect, 'top' | 'bottom'>;

function revealInScroller(scroller: HTMLElement, target: VerticalBounds | null | undefined, margin: number) {
  if (!target || target.bottom <= target.top) return;
  const box = scroller.getBoundingClientRect();
  if (target.bottom > box.bottom - margin) scroller.scrollTop += target.bottom - box.bottom + margin;
  else if (target.top < box.top + margin) scroller.scrollTop -= box.top + margin - target.top;
}

export function keepPanelTargetVisible(
  workspace: HTMLElement,
  scroller: HTMLElement,
  panel: HTMLDialogElement,
  target: () => VerticalBounds | null | undefined,
) {
  const initialPadding = workspace.style.paddingBottom;
  let frame = 0;
  function refresh() {
    cancelAnimationFrame(frame);
    frame = requestAnimationFrame(() => {
      const overlap = Math.max(0, workspace.getBoundingClientRect().bottom - panel.getBoundingClientRect().top);
      workspace.style.paddingBottom = overlap ? `${overlap}px` : initialPadding;
      revealInScroller(scroller, target(), 8);
    });
  }
  const observer = new ResizeObserver(refresh);
  observer.observe(workspace, { box: 'border-box' });
  observer.observe(panel);
  refresh();
  return {
    refresh,
    stop() {
      observer.disconnect();
      cancelAnimationFrame(frame);
      workspace.style.paddingBottom = initialPadding;
    },
  };
}
