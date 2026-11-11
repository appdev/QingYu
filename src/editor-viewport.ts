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
  if (!caret?.height) return;
  const box = root.getBoundingClientRect();
  // Leave room for Android's native selection handle above the format bar.
  const margin = 32;
  if (caret.bottom > box.bottom - margin) root.scrollTop += caret.bottom - box.bottom + margin;
  else if (caret.top < box.top + margin) root.scrollTop -= box.top + margin - caret.top;
}
